use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use genie_core::{Locus, Named, Names, Region, Span, Target, Viewport, grouped};
use genie_render::{alignments, features, overview, ruler, sequence};
use iced::futures::channel::mpsc;
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

use crate::card::{self, Card, Detail};
use crate::chrome::{self, Controls, LocusField};
use crate::context::Context;
use crate::export;
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
use crate::track::{self, Change, Content, Loaded, Source, Track};
use crate::{font, paths, scene, style, timer};

const INITIAL_WIDTH: f32 = 1_000.0;
const OVERVIEW_HEIGHT: f32 = 12.0;
const RULER_HEIGHT: f32 = 20.0;
const BASE_HEIGHT: f32 = 16.0;
const TRACK_PADDING: f32 = 2.0;
const EMPTY_HEIGHT: f32 = 20.0;
const LABEL_SIZE: f32 = 10.0;
const CAPTION_HEIGHT: f32 = 22.0;
const CAPTION_SIZE: f32 = 11.0;
const READ_HINT: &str = "Zoom in to see the reads";
const DROP_HINT: &str = "Drop the file to open it";
const ZOOM_HINT: &str = "Zoom in to see the features";
const NO_REFERENCE: &str = "Open a reference sequence before you add tracks.";
const DRAW_FAILED: &str = "The view could not be drawn.";
const FADE: Duration = Duration::from_secs(4);
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
    notice: Option<Notice>,
    notices: u64,
    indexing: Option<(String, u8)>,
    menu: Option<(u64, Point)>,
    scroll: f32,
    window: Size,
    hover: Option<Card>,
    pinned: Option<Card>,
    pending: Vec<PathBuf>,
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
            notices: 0,
            indexing: None,
            menu: None,
            scroll: 0.0,
            window: chrome::WINDOW,
            hover: None,
            pinned: None,
            pending: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Notice {
    id: u64,
    text: String,
    fades: bool,
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
    ExportPicked(Option<PathBuf>),
    Exported(Result<PathBuf, String>),
    NoticeExpired(u64),
    Indexing(String, u8),
}

enum Opening {
    Progress(f32),
    Done(Result<Arc<Reference>, String>),
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
        let mut genie = Self {
            recent,
            ..Self::default()
        };
        let files = std::env::args_os().skip(1).map(PathBuf::from).collect();
        let open = genie.launch(files);
        (
            genie,
            Task::batch([system::theme().map(Message::SystemModeChanged), open]),
        )
    }

    fn launch(&mut self, files: Vec<PathBuf>) -> Task<Message> {
        let (tracks, references): (Vec<PathBuf>, Vec<PathBuf>) =
            files.into_iter().partition(|path| track::is_track(path));
        if let Some(reference) = references.into_iter().next() {
            self.pending = tracks;
            return self.open(reference);
        }
        let tasks: Vec<Task<Message>> = tracks.into_iter().map(|path| self.open(path)).collect();
        Task::batch(tasks)
    }

    fn opened(&mut self, result: Result<Arc<Reference>, String>) -> Task<Message> {
        self.indexing = None;
        let reference = match result {
            Ok(reference) => reference,
            Err(error) => {
                self.pending.clear();
                self.fail(error);
                return Task::none();
            }
        };
        self.status = Status::Idle;
        self.generation += 1;
        self.tiles.clear();
        self.recent.add(reference.path.clone());
        (self.hover, self.pinned) = (None, None);
        self.session = Some(Session::new(reference, self.width));
        let mut tasks = vec![self.fetch(), self.remember()];
        for path in std::mem::take(&mut self.pending) {
            tasks.push(self.open(path));
        }
        Task::batch(tasks)
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
            Message::Opened(result) => return self.opened(result),
            Message::Indexing(name, percent) => self.indexing = Some((name, percent)),
            Message::TrackOpened(Err(error)) => self.fail(error),
            Message::TrackOpened(Ok(source)) => return self.add_track(source),
            Message::TrackLoaded(id, contig, region, result) => {
                self.on_session(|session| session.accept(id, contig, region, result));
            }
            Message::NoticeDismissed => self.notice = None,
            Message::TrackMenu(id, at) => {
                self.menu = Some((id, Point::new(at.x, at.y - self.scroll)));
            }
            Message::TrackChanged(id, change) => return self.change_track(id, change),
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
            Message::ExportPicked(path) => return self.export_to(path),
            Message::Exported(result) => return self.exported(result),
            Message::NoticeExpired(id) => self.expire(id),
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
        let (viewport, x, y) = (session.viewport, pointer.local.x, pointer.local.y);
        let detail = match &data.content {
            Content::Features(annotation) => {
                let index =
                    features::hit(viewport, &annotation.features, &annotation.rows, mode, x, y)?;
                Detail::Feature(annotation.features[index].clone())
            }
            Content::Reads(pile) => {
                let index = alignments::hit(viewport, &pile.reads, &pile.rows, mode, x, y)?;
                Detail::Read(pile.reads[index].alignment.clone())
            }
        };
        Some(Card {
            track: id,
            contig: session.reference.assembly.contigs()[session.contig]
                .name
                .clone(),
            detail,
            at: Point::new(pointer.at.x, pointer.at.y - self.scroll),
        })
    }

    fn change_track(&mut self, id: u64, change: Change) -> Task<Message> {
        self.menu = None;
        if change == Change::Remove {
            self.forget(id);
        }
        self.on_session(|session| track::change(&mut session.tracks, id, change));
        self.fetch_features()
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
            Command::Export => self.export(),
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
        self.hover = None;
        if let Some(session) = &mut self.session {
            session.transition = None;
            session.viewport = change(session.viewport);
        }
        self.fetch()
    }

    fn glide(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        self.hover = None;
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
        let contig = session.contig;
        let length = viewport.length();
        let wanted = visible.expand(visible.len(), length);
        let reference = &session.reference;
        let tasks: Vec<Task<Message>> = session
            .tracks
            .iter_mut()
            .filter(|track| track.needs(contig, visible))
            .filter_map(|track| {
                let name = track.contigs[contig].clone()?;
                track.wanted = Some((contig, wanted));
                let (id, source, options) = (track.id, Arc::clone(&track.source), track.options);
                let reference = Arc::clone(reference);
                Some(Task::perform(
                    async move {
                        let bases =
                            |window: Region| reference.fetch(contig, window.clamp_to(length));
                        track::load(&source, &name, wanted, options, bases).map(Arc::new)
                    },
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
                .is_none_or(|pinned| pinned.detail != hover.detail)
        {
            layers = layers.push(card::view(hover, false, self.window));
        }
        let progress = self
            .session
            .as_ref()
            .and(self.indexing.as_ref())
            .map(|(name, percent)| indexing(name, *percent));
        let notice = self.notice.as_ref().map(|notice| notice.text.as_str());
        if progress.is_some() || notice.is_some() {
            layers = layers.push(toasts(progress, notice));
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
        let message = match (&self.status, &self.indexing) {
            (Status::Idle, _) => text("Open a reference sequence to start.").style(style::muted),
            (Status::Opening(_), Some((name, percent))) => {
                text(indexing(name, *percent)).style(style::muted)
            }
            (Status::Opening(name), None) => text(format!("Opening {name}…")).style(style::muted),
            (Status::Failed(error), _) => text(error.as_str()),
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
        let name = file_name(&path);
        if self.session.is_none() {
            self.status = Status::Opening(name.clone());
        }
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            let mut report = |share| {
                let _ = sender.unbounded_send(Opening::Progress(share));
            };
            let result = reference::open(path, paths::cache_dir().as_deref(), &mut report);
            let _ = sender.unbounded_send(Opening::Done(result.map(Arc::new)));
        });
        Task::run(receiver, move |event| match event {
            Opening::Progress(share) => Message::Indexing(name.clone(), (share * 100.0) as u8),
            Opening::Done(result) => Message::Opened(result),
        })
    }

    fn add_track(&mut self, source: Arc<Source>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        };
        if session.tracks.iter().any(|track| track.source == source) {
            return self.inform(format!("The track {} is already open.", source.name));
        }
        self.notice = None;
        let id = self.next_track;
        self.next_track += 1;
        let track = Track::new(id, Arc::clone(&source), &session.reference.assembly);
        session.tracks.push(track);
        if source.has_reads() {
            return self.fetch_features();
        }
        let reference = Arc::clone(&session.reference);
        let names = Task::perform(
            async move { track::names(&source, &reference.assembly).map(Arc::new) },
            move |result| Message::NamesLoaded(id, result),
        );
        Task::batch([self.fetch_features(), names])
    }

    fn fail(&mut self, error: String) {
        if self.session.is_some() {
            self.warn(error);
        } else {
            self.status = Status::Failed(error);
        }
    }

    fn show(&mut self, text: String, fades: bool) -> u64 {
        self.notices += 1;
        self.notice = Some(Notice {
            id: self.notices,
            text,
            fades,
        });
        self.notices
    }

    fn inform(&mut self, text: String) -> Task<Message> {
        let id = self.show(text, true);
        Task::perform(timer::sleep(FADE), move |()| Message::NoticeExpired(id))
    }

    fn warn(&mut self, text: String) {
        self.show(text, false);
    }

    fn expire(&mut self, id: u64) {
        if self
            .notice
            .as_ref()
            .is_some_and(|notice| notice.id == id && notice.fades)
        {
            self.notice = None;
        }
    }

    fn exported(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        match result {
            Ok(path) => self.inform(format!("Saved {}.", file_name(&path))),
            Err(error) => {
                self.warn(error);
                Task::none()
            }
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

    fn tracks<'a>(&'a self, session: &'a Session, repack: bool) -> Element<'a, Message> {
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
            data = data
                .push(track_view(session, track, repack))
                .push(hairline());
        }
        data.into()
    }

    fn browser<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let viewport = session.viewport;
        let tracks = column![
            container(
                scene::view(overview::scene(viewport, OVERVIEW_HEIGHT), OVERVIEW_HEIGHT)
                    .on_press(Message::OverviewPressed)
                    .on_drag(Message::OverviewDragged)
                    .on_release(Message::OverviewReleased)
            )
            .id("overview"),
            ruler_view(viewport),
            hairline(),
            scrollable(self.tracks(session, false))
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

    fn export_view<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let caption = container(
            row![
                text(&session.reference.name).size(CAPTION_SIZE),
                text("·").size(CAPTION_SIZE).style(style::muted),
                text(session.locus())
                    .font(font::MONO)
                    .size(CAPTION_SIZE)
                    .style(style::muted),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([0, 8])
        .height(CAPTION_HEIGHT)
        .align_y(Alignment::Center);
        column![
            caption,
            hairline(),
            ruler_view(session.viewport),
            hairline(),
            self.tracks(session, true)
        ]
        .into()
    }

    fn export(&self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let contig = &session.reference.assembly.contigs()[session.contig].name;
        let name = export::file_name(contig, session.viewport.visible());
        Task::perform(save_as(name), Message::ExportPicked)
    }

    fn export_to(&mut self, path: Option<PathBuf>) -> Task<Message> {
        let (Some(path), Some(session)) = (path, &self.session) else {
            return Task::none();
        };
        if let Some(image) = export::render(self.export_view(session), self.width, &self.theme()) {
            Task::perform(async move { export::save(path, &image) }, Message::Exported)
        } else {
            self.warn(DRAW_FAILED.to_owned());
            Task::none()
        }
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

fn track_view<'a>(session: &Session, track: &'a Track, repack: bool) -> Element<'a, Message> {
    let viewport = session.viewport;
    let contig = session.contig;
    let id = track.id;
    let interactive = |primitives, height| {
        scene::view(primitives, height)
            .on_hover(move |pointer| Message::TrackHovered(id, pointer))
            .on_click(move |pointer| Message::TrackClicked(id, pointer))
    };
    let mode = track.mode(contig);
    let body: Element<'a, Message> = if let Some(error) = &track.error {
        hint(error.clone())
    } else if viewport.visible().len() > track.source.span() {
        hint(
            if track.source.has_reads() {
                READ_HINT
            } else {
                ZOOM_HINT
            }
            .to_owned(),
        )
    } else {
        match track.data(contig).map(|data| &data.content) {
            Some(Content::Features(annotation)) => {
                let rows = if mode == features::Mode::Collapsed {
                    1
                } else {
                    annotation.count.max(1)
                };
                let primitives = features::scene(
                    viewport,
                    &annotation.features,
                    &annotation.rows,
                    mode,
                    track.hue,
                );
                interactive(primitives, rows as f32 * mode.row_height()).into()
            }
            Some(Content::Reads(pile)) => {
                let depth = alignments::coverage(
                    viewport,
                    &pile.coverage,
                    |at| pile.base(at),
                    alignments::COVERAGE_HEIGHT,
                );
                let mut body = column![scene::view(depth, alignments::COVERAGE_HEIGHT)].spacing(1);
                if mode != features::Mode::Collapsed {
                    let packed;
                    let (rows, count) = if repack {
                        packed = pile.packed(viewport.visible());
                        (packed.0.as_slice(), packed.1)
                    } else {
                        (pile.rows.as_slice(), pile.count)
                    };
                    let primitives = alignments::reads(viewport, &pile.reads, rows, mode);
                    let height = count.max(1) as f32 * alignments::row_height(mode);
                    body = body.push(interactive(primitives, height));
                }
                body.into()
            }
            None => space().width(Length::Fill).height(EMPTY_HEIGHT).into(),
        }
    };
    Context::new(
        stack![
            container(body).padding([TRACK_PADDING, 0.0]),
            label(&track.source.name)
        ],
        move |at| Message::TrackMenu(id, at),
    )
    .into()
}

fn ruler_view<'a>(viewport: Viewport) -> Element<'a, Message> {
    container(scene::view(
        ruler::scene(viewport, RULER_HEIGHT),
        RULER_HEIGHT,
    ))
    .id("ruler")
    .into()
}

fn hint<'a>(message: String) -> Element<'a, Message> {
    container(text(message).size(11).style(style::muted))
        .center_x(Length::Fill)
        .height(EMPTY_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

fn indexing(name: &str, percent: u8) -> String {
    format!("Indexing {name}… {percent} %")
}

fn toasts<'a>(progress: Option<String>, notice: Option<&'a str>) -> Element<'a, Message> {
    let card = |content: Element<'a, Message>| {
        container(content)
            .max_width(360)
            .padding([6, 6])
            .style(style::menu)
    };
    let progress = progress.map(|line| card(text(line).size(12).into()));
    let notice = notice.map(|notice| {
        let close = button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::NoticeDismissed);
        card(
            row![text(notice).size(12).width(Length::Fill), close]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
        )
    });
    container(
        column![]
            .push(progress)
            .push(notice)
            .spacing(6)
            .align_x(Alignment::End),
    )
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
    let references = ["fa", "fasta", "fna", "fas", "gz", "bgz", "2bit"];
    let dialog = if tracks {
        rfd::AsyncFileDialog::new()
            .set_title("Open a track or a reference sequence")
            .add_filter("Tracks", &["gz", "bgz", "bed", "gff", "gff3", "bam"])
            .add_filter("Reference sequences", &references)
    } else {
        rfd::AsyncFileDialog::new()
            .set_title("Open a reference sequence")
            .add_filter("Reference sequences", &references)
    };
    dialog.pick_file().await.map(|file| file.path().to_owned())
}

async fn save_as(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export PNG")
        .set_file_name(name)
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
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
    use crate::export;
    use crate::menu;
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::shortcuts::Command;
    use crate::theme::Appearance;
    use crate::tiles::Key;
    use crate::track::{Change, Format, Loaded, Options, Source, Toggle};

    fn source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from(format!("/data/{name}")),
            name: name.to_owned(),
            format: Format::Features {
                kind: Kind::Bed,
                index: Index::default(),
            },
        })
    }

    fn with_track() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        genie.session.as_mut().unwrap().tracks[0].contigs = vec![Some("1".to_owned()), None];
        genie
    }

    #[test]
    fn command_line_files_open_the_reference_then_the_tracks() {
        let mut genie = Genie::default();
        let track = PathBuf::from("/data/genes.bed.gz");
        let _ = genie.launch(vec![track.clone(), PathBuf::from("/data/ref.fa")]);
        assert!(shows(&genie, "Opening ref.fa…"));
        assert_eq!(genie.pending, [track]);
        let _ = genie.update(Message::Opened(Ok(reference())));
        assert_eq!(genie.pending, [] as [PathBuf; 0]);
    }

    #[test]
    fn indexing_progress_shows_while_opening() {
        let mut genie = Genie::default();
        let _ = genie.launch(vec![PathBuf::from("/data/genome.fa.gz")]);
        let _ = genie.update(Message::Indexing("genome.fa.gz".to_owned(), 45));
        assert!(shows(&genie, "Indexing genome.fa.gz… 45 %"));
        let _ = genie.update(Message::Opened(Ok(reference())));
        assert_eq!(genie.indexing, None);
    }

    #[test]
    fn indexing_another_reference_shows_a_toast() {
        let mut genie = opened();
        let _ = genie.update(Message::Indexing("genome.fa.gz".to_owned(), 7));
        assert!(shows(&genie, "Indexing genome.fa.gz… 7 %"));
        let _ = genie.update(Message::Opened(
            Err("The file has no sequences.".to_owned()),
        ));
        assert!(!shows(&genie, "Indexing genome.fa.gz… 7 %"));
        assert!(shows(&genie, "The file has no sequences."));
    }

    #[test]
    fn command_line_tracks_need_a_reference() {
        let mut genie = Genie::default();
        let _ = genie.launch(vec![PathBuf::from("/data/genes.bed.gz")]);
        assert!(shows(&genie, NO_REFERENCE));
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
        let loaded = Arc::new(Loaded::features(region, vec![feature; 12]));
        let _ = genie.update(Message::TrackLoaded(0, 0, region, Ok(loaded)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.data(0).unwrap().count(), 12);
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
        track.accept(
            0,
            region,
            Ok(Arc::new(Loaded::features(region, vec![feature]))),
        );
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
    fn navigating_hides_the_hover_card() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackHovered(0, Some(over(15.0, 5.0))));
        let _ = genie.update(Message::Pan(-40.0));
        assert!(genie.hover.is_none());
        let _ = genie.update(Message::TrackHovered(0, Some(over(15.0, 5.0))));
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        assert!(genie.hover.is_none());
    }

    fn read(name: &str, start: u64) -> genie_core::alignment::Alignment {
        genie_core::alignment::Alignment {
            name: name.to_owned(),
            start,
            flags: 0,
            mapq: 60,
            cigar: vec![(genie_core::alignment::Op::Match, 40)],
            bases: vec![b'A'; 40],
            qualities: Vec::new(),
        }
    }

    fn add_reads(genie: &mut Genie, reads: Vec<genie_core::alignment::Alignment>) {
        let source = Arc::new(Source {
            path: PathBuf::from("/data/reads.bam"),
            name: "reads.bam".to_owned(),
            format: Format::Reads {
                index: genie_formats::bai::Index::default(),
                contigs: vec!["chr1".to_owned()],
            },
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        let loaded = Loaded::reads(region, reads, vec![b'A'; 1_000], 0, Options::default());
        track.accept(0, region, Ok(Arc::new(loaded)));
    }

    #[test]
    fn read_menus_offer_filter_toggles() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for item in ["Show duplicates", "Show soft clips", "Down-sample"] {
            assert!(shows(&genie, item), "{item}");
        }
    }

    #[test]
    fn toggling_a_filter_reloads_the_reads() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackChanged(0, Change::Toggle(Toggle::Duplicates)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.options.duplicates);
        assert!(track.wanted.is_some());
    }

    #[test]
    fn menus_know_their_height() {
        for mut genie in [with_track(), with_reads()] {
            let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
            let expected = menu::height(&genie.session.as_ref().unwrap().tracks[0]);
            let mut ui = simulator(genie.view());
            let height = ui.find(selector::id("menu")).unwrap().bounds().height;
            assert!(
                height <= expected && expected - height < 1.0,
                "{height} {expected}"
            );
        }
    }

    fn with_reads() -> Genie {
        let mut genie = opened();
        add_reads(&mut genie, vec![read("read7", 10)]);
        genie
    }

    #[test]
    fn exports_pack_only_the_reads_in_view() {
        let mut genie = opened();
        go(&mut genie, "chr1:1-100");
        settle(&mut genie);
        let reads = [10, 500, 510, 520]
            .map(|start| read("read", start))
            .to_vec();
        add_reads(&mut genie, reads);
        let session = genie.session.as_ref().unwrap();
        let theme = genie.theme();
        let live = export::render(genie.tracks(session, false), genie.width, &theme).unwrap();
        let packed = export::render(genie.tracks(session, true), genie.width, &theme).unwrap();
        assert_eq!(live.height - packed.height, 2 * 2 * 10);
    }

    #[test]
    fn hovering_a_read_shows_its_card() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackHovered(0, Some(over(20.0, 5.0))));
        assert!(shows(&genie, "read7"));
        assert!(shows(&genie, "CIGAR 40M"));
        assert!(shows(&genie, "40 bp · forward strand · MAPQ 60"));
    }

    #[test]
    fn only_feature_tracks_offer_colors() {
        let mut reads = with_reads();
        let _ = reads.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&reads, "Squished"));
        assert!(
            simulator(reads.view())
                .find(selector::id("swatches"))
                .is_err()
        );
        let mut features = with_track();
        let _ = features.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(
            simulator(features.view())
                .find(selector::id("swatches"))
                .is_ok()
        );
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

    #[test]
    fn export_view_has_a_locus_line_and_every_track() {
        let genie = with_track();
        let session = genie.session.as_ref().unwrap();
        let mut ui = simulator(genie.export_view(session));
        for text in ["ref.fa", "chr1:1-1,000", "Sequence", "genes.bed.gz"] {
            assert!(ui.find(text).is_ok(), "{text}");
        }
        assert!(ui.find(selector::id("overview")).is_err());
    }

    #[test]
    fn export_renders_at_twice_the_width() {
        let genie = with_track();
        let session = genie.session.as_ref().unwrap();
        let image = export::render(genie.export_view(session), genie.width, &genie.theme());
        let image = image.unwrap();
        assert_eq!(image.width, 2_000);
        assert!(image.height > 100);
    }

    fn notice_id(genie: &Genie) -> u64 {
        genie.notice.as_ref().unwrap().id
    }

    #[test]
    fn info_notices_fade_but_errors_stay() {
        let mut genie = opened();
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/a.png"))));
        let _ = genie.update(Message::NoticeExpired(notice_id(&genie)));
        assert!(genie.notice.is_none());
        let _ = genie.update(Message::Exported(Err("The disk is full.".to_owned())));
        let _ = genie.update(Message::NoticeExpired(notice_id(&genie)));
        assert!(shows(&genie, "The disk is full."));
    }

    #[test]
    fn old_timers_leave_newer_notices() {
        let mut genie = opened();
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/a.png"))));
        let first = notice_id(&genie);
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/b.png"))));
        let _ = genie.update(Message::NoticeExpired(first));
        assert!(shows(&genie, "Saved b.png."));
    }

    #[test]
    fn saved_exports_are_announced() {
        let mut genie = opened();
        let path = PathBuf::from("/data/chr1_1-1000.png");
        let _ = genie.update(Message::Exported(Ok(path)));
        assert!(shows(&genie, "Saved chr1_1-1000.png."));
        let _ = genie.update(Message::Exported(Err("The disk is full.".to_owned())));
        assert!(shows(&genie, "The disk is full."));
    }
}
