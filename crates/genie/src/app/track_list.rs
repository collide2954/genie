//! The tracks panel at the left of the tracks: a row for each track with an eye that hides or
//! shows the track, folders that group tracks, a field that finds tracks by name, and the track
//! menu on a right-click.

use std::collections::BTreeSet;

use genie_render::scene::Hue;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, column, container, operation, row, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::context::Context;
use crate::icon::{self, Icon};
use crate::menu;
use crate::track::{self, Change};
use crate::{font, style};

use super::{Genie, Message, Session};

/// The width of the panel.
pub(super) const WIDTH: f32 = 240.0;
const ROW_HEIGHT: f32 = 22.0;
const SEARCH: &str = "track-search";
const NAME: &str = "folder-name";
/// The indent of the tracks of a folder.
const INDENT: f32 = 14.0;
const MENU_HEIGHT: f32 = 2.0 * 26.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    /// Shows the panel, or hides it.
    Toggle,
    /// The words that find tracks by name.
    Query(String),
    /// Asks for the name of a new folder for a track.
    Name(u64),
    /// Asks for a new name of a folder.
    Rename(String),
    /// The name in the field.
    Draft(String),
    /// Uses the name in the field.
    Named,
    /// Folds the rows of a folder, or shows them again.
    Fold(String),
}

/// What a folder does: with one track, or with all its tracks.
#[derive(Debug, Clone, PartialEq)]
pub enum Foldering {
    /// Puts a track in the folder, after its other tracks.
    Add(u64),
    /// Takes a track out of the folder.
    Take(u64),
    /// Shows all tracks of the folder, or hides them.
    Visible(bool),
    Rename(String),
    /// Takes the tracks out of the folder. The tracks stay.
    Remove,
}

/// What the name field names: a new folder for a track, or a folder.
#[derive(Debug, Clone, PartialEq)]
enum Naming {
    New(u64),
    Folder(String),
}

/// The state of the open panel.
#[derive(Debug, Default)]
pub(super) struct TrackList {
    query: String,
    /// The folders whose rows are folded.
    folded: BTreeSet<String>,
    /// The name field and its text.
    naming: Option<(Naming, String)>,
}

/// A track row of the panel: a track, its name, its color, the track that it overlays, and
/// whether it is in a folder.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Row {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) hidden: bool,
    pub(super) hue: Hue,
    pub(super) over: Option<String>,
    pub(super) inside: bool,
}

/// A row of the panel.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Item {
    /// A folder: its name, the number of its tracks, true when all are hidden, and true when its
    /// rows are folded.
    Folder {
        name: String,
        count: usize,
        hidden: bool,
        folded: bool,
    },
    Track(Row),
}

/// The rows of the panel in the order of the tracks: each folder before its tracks. With words
/// in the field, only the tracks whose names hold all the words, in any case, and their folders.
pub(super) fn items(session: &Session, list: Option<&TrackList>) -> Vec<Item> {
    let query = list.map_or(String::new(), |list| list.query.to_lowercase());
    let words: Vec<&str> = query.split_whitespace().collect();
    let found = |name: &str| {
        let name = name.to_lowercase();
        words.iter().all(|word| name.contains(word))
    };
    let name_of = |id: u64| {
        let other = session.tracks.iter().find(|track| track.id == id);
        other.map(|track| track.name.clone())
    };
    let mut items = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for track in &session.tracks {
        if let Some(folder) = track.folder.as_deref() {
            if seen.contains(&folder) {
                continue;
            }
            seen.push(folder);
            let members: Vec<&track::Track> = session
                .tracks
                .iter()
                .filter(|other| other.folder.as_deref() == Some(folder))
                .collect();
            let shown: Vec<&&track::Track> = members
                .iter()
                .filter(|member| found(&member.name))
                .collect();
            if shown.is_empty() && !found(folder) {
                continue;
            }
            let folded = list.is_some_and(|list| list.folded.contains(folder));
            items.push(Item::Folder {
                name: folder.to_owned(),
                count: members.len(),
                hidden: members.iter().all(|member| member.hidden),
                folded,
            });
            if !folded {
                items.extend(shown.into_iter().map(|member| {
                    Item::Track(row_of(member, member.over.and_then(name_of), true))
                }));
            }
        } else if found(&track.name) {
            items.push(Item::Track(row_of(
                track,
                track.over.and_then(name_of),
                false,
            )));
        }
    }
    items
}

fn row_of(track: &track::Track, over: Option<String>, inside: bool) -> Row {
    Row {
        id: track.id,
        name: track.name.clone(),
        hidden: track.hidden,
        hue: track.hue,
        over,
        inside,
    }
}

/// The names of the folders of the tracks, in the order of the tracks.
pub(super) fn folders(session: &Session) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for name in session
        .tracks
        .iter()
        .filter_map(|track| track.folder.as_ref())
    {
        if !found.contains(name) {
            found.push(name.clone());
        }
    }
    found
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
            Listing::Name(id) => return self.ask_name(Naming::New(id), String::new()),
            Listing::Rename(folder) => {
                return self.ask_name(Naming::Folder(folder.clone()), folder);
            }
            Listing::Draft(text) => {
                if let Some((_, draft)) = self
                    .track_list
                    .as_mut()
                    .and_then(|list| list.naming.as_mut())
                {
                    *draft = text;
                }
            }
            Listing::Named => return self.use_name(),
            Listing::Fold(folder) => {
                if let Some(list) = &mut self.track_list
                    && !list.folded.remove(&folder)
                {
                    list.folded.insert(folder);
                }
            }
        }
        Task::none()
    }

    /// Shows the panel with the name field.
    fn ask_name(&mut self, naming: Naming, draft: String) -> Task<Message> {
        self.close_menus();
        let list = self.track_list.get_or_insert_default();
        list.naming = Some((naming, draft));
        Task::batch([operation::focus(NAME), operation::select_all(NAME)])
    }

    /// Makes the new folder of a track, or renames a folder, with the name in the field.
    fn use_name(&mut self) -> Task<Message> {
        let Some((naming, draft)) = self.track_list.as_mut().and_then(|list| list.naming.take())
        else {
            return Task::none();
        };
        let name = draft.trim().to_owned();
        if name.is_empty() {
            return Task::none();
        }
        match naming {
            Naming::New(id) => self.folder(&name, &Foldering::Add(id)),
            Naming::Folder(folder) => self.folder(&folder, &Foldering::Rename(name)),
        }
    }

    /// Puts a track in a folder or takes it out, or shows, hides, renames or removes a folder:
    /// one step to undo.
    fn folder(&mut self, folder: &str, foldering: &Foldering) -> Task<Message> {
        self.close_menus();
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let tracks = &mut session.tracks;
        let at =
            |tracks: &Vec<track::Track>, id: u64| tracks.iter().position(|track| track.id == id);
        let members: Vec<u64> = tracks
            .iter()
            .filter(|track| track.folder.as_deref() == Some(folder))
            .map(|track| track.id)
            .collect();
        match foldering {
            Foldering::Add(id) => {
                if let Some(position) = at(tracks, *id) {
                    track::file(tracks, position, Some(folder.to_owned()));
                }
            }
            Foldering::Take(id) => {
                if let Some(position) = at(tracks, *id) {
                    track::file(tracks, position, None);
                }
            }
            Foldering::Visible(visible) => {
                for id in members {
                    track::change(tracks, id, Change::Visible(*visible));
                }
            }
            Foldering::Rename(name) => {
                for id in members {
                    if let Some(position) = at(tracks, id) {
                        track::file(tracks, position, Some(name.clone()));
                    }
                }
            }
            Foldering::Remove => {
                for track in tracks.iter_mut() {
                    if track.folder.as_deref() == Some(folder) {
                        track.folder = None;
                    }
                }
            }
        }
        if let (Foldering::Rename(name), Some(list)) = (foldering, &mut self.track_list)
            && list.folded.remove(folder)
        {
            list.folded.insert(name.clone());
        }
        self.fetch()
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
        let mut content = column![heading, search].spacing(8).padding(10);
        if let Some((naming, draft)) = &list.naming {
            let hint = match naming {
                Naming::New(_) => "Name of the new folder",
                Naming::Folder(_) => "New name of the folder",
            };
            content = content.push(
                text_input(hint, draft)
                    .id(NAME)
                    .size(12)
                    .padding([3, 8])
                    .style(style::locus)
                    .on_input(|text| Message::TrackList(Listing::Draft(text)))
                    .on_submit(Message::TrackList(Listing::Named)),
            );
        }
        let rows = items(session, Some(list)).into_iter().map(item_view);
        let rows = scrollable(Column::with_children(rows).spacing(1))
            .direction(Direction::Vertical(
                Scrollbar::new().width(6).scroller_width(6).margin(0),
            ))
            .style(style::scrollbar)
            .height(Length::Fill);
        Some(container(content.push(rows)).id("tracks-panel").into())
    }

    /// The menu of a folder row, when it is open: rename the folder, or take its tracks out of
    /// it.
    pub(super) fn folder_menu(&self) -> Option<Element<'_, Message>> {
        let (folder, at) = self.folder_menu.as_ref()?;
        let rename = Message::TrackList(Listing::Rename(folder.clone()));
        let remove = Message::Folder(folder.clone(), Foldering::Remove);
        let content = column![
            menu::item("Rename folder…", rename, false, true),
            menu::item("Remove the folder", remove, false, true),
        ]
        .spacing(1);
        Some(menu::card(content, *at, self.window, MENU_HEIGHT))
    }

    /// The messages of the tracks panel and its folders.
    pub(super) fn panel_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TrackList(listing) => self.listing(listing),
            Message::Folder(folder, foldering) => self.folder(&folder, &foldering),
            Message::FolderMenu(folder, at) => {
                self.close_menus();
                self.folder_menu = Some((folder, at));
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

fn eye<'a>(hidden: bool, message: Message) -> button::Button<'a, Message> {
    let icon = if hidden { Icon::EyeOff } else { Icon::Eye };
    button(icon::view(icon, 12.0))
        .width(20)
        .height(20)
        .padding(0)
        .style(style::ghost)
        .on_press(message)
}

fn item_view<'a>(item: Item) -> Element<'a, Message> {
    match item {
        Item::Track(row) => row_view(row),
        Item::Folder {
            name,
            count,
            hidden,
            folded,
        } => {
            let fold = button(icon::view(
                if folded { Icon::Forward } else { Icon::Down },
                10.0,
            ))
            .width(16)
            .height(16)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::TrackList(Listing::Fold(name.clone())));
            let visible = Message::Folder(name.clone(), Foldering::Visible(hidden));
            let line = row![
                fold,
                container(eye(hidden, visible)).id(format!("folder-eye-{name}")),
                icon::view(Icon::Open, 12.0),
                text(name.clone())
                    .size(12)
                    .font(font::UI_STRONG)
                    .wrapping(text::Wrapping::None),
                text(count.to_string()).size(11).style(style::muted),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .height(ROW_HEIGHT);
            let line = container(line)
                .id(format!("folder-{name}"))
                .width(Length::Fill)
                .clip(true);
            Context::new(line, move |point| Message::FolderMenu(name.clone(), point)).into()
        }
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
        inside,
    } = row;
    let dot = container(space())
        .width(8)
        .height(8)
        .style(move |theme| style::dot(theme, hue));
    let mut line = row![
        space().width(if inside { INDENT } else { 0.0 }),
        container(eye(
            hidden,
            Message::TrackChanged(id, Change::Visible(hidden))
        ))
        .id(format!("eye-{id}")),
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
