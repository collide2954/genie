use genie_formats::http::Url;
use iced::Task;

use crate::paths;
use crate::update::{self, CURRENT, Release, State};

use super::{Genie, Message};

#[derive(Debug, Clone, PartialEq)]
pub enum Updating {
    /// Check now, from the menu or the palette.
    Check,
    /// The answer of a check, and whether the user asked for it.
    Checked(bool, Result<Release, String>),
    /// Check at start or not.
    Automatic(bool),
    /// Open the page of the newer release.
    Open,
}

impl Genie {
    pub(super) fn updating(&mut self, updating: Updating) -> Task<Message> {
        match updating {
            Updating::Check => {
                self.close_menus();
                self.check_updates(true)
            }
            Updating::Checked(asked, result) => self.updates_checked(asked, result),
            Updating::Automatic(automatic) => {
                self.checks.automatic = automatic;
                self.remember_checks()
            }
            Updating::Open => match &self.release {
                State::Available(release) => {
                    let page = release.page.clone();
                    Task::future(async move {
                        if let Err(error) = update::browse(&page) {
                            log::warn!("genie cannot open {page}. {error}");
                        }
                    })
                    .discard()
                }
                _ => Task::none(),
            },
        }
    }

    /// Asks GitHub for the newest release.
    pub(super) fn check_updates(&mut self, asked: bool) -> Task<Message> {
        let Ok(url) = Url::parse(update::LATEST) else {
            return Task::none();
        };
        self.release = State::Checking;
        Task::perform(async move { update::latest(&url) }, move |result| {
            Message::Update(Updating::Checked(asked, result))
        })
    }

    fn updates_checked(&mut self, asked: bool, result: Result<Release, String>) -> Task<Message> {
        match result {
            Ok(release) => {
                self.checks.checked = update::now();
                let remember = self.remember_checks();
                if update::newer(&release.version, CURRENT) {
                    let text = format!("genie {} is available.", release.version);
                    self.release = State::Available(release);
                    let download = ("Download".to_owned(), Message::Update(Updating::Open));
                    self.show_with(text, false, Some(download));
                    return remember;
                }
                self.release = State::Current;
                if asked {
                    let note = self.inform(format!("genie {CURRENT} is the newest version."));
                    return Task::batch([remember, note]);
                }
                remember
            }
            Err(error) => {
                log::warn!("genie cannot check for updates. {error}");
                self.release = State::Failed;
                if asked {
                    self.warn(format!("genie cannot check for updates. {error}"));
                }
                Task::none()
            }
        }
    }

    fn remember_checks(&self) -> Task<Message> {
        let Some(folder) = paths::config_dir() else {
            return Task::none();
        };
        let checks = self.checks;
        Task::future(async move {
            let _ = checks.save(&folder);
        })
        .discard()
    }

    /// The version line of the settings panel: the version and what genie knows about updates.
    pub(super) fn version_line(&self) -> String {
        match &self.release {
            State::Unknown => format!("genie {CURRENT}"),
            State::Checking => format!("genie {CURRENT} · Checking for updates…"),
            State::Current => format!("genie {CURRENT} · Up to date"),
            State::Available(release) => {
                format!("genie {CURRENT} · {} is available", release.version)
            }
            State::Failed => format!("genie {CURRENT} · The check for updates failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use iced_test::simulator;

    use super::Updating;
    use crate::app::{Genie, Message};
    use crate::update::{CURRENT, Release, State};

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    fn release(version: &str) -> Release {
        Release {
            version: version.to_owned(),
            page: format!("https://github.com/collide2954/genie/releases/tag/v{version}"),
        }
    }

    #[test]
    fn newer_releases_show_a_notice_with_a_download_button() {
        let mut genie = Genie::default();
        let checked = Updating::Checked(false, Ok(release("99.0.0")));
        let _ = genie.update(Message::Update(checked));
        assert!(shows(&genie, "genie 99.0.0 is available."));
        assert_eq!(genie.release, State::Available(release("99.0.0")));
        let mut ui = simulator(genie.view());
        ui.click("Download").unwrap();
        let open = Message::Update(Updating::Open);
        assert!(ui.into_messages().any(|message| message == open));
        assert!(genie.checks.checked > 0);
    }

    #[test]
    fn asked_checks_say_when_genie_is_current_or_cannot_check() {
        let mut genie = Genie::default();
        let same = Updating::Checked(true, Ok(release(CURRENT)));
        let _ = genie.update(Message::Update(same));
        assert!(shows(
            &genie,
            &format!("genie {CURRENT} is the newest version.")
        ));
        assert_eq!(
            genie.version_line(),
            format!("genie {CURRENT} · Up to date")
        );
        let quiet = Updating::Checked(false, Err("The server answered 503.".to_owned()));
        let mut other = Genie::default();
        let _ = other.update(Message::Update(quiet));
        assert!(!shows(
            &other,
            "genie cannot check for updates. The server answered 503."
        ));
        let failed = Updating::Checked(true, Err("The server answered 503.".to_owned()));
        let _ = other.update(Message::Update(failed));
        assert!(shows(
            &other,
            "genie cannot check for updates. The server answered 503."
        ));
        let _ = other.update(Message::Update(Updating::Automatic(false)));
        assert!(!other.checks.automatic);
    }

    #[test]
    fn the_settings_panel_shows_the_version_and_the_update_switch() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::ToggleSettings);
        assert!(shows(&genie, &format!("genie {CURRENT}")));
        let mut ui = simulator(genie.view());
        ui.click("Check now").unwrap();
        let check = Message::Update(Updating::Check);
        assert!(ui.into_messages().any(|message| message == check));
        let checked = Updating::Checked(false, Ok(release("99.0.0")));
        let _ = genie.update(Message::Update(checked));
        let line = format!("genie {CURRENT} · 99.0.0 is available");
        assert!(shows(&genie, &line), "{line}");
    }
}
