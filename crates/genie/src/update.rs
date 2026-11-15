//! The check for a new release of genie on GitHub.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use genie_formats::http::{self, Url};
use genie_formats::ini::{self, Section};

use crate::settings::{self, FILE};

/// The newest release of genie, as the GitHub API gives it.
pub const LATEST: &str = "https://api.github.com/repos/collide2954/genie/releases/latest";
const PAGE: &str = "https://github.com/collide2954/genie/releases/tag/";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const DAY: u64 = 24 * 60 * 60;
const SECTION: &str = "updates";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The release page on GitHub.
    pub page: String,
}

/// What genie knows about a newer release.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Unknown,
    Checking,
    Current,
    Available(Release),
    Failed,
}

/// Whether genie checks for updates at start, and when it last checked, in seconds since 1970.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checks {
    pub automatic: bool,
    pub checked: u64,
}

impl Default for Checks {
    fn default() -> Self {
        Self {
            automatic: true,
            checked: 0,
        }
    }
}

impl Checks {
    /// True when genie checks at start and its last check is a day old or more.
    pub fn due(self, now: u64) -> bool {
        self.automatic && now.saturating_sub(self.checked) >= DAY
    }

    pub fn parse(text: &str) -> Self {
        let document = ini::parse(text).ok();
        let Some(section) = document
            .as_ref()
            .and_then(|document| document.section(SECTION))
        else {
            return Self::default();
        };
        Self {
            automatic: section.value("check") != Some("off"),
            checked: section
                .value("checked")
                .and_then(|text| text.parse().ok())
                .unwrap_or(0),
        }
    }

    fn section(self) -> Section {
        let check = if self.automatic { "on" } else { "off" };
        Section {
            name: SECTION.to_owned(),
            entries: vec![
                ("check".to_owned(), check.to_owned()),
                ("checked".to_owned(), self.checked.to_string()),
            ],
        }
    }

    pub fn load(folder: &Path) -> Self {
        fs::read_to_string(folder.join(FILE))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(self, folder: &Path) -> io::Result<()> {
        settings::store(folder, self.section())
    }
}

/// The time now in seconds since 1970.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Reads the newest release from the GitHub API.
pub fn latest(url: &Url) -> Result<Release, String> {
    let bytes = http::get(url).map_err(|error| error.to_string())?;
    release(&String::from_utf8_lossy(&bytes))
        .ok_or_else(|| "The answer from GitHub has no release.".to_owned())
}

/// The release in a GitHub API answer, if its tag is a version such as v0.3.0.
fn release(json: &str) -> Option<Release> {
    let tag = text_field(json, "tag_name")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let plain = version
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || ".-+".contains(letter));
    let numbered = version.starts_with(|letter: char| letter.is_ascii_digit());
    (plain && numbered).then(|| Release {
        version: version.to_owned(),
        page: format!("{PAGE}{tag}"),
    })
}

/// The text of the first field with this name in a JSON object, when it has no escapes.
fn text_field<'a>(json: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\"");
    let rest = &json[json.find(&key)? + key.len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let value = &rest[..rest.find('"')?];
    (!value.contains('\\')).then_some(value)
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// True when the found version is newer than the current one.
pub fn newer(found: &str, current: &str) -> bool {
    let (mut found, mut current) = (numbers(found), numbers(current));
    let length = found.len().max(current.len());
    found.resize(length, 0);
    current.resize(length, 0);
    found > current
}

/// Opens a page in the browser of the system.
pub fn browse(page: &str) -> io::Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command.arg(page).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use genie_formats::http::Url;

    use super::{Checks, DAY, Release, latest, newer, release};

    const ANSWER: &str = r#"{"url":"https://api.github.com/repos/collide2954/genie/releases/1",
        "html_url":"https://github.com/collide2954/genie/releases/tag/v0.3.0",
        "author":{"login":"someone","html_url":"https://github.com/someone"},
        "tag_name" : "v0.3.0","name":"genie 0.3.0","draft":false,"prerelease":false}"#;

    #[test]
    fn answers_give_the_version_and_the_release_page() {
        assert_eq!(
            release(ANSWER),
            Some(Release {
                version: "0.3.0".to_owned(),
                page: "https://github.com/collide2954/genie/releases/tag/v0.3.0".to_owned(),
            })
        );
        assert_eq!(release(r#"{"tag_name":"v0.3.0<script>"}"#), None);
        assert_eq!(release(r#"{"tag_name":"latest"}"#), None);
        assert_eq!(release(r#"{"name":"genie"}"#), None);
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("0.3.0", "0.2.0"));
        assert!(newer("0.10.0", "0.9.1"));
        assert!(newer("0.2.1", "0.2.0"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
        assert!(!newer("0.2.0-rc1", "0.2.0"));
    }

    #[test]
    fn checks_are_due_once_a_day_when_they_are_on() {
        let checks = Checks {
            automatic: true,
            checked: 1_000,
        };
        assert!(!checks.due(1_000 + DAY - 1));
        assert!(checks.due(1_000 + DAY));
        assert!(Checks::default().due(1_700_000_000));
        let off = Checks {
            automatic: false,
            ..checks
        };
        assert!(!off.due(1_000 + 2 * DAY));
        let text = genie_formats::ini::write(&genie_formats::ini::Document {
            sections: vec![off.section()],
        });
        assert!(text.contains("check = off"), "{text}");
        assert_eq!(Checks::parse(&text), off);
        assert_eq!(Checks::parse("not ini"), Checks::default());
    }

    #[test]
    fn the_newest_release_comes_from_the_api() {
        let server = crate::server::serve(vec![(
            "/repos/x/releases/latest".to_owned(),
            ANSWER.as_bytes().to_vec(),
        )]);
        let url = Url::parse(&server.url("/repos/x/releases/latest")).unwrap();
        assert_eq!(latest(&url).unwrap().version, "0.3.0");
        let missing = Url::parse(&server.url("/repos/x/releases/none")).unwrap();
        assert!(latest(&missing).is_err());
    }
}
