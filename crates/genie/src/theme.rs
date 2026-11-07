#![expect(clippy::unreadable_literal)]

use std::sync::LazyLock;

use iced::theme::{Mode, Palette};
use iced::{Color, Theme, color};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    pub fn is_dark(self, system: Mode) -> bool {
        match self {
            Self::System => system == Mode::Dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub canvas: Color,
    pub surface: Color,
    pub bar: Color,
    pub line: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub field: Color,
    pub danger: Color,
}

pub const LIGHT: Tokens = Tokens {
    canvas: color!(0xEEF2F6),
    surface: color!(0xFFFFFF),
    bar: color!(0xF8FAFC),
    line: color!(0xDCE3EA),
    text: color!(0x0F172A),
    muted: color!(0x5B6B7E),
    accent: color!(0x0F766E),
    accent_soft: color!(0x0F766E, 0.14),
    field: color!(0xF1F5F9),
    danger: color!(0xB42318),
};

pub const DARK: Tokens = Tokens {
    canvas: color!(0x0A111C),
    surface: color!(0x121B29),
    bar: color!(0x0F1723),
    line: color!(0x223044),
    text: color!(0xE2E8F0),
    muted: color!(0x8FA0B5),
    accent: color!(0x2DD4BF),
    accent_soft: color!(0x2DD4BF, 0.16),
    field: color!(0x0B131E),
    danger: color!(0xF97066),
};

pub fn light() -> Theme {
    static THEME: LazyLock<Theme> = LazyLock::new(|| build("genie light", &LIGHT));
    THEME.clone()
}

pub fn dark() -> Theme {
    static THEME: LazyLock<Theme> = LazyLock::new(|| build("genie dark", &DARK));
    THEME.clone()
}

pub fn tokens(theme: &Theme) -> &'static Tokens {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

fn build(name: &'static str, tokens: &Tokens) -> Theme {
    Theme::custom(
        name,
        Palette {
            background: tokens.canvas,
            text: tokens.text,
            primary: tokens.accent,
            success: tokens.accent,
            warning: color!(0xB54708),
            danger: tokens.danger,
        },
    )
}

#[cfg(test)]
mod tests {
    use iced::theme::Mode;

    use super::{Appearance, DARK, LIGHT, dark, light, tokens};

    #[test]
    fn themes_report_their_mode() {
        assert!(!light().extended_palette().is_dark);
        assert!(dark().extended_palette().is_dark);
    }

    #[test]
    fn tokens_follow_the_theme() {
        assert_eq!(tokens(&light()).canvas, LIGHT.canvas);
        assert_eq!(tokens(&dark()).canvas, DARK.canvas);
    }

    #[test]
    fn appearance_cycles_system_light_dark() {
        assert_eq!(Appearance::System.next(), Appearance::Light);
        assert_eq!(Appearance::Light.next(), Appearance::Dark);
        assert_eq!(Appearance::Dark.next(), Appearance::System);
    }

    #[test]
    fn only_system_appearance_follows_the_os() {
        assert!(Appearance::System.is_dark(Mode::Dark));
        assert!(!Appearance::System.is_dark(Mode::Light));
        assert!(!Appearance::Light.is_dark(Mode::Dark));
        assert!(Appearance::Dark.is_dark(Mode::Light));
    }
}
