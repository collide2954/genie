use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use genie_formats::ini::{self, Section};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::app::Message;
use crate::chrome::BAR_HEIGHT;
use crate::theme::Appearance;
use crate::{font, style};

pub const FILE: &str = "settings.ini";
const SECTION: &str = "settings";
pub const SCALES: [u16; 5] = [90, 100, 110, 125, 150];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    #[default]
    Compact,
    Comfortable,
}

impl Density {
    pub fn padding(self) -> f32 {
        match self {
            Self::Compact => 2.0,
            Self::Comfortable => 6.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Scale(i8),
    Density(Density),
    Motion(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub scale: u16,
    pub density: Density,
    pub motion: bool,
    pub appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 100,
            density: Density::Compact,
            motion: true,
            appearance: Appearance::System,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let defaults = Self::default();
        let Some(section) = ini::parse(text)
            .ok()
            .and_then(|document| document.section(SECTION).cloned())
        else {
            return defaults;
        };
        Self {
            scale: section
                .value("scale")
                .and_then(|value| value.parse().ok())
                .filter(|scale| SCALES.contains(scale))
                .unwrap_or(defaults.scale),
            density: match section.value("density") {
                Some("comfortable") => Density::Comfortable,
                _ => Density::Compact,
            },
            motion: section.value("motion") != Some("off"),
            appearance: match section.value("theme") {
                Some("light") => Appearance::Light,
                Some("dark") => Appearance::Dark,
                _ => Appearance::System,
            },
        }
    }

    pub fn section(self) -> Section {
        let density = match self.density {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
        };
        let theme = match self.appearance {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        };
        let motion = if self.motion { "on" } else { "off" };
        Section {
            name: SECTION.to_owned(),
            entries: [
                ("scale", self.scale.to_string()),
                ("density", density.to_owned()),
                ("motion", motion.to_owned()),
                ("theme", theme.to_owned()),
            ]
            .map(|(key, value)| (key.to_owned(), value))
            .to_vec(),
        }
    }

    #[must_use]
    pub fn scale_by(self, steps: i8) -> Self {
        let at = SCALES
            .iter()
            .position(|scale| *scale == self.scale)
            .unwrap_or(1);
        let at = at.saturating_add_signed(isize::from(steps));
        Self {
            scale: SCALES[at.min(SCALES.len() - 1)],
            ..self
        }
    }

    #[must_use]
    pub fn apply(self, setting: Setting) -> Self {
        match setting {
            Setting::Scale(steps) => self.scale_by(steps),
            Setting::Density(density) => Self { density, ..self },
            Setting::Motion(motion) => Self { motion, ..self },
        }
    }

    pub fn load(folder: &Path) -> Self {
        fs::read_to_string(folder.join(FILE))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(self, folder: &Path) -> io::Result<()> {
        store(folder, self.section())
    }
}

pub fn panel<'a>(settings: Settings, inset: f32) -> Element<'a, Message> {
    let choice = |label: &'a str, setting: Setting, chosen: bool| -> Element<'a, Message> {
        button(text(label).size(12))
            .padding([3, 8])
            .style(if chosen { style::chosen } else { style::ghost })
            .on_press(Message::SettingChanged(setting))
            .into()
    };
    let step = |label: &'a str, steps: i8| {
        button(text(label).size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press(Message::SettingChanged(Setting::Scale(steps)))
    };
    let line = |label: &'a str, control: Element<'a, Message>| {
        row![text(label).size(12).width(72).style(style::muted), control]
            .spacing(8)
            .align_y(Alignment::Center)
    };
    let scale = row![
        step("−", -1),
        text(format!("{} %", settings.scale))
            .size(12)
            .font(font::MONO)
            .width(48)
            .align_x(Alignment::Center),
        step("+", 1),
    ]
    .align_y(Alignment::Center);
    let density = row![
        choice(
            "Compact",
            Setting::Density(Density::Compact),
            settings.density == Density::Compact
        ),
        choice(
            "Comfortable",
            Setting::Density(Density::Comfortable),
            settings.density == Density::Comfortable
        ),
    ]
    .spacing(2);
    let motion = row![
        choice("On", Setting::Motion(true), settings.motion),
        choice("Off", Setting::Motion(false), !settings.motion),
    ]
    .spacing(2);
    let content = column![
        text("Settings").font(font::UI_STRONG).size(12),
        line("UI scale", scale.into()),
        line("Density", density.into()),
        line("Motion", motion.into()),
    ]
    .spacing(8);
    let card = container(content)
        .id("settings-panel")
        .padding(10)
        .style(style::menu);
    container(card)
        .width(Length::Fill)
        .align_right(Length::Fill)
        .padding(Padding {
            top: BAR_HEIGHT + 4.0,
            right: inset,
            ..Padding::ZERO
        })
        .into()
}

pub fn store(folder: &Path, section: Section) -> io::Result<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    fs::create_dir_all(folder)?;
    let path = folder.join(FILE);
    let mut document = fs::read_to_string(&path)
        .ok()
        .and_then(|text| ini::parse(&text).ok())
        .unwrap_or_default();
    document.sections.retain(|known| known.name != section.name);
    document.sections.push(section);
    crate::paths::replace(&path, &ini::write(&document))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use genie_formats::ini::{self, Document};

    use super::{Density, Settings};
    use crate::recent::Recent;
    use crate::scratch::Scratch;
    use crate::theme::Appearance;

    fn text(settings: Settings) -> String {
        ini::write(&Document {
            sections: vec![settings.section()],
        })
    }

    #[test]
    fn settings_read_back_what_they_write() {
        let settings = Settings {
            scale: 125,
            density: Density::Comfortable,
            motion: false,
            appearance: Appearance::Dark,
        };
        assert_eq!(Settings::parse(&text(settings)), settings);
    }

    #[test]
    fn missing_or_bad_values_fall_back() {
        assert_eq!(Settings::parse("not ini"), Settings::default());
        let settings = Settings::parse("[settings]\nscale = 300\ntheme = pink\n");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn scale_steps_stop_at_the_ends() {
        let settings = Settings::default();
        assert_eq!(settings.scale_by(1).scale, 110);
        assert_eq!(settings.scale_by(-1).scale, 90);
        assert_eq!(settings.scale_by(-1).scale_by(-1).scale, 90);
        assert_eq!(settings.scale_by(9).scale, 150);
    }

    #[test]
    fn settings_and_recent_files_share_the_file() {
        let scratch = Scratch::new("settings");
        let folder = scratch.file("unused", "").with_file_name("config");
        let mut recent = Recent::default();
        recent.add(PathBuf::from("/data/a.fa").into());
        recent.save(&folder).unwrap();
        let settings = Settings {
            motion: false,
            ..Settings::default()
        };
        settings.save(&folder).unwrap();
        recent.add(PathBuf::from("/data/b.fa").into());
        recent.save(&folder).unwrap();
        assert_eq!(Settings::load(&folder), settings);
        assert_eq!(Recent::load(&folder), recent);
    }
}
