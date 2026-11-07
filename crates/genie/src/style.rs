use iced::widget::{button, container};
use iced::{Border, Theme};

use crate::theme::{CLOSE, tokens};

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

pub fn field(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.field.into()),
        text_color: Some(tokens.text),
        border: Border::default().rounded(6).width(1).color(tokens.line),
        ..container::Style::default()
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

pub fn window_close(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: is_active(status).then(|| CLOSE.into()),
        text_color: tokens(theme).text,
        ..button::Style::default()
    }
}

fn is_active(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
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
