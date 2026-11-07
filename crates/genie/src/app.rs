use iced::theme::Mode;
use iced::widget::{button, center, column, container, row, space, text};
use iced::{Element, Task, Theme, system};

use crate::font;
use crate::theme::{self, Appearance, tokens};

#[derive(Debug, Default)]
pub struct Genie {
    appearance: Appearance,
    system: Mode,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    CycleAppearance,
    SystemModeChanged(Mode),
}

impl Genie {
    fn boot() -> (Self, Task<Message>) {
        (
            Self::default(),
            system::theme().map(Message::SystemModeChanged),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CycleAppearance => self.appearance = self.appearance.next(),
            Message::SystemModeChanged(mode) => self.system = mode,
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let bar = container(
            row![
                text("genie"),
                space::horizontal(),
                button(text(self.appearance.label())).on_press(Message::CycleAppearance),
            ]
            .padding(8),
        )
        .style(|theme| container::background(tokens(theme).bar));
        let welcome = text("Open a reference sequence to start.").style(|theme| text::Style {
            color: Some(tokens(theme).muted),
        });
        column![bar, center(welcome)].into()
    }

    fn theme(&self) -> Theme {
        if self.appearance.is_dark(self.system) {
            theme::dark()
        } else {
            theme::light()
        }
    }
}

pub fn run() -> iced::Result {
    font::FILES
        .into_iter()
        .fold(
            iced::application(Genie::boot, Genie::update, Genie::view),
            |application, (bytes, _)| application.font(bytes),
        )
        .default_font(font::UI)
        .title("genie")
        .theme(Genie::theme)
        .subscription(|_| system::theme_changes().map(Message::SystemModeChanged))
        .run()
}

#[cfg(test)]
mod tests {
    use iced::theme::Mode;
    use iced_test::simulator;

    use super::{Genie, Message};
    use crate::theme::Appearance;

    #[test]
    fn welcome_view_asks_for_a_reference() {
        let genie = Genie::default();
        let mut ui = simulator(genie.view());
        assert!(ui.find("Open a reference sequence to start.").is_ok());
    }

    #[test]
    fn appearance_button_cycles_appearance() {
        let mut genie = Genie::default();
        let mut ui = simulator(genie.view());
        ui.click("System").unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        assert_eq!(genie.appearance, Appearance::Light);
    }

    #[test]
    fn system_mode_changes_the_theme() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::SystemModeChanged(Mode::Dark));
        assert!(genie.theme().extended_palette().is_dark);
    }
}
