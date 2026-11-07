use iced::widget::button;
use iced::{Border, Theme};

use crate::theme::tokens;

pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    let active = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: active.then(|| tokens.line.into()),
        text_color: tokens.text,
        border: Border::default().rounded(6),
        ..button::Style::default()
    }
}
