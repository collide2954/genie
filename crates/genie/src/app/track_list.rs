//! The tracks panel at the left of the tracks: a row for each track with an eye that hides or
//! shows the track, a field that finds tracks by name, and the track menu on a right-click.

use genie_render::scene::Hue;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, column, container, operation, row, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::context::Context;
use crate::icon::{self, Icon};
use crate::track::Change;
use crate::{font, style};

use super::{Genie, Message, Session};

/// The width of the panel.
pub(super) const WIDTH: f32 = 240.0;
const ROW_HEIGHT: f32 = 22.0;
const SEARCH: &str = "track-search";

#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    /// Shows the panel, or hides it.
    Toggle,
    /// The words that find tracks by name.
    Query(String),
}

/// The state of the open panel.
#[derive(Debug, Default)]
pub(super) struct TrackList {
    query: String,
}

/// A row of the panel: a track, its name, its color, and the track that it overlays.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Row {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) hidden: bool,
    pub(super) hue: Hue,
    pub(super) over: Option<String>,
}

/// The rows of the tracks whose names hold all words of the query, in any case, in the order
/// of the tracks.
pub(super) fn rows(session: &Session, query: &str) -> Vec<Row> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let name_of = |id: u64| {
        session
            .tracks
            .iter()
            .find(|track| track.id == id)
            .map(|track| track.name.clone())
    };
    session
        .tracks
        .iter()
        .filter(|track| {
            let name = track.name.to_lowercase();
            words.iter().all(|word| name.contains(word))
        })
        .map(|track| Row {
            id: track.id,
            name: track.name.clone(),
            hidden: track.hidden,
            hue: track.hue,
            over: track.over.and_then(name_of),
        })
        .collect()
}

impl Genie {
    pub(super) fn listing(&mut self, listing: Listing) -> Task<Message> {
        match listing {
            Listing::Toggle => {
                if self.track_list.take().is_none() {
                    self.track_list = Some(TrackList::default());
                    return operation::focus(SEARCH);
                }
            }
            Listing::Query(query) => {
                if let Some(list) = &mut self.track_list {
                    list.query = query;
                }
            }
        }
        Task::none()
    }

    /// The panel, when it is open.
    pub(super) fn track_list_view<'a>(
        &'a self,
        session: &'a Session,
    ) -> Option<Element<'a, Message>> {
        let list = self.track_list.as_ref()?;
        let count = session.tracks.len();
        let words = if count == 1 {
            "1 track".to_owned()
        } else {
            format!("{count} tracks")
        };
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::TrackList(Listing::Toggle));
        let heading = row![
            text("Tracks").font(font::UI_STRONG).size(13),
            space::horizontal(),
            text(words).size(11).style(style::muted),
            close
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let search = text_input("Find a track", &list.query)
            .id(SEARCH)
            .size(12)
            .padding([3, 8])
            .style(style::locus)
            .on_input(|query| Message::TrackList(Listing::Query(query)));
        let rows = rows(session, &list.query).into_iter().map(row_view);
        let list = scrollable(Column::with_children(rows).spacing(1))
            .direction(Direction::Vertical(
                Scrollbar::new().width(6).scroller_width(6).margin(0),
            ))
            .style(style::scrollbar)
            .height(Length::Fill);
        let content = column![heading, search, list].spacing(8).padding(10);
        Some(container(content).id("tracks-panel").into())
    }
}

/// A row of the panel: the eye, the color of the track and its name.
fn row_view<'a>(row: Row) -> Element<'a, Message> {
    let Row {
        id,
        name,
        hidden,
        hue,
        over,
    } = row;
    let eye = button(icon::view(
        if hidden { Icon::EyeOff } else { Icon::Eye },
        12.0,
    ))
    .width(20)
    .height(20)
    .padding(0)
    .style(style::ghost)
    .on_press(Message::TrackChanged(id, Change::Visible(hidden)));
    let dot = container(space())
        .width(8)
        .height(8)
        .style(move |theme| style::dot(theme, hue));
    let mut line = row![
        container(eye).id(format!("eye-{id}")),
        dot,
        text(name)
            .size(12)
            .wrapping(text::Wrapping::None)
            .style(if hidden { style::muted } else { text::default })
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if let Some(base) = over {
        line = line.push(
            text(format!("on {base}"))
                .size(11)
                .wrapping(text::Wrapping::None)
                .style(style::muted),
        );
    }
    let line = container(line.height(ROW_HEIGHT))
        .id(format!("row-{id}"))
        .width(Length::Fill)
        .clip(true);
    Context::new(line, move |point| Message::TrackMenu(id, point)).into()
}
