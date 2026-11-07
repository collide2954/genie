use iced::widget::scrollable::{self, Rail, Scroller};
use iced::widget::{button, container, text, text_input};
use iced::{Border, Color, Shadow, Theme, Vector};

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

pub fn chosen(theme: &Theme, _status: button::Status) -> button::Style {
    let tokens = tokens(theme);
    button::Style {
        background: Some(tokens.line.into()),
        text_color: tokens.text,
        border: Border::default().rounded(4),
        ..button::Style::default()
    }
}

pub fn menu(theme: &Theme) -> container::Style {
    let tokens = tokens(theme);
    container::Style {
        background: Some(tokens.surface.into()),
        text_color: Some(tokens.text),
        border: Border::default().rounded(6).width(1).color(tokens.line),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.18),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

pub fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let tokens = tokens(theme);
    let active = matches!(
        status,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        } | scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged: true,
            ..
        }
    );
    let rail = Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller {
            background: if active { tokens.muted } else { tokens.line }.into(),
            border: Border::default().rounded(3),
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        ..scrollable::default(theme, status)
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
