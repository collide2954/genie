use iced::widget::{button, text, text_input};
use iced::{Border, Theme};

use crate::theme::tokens;

pub fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).muted),
    }
}

pub fn locus(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let tokens = tokens(theme);
    let border = match status {
        text_input::Status::Focused { .. } => tokens.muted,
        _ => tokens.line,
    };
    text_input::Style {
        background: tokens.field.into(),
        border: Border::default().rounded(6).width(1).color(border),
        icon: tokens.muted,
        placeholder: tokens.muted,
        value: tokens.text,
        selection: tokens.accent_soft,
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let background = if is_active(status) {
        tokens.accent.scale_alpha(0.88)
    } else {
        tokens.accent
    };
    button::Style {
        background: Some(background.into()),
        text_color: tokens.on_accent,
        border: Border::default().rounded(6),
        ..button::Style::default()
    }
}

pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    button::Style {
        background: is_active(status).then(|| tokens.line.into()),
        text_color: tokens.text,
        border: Border::default().rounded(6),
        ..button::Style::default()
    }
}

pub fn window_control(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    button::Style {
        background: is_active(status).then(|| tokens.line.into()),
        text_color: tokens.text,
        ..button::Style::default()
    }
}

fn is_active(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}
