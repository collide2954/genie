use iced::widget::{Column, button, mouse_area, row, space, stack, text};
use iced::{Alignment, Element, Length, Point};

use crate::chrome::{self, Controls, keys};
use crate::genome;
use crate::menu;
use crate::recent::Entry;
use crate::shortcuts::Command;
use crate::style;

use super::{Genie, Message, Popup};

const WIDTH: f32 = 200.0;
const RECENT_WIDTH: f32 = 280.0;
const ROW: f32 = 24.0;
const ROWS: f32 = 7.0;
const SEPARATORS: f32 = 2.0;
const SEPARATOR: f32 = 8.0;
const PADDING: f32 = 4.0;
const OVERLAP: f32 = 4.0;

fn hover(item: Element<'static, Message>, recent: bool) -> Element<'static, Message> {
    mouse_area(item)
        .on_enter(Message::RecentMenu(recent))
        .into()
}

fn keyed(
    label: &'static str,
    keys: String,
    message: Message,
    enabled: bool,
) -> Element<'static, Message> {
    hover(menu::keyed(label, keys, message, enabled), false)
}

impl Genie {
    pub(super) fn toggle_app_menu(&mut self) {
        if matches!(self.popup, Some(Popup::App | Popup::AppRecent)) {
            self.close_menus();
        } else {
            self.toggle(Popup::App);
        }
    }

    pub(super) fn show_recent(&mut self, open: bool) {
        if matches!(self.popup, Some(Popup::App | Popup::AppRecent)) {
            let recent = open && !self.recent.entries().is_empty();
            self.popup = Some(if recent { Popup::AppRecent } else { Popup::App });
        }
    }

    pub(super) fn app_menu(&self) -> Option<Element<'_, Message>> {
        if !matches!(self.popup, Some(Popup::App | Popup::AppRecent)) {
            return None;
        }
        let at = Point::new(chrome::brand_x(Controls::CURRENT), chrome::BAR_HEIGHT + 1.0);
        let height = ROWS * ROW + SEPARATORS * SEPARATOR + 2.0 * PADDING;
        let main = menu::sized(self.app_items(), at, self.window, (WIDTH, height));
        if self.popup != Some(Popup::AppRecent) {
            return Some(main);
        }
        let count = self.recent.entries().len() as f32;
        let beside = Point::new(at.x + WIDTH - OVERLAP, at.y + PADDING + 2.0 * ROW);
        let extent = (RECENT_WIDTH, count * ROW + 2.0 * PADDING);
        let submenu = menu::sized(self.recent_items(), beside, self.window, extent);
        Some(stack![main, submenu].into())
    }

    fn app_items(&self) -> Column<'_, Message> {
        let session = self.session.is_some();
        let any = !self.recent.entries().is_empty();
        let open = self.popup == Some(Popup::AppRecent);
        let recent = button(
            row![
                text("Recent").size(12),
                space::horizontal(),
                text("›").size(12).style(style::muted)
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([3, 8])
        .style(if open { style::chosen } else { style::ghost })
        .on_press_maybe(any.then_some(Message::RecentMenu(true)));
        Column::new()
            .spacing(1)
            .push(keyed(
                "Open file…",
                keys('O'),
                Message::Command(Command::Open),
                true,
            ))
            .push(keyed(
                "Open URL…",
                keys('U'),
                Message::Command(Command::OpenUrl),
                true,
            ))
            .push(hover(recent.into(), true))
            .push(menu::separator())
            .push(keyed(
                "Save session",
                keys('S'),
                Message::Command(Command::SaveSession),
                session,
            ))
            .push(keyed(
                "Export image…",
                keys('E'),
                Message::Command(Command::Export),
                session,
            ))
            .push(menu::separator())
            .push(keyed(
                "Keys and mouse…",
                "F1".to_owned(),
                Message::Command(Command::Help),
                true,
            ))
            .push(hover(
                menu::item("Settings", Message::ToggleSettings, false, true),
                false,
            ))
    }

    fn recent_items(&self) -> Column<'_, Message> {
        let items = self.recent.entries().iter().filter_map(|entry| {
            let (name, place, message) = match entry {
                Entry::Location(location) => (
                    location.name(),
                    location.place(),
                    Message::Reopen(location.clone()),
                ),
                Entry::Genome(id) => {
                    let genome = genome::find(id)?;
                    (
                        genome.label,
                        format!("{} {}", genome.species, genome.assembly),
                        Message::Genome(genome.id),
                    )
                }
            };
            let line = row![
                text(name).size(12).wrapping(text::Wrapping::None),
                text(place)
                    .size(11)
                    .style(style::muted)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            Some(
                button(line)
                    .width(Length::Fill)
                    .padding([3, 8])
                    .clip(true)
                    .style(style::ghost)
                    .on_press(message)
                    .into(),
            )
        });
        Column::new().spacing(1).extend(items)
    }
}
