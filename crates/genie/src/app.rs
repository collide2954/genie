use std::path::PathBuf;
use std::sync::Arc;

use genie_core::{Locus, Named, Names, Region, Span, Target, Viewport, grouped};
use genie_render::{features, overview, ruler, sequence};
use iced::theme::Mode;
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    button, center, column, container, mouse_area, operation, pin, row, scrollable, space, stack,
    text,
};
use iced::{
    Alignment, Border, Element, Length, Point, Size, Subscription, Task, Theme, keyboard, system,
    window,
};

use crate::card::{self, Card};
use crate::chrome::{self, Controls, LocusField};
use crate::context::Context;
use crate::icon::{self, Icon};
use crate::menu::{self, Place};
use crate::motion::Transition;
use crate::navigator::Navigator;
use crate::paths::file_name;
use crate::recent::Recent;
use crate::reference::{self, Reference};
use crate::scene::Pointer;
use crate::shortcuts::{self, Command};
use crate::theme::{self, Appearance, tokens};
use crate::tiles::{Key, TILING, Tiles};
use crate::track::{self, Change, Loaded, Source, Track};
use crate::{font, paths, scene, style};

const INITIAL_WIDTH: f32 = 1_000.0;
const OVERVIEW_HEIGHT: f32 = 12.0;
const RULER_HEIGHT: f32 = 20.0;
const BASE_HEIGHT: f32 = 16.0;
const TRACK_PADDING: f32 = 2.0;
const EMPTY_HEIGHT: f32 = 20.0;
const LABEL_SIZE: f32 = 10.0;
const FEATURE_SPAN: u64 = 10_000_000;
const DROP_HINT: &str = "Drop the file to open it";
const ZOOM_HINT: &str = "Zoom in to see the features";
const NO_REFERENCE: &str = "Open a reference sequence before you add tracks.";
const SUGGESTIONS: usize = 8;

#[derive(Debug)]
pub struct Genie {
    appearance: Appearance,
    system: Mode,
    status: Status,
    session: Option<Session>,
    width: f32,
    tiles: Tiles,
    generation: u64,
    draft: Option<String>,
    locus_error: Option<String>,
    recent: Recent,
    hovering: bool,
    highlight: Option<usize>,
    next_track: u64,
    notice: Option<String>,
    menu: Option<(u64, Point)>,
    scroll: f32,
    window: Size,
    hover: Option<Card>,
    pinned: Option<Card>,
}

impl Default for Genie {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            system: Mode::default(),
            status: Status::default(),
            session: None,
            width: INITIAL_WIDTH,
            tiles: Tiles::default(),
            generation: 0,
            draft: None,
            locus_error: None,
            recent: Recent::default(),
            hovering: false,
            highlight: None,
            next_track: 0,
            notice: None,
            menu: None,
            scroll: 0.0,
            window: chrome::WINDOW,
            hover: None,
            pinned: None,
        }
    }
}

#[derive(Debug, Default)]
enum Status {
    #[default]
    Idle,
    Opening(String),
    Failed(String),
}

#[derive(Debug)]
struct Session {
    reference: Arc<Reference>,
    contig: usize,
    viewport: Viewport,
    transition: Option<Transition>,
    grip: Option<f64>,
    tracks: Vec<Track>,
}

impl Session {
    fn new(reference: Arc<Reference>, width: f32) -> Self {
        let length = reference.assembly.contigs()[0].length;
        Self {
            reference,
            contig: 0,
            viewport: Viewport::new(length, f64::from(width)),
            transition: None,
            grip: None,
            tracks: Vec::new(),
        }
    }

    fn grab(&mut self, share: f32) {
        self.transition = None;
        let position = f64::from(share) * self.viewport.length() as f64;
        if position >= self.viewport.start() && position <= self.viewport.end() {
            self.grip = Some(position - self.viewport.center());
        } else {
            self.viewport = self.viewport.center_on(position);
            self.grip = Some(0.0);
        }
    }

    fn slide(&mut self, share: f32) {
        if let Some(grip) = self.grip {
            let position = f64::from(share) * self.viewport.length() as f64;
            self.viewport = self.viewport.center_on(position - grip);
        }
    }

    fn advance(&mut self, now: Instant) {
        if let Some(transition) = &self.transition {
            self.viewport = transition.at(now);
            if transition.is_done(now) {
                self.transition = None;
            }
        }
    }

    fn accept(
        &mut self,
        id: u64,
        contig: usize,
        region: Region,
        result: Result<Arc<Loaded>, String>,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == id) {
            track.accept(contig, region, result);
        }
    }

    fn name(&mut self, id: u64, names: Arc<Names>) {
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == id) {
            track.names = Some(names);
        }
    }

    fn locus_of(&self, named: &Named) -> String {
        Locus {
            name: self.reference.assembly.contigs()[named.contig].name.clone(),
            span: Span::Region(named.span),
        }
        .to_string()
    }

    fn destination(&self) -> Viewport {
        self.transition
            .as_ref()
            .map_or(self.viewport, Transition::target)
    }

    fn glide(&mut self, to: Viewport) {
        self.transition = Some(Transition::new(self.viewport, to, Instant::now()));
    }

    fn locus(&self) -> String {
        Locus {
            name: self.reference.assembly.contigs()[self.contig].name.clone(),
            span: Span::Region(self.viewport.visible()),
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    CycleAppearance,
    SystemModeChanged(Mode),
    Window(chrome::Action),
    Open,
    Picked(Option<PathBuf>),
    Opened(Result<Arc<Reference>, String>),
    Pan(f32),
    Zoom(f32, f32),
    Resized(f32),
    TileLoaded(u64, Key, Result<Arc<[u8]>, String>),
    LocusEdited(String),
    LocusSubmitted,
    Command(Command),
    Frame(Instant),
    OverviewPressed(f32),
    OverviewDragged(f32),
    OverviewReleased,
    FileHovered,
    FileHoverEnded,
    FileDropped(PathBuf),
    SuggestionChosen(usize),
    TrackOpened(Result<Arc<Source>, String>),
    TrackLoaded(u64, usize, Region, Result<Arc<Loaded>, String>),
    NoticeDismissed,
    TrackMenu(u64, Point),
    TrackChanged(u64, Change),
    MenuDismissed,
    Scrolled(f32),
    WindowResized(Size),
    TrackHovered(u64, Option<Pointer>),
    TrackClicked(u64, Pointer),
    CardClosed,
    NamesLoaded(u64, Result<Arc<Names>, String>),
}

#[derive(Debug, Clone, PartialEq)]
enum Suggestion {
    Contig(usize),
    Feature(Named),
}

impl Genie {
    fn boot() -> (Self, Task<Message>) {
        let recent = paths::config_dir()
            .map(|folder| Recent::load(&folder))
            .unwrap_or_default();
        (
            Self {
                recent,
                ..Self::default()
            },
            system::theme().map(Message::SystemModeChanged),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CycleAppearance => self.appearance = self.appearance.next(),
            Message::SystemModeChanged(mode) => self.system = mode,
            Message::Window(action) => return chrome::perform(action),
            Message::Open => return Task::perform(pick(self.session.is_some()), Message::Picked),
            Message::Picked(Some(path)) => return self.open(path),
            Message::Picked(None) | Message::NamesLoaded(_, Err(_)) => {}
            Message::NamesLoaded(id, Ok(names)) => {
                self.on_session(|session| session.name(id, names));
            }
            Message::FileHovered => self.hovering = true,
            Message::FileHoverEnded => self.hovering = false,
            Message::FileDropped(path) => {
                self.hovering = false;
                return self.open(path);
            }
            Message::Opened(Ok(reference)) => {
                self.status = Status::Idle;
                self.generation += 1;
                self.tiles.clear();
                self.recent.add(reference.path.clone());
                (self.hover, self.pinned) = (None, None);
                self.session = Some(Session::new(reference, self.width));
                return Task::batch([self.fetch(), self.remember()]);
            }
            Message::Opened(Err(error)) | Message::TrackOpened(Err(error)) => self.fail(error),
            Message::TrackOpened(Ok(source)) => return self.add_track(source),
            Message::TrackLoaded(id, contig, region, result) => {
                self.on_session(|session| session.accept(id, contig, region, result));
            }
            Message::NoticeDismissed => self.notice = None,
            Message::TrackMenu(id, at) => {
                self.menu = Some((id, Point::new(at.x, at.y - self.scroll)));
            }
            Message::TrackChanged(id, change) => {
                self.menu = None;
                if change == Change::Remove {
                    self.forget(id);
                }
                self.on_session(|session| track::change(&mut session.tracks, id, change));
            }
            Message::TrackHovered(id, pointer) => {
                self.hover = pointer.and_then(|pointer| self.card(id, pointer));
            }
            Message::TrackClicked(id, pointer) => self.pinned = self.card(id, pointer),
            Message::CardClosed => self.pinned = None,
            Message::MenuDismissed => self.menu = None,
            Message::Scrolled(offset) => {
                self.scroll = offset;
                self.menu = None;
            }
            Message::WindowResized(size) => self.window = size,
            Message::Pan(dx) => {
                self.drop_draft();
                return self.navigate(|viewport| viewport.pan(f64::from(dx)));
            }
            Message::Zoom(factor, anchor) => {
                self.drop_draft();
                return self
                    .navigate(|viewport| viewport.zoom(f64::from(factor), f64::from(anchor)));
            }
            Message::Resized(width) => {
                self.width = width;
                return self.navigate(|viewport| viewport.resize(f64::from(width)));
            }
            Message::TileLoaded(generation, key, result) => {
                if generation == self.generation {
                    match result {
                        Ok(bases) => self.tiles.insert(key, bases),
                        Err(_) => self.tiles.cancel(key),
                    }
                }
            }
            Message::LocusEdited(text) => {
                self.draft = Some(text);
                self.locus_error = None;
                self.highlight = None;
            }
            Message::LocusSubmitted => {
                return match self.highlight {
                    Some(position) => self.choose(position),
                    None => self.submit(),
                };
            }
            Message::SuggestionChosen(position) => return self.choose(position),
            Message::Command(command) => return self.command(command),
            Message::Frame(now) => self.on_session(|session| session.advance(now)),
            Message::OverviewPressed(share) => {
                self.drop_draft();
                self.on_session(|session| session.grab(share));
                return self.fetch();
            }
            Message::OverviewDragged(share) => {
                self.on_session(|session| session.slide(share));
                return self.fetch();
            }
            Message::OverviewReleased => self.on_session(|session| session.grip = None),
        }
        Task::none()
    }

    fn on_session(&mut self, change: impl FnOnce(&mut Session)) {
        if let Some(session) = &mut self.session {
            change(session);
        }
    }

    fn card(&self, id: u64, pointer: Pointer) -> Option<Card> {
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        let data = track.data(session.contig)?;
        let mode = track.mode(session.contig);
        let (x, y) = (pointer.local.x, pointer.local.y);
        let index = features::hit(session.viewport, &data.features, &data.rows, mode, x, y)?;
        Some(Card {
            track: id,
            contig: session.reference.assembly.contigs()[session.contig]
                .name
                .clone(),
            feature: data.features[index].clone(),
            at: Point::new(pointer.at.x, pointer.at.y - self.scroll),
        })
    }

    fn forget(&mut self, id: u64) {
        if self.hover.as_ref().is_some_and(|card| card.track == id) {
            self.hover = None;
        }
        if self.pinned.as_ref().is_some_and(|card| card.track == id) {
            self.pinned = None;
        }
    }

    fn is_animating(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.transition.is_some())
    }

    fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Pan(share) => {
                self.drop_draft();
                self.glide(|viewport| viewport.pan(f64::from(share) * viewport.width()))
            }
            Command::Zoom(factor) => {
                self.drop_draft();
                self.glide(|viewport| viewport.zoom(f64::from(factor), viewport.width() / 2.0))
            }
            Command::FocusLocus => Task::batch([
                operation::focus(chrome::LOCUS),
                operation::select_all(chrome::LOCUS),
            ]),
            Command::NextSuggestion => {
                self.step_highlight(true);
                Task::none()
            }
            Command::PreviousSuggestion => {
                self.step_highlight(false);
                Task::none()
            }
            Command::Dismiss => {
                self.menu = None;
                self.notice = None;
                self.pinned = None;
                Task::none()
            }
        }
    }

    fn suggestions(&self) -> Vec<Suggestion> {
        let (Some(session), Some(draft)) = (&self.session, &self.draft) else {
            return Vec::new();
        };
        if draft.contains(':') {
            return Vec::new();
        }
        let mut list: Vec<Suggestion> = session
            .reference
            .assembly
            .suggest(draft, SUGGESTIONS)
            .into_iter()
            .map(Suggestion::Contig)
            .collect();
        let named = session
            .tracks
            .iter()
            .filter_map(|track| track.names.as_deref())
            .flat_map(|names| names.search(draft, SUGGESTIONS));
        for named in named {
            if list.len() == SUGGESTIONS {
                break;
            }
            let suggestion = Suggestion::Feature(named.clone());
            if !list.contains(&suggestion) {
                list.push(suggestion);
            }
        }
        list
    }

    fn step_highlight(&mut self, forward: bool) {
        let count = self.suggestions().len();
        if count == 0 {
            return;
        }
        self.highlight = Some(match (self.highlight, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(position), true) => (position + 1) % count,
            (Some(position), false) => (position + count - 1) % count,
        });
    }

    fn choose(&mut self, position: usize) -> Task<Message> {
        let chosen = self.suggestions().into_iter().nth(position);
        let Some(session) = &self.session else {
            return Task::none();
        };
        if let Some(suggestion) = chosen {
            self.draft = Some(match suggestion {
                Suggestion::Contig(contig) => {
                    session.reference.assembly.contigs()[contig].name.clone()
                }
                Suggestion::Feature(named) => session.locus_of(&named),
            });
        }
        self.highlight = None;
        self.submit()
    }

    fn drop_draft(&mut self) {
        self.draft = None;
        self.locus_error = None;
        self.highlight = None;
    }

    fn submit(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let text = self.draft.take().unwrap_or_else(|| session.locus());
        let resolved = session.reference.assembly.resolve(&text).or_else(|error| {
            session
                .tracks
                .iter()
                .filter_map(|track| track.names.as_deref())
                .find_map(|names| names.find(&text))
                .map(|named| Target {
                    contig: named.contig,
                    span: Span::Region(named.span),
                })
                .ok_or(error)
        });
        match resolved {
            Ok(target) => {
                if target.contig == session.contig {
                    session.glide(session.destination().jump(target.span));
                } else {
                    let length = session.reference.assembly.contigs()[target.contig].length;
                    session.transition = None;
                    session.contig = target.contig;
                    session.viewport = session.viewport.retarget(length).jump(target.span);
                }
                self.locus_error = None;
                self.fetch()
            }
            Err(error) => {
                self.locus_error = Some(error.to_string());
                self.draft = Some(text);
                Task::none()
            }
        }
    }

    fn navigate(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        if let Some(session) = &mut self.session {
            session.transition = None;
            session.viewport = change(session.viewport);
        }
        self.fetch()
    }

    fn glide(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        if let Some(session) = &mut self.session {
            session.glide(change(session.destination()));
        }
        self.fetch()
    }

    fn fetch(&mut self) -> Task<Message> {
        Task::batch([self.fetch_bases(), self.fetch_features()])
    }

    fn fetch_features(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let viewport = session.destination();
        let visible = viewport.visible();
        if visible.len() > FEATURE_SPAN {
            return Task::none();
        }
        let contig = session.contig;
        let wanted = visible.expand(visible.len(), viewport.length());
        let tasks: Vec<Task<Message>> = session
            .tracks
            .iter_mut()
            .filter(|track| track.needs(contig, visible))
            .filter_map(|track| {
                let name = track.contigs[contig].clone()?;
                track.wanted = Some((contig, wanted));
                let (id, source) = (track.id, Arc::clone(&track.source));
                Some(Task::perform(
                    async move { track::load(&source, &name, wanted).map(Arc::new) },
                    move |result| Message::TrackLoaded(id, contig, wanted, result),
                ))
            })
            .collect();
        Task::batch(tasks)
    }

    fn fetch_bases(&mut self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let viewport = session.destination();
        if viewport.scale() > 1.0 {
            return Task::none();
        }
        let visible = viewport.visible();
        let wanted = visible.expand(visible.len(), viewport.length());
        let generation = self.generation;
        Task::batch(
            self.tiles
                .request(session.contig, wanted)
                .into_iter()
                .map(|key| {
                    let reference = Arc::clone(&session.reference);
                    Task::perform(
                        async move {
                            reference
                                .fetch(key.contig, TILING.span(key.index))
                                .map(Arc::from)
                        },
                        move |result| Message::TileLoaded(generation, key, result),
                    )
                }),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        let appearance = match self.appearance {
            Appearance::System => Icon::Monitor,
            Appearance::Light => Icon::Sun,
            Appearance::Dark => Icon::Moon,
        };
        let locus = self.session.as_ref().map(|session| LocusField {
            value: self.draft.clone().unwrap_or_else(|| session.locus()),
            error: self.locus_error.clone(),
        });
        let mut layers = stack![column![
            chrome::title_bar(Controls::CURRENT, appearance, locus),
            self.body()
        ]];
        if let Some(menu) = self.suggestion_menu() {
            layers = layers.push(menu);
        }
        if let Some(pinned) = &self.pinned {
            layers = layers.push(card::view(pinned, true, self.window));
        }
        if let Some(hover) = &self.hover
            && self
                .pinned
                .as_ref()
                .is_none_or(|pinned| pinned.feature != hover.feature)
        {
            layers = layers.push(card::view(hover, false, self.window));
        }
        if let Some(notice) = &self.notice {
            layers = layers.push(toast(notice));
        }
        if let Some(menu) = self.track_menu() {
            let dismiss = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::MenuDismissed)
                .on_right_press(Message::MenuDismissed);
            layers = layers.push(dismiss).push(menu);
        }
        if self.hovering {
            layers = layers.push(drop_hint());
        }
        if Controls::CURRENT == Controls::Custom {
            layers = layers.push(chrome::resize_handles());
        }
        layers.into()
    }

    fn track_menu(&self) -> Option<Element<'_, Message>> {
        let (id, at) = self.menu?;
        let session = self.session.as_ref()?;
        let position = session.tracks.iter().position(|track| track.id == id)?;
        let track = &session.tracks[position];
        let place = Place {
            position,
            count: session.tracks.len(),
            at,
            window: self.window,
        };
        Some(menu::view(track, track.mode(session.contig), &place))
    }

    fn suggestion_menu(&self) -> Option<Element<'_, Message>> {
        let session = self.session.as_ref()?;
        let suggestions = self.suggestions();
        if suggestions.is_empty() {
            return None;
        }
        let contigs = session.reference.assembly.contigs();
        let rows = suggestions
            .into_iter()
            .enumerate()
            .map(|(position, suggestion)| {
                let (name, detail) = match suggestion {
                    Suggestion::Contig(index) => (
                        text(contigs[index].name.as_str()).font(font::MONO),
                        grouped(contigs[index].length),
                    ),
                    Suggestion::Feature(named) => (
                        text(named.name),
                        format!(
                            "{}:{}",
                            contigs[named.contig].name,
                            grouped(named.span.start() + 1)
                        ),
                    ),
                };
                let chosen = self.highlight == Some(position);
                button(
                    row![
                        name.size(12),
                        space::horizontal(),
                        text(detail).size(11).style(style::muted),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([3, 8])
                .style(if chosen { style::chosen } else { style::ghost })
                .on_press(Message::SuggestionChosen(position))
                .into()
            });
        let menu = container(column(rows).spacing(1))
            .width(chrome::LOCUS_WIDTH)
            .padding(4)
            .style(style::menu);
        Some(
            pin(menu)
                .x(chrome::locus_x(Controls::CURRENT))
                .y(chrome::BAR_HEIGHT + 2.0)
                .into(),
        )
    }

    fn body(&self) -> Element<'_, Message> {
        if let Some(session) = &self.session {
            return self.browser(session);
        }
        let message = match &self.status {
            Status::Idle => text("Open a reference sequence to start.").style(style::muted),
            Status::Opening(name) => text(format!("Opening {name}…")).style(style::muted),
            Status::Failed(error) => text(error.as_str()),
        };
        let open = button(text("Open reference").font(font::UI_STRONG).size(13))
            .padding([8, 16])
            .style(style::primary)
            .on_press(Message::Open);
        let mut welcome = column![message, open]
            .spacing(16)
            .align_x(Alignment::Center);
        if !self.recent.paths().is_empty() {
            let files = self.recent.paths().iter().map(|path| {
                let folder = path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default();
                button(
                    row![
                        text(file_name(path)).size(12),
                        text(folder).size(11).style(style::muted)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(style::ghost)
                .on_press(Message::Picked(Some(path.clone())))
                .into()
            });
            welcome = welcome.push(
                column![text("Recent").size(11).style(style::muted)]
                    .extend(files)
                    .spacing(2),
            );
        }
        center(welcome).into()
    }

    fn open(&mut self, path: PathBuf) -> Task<Message> {
        if track::is_track(&path) {
            if self.session.is_none() {
                self.status = Status::Failed(NO_REFERENCE.to_owned());
                return Task::none();
            }
            return Task::perform(
                async move { track::open(path).map(Arc::new) },
                Message::TrackOpened,
            );
        }
        if self.session.is_none() {
            self.status = Status::Opening(file_name(&path));
        }
        Task::perform(
            async move { reference::open(path, paths::cache_dir().as_deref()).map(Arc::new) },
            Message::Opened,
        )
    }

    fn add_track(&mut self, source: Arc<Source>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        };
        if session.tracks.iter().any(|track| track.source == source) {
            self.notice = Some(format!("The track {} is already open.", source.name));
            return Task::none();
        }
        self.notice = None;
        let id = self.next_track;
        self.next_track += 1;
        let track = Track::new(id, Arc::clone(&source), &session.reference.assembly);
        session.tracks.push(track);
        let reference = Arc::clone(&session.reference);
        let names = Task::perform(
            async move { track::names(&source, &reference.assembly).map(Arc::new) },
            move |result| Message::NamesLoaded(id, result),
        );
        Task::batch([self.fetch_features(), names])
    }

    fn fail(&mut self, error: String) {
        if self.session.is_some() {
            self.notice = Some(error);
        } else {
            self.status = Status::Failed(error);
        }
    }

    fn remember(&self) -> Task<Message> {
        let Some(folder) = paths::config_dir() else {
            return Task::none();
        };
        let recent = self.recent.clone();
        Task::future(async move {
            let _ = recent.save(&folder);
        })
        .discard()
    }

    fn browser<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let viewport = session.viewport;
        let bases = sequence::scene(
            viewport,
            self.tiles.chunks(session.contig, viewport.visible()),
            BASE_HEIGHT,
        );
        let sequence = stack![
            container(scene::view(bases, BASE_HEIGHT)).padding([TRACK_PADDING, 0.0]),
            label("Sequence"),
        ];
        let mut data = column![container(sequence).id("sequence"), hairline()];
        for track in &session.tracks {
            data = data.push(track_view(session, track)).push(hairline());
        }
        let tracks = column![
            container(
                scene::view(overview::scene(viewport, OVERVIEW_HEIGHT), OVERVIEW_HEIGHT)
                    .on_press(Message::OverviewPressed)
                    .on_drag(Message::OverviewDragged)
                    .on_release(Message::OverviewReleased)
            )
            .id("overview"),
            container(scene::view(
                ruler::scene(viewport, RULER_HEIGHT),
                RULER_HEIGHT
            ))
            .id("ruler"),
            hairline(),
            scrollable(data)
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0)
                ))
                .style(style::scrollbar)
                .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                .height(Length::Fill),
        ]
        .height(Length::Fill);
        Navigator::new(tracks, self.width)
            .on_pan(Message::Pan)
            .on_zoom(Message::Zoom)
            .on_resize(Message::Resized)
            .into()
    }

    fn title(&self) -> String {
        match &self.session {
            Some(session) => format!("{} · genie", session.reference.name),
            None => "genie".to_owned(),
        }
    }

    fn theme(&self) -> Theme {
        if self.appearance.is_dark(self.system) {
            theme::dark()
        } else {
            theme::light()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let frames = if self.is_animating() {
            window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        };
        Subscription::batch([
            system::theme_changes().map(Message::SystemModeChanged),
            keyboard::listen().filter_map(shortcut),
            window::events().filter_map(window_event),
            frames,
        ])
    }
}

fn track_view<'a>(session: &Session, track: &'a Track) -> Element<'a, Message> {
    let viewport = session.viewport;
    let contig = session.contig;
    let body: Element<'a, Message> = if let Some(error) = &track.error {
        hint(error.clone())
    } else if viewport.visible().len() > FEATURE_SPAN {
        hint(ZOOM_HINT.to_owned())
    } else if let Some(data) = track.data(contig) {
        let mode = track.mode(contig);
        let rows = if mode == features::Mode::Collapsed {
            1
        } else {
            data.count.max(1)
        };
        let primitives = features::scene(viewport, &data.features, &data.rows, mode, track.hue);
        let id = track.id;
        scene::view(primitives, rows as f32 * mode.row_height())
            .on_hover(move |pointer| Message::TrackHovered(id, pointer))
            .on_click(move |pointer| Message::TrackClicked(id, pointer))
            .into()
    } else {
        space().width(Length::Fill).height(EMPTY_HEIGHT).into()
    };
    let id = track.id;
    Context::new(
        stack![
            container(body).padding([TRACK_PADDING, 0.0]),
            label(&track.source.name)
        ],
        move |at| Message::TrackMenu(id, at),
    )
    .into()
}

fn hint<'a>(message: String) -> Element<'a, Message> {
    container(text(message).size(11).style(style::muted))
        .center_x(Length::Fill)
        .height(EMPTY_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

fn toast(notice: &str) -> Element<'_, Message> {
    let close = button(icon::view(Icon::Close, 12.0))
        .width(20)
        .height(20)
        .padding(0)
        .style(style::ghost)
        .on_press(Message::NoticeDismissed);
    let card = container(
        row![text(notice).size(12).width(Length::Fill), close]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .max_width(360)
    .padding([6, 6])
    .style(style::menu);
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(12)
        .into()
}

fn label(name: &str) -> Element<'_, Message> {
    container(text(name).size(LABEL_SIZE).style(style::muted))
        .padding([0, 4])
        .style(|theme| container::Style {
            background: Some(tokens(theme).canvas.scale_alpha(0.85).into()),
            border: Border::default().rounded(3),
            ..container::Style::default()
        })
        .into()
}

fn hairline<'a>() -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|theme| container::background(tokens(theme).line))
        .into()
}

fn drop_hint<'a>() -> Element<'a, Message> {
    let frame = container(center(text(DROP_HINT).size(13))).style(|theme| {
        let tokens = tokens(theme);
        container::Style {
            background: Some(tokens.canvas.scale_alpha(0.92).into()),
            border: Border::default().rounded(8).width(1).color(tokens.muted),
            ..container::Style::default()
        }
    });
    container(frame).padding(8).into()
}

fn window_event((_, event): (window::Id, window::Event)) -> Option<Message> {
    match event {
        window::Event::FileHovered(_) => Some(Message::FileHovered),
        window::Event::FilesHoveredLeft => Some(Message::FileHoverEnded),
        window::Event::FileDropped(path) => Some(Message::FileDropped(path)),
        window::Event::Opened { size, .. } | window::Event::Resized(size) => {
            Some(Message::WindowResized(size))
        }
        _ => None,
    }
}

fn shortcut(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => {
            shortcuts::command(&key, modifiers).map(Message::Command)
        }
        _ => None,
    }
}

async fn pick(tracks: bool) -> Option<PathBuf> {
    let references = ["fa", "fasta", "fna", "fas", "2bit"];
    let dialog = if tracks {
        rfd::AsyncFileDialog::new()
            .set_title("Open a track or a reference sequence")
            .add_filter("Tracks", &["gz", "bgz", "bed", "gff", "gff3"])
            .add_filter("Reference sequences", &references)
    } else {
        rfd::AsyncFileDialog::new()
            .set_title("Open a reference sequence")
            .add_filter("Reference sequences", &references)
    };
    dialog.pick_file().await.map(|file| file.path().to_owned())
}

pub fn run() -> iced::Result {
    font::FILES
        .into_iter()
        .fold(
            iced::application(Genie::boot, Genie::update, Genie::view),
            |application, (bytes, _)| application.font(bytes),
        )
        .default_font(font::UI)
        .title(Genie::title)
        .window(chrome::settings())
        .theme(Genie::theme)
        .subscription(Genie::subscription)
        .run()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::{Feature, Names, Region, Strand};
    use genie_formats::fai::FaiRecord;
    use genie_formats::features::Kind;
    use genie_formats::tabix::Index;
    use genie_render::features::Mode as Rows;
    use iced::keyboard::{self, key::Named};
    use iced::theme::Mode;
    use iced::time::{Duration, Instant};
    use iced_test::{selector, simulator};

    use iced::{Point, Size};

    use super::{DROP_HINT, Genie, Message, NO_REFERENCE, ZOOM_HINT, shortcut, window_event};
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::shortcuts::Command;
    use crate::theme::Appearance;
    use crate::tiles::Key;
    use crate::track::{Change, Loaded, Source};

    fn source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from(format!("/data/{name}")),
            name: name.to_owned(),
            kind: Kind::Bed,
            index: Index::default(),
        })
    }

    fn with_track() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        genie.session.as_mut().unwrap().tracks[0].contigs = vec![Some("1".to_owned()), None];
        genie
    }

    #[test]
    fn tracks_need_a_reference_first() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Picked(Some(PathBuf::from("/data/genes.bed.gz"))));
        assert!(shows(&genie, NO_REFERENCE));
    }

    #[test]
    fn opened_tracks_show_their_name() {
        assert!(shows(&with_track(), "genes.bed.gz"));
    }

    #[test]
    fn opening_a_track_twice_says_so() {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        assert!(shows(&genie, "The track genes.bed.gz is already open."));
        assert_eq!(genie.session.as_ref().unwrap().tracks.len(), 1);
    }

    #[test]
    fn errors_with_a_reference_open_become_notices() {
        let error = "There is no index genes.bed.gz.tbi. Index the file with tabix.";
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Err(error.to_owned())));
        assert!(shows(&genie, error));
        let _ = genie.update(Message::NoticeDismissed);
        assert!(!shows(&genie, error));
    }

    #[test]
    fn navigating_requests_features_around_the_view() {
        let mut genie = with_track();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.wanted, Some((0, Region::between(0, 1_000))));
    }

    #[test]
    fn loaded_features_fill_the_track() {
        let mut genie = with_track();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let region = Region::between(0, 1_000);
        let feature = Feature::new("a", Region::between(10, 20), Strand::Forward);
        let loaded = Arc::new(Loaded::new(region, vec![feature; 12]));
        let _ = genie.update(Message::TrackLoaded(0, 0, region, Ok(loaded)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.data(0).unwrap().count, 12);
        assert_eq!(track.mode(0), Rows::Squished);
    }

    #[test]
    fn load_errors_show_inside_the_track() {
        let mut genie = with_track();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let error = "The strand is not +, - or ..".to_owned();
        let region = Region::between(0, 1_000);
        let _ = genie.update(Message::TrackLoaded(0, 0, region, Err(error.clone())));
        assert!(shows(&genie, &error));
    }

    #[test]
    fn zoomed_out_tracks_ask_to_zoom_in() {
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 50_000_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        let reference = Reference::fasta(PathBuf::from("big.fa"), records).unwrap();
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(Arc::new(reference))));
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        assert!(shows(&genie, ZOOM_HINT));
    }

    fn reference() -> Arc<Reference> {
        let records = vec![
            FaiRecord {
                name: "chr1".to_owned(),
                length: 1_000,
                offset: 6,
                line_bases: 60,
                line_width: 61,
            },
            FaiRecord {
                name: "chr2".to_owned(),
                length: 500,
                offset: 1_029,
                line_bases: 60,
                line_width: 61,
            },
        ];
        Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap())
    }

    fn go(genie: &mut Genie, locus: &str) {
        let _ = genie.update(Message::LocusEdited(locus.to_owned()));
        let _ = genie.update(Message::LocusSubmitted);
    }

    #[test]
    fn submitting_a_locus_jumps_there() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn submitting_another_contig_switches_to_it() {
        let mut genie = opened();
        go(&mut genie, "2");
        assert!(shows(&genie, "chr2:1-500"));
    }

    #[test]
    fn bad_loci_keep_the_draft_and_explain() {
        let mut genie = opened();
        go(&mut genie, "chr9:1-10");
        assert!(shows(&genie, "chr9:1-10"));
        assert!(shows(&genie, "The assembly has no contig \"chr9\"."));
    }

    #[test]
    fn arrow_keys_pan_a_quarter_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let _ = genie.update(Message::Command(Command::Pan(-0.25)));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:126-625"));
    }

    #[test]
    fn plus_zooms_in_around_the_center() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:251-750"));
    }

    fn settle(genie: &mut Genie) {
        let _ = genie.update(Message::Frame(Instant::now() + Duration::from_secs(1)));
    }

    #[test]
    fn keyboard_zoom_starts_from_the_current_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        assert!(shows(&genie, "chr1:1-1,000"));
        assert!(genie.is_animating());
        settle(&mut genie);
        assert!(!genie.is_animating());
    }

    #[test]
    fn repeated_keys_build_on_the_destination() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:376-625"));
    }

    #[test]
    fn dragging_cancels_a_running_transition() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        let _ = genie.update(Message::Pan(-100.0));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:51-550"));
    }

    #[test]
    fn key_presses_become_commands() {
        let press = keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::ArrowLeft),
            modified_key: keyboard::Key::Named(Named::ArrowLeft),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ArrowLeft),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        };
        assert_eq!(shortcut(press), Some(Message::Command(Command::Pan(0.25))));
        assert_eq!(
            shortcut(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::SHIFT
            )),
            None
        );
    }

    #[test]
    fn navigating_drops_an_unsubmitted_draft() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr1:5".to_owned()));
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        assert!(shows(&genie, "chr1:1-500"));
    }

    #[test]
    fn opened_reference_shows_its_first_contig() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let mut ui = simulator(genie.view());
        assert!(ui.find("chr1:1-1,000").is_ok());
    }

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    #[test]
    fn zoom_and_pan_move_the_locus() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        assert!(shows(&genie, "chr1:1-500"));
        let _ = genie.update(Message::Pan(-100.0));
        assert!(shows(&genie, "chr1:51-550"));
    }

    #[test]
    fn resizing_keeps_the_scale() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let _ = genie.update(Message::Resized(500.0));
        assert!(shows(&genie, "chr1:1-500"));
    }

    fn opened() -> Genie {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        genie
    }

    #[test]
    fn opening_requests_the_visible_tiles() {
        let mut genie = opened();
        assert_eq!(genie.tiles.request(0, Region::between(0, 1_000)), []);
    }

    #[test]
    fn loaded_tiles_feed_the_sequence() {
        let mut genie = opened();
        let tile = Key {
            contig: 0,
            index: 0,
        };
        let bases: Arc<[u8]> = Arc::from(vec![b'A'; 1_000]);
        let _ = genie.update(Message::TileLoaded(genie.generation, tile, Ok(bases)));
        assert_eq!(genie.tiles.chunks(0, Region::between(0, 10)).count(), 1);
    }

    #[test]
    fn tiles_from_an_older_reference_are_ignored() {
        let mut genie = opened();
        let tile = Key {
            contig: 0,
            index: 0,
        };
        let bases: Arc<[u8]> = Arc::from(vec![b'A'; 1_000]);
        let _ = genie.update(Message::TileLoaded(genie.generation - 1, tile, Ok(bases)));
        assert_eq!(genie.tiles.chunks(0, Region::between(0, 10)).count(), 0);
    }

    #[test]
    fn browser_shows_a_labelled_sequence_track() {
        let genie = opened();
        let mut ui = simulator(genie.view());
        let bounds = ui.find(selector::id("sequence")).unwrap().bounds();
        assert!((bounds.height - 20.0).abs() < f32::EPSILON);
        assert!(ui.find("Sequence").is_ok());
    }

    #[test]
    fn browser_stacks_a_thin_overview_and_ruler() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let mut ui = simulator(genie.view());
        for (id, height) in [("overview", 12.0), ("ruler", 20.0)] {
            let bounds = ui.find(selector::id(id)).unwrap().bounds();
            assert!((bounds.height - height).abs() < f32::EPSILON, "{id}");
        }
    }

    #[test]
    fn failed_open_shows_the_error() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Err(
            "Line 1: a field is missing.".to_owned()
        )));
        let mut ui = simulator(genie.view());
        assert!(ui.find("Line 1: a field is missing.").is_ok());
    }

    #[test]
    fn picking_no_file_keeps_the_welcome_view() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Picked(None));
        let mut ui = simulator(genie.view());
        assert!(ui.find("Open a reference sequence to start.").is_ok());
    }

    #[test]
    fn dragging_the_overview_window_moves_the_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(4.0, 0.0));
        let _ = genie.update(Message::OverviewPressed(0.0625));
        let _ = genie.update(Message::OverviewDragged(0.5));
        let _ = genie.update(Message::OverviewReleased);
        assert!(shows(&genie, "chr1:438-688"));
    }

    #[test]
    fn pressing_the_overview_outside_the_window_centers_there() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(4.0, 0.0));
        let _ = genie.update(Message::OverviewPressed(0.75));
        assert!(shows(&genie, "chr1:626-875"));
    }

    #[test]
    fn overview_drags_stop_at_release() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(4.0, 0.0));
        let _ = genie.update(Message::OverviewPressed(0.1));
        let _ = genie.update(Message::OverviewReleased);
        let _ = genie.update(Message::OverviewDragged(0.9));
        assert!(shows(&genie, "chr1:1-250"));
    }

    fn with_recent(paths: &[&str]) -> Genie {
        let mut genie = Genie::default();
        for path in paths.iter().rev() {
            genie.recent.add(PathBuf::from(path));
        }
        genie
    }

    #[test]
    fn welcome_lists_recent_files() {
        let genie = with_recent(&["/data/hg38.2bit", "/data/demo.fa"]);
        let mut ui = simulator(genie.view());
        assert!(ui.find("hg38.2bit").is_ok());
        assert!(ui.find("demo.fa").is_ok());
    }

    #[test]
    fn clicking_a_recent_file_opens_it() {
        let genie = with_recent(&["/data/hg38.2bit"]);
        let mut ui = simulator(genie.view());
        ui.click("hg38.2bit").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::Picked(Some(PathBuf::from("/data/hg38.2bit")))]
        );
    }

    #[test]
    fn picking_a_file_shows_that_it_is_opening() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Picked(Some(PathBuf::from("/data/ref.fa"))));
        assert!(shows(&genie, "Opening ref.fa…"));
    }

    #[test]
    fn dropping_a_file_opens_it() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::FileHovered);
        let _ = genie.update(Message::FileDropped(PathBuf::from("/data/ref.fa")));
        assert!(shows(&genie, "Opening ref.fa…"));
        assert!(!shows(&genie, DROP_HINT));
    }

    #[test]
    fn hovering_a_file_shows_the_drop_hint() {
        let mut genie = opened();
        let _ = genie.update(Message::FileHovered);
        assert!(shows(&genie, DROP_HINT));
        let _ = genie.update(Message::FileHoverEnded);
        assert!(!shows(&genie, DROP_HINT));
    }

    #[test]
    fn window_events_become_messages() {
        let id = iced::window::Id::unique();
        let path = PathBuf::from("/data/ref.fa");
        assert_eq!(
            window_event((id, iced::window::Event::FileDropped(path.clone()))),
            Some(Message::FileDropped(path))
        );
        assert_eq!(
            window_event((id, iced::window::Event::FilesHoveredLeft)),
            Some(Message::FileHoverEnded)
        );
        let size = Size::new(900.0, 600.0);
        assert_eq!(
            window_event((id, iced::window::Event::Resized(size))),
            Some(Message::WindowResized(size))
        );
        assert_eq!(window_event((id, iced::window::Event::Focused)), None);
    }

    fn with_menu() -> Genie {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackOpened(Ok(source("other.gff3.gz"))));
        let _ = genie.update(Message::TrackMenu(1, Point::new(100.0, 200.0)));
        genie
    }

    fn ids(genie: &Genie) -> Vec<u64> {
        let tracks = &genie.session.as_ref().unwrap().tracks;
        tracks.iter().map(|track| track.id).collect()
    }

    #[test]
    fn empty_tracks_keep_their_label() {
        let genie = with_menu();
        let mut ui = simulator(genie.view());
        let label = ui.find("other.gff3.gz").unwrap();
        assert!(label.bounds().width > 40.0);
    }

    #[test]
    fn right_clicking_a_track_opens_its_menu() {
        let genie = with_menu();
        for item in ["Expanded", "Squished", "Collapsed", "Move up", "Remove"] {
            assert!(shows(&genie, item), "{item}");
        }
    }

    #[test]
    fn choosing_a_mode_sets_it_and_closes_the_menu() {
        let mut genie = with_menu();
        let mut ui = simulator(genie.view());
        ui.click("Collapsed").unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        let track = &genie.session.as_ref().unwrap().tracks[1];
        assert_eq!(track.mode, Some(Rows::Collapsed));
        assert!(!shows(&genie, "Squished"));
    }

    #[test]
    fn menu_items_move_and_remove_tracks() {
        let mut genie = with_menu();
        let _ = genie.update(Message::TrackChanged(1, Change::Up));
        assert_eq!(ids(&genie), [1, 0]);
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert_eq!(ids(&genie), [1]);
    }

    #[test]
    fn menus_close_on_escape_or_outside_presses() {
        let mut genie = with_menu();
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "Squished"));
        let _ = genie.update(Message::TrackMenu(1, Point::new(100.0, 200.0)));
        let _ = genie.update(Message::MenuDismissed);
        assert!(!shows(&genie, "Squished"));
    }

    fn with_features() -> Genie {
        let mut genie = with_track();
        let region = Region::between(0, 1_000);
        let feature = Feature::new("BRCA1", Region::between(10, 20), Strand::Reverse);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        track.accept(0, region, Ok(Arc::new(Loaded::new(region, vec![feature]))));
        genie
    }

    fn over(x: f32, y: f32) -> Pointer {
        Pointer {
            local: Point::new(x, y),
            at: Point::new(x, y + 100.0),
        }
    }

    #[test]
    fn hovering_a_feature_shows_its_card() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackHovered(0, Some(over(15.0, 5.0))));
        assert!(shows(&genie, "BRCA1"));
        assert!(shows(&genie, "chr1:11-20"));
        assert!(shows(&genie, "10 bp · reverse strand"));
        let _ = genie.update(Message::TrackHovered(0, None));
        assert!(!shows(&genie, "chr1:11-20"));
    }

    fn with_names() -> Genie {
        let mut genie = with_track();
        let names = Names::new([genie_core::Named {
            name: "BRCA1".to_owned(),
            contig: 0,
            span: Region::between(100, 200),
        }]);
        let _ = genie.update(Message::NamesLoaded(0, Ok(Arc::new(names))));
        genie
    }

    #[test]
    fn typing_a_feature_name_suggests_it() {
        let mut genie = with_names();
        let _ = genie.update(Message::LocusEdited("brc".to_owned()));
        assert!(shows(&genie, "BRCA1"));
        assert!(shows(&genie, "chr1:101"));
    }

    #[test]
    fn choosing_a_feature_suggestion_jumps_to_it() {
        let mut genie = with_names();
        let _ = genie.update(Message::LocusEdited("brc".to_owned()));
        let mut ui = simulator(genie.view());
        ui.click("BRCA1").unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn submitting_a_feature_name_jumps_to_it() {
        let mut genie = with_names();
        go(&mut genie, "brca1");
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn hovering_empty_space_shows_no_card() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackHovered(0, Some(over(500.0, 5.0))));
        assert!(genie.hover.is_none());
    }

    #[test]
    fn clicking_pins_the_card_until_dismissed() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackClicked(0, over(15.0, 5.0)));
        let _ = genie.update(Message::TrackHovered(0, None));
        assert!(shows(&genie, "chr1:11-20"));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "chr1:11-20"));
    }

    #[test]
    fn removing_a_track_drops_its_cards() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackClicked(0, over(15.0, 5.0)));
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert!(genie.pinned.is_none());
    }

    #[test]
    fn menus_open_where_the_pointer_is_on_screen() {
        let mut genie = with_track();
        let _ = genie.update(Message::Scrolled(50.0));
        let _ = genie.update(Message::TrackMenu(0, Point::new(10.0, 300.0)));
        assert_eq!(genie.menu, Some((0, Point::new(10.0, 250.0))));
    }

    #[test]
    fn opening_a_reference_remembers_it() {
        assert_eq!(opened().recent.paths(), [PathBuf::from("ref.fa")]);
    }

    #[test]
    fn typing_a_contig_prefix_lists_matches() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr".to_owned()));
        assert!(shows(&genie, "chr1"));
        assert!(shows(&genie, "chr2"));
    }

    #[test]
    fn typing_a_range_hides_suggestions() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr1:5".to_owned()));
        assert!(!shows(&genie, "chr2"));
    }

    #[test]
    fn clicking_a_suggestion_opens_that_contig() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr".to_owned()));
        let mut ui = simulator(genie.view());
        ui.click("chr2").unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        assert!(shows(&genie, "chr2:1-500"));
    }

    #[test]
    fn arrow_keys_pick_a_suggestion() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr".to_owned()));
        let _ = genie.update(Message::Command(Command::NextSuggestion));
        let _ = genie.update(Message::Command(Command::NextSuggestion));
        let _ = genie.update(Message::LocusSubmitted);
        assert!(shows(&genie, "chr2:1-500"));
    }

    #[test]
    fn previous_wraps_to_the_last_suggestion() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr".to_owned()));
        let _ = genie.update(Message::Command(Command::PreviousSuggestion));
        let _ = genie.update(Message::LocusSubmitted);
        assert!(shows(&genie, "chr2:1-500"));
    }

    #[test]
    fn window_title_names_the_open_reference() {
        assert_eq!(Genie::default().title(), "genie");
        assert_eq!(opened().title(), "ref.fa · genie");
    }

    #[test]
    fn welcome_open_button_asks_for_a_file() {
        let genie = Genie::default();
        let mut ui = simulator(genie.view());
        ui.click("Open reference").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), [Message::Open]);
    }

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
        ui.click(selector::id("appearance")).unwrap();
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
