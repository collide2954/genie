use iced::widget::{Column, button, mouse_area, row, space, stack, text};
use iced::{Alignment, Element, Length, Point};

use crate::chrome::{self, Controls, keys, shift_keys};
use crate::genome;
use crate::menu;
use crate::recent::Entry;
use crate::shortcuts::Command;
use crate::style;

use super::hubs::Hubbing;
use super::{Genie, Message, Popup, Updating, additions};

const WIDTH: f32 = 200.0;
const RECENT_WIDTH: f32 = 280.0;
const MADE_WIDTH: f32 = 168.0;
const ROW: f32 = 24.0;
const ROWS: f32 = 12.0;
/// The place of the Recent row in the menu, from the top.
const RECENT_ROW: f32 = 3.0;
/// The place of the row of the tracks made from the reference, under one separator.
const MADE_ROW: f32 = 7.0;
const SEPARATORS: f32 = 2.0;
const SEPARATOR: f32 = 8.0;
const PADDING: f32 = 4.0;
const OVERLAP: f32 = 4.0;

fn hover(item: Element<'static, Message>, recent: bool) -> Element<'static, Message> {
    mouse_area(item)
        .on_enter(Message::RecentMenu(recent))
        .into()
}

/// The tracks that genie makes from the reference, for the list beside the genie menu.
fn made_items() -> Vec<Element<'static, Message>> {
    additions()
        .into_iter()
        .map(|(label, message)| menu::item(label, message, false, true))
        .collect()
}

/// A row of the genie menu that opens a list beside the menu.
fn opener(
    label: &'static str,
    open: bool,
    message: Option<Message>,
) -> button::Button<'static, Message> {
    button(
        row![
            text(label).size(12),
            space::horizontal(),
            text("›").size(12).style(style::muted)
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([3, 8])
    .style(if open { style::chosen } else { style::ghost })
    .on_press_maybe(message)
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
        if matches!(
            self.popup,
            Some(Popup::App | Popup::AppRecent | Popup::AppMade)
        ) {
            self.close_menus();
        } else {
            self.toggle(Popup::App);
        }
    }

    pub(super) fn show_recent(&mut self, open: bool) {
        if matches!(
            self.popup,
            Some(Popup::App | Popup::AppRecent | Popup::AppMade)
        ) {
            let recent = open && !self.recent.entries().is_empty();
            self.popup = Some(if recent { Popup::AppRecent } else { Popup::App });
        }
    }

    /// Shows the list of the tracks made from the reference beside the genie menu, or hides it.
    pub(super) fn show_made(&mut self, open: bool) {
        if matches!(
            self.popup,
            Some(Popup::App | Popup::AppRecent | Popup::AppMade)
        ) {
            let made = open && self.session.is_some();
            self.popup = Some(if made { Popup::AppMade } else { Popup::App });
        }
    }

    pub(super) fn app_menu(&self) -> Option<Element<'_, Message>> {
        if !matches!(
            self.popup,
            Some(Popup::App | Popup::AppRecent | Popup::AppMade)
        ) {
            return None;
        }
        let at = Point::new(chrome::brand_x(Controls::CURRENT), chrome::BAR_HEIGHT + 1.0);
        let height = ROWS * ROW + SEPARATORS * SEPARATOR + 2.0 * PADDING;
        let main = menu::sized(self.app_items(), at, self.window, (WIDTH, height));
        let (items, beside, extent) = match self.popup {
            Some(Popup::AppRecent) => {
                let count = self.recent.entries().len() as f32;
                let beside = Point::new(at.x + WIDTH - OVERLAP, at.y + PADDING + RECENT_ROW * ROW);
                (
                    self.recent_items(),
                    beside,
                    (RECENT_WIDTH, count * ROW + 2.0 * PADDING),
                )
            }
            Some(Popup::AppMade) => {
                let top = at.y + PADDING + MADE_ROW * ROW + SEPARATOR;
                let count = made_items().len() as f32;
                (
                    Column::new().spacing(1).extend(made_items()),
                    Point::new(at.x + WIDTH - OVERLAP, top),
                    (MADE_WIDTH, count * ROW + 2.0 * PADDING),
                )
            }
            _ => return Some(main),
        };
        let submenu = menu::sized(items, beside, self.window, extent);
        Some(stack![main, submenu].into())
    }

    fn app_items(&self) -> Column<'_, Message> {
        let session = self.session.is_some();
        let any = !self.recent.entries().is_empty();
        let recent = opener(
            "Recent",
            self.popup == Some(Popup::AppRecent),
            any.then_some(Message::RecentMenu(true)),
        );
        let made = opener(
            "Reference tracks",
            self.popup == Some(Popup::AppMade),
            session.then_some(Message::MadeMenu(true)),
        );
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
            .push(hover(
                menu::item("Open a track hub…", Message::Hub(Hubbing::Ask), false, true),
                false,
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
            .push(keyed(
                "Find a sequence…",
                shift_keys('F'),
                Message::Command(Command::Find),
                session,
            ))
            .push(mouse_area(made).on_enter(Message::MadeMenu(true)))
            .push(hover(
                menu::item(
                    "Show the whole genome",
                    Message::Command(Command::Genome),
                    false,
                    session,
                ),
                false,
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
            .push(hover(
                menu::item(
                    "Check for updates…",
                    Message::Update(Updating::Check),
                    false,
                    true,
                ),
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
