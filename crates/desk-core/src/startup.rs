//! What the client opens on start: the command line and `nests.toml`, turned into tabs.

use nest_client::config::{DEFAULT_URL, normalize_url, parse_nests};

/// The usage text.
pub const USAGE: &str = "\
nuthatch-desk [--url URL]...

  --url URL   open a tab for the nest at URL, beside those in nests.toml
  --version   print the version

With no --url and no nests.toml, one tab opens on http://127.0.0.1:8288.
nests.toml is the terminal client's: ~/.config/nuthatch-tui/nests.toml.";

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Run, with these extra nests open.
    Run(Vec<String>),
    /// Print the version and stop.
    Version,
    /// Print the usage and stop.
    Help,
}

/// Reads the command line, without the program's name.
pub fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Invocation, String> {
    let mut urls = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => urls.push(args.next().ok_or("--url needs a nest's URL")?),
            "-V" | "--version" => return Ok(Invocation::Version),
            "-h" | "--help" => return Ok(Invocation::Help),
            other => return Err(format!("unknown argument '{other}'; try --help")),
        }
    }
    Ok(Invocation::Run(urls))
}

/// One tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// The tab's title.
    pub name: String,
    /// The nest's URL, as written. It is checked when the tab connects.
    pub url: String,
    /// Something the reader must do before the tab can work, or empty.
    pub note: String,
}

/// The tabs to open, and why `nests.toml` could not be used when it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// At least one tab.
    pub tabs: Vec<Tab>,
    /// Empty unless the file was there and unusable.
    pub problem: String,
}

/// Plans the tabs from the `--url` arguments and the text of `nests.toml`, `None` where there is
/// no such file. Configured nests come first, by name.
pub fn plan(urls: &[String], config: Option<&str>) -> Plan {
    let (nests, problem) = match config.map(parse_nests) {
        Some(Ok(nests)) => (nests, String::new()),
        Some(Err(error)) => (Default::default(), format!("nests.toml was not used: {error}")),
        None => (Default::default(), String::new()),
    };
    let mut tabs: Vec<Tab> = nests
        .into_iter()
        .map(|(name, target)| {
            let url = normalize_url(target.url.as_deref().unwrap_or(DEFAULT_URL));
            // No command is built from the file: the host is named, and the forward is the
            // reader's to open.
            let note = target.ssh.map_or_else(String::new, |host| {
                format!(
                    "nests.toml reaches this nest through the ssh host '{host}'. \
                     nuthatch-desk opens no forward of its own: open one to {url} first."
                )
            });
            Tab { name, url, note }
        })
        .collect();
    tabs.extend(urls.iter().map(|url| Tab {
        name: normalize_url(url),
        url: normalize_url(url),
        note: String::new(),
    }));
    if tabs.is_empty() {
        tabs.push(Tab {
            name: "local".into(),
            url: DEFAULT_URL.into(),
            note: String::new(),
        });
    }
    Plan { tabs, problem }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Invocation, String> {
        parse_args(list.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn the_command_line_is_read() {
        assert_eq!(args(&[]), Ok(Invocation::Run(Vec::new())));
        assert_eq!(
            args(&["--url", "http://a", "--url", "http://b"]),
            Ok(Invocation::Run(vec!["http://a".into(), "http://b".into()]))
        );
        assert_eq!(args(&["--version"]), Ok(Invocation::Version));
        assert_eq!(args(&["--url", "http://a", "-h"]), Ok(Invocation::Help));
        assert_eq!(args(&["--url"]), Err("--url needs a nest's URL".into()));
        assert_eq!(args(&["--nest", "x"]), Err("unknown argument '--nest'; try --help".into()));
    }

    #[test]
    fn with_nothing_to_go_on_one_tab_opens_on_the_default_listener() {
        let plan = plan(&[], None);
        assert_eq!(plan.tabs.len(), 1);
        assert_eq!(plan.tabs[0].name, "local");
        assert_eq!(plan.tabs[0].url, "http://127.0.0.1:8288");
        assert_eq!(plan.problem, "");
    }

    #[test]
    fn configured_nests_come_first_by_name_then_the_command_lines() {
        let config = "[zeta]\nurl = \"http://z:1/\"\n\n[alpha]\n";
        let plan = plan(&["http://x:2/".into()], Some(config));
        let seen: Vec<(&str, &str)> = plan
            .tabs
            .iter()
            .map(|tab| (tab.name.as_str(), tab.url.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                ("alpha", "http://127.0.0.1:8288"),
                ("zeta", "http://z:1"),
                ("http://x:2", "http://x:2"),
            ]
        );
    }

    #[test]
    fn a_nest_behind_ssh_says_so_and_builds_no_command() {
        let config = "[prod]\nurl = \"http://127.0.0.1:8288\"\nssh = \"root@nest; rm -rf ~\"\n";
        let plan = plan(&[], Some(config));
        assert!(plan.tabs[0].note.contains("the ssh host 'root@nest; rm -rf ~'"));
        assert!(!plan.tabs[0].note.contains("ssh -"));
    }

    #[test]
    fn a_broken_file_is_reported_and_the_client_still_opens() {
        let plan = plan(&[], Some("[local]\nurl = 5\n"));
        assert!(plan.problem.starts_with("nests.toml was not used: "), "{}", plan.problem);
        assert_eq!(plan.tabs.len(), 1);
        assert_eq!(plan.tabs[0].url, "http://127.0.0.1:8288");
    }
}
