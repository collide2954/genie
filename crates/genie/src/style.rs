use iced::widget::button;
use iced::{Border, Theme};

use crate::theme::{CLOSE, tokens};

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
