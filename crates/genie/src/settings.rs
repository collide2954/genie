use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use genie_formats::ini::{self, Section};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{Caching, Message};
use crate::chrome::BAR_HEIGHT;
use crate::theme::Appearance;
use crate::{font, style};

pub const FILE: &str = "settings.ini";
const SECTION: &str = "settings";
pub const SCALES: [u16; 5] = [90, 100, 110, 125, 150];
const VERSION: &str = concat!("genie ", env!("CARGO_PKG_VERSION"), " · Apache-2.0");
const KILO: u64 = 1 << 10;
const MEGA: u64 = 1 << 20;
const GIGA: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    pub size: Option<u64>,
    pub can_clear: bool,
}

pub fn size_text(bytes: u64) -> String {
    let decimal = |unit: u64, name: &str| {
        let value = bytes as f64 / unit as f64;
        if value < 10.0 {
            format!("{value:.1} {name}")
        } else {
            format!("{value:.0} {name}")
        }
    };
    match bytes {
        _ if bytes >= GIGA => decimal(GIGA, "GB"),
        _ if bytes >= MEGA => decimal(MEGA, "MB"),
        _ => format!("{} KB", bytes.div_ceil(KILO)),
    }
}

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
    Port(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub scale: u16,
    pub density: Density,
    pub motion: bool,
    pub appearance: Appearance,
    pub port: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 100,
            density: Density::Compact,
            motion: true,
            appearance: Appearance::System,
            port: true,
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
            port: section.value("port") != Some("off"),
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
        let switch = |on: bool| if on { "on" } else { "off" }.to_owned();
        Section {
            name: SECTION.to_owned(),
            entries: [
                ("scale", self.scale.to_string()),
                ("density", density.to_owned()),
                ("motion", switch(self.motion)),
                ("theme", theme.to_owned()),
                ("port", switch(self.port)),
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
            Setting::Port(port) => Self { port, ..self },
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

pub fn panel<'a>(settings: Settings, inset: f32, store: Store) -> Element<'a, Message> {
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
    let switch = |make: fn(bool) -> Setting, on: bool| {
        row![
            choice("On", make(true), on),
            choice("Off", make(false), !on)
        ]
        .spacing(2)
    };
    let cache = row![
        text(store.size.map_or_else(|| "…".to_owned(), size_text))
            .size(12)
            .font(font::MONO)
            .width(56),
        button(text("Clear").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe(store.can_clear.then_some(Message::Cache(Caching::Clear))),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    let content = column![
        text("Settings").font(font::UI_STRONG).size(12),
        line("UI scale", scale.into()),
        line("Density", density.into()),
        line("Motion", switch(Setting::Motion, settings.motion).into()),
        line("Commands", switch(Setting::Port, settings.port).into()),
        line("Cache", cache.into()),
        text(VERSION).size(11).style(style::muted),
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

    use super::{Density, Settings, size_text};
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
            port: false,
        };
        assert_eq!(Settings::parse(&text(settings)), settings);
        assert!(Settings::default().port);
    }

    #[test]
    fn missing_or_bad_values_fall_back() {
        assert_eq!(Settings::parse("not ini"), Settings::default());
        let settings = Settings::parse("[settings]\nscale = 300\ntheme = pink\n");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn sizes_read_in_the_largest_whole_unit() {
        assert_eq!(size_text(0), "0 KB");
        assert_eq!(size_text(1_500), "2 KB");
        assert_eq!(size_text(5 << 20), "5.0 MB");
        assert_eq!(size_text(250 << 20), "250 MB");
        assert_eq!(size_text((3 << 30) / 2), "1.5 GB");
        assert_eq!(size_text(12 << 30), "12 GB");
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
