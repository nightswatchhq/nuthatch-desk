//! An `ssh -N -L` forward to a nest that listens only on another host's loopback.
//!
//! Ported from the terminal client, which reaches the same nests the same way. `BatchMode`
//! because there is nowhere for a password prompt to go, and `ExitOnForwardFailure` so that a
//! forward which cannot bind is an exit rather than a quiet ssh session forwarding nothing.
//!
//! The host comes from `nests.toml`, which is not trusted. It is passed to `ssh` as one argument,
//! never through a shell, and [`check_host`] refuses anything `ssh` could read as an option.

use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use crate::config::normalize_url;

/// How long `ssh` is given to open the forward.
const OPENING_LIMIT: Duration = Duration::from_secs(15);
/// A forward that has stayed up this long is healthy, and its next failure starts the backoff over.
const SETTLED: Duration = Duration::from_secs(60);

/// How long to wait before reopening a forward that has failed `failures` times running.
pub fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.min(5)).min(30))
}

/// Whether `host` may be handed to `ssh` as its destination.
///
/// A destination beginning with `-` would be read as an option, and `-oProxyCommand=...` runs
/// whatever follows. Whitespace and control characters have no place in one either.
pub fn check_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        Err("the ssh host is empty".into())
    } else if host.starts_with('-') {
        Err(format!(
            "'{host}' is not an ssh host: it begins with a dash"
        ))
    } else if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Err("the ssh host has a space or a control character in it".into())
    } else {
        Ok(())
    }
}

/// The arguments `ssh` is run with.
pub fn ssh_args(host: &str, forward: &str) -> Vec<String> {
    [
        "-N",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-L",
        forward,
        host,
    ]
    .map(String::from)
    .to_vec()
}

fn spawn(program: &str, host: &str, forward: &str) -> std::io::Result<Child> {
    Command::new(program)
        .args(ssh_args(host, forward))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
}

/// An open forward. Dropping it ends the `ssh` process.
#[derive(Debug)]
pub struct Tunnel {
    program: String,
    host: String,
    forward: String,
    local_url: String,
    local_port: u16,
    child: Child,
    opened_at: Instant,
    failures: u32,
    retry_at: Option<Instant>,
    last_error: String,
}

impl Tunnel {
    /// Opens a forward through `host` to the nest at `nest_url`, a URL as seen from `host`, and
    /// waits until it is listening. `quit` is asked now and then whether to give up.
    pub fn open(
        program: &str,
        host: &str,
        nest_url: &str,
        quit: &dyn Fn() -> bool,
    ) -> Result<Self, String> {
        check_host(host)?;
        let mut url =
            reqwest::Url::parse(nest_url).map_err(|_| format!("'{nest_url}' is not a URL"))?;
        let remote_host = url
            .host_str()
            .ok_or_else(|| format!("'{nest_url}' names no host"))?
            .to_owned();
        let remote_port = url
            .port_or_known_default()
            .ok_or_else(|| format!("'{nest_url}' names no port"))?;
        // A port nothing holds, found by taking one and letting it go.
        let local_port = TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .map_err(|error| format!("no local port for the forward: {error}"))?
            .port();
        let unsettable = || format!("cannot point '{nest_url}' at the forward");
        url.set_host(Some("127.0.0.1")).map_err(|_| unsettable())?;
        url.set_port(Some(local_port)).map_err(|()| unsettable())?;
        let forward = format!("127.0.0.1:{local_port}:{remote_host}:{remote_port}");
        let child = spawn(program, host, &forward)
            .map_err(|error| format!("could not start {program}: {error}"))?;
        let mut tunnel = Self {
            program: program.to_owned(),
            host: host.to_owned(),
            forward,
            local_url: normalize_url(url.as_str()),
            local_port,
            child,
            opened_at: Instant::now(),
            failures: 0,
            retry_at: None,
            last_error: String::new(),
        };
        tunnel.wait_until_listening(quit)?;
        Ok(tunnel)
    }

    /// The nest's URL through the forward.
    pub fn local_url(&self) -> &str {
        &self.local_url
    }

    fn wait_until_listening(&mut self, quit: &dyn Fn() -> bool) -> Result<(), String> {
        let started = Instant::now();
        let address = ([127, 0, 0, 1], self.local_port).into();
        loop {
            if quit() {
                return Err("given up".into());
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let reason = self.stderr();
                    return Err(format!("ssh to {} exited ({status}): {reason}", self.host));
                }
                Ok(None) => {}
                Err(error) => return Err(format!("ssh to {}: {error}", self.host)),
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            if started.elapsed() >= OPENING_LIMIT {
                return Err(format!(
                    "ssh to {} had not opened the forward after {}s",
                    self.host,
                    OPENING_LIMIT.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The last line `ssh` wrote, which is where it says why it gave up.
    fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            let _ = stderr.read_to_string(&mut text);
        }
        match text.trim().lines().last() {
            Some(line) if !line.is_empty() => line.to_owned(),
            _ => "no message".into(),
        }
    }

    /// Notices `ssh` exiting and reopens the forward with a doubling backoff. Call before each use.
    /// Returns what to say while the forward is down, or `None` while it is up.
    pub fn supervise(&mut self) -> Option<String> {
        if let Some(at) = self.retry_at {
            let now = Instant::now();
            if now < at {
                return Some(format!(
                    "ssh to {} exited: {}. Reopening in {}s",
                    self.host,
                    self.last_error,
                    // A countdown rounds up, or the first second reads "in 0s".
                    (at - now).as_secs_f64().ceil() as u64
                ));
            }
            self.retry_at = None;
            match spawn(&self.program, &self.host, &self.forward) {
                Ok(child) => {
                    self.child = child;
                    self.opened_at = now;
                }
                Err(error) => {
                    self.last_error = error.to_string();
                    self.schedule_retry();
                }
            }
            return Some(format!("ssh to {}: reopening the forward", self.host));
        }
        match self.child.try_wait() {
            Ok(None) => {
                if self.opened_at.elapsed() > SETTLED {
                    self.failures = 0;
                }
                None
            }
            Ok(Some(_)) => {
                self.last_error = self.stderr();
                self.schedule_retry();
                self.supervise()
            }
            Err(error) => Some(format!("ssh to {}: {error}", self.host)),
        }
    }

    fn schedule_retry(&mut self) {
        self.retry_at = Some(Instant::now() + backoff(self.failures));
        self.failures += 1;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_ssh_could_read_as_an_option_is_refused() {
        for bad in [
            "",
            "-oProxyCommand=touch /tmp/x",
            "-v",
            "a b",
            "host\n-oX=y",
            "a\tb",
        ] {
            assert!(check_host(bad).is_err(), "{bad:?}");
        }
        for good in [
            "root@89.0.0.1",
            "nest",
            "user@host.example.com",
            "[::1]",
            "alias-in-ssh-config",
        ] {
            assert!(check_host(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn the_host_is_the_last_argument_and_one_argument() {
        let args = ssh_args("root@nest", "127.0.0.1:5000:127.0.0.1:8288");
        assert_eq!(args.last().map(String::as_str), Some("root@nest"));
        assert!(args.contains(&"BatchMode=yes".to_owned()));
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_owned()));
        let at = args.iter().position(|arg| arg == "-L").unwrap();
        assert_eq!(args[at + 1], "127.0.0.1:5000:127.0.0.1:8288");
    }

    #[test]
    fn the_backoff_doubles_to_half_a_minute() {
        let secs: Vec<u64> = (0..7).map(|failures| backoff(failures).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30]);
    }

    #[test]
    fn a_refused_host_starts_nothing() {
        let error = Tunnel::open("ssh", "-oProxyCommand=x", "http://127.0.0.1:8288", &|| {
            false
        })
        .unwrap_err();
        assert!(error.contains("begins with a dash"), "{error}");
    }

    #[test]
    fn a_program_that_is_not_there_says_so() {
        let error = Tunnel::open(
            "no-such-ssh-program",
            "nest",
            "http://127.0.0.1:8288",
            &|| false,
        )
        .unwrap_err();
        assert!(
            error.starts_with("could not start no-such-ssh-program: "),
            "{error}"
        );
    }

    #[test]
    fn a_url_that_is_not_one_is_refused_before_ssh_runs() {
        let error =
            Tunnel::open("no-such-ssh-program", "nest", "not a url", &|| false).unwrap_err();
        assert_eq!(error, "'not a url' is not a URL");
    }

    /// `false` takes ssh's arguments and exits 1, which is ssh failing to connect.
    #[cfg(unix)]
    #[test]
    fn an_ssh_that_exits_is_reported_with_its_status() {
        let error = Tunnel::open("false", "nest", "http://127.0.0.1:8288", &|| false).unwrap_err();
        assert!(error.starts_with("ssh to nest exited ("), "{error}");
        assert!(error.ends_with("no message"), "{error}");
    }

    /// `sleep` with ssh's arguments fails at once on some systems and sleeps on none, so the
    /// stand-in for an ssh that hangs is a shell told to wait.
    #[cfg(unix)]
    #[test]
    fn opening_gives_up_when_asked_to() {
        let dir = std::env::temp_dir().join(format!("desk-fake-ssh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hangs");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let started = Instant::now();
        let asked = std::cell::Cell::new(0);
        let error = Tunnel::open(
            script.to_str().unwrap(),
            "nest",
            "http://127.0.0.1:8288",
            &|| {
                asked.set(asked.get() + 1);
                asked.get() > 3
            },
        )
        .unwrap_err();
        assert_eq!(error, "given up");
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
