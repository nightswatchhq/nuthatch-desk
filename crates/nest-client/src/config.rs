//! `nests.toml`, the file the terminal client reads, and what a nest URL is allowed to be.

use std::{collections::BTreeMap, net::IpAddr, path::PathBuf};

use serde::Deserialize;

/// Where a nest listens unless told otherwise.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8288";

/// One entry in `nests.toml`.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NestTarget {
    /// Where the nest listens, as seen from where it runs.
    pub url: Option<String>,
    /// The ssh host the terminal client reaches the nest through, when it listens only on that
    /// host's loopback. This client opens no forward of its own.
    pub ssh: Option<String>,
    /// Token decimals for amount columns. Read so the file parses; not used here yet.
    #[serde(default)]
    pub decimals: BTreeMap<String, u32>,
}

/// The file both clients read: `nuthatch-tui/nests.toml` under the user's config directory.
pub fn config_path() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let base = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| env("APPDATA").map(PathBuf::from))?;
    Some(base.join("nuthatch-tui").join("nests.toml"))
}

/// The entries of a `nests.toml`, by name.
pub fn parse_nests(text: &str) -> Result<BTreeMap<String, NestTarget>, String> {
    toml::from_str(text).map_err(|error| error.message().to_owned())
}

/// `value` without its trailing slashes.
pub fn normalize_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

/// Why a URL is not one this client will poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlProblem {
    /// Not a URL at all.
    Malformed,
    /// A scheme other than `http` or `https`.
    Scheme(String),
    /// No host.
    NoHost,
}

impl std::fmt::Display for UrlProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("not a URL"),
            Self::Scheme(scheme) => write!(f, "scheme '{scheme}' is not http or https"),
            Self::NoHost => f.write_str("no host in the URL"),
        }
    }
}

/// A URL this client will poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The normalised URL, without a trailing slash.
    pub url: String,
    /// Plain HTTP to a host that is not this machine: readable and alterable in transit.
    pub plain_remote: bool,
}

/// Checks a configured or typed URL. Only `http` and `https` are accepted, so nothing in
/// `nests.toml` can point the client at a file or another handler.
pub fn check_url(value: &str) -> Result<Endpoint, UrlProblem> {
    let url = normalize_url(value);
    let parsed = reqwest::Url::parse(&url).map_err(|_| UrlProblem::Malformed)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(UrlProblem::Scheme(parsed.scheme().to_owned()));
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or(UrlProblem::NoHost)?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    Ok(Endpoint {
        plain_remote: parsed.scheme() == "http" && !loopback,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_clients_file_parses() {
        let nests = parse_nests(
            "[local]\nurl = \"http://127.0.0.1:8288/\"\n\n\
             [prod]\nurl = \"http://127.0.0.1:8288\"\nssh = \"root@nest\"\n\n\
             [prod.decimals]\nvalue = 6\n\"usdc__transfer.value\" = 6\n",
        )
        .unwrap();
        assert_eq!(nests.len(), 2);
        assert_eq!(nests["prod"].ssh.as_deref(), Some("root@nest"));
        assert_eq!(nests["prod"].decimals["value"], 6);
    }

    #[test]
    fn an_unknown_key_is_an_error_not_a_silent_skip() {
        assert!(parse_nests("[local]\nurl = \"http://x\"\ncommand = \"rm -rf\"\n").is_err());
    }

    #[test]
    fn only_http_and_https_are_polled() {
        for bad in [
            "file:///etc/passwd",
            "ssh://host",
            "javascript:alert(1)",
            "ftp://x/",
        ] {
            assert!(
                matches!(check_url(bad), Err(UrlProblem::Scheme(_))),
                "{bad}"
            );
        }
        assert_eq!(check_url("not a url"), Err(UrlProblem::Malformed));
        assert_eq!(check_url("http://"), Err(UrlProblem::Malformed));
    }

    #[test]
    fn plain_http_is_flagged_everywhere_but_loopback() {
        for local in [
            "http://127.0.0.1:8288",
            "http://localhost:8288/",
            "http://LOCALHOST:1",
            "http://[::1]:8288",
            "http://127.9.9.9",
        ] {
            assert!(!check_url(local).unwrap().plain_remote, "{local}");
        }
        for remote in [
            "http://203.0.113.7:8288",
            "http://nest.example.com",
            "http://[2001:db8::1]",
        ] {
            assert!(check_url(remote).unwrap().plain_remote, "{remote}");
        }
        assert!(!check_url("https://nest.example.com").unwrap().plain_remote);
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        assert_eq!(
            check_url(" http://localhost:8288// ").unwrap().url,
            "http://localhost:8288"
        );
    }
}
