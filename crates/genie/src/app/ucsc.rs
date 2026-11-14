use std::sync::Arc;

use genie_formats::http::Url;
use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Alignment, Element, Length, Point, Task};

use crate::catalog::{self, Entry, Group};
use crate::chrome::{self, Controls};
use crate::location::Location;
use crate::track::{self, Source};
use crate::{menu, style};

use super::hubs::Hubbing;
use super::{Genie, Message, Popup};

const WIDTH: f32 = 340.0;
const HEADING: f32 = 23.0;
const CHOICE: f32 = 40.0;
const ITEM: f32 = 22.0;
const SEPARATOR: f32 = 7.0;
const NOTE: &str = "genie reads these tracks in parts from the UCSC Genome Browser.";

impl Genie {
    fn catalog(&self) -> &'static [Entry] {
        self.session
            .as_ref()
            .and_then(|session| session.reference.genome.as_deref())
            .map_or(&[], catalog::entries)
    }

    pub(super) fn catalog_menu(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Catalog) {
            return None;
        }
        let session = self.session.as_ref()?;
        let entries = self.catalog();
        let mut content = Column::new();
        let mut height = 8.0;
        for group in Group::ALL {
            let members: Vec<(usize, &Entry)> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.group == group)
                .collect();
            if members.is_empty() {
                continue;
            }
            content = content.push(heading(group.label()));
            height += HEADING;
            for (index, entry) in members {
                let url = entry.url();
                let added = session
                    .tracks
                    .iter()
                    .any(|track| track.source.location.to_string() == url);
                content = content.push(choice(index, entry, added));
                height += CHOICE;
            }
        }
        if !entries.is_empty() {
            content = content
                .push(container(text(NOTE).size(11).style(style::muted)).padding([2, 8]))
                .push(menu::separator());
            height += HEADING + SEPARATOR;
        }
        content = content.push(heading("Track hubs"));
        height += HEADING;
        for (index, hub) in self.hubs.iter().enumerate() {
            let show = Message::Hub(Hubbing::Show(index));
            content = content.push(hub_choice(&hub.short, &hub.location, show));
            height += CHOICE;
        }
        let recent = self
            .known_hubs
            .entries()
            .iter()
            .filter(|(location, _)| self.hubs.iter().all(|hub| hub.location != *location));
        for (location, name) in recent {
            let open = Message::Hub(Hubbing::Open(location.clone()));
            content = content.push(hub_choice(name, location, open));
            height += CHOICE;
        }
        content = content.push(menu::item(
            "Open a track hub…",
            Message::Hub(Hubbing::Ask),
            false,
            true,
        ));
        height += ITEM;
        let at = Point::new(
            chrome::catalog_x(Controls::CURRENT),
            chrome::BAR_HEIGHT + 1.0,
        );
        Some(menu::sized(content, at, self.window, (WIDTH, height)))
    }

    pub(super) fn open_catalog(&mut self, index: usize) -> Task<Message> {
        self.close_menus();
        let Some(entry) = self.catalog().get(index) else {
            return Task::none();
        };
        let url = match Url::parse(&entry.url()) {
            Ok(url) => url,
            Err(error) => {
                self.fail(error.to_string());
                return Task::none();
            }
        };
        self.busy += 1;
        Task::perform(
            async move { track::open(Location::Url(url)).map(Arc::new) },
            move |result| Message::CatalogOpened(index, result),
        )
    }

    pub(super) fn catalog_opened(
        &mut self,
        index: usize,
        result: Result<Arc<Source>, String>,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let source = match result {
            Ok(source) => source,
            Err(error) => {
                self.fail(error);
                return Task::none();
            }
        };
        let name = self.catalog().get(index).map(|entry| entry.name);
        let task = self.add_track(Arc::clone(&source));
        if let (Some(name), Some(session)) = (name, &mut self.session)
            && let Some(track) = session
                .tracks
                .iter_mut()
                .find(|track| track.source == source)
        {
            name.clone_into(&mut track.name);
        }
        task
    }
}

fn heading(label: &str) -> Element<'_, Message> {
    container(text(label).size(11).style(style::muted))
        .padding([4, 8])
        .into()
}

fn hub_choice<'a>(name: &str, location: &Location, message: Message) -> Element<'a, Message> {
    let name = if name.is_empty() {
        location.name()
    } else {
        name.to_owned()
    };
    let lines = column![
        text(name).size(12).wrapping(text::Wrapping::None),
        text(location.place())
            .size(11)
            .style(style::muted)
            .wrapping(text::Wrapping::None)
    ]
    .spacing(1);
    button(lines)
        .width(Length::Fill)
        .padding([3, 8])
        .clip(true)
        .style(style::ghost)
        .on_press(message)
        .into()
}

fn choice<'a>(index: usize, entry: &Entry, added: bool) -> Element<'a, Message> {
    let mut title = row![text(entry.name).size(12)].align_y(Alignment::Center);
    if added {
        title = title
            .push(space::horizontal())
            .push(text("Added").size(11).style(style::muted));
    }
    button(column![title, text(entry.about).size(11).style(style::muted)].spacing(1))
        .width(Length::Fill)
        .padding([3, 8])
        .style(style::ghost)
        .on_press_maybe((!added).then_some(Message::Catalog(index)))
        .into()
}
