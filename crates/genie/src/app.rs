use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use genie_core::{History, Locus, Marks, Named, Names, Region, Span, Target, Viewport, Visit};
use iced::theme::Mode;
use iced::time::Instant;
use iced::{
    Animation, Event, Point, Size, Subscription, Task, Theme, event, keyboard, mouse, system,
    window,
};

use crate::card::Card;
use crate::chrome::{self, Action};
use crate::motion::{self, Transition};
use crate::reading::{self, Reading};
use crate::recent::Recent;
use crate::reference::Reference;
use crate::saved::Saved;
use crate::scene::Pointer;
use crate::settings::{Setting, Settings};
use crate::shortcuts::{self, Command};
use crate::theme;
use crate::tiles::{Key, Tiles};
use crate::track::{self, Change, Loaded, Source, Track};
use crate::{font, paths, timer};

mod export;
mod navigation;
mod regions;
mod sessions;
mod tracks;
mod view;

use tracks::pick;

const INITIAL_WIDTH: f32 = 1_000.0;
const FADE: Duration = Duration::from_secs(4);
const LAST: &str = "last.genie";

#[derive(Debug)]
pub struct Genie {
    settings: Settings,
    settings_open: bool,
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
    restoring: Option<Saved>,
    waiting: usize,
    last: Option<Saved>,
    diagnostics: bool,
    stats: Stats,
    menu: Option<(u64, Point)>,
    sequence_menu: Option<Point>,
    resize: Option<(u64, f32)>,
    moving: Option<(u64, f32)>,
    scroll: f32,
    window: Size,
    hover: Option<Card>,
    pinned: Option<Card>,
    pending: Vec<PathBuf>,
    sweep: Option<(f64, f64)>,
    pointed: Option<u64>,
    anchor: Option<(Visit, u64)>,
    sidebar: Option<Sidebar>,
    slide: Animation<bool>,
    now: Instant,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Sidebar {
    chosen: Option<u64>,
    editing: Option<(u64, String)>,
}

impl Default for Genie {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            settings_open: false,
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
            restoring: None,
            waiting: 0,
            last: None,
            diagnostics: false,
            stats: Stats::default(),
            menu: None,
            sequence_menu: None,
            resize: None,
            moving: None,
            scroll: 0.0,
            window: chrome::WINDOW,
            hover: None,
            pinned: None,
            pending: Vec::new(),
            sweep: None,
            pointed: None,
            anchor: None,
            sidebar: None,
            slide: motion::slide(false),
            now: Instant::now(),
        }
    }
}

#[derive(Debug, Default)]
struct Stats {
    update: Duration,
    view: Cell<Duration>,
    frame: Option<Duration>,
    last_frame: Option<Instant>,
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
    reading: Reading,
    history: History,
    marks: Marks,
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
            reading: Reading::default(),
            history: History::default(),
            marks: Marks::default(),
        }
    }

    fn visit(&self) -> Visit {
        Visit {
            contig: self.contig,
            region: self.destination().visible(),
        }
    }

    fn show(&mut self, target: Target) {
        if target.contig == self.contig {
            self.glide(self.destination().jump(target.span));
        } else {
            let length = self.reference.assembly.contigs()[target.contig].length;
            self.transition = None;
            self.contig = target.contig;
            self.viewport = self.viewport.retarget(length).jump(target.span);
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
    Overview(Sweep),
    Ruler(Sweep),
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
    Jump(u64, bool),
    Jumped(u64, bool, Result<Option<(usize, Region)>, String>),
    Regions(Regions),
    CardClosed,
    NamesLoaded(u64, Result<Arc<Names>, String>),
    Looked(Option<Named>, String),
    ExportPicked(Option<PathBuf>),
    SessionPicked(Option<PathBuf>),
    Exported(Result<PathBuf, String>, bool),
    NoticeExpired(u64),
    Indexing(String, u8),
    SequenceMenu(Point),
    SequenceChanged(reading::Change),
    ToggleSettings,
    SettingChanged(Setting),
    Lane(u64, Lane),
    RestoreLast,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Regions {
    Toggle,
    Add,
    Choose(u64),
    Edit(u64),
    Draft(String),
    Rename,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sweep {
    Pressed(f32),
    Dragged(f32),
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lane {
    Resized(f32),
    Released,
    Fit,
    Moved(f32),
    Dropped,
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
        let folder = paths::config_dir();
        let mut genie = Self {
            recent: folder.as_deref().map(Recent::load).unwrap_or_default(),
            settings: folder.as_deref().map(Settings::load).unwrap_or_default(),
            last: folder
                .as_deref()
                .and_then(|folder| Saved::load(&folder.join(LAST)).ok()),
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
        self.resume();
        let mut tasks = vec![self.fetch(), self.remember()];
        let pending = std::mem::take(&mut self.pending);
        self.waiting = pending.len();
        if self.waiting == 0 {
            self.restoring = None;
        }
        for path in pending {
            tasks.push(self.open(path));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let started = Instant::now();
        let task = self.handle(message);
        self.stats.update = started.elapsed();
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CycleAppearance => {
                self.settings.appearance = self.settings.appearance.next();
                return self.save_settings();
            }
            Message::SystemModeChanged(mode) => self.system = mode,
            Message::Window(action) => return self.window(action),
            Message::Open => return Task::perform(pick(self.session.is_some()), Message::Picked),
            Message::Picked(Some(path)) => return self.open(path),
            Message::Picked(None) | Message::NamesLoaded(_, Err(_)) => {}
            Message::NamesLoaded(id, Ok(names)) => {
                self.on_session(|session| session.name(id, names));
            }
            Message::Looked(found, error) => return self.looked(found, error),
            Message::FileHovered => self.hovering = true,
            Message::FileHoverEnded => self.hovering = false,
            Message::FileDropped(path) => {
                self.hovering = false;
                return self.open(path);
            }
            Message::Opened(result) => return self.opened(result),
            Message::Indexing(name, percent) => self.indexing = Some((name, percent)),
            Message::TrackOpened(result) => return self.track_opened(result),
            Message::TrackLoaded(id, contig, region, result) => {
                self.on_session(|session| session.accept(id, contig, region, result));
            }
            Message::NoticeDismissed => self.notice = None,
            Message::TrackMenu(id, at) => self.open_menu(id, at),
            Message::TrackChanged(id, change) => return self.change_track(id, change),
            Message::TrackHovered(id, pointer) => self.hovered(id, pointer),
            Message::TrackClicked(id, pointer) => self.pinned = self.card(id, pointer),
            Message::Jump(id, forward) => return self.jump(id, forward),
            Message::Jumped(id, forward, result) => return self.jumped(id, forward, result),
            Message::Regions(action) => return self.regions(action),
            Message::CardClosed => self.pinned = None,
            Message::MenuDismissed => self.close_menus(),
            Message::Scrolled(offset) => {
                self.scroll = offset;
                self.close_menus();
            }
            Message::SequenceMenu(at) => {
                self.sequence_menu = Some(Point::new(at.x, at.y - self.scroll));
            }
            Message::SequenceChanged(change) => self.change_reading(change),
            Message::ToggleSettings => self.settings_open = !self.settings_open,
            Message::Lane(id, lane) => self.lane(id, lane),
            Message::RestoreLast => return self.restore_last(),
            Message::SettingChanged(setting) => return self.change_setting(setting),
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
                self.tile_loaded(generation, key, result);
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
            Message::Frame(now) => self.frame(now),
            Message::Overview(sweep) => return self.sweep_overview(sweep),
            Message::Ruler(sweep) => return self.sweep_ruler(sweep),
            Message::ExportPicked(path) => return self.export_to(path),
            Message::SessionPicked(path) => return self.session_picked(path),
            Message::Exported(result, cut) => return self.exported(result, cut),
            Message::NoticeExpired(id) => self.expire(id),
        }
        Task::none()
    }

    fn on_session(&mut self, change: impl FnOnce(&mut Session)) {
        if let Some(session) = &mut self.session {
            change(session);
        }
    }

    fn frame(&mut self, now: Instant) {
        if let Some(last) = self.stats.last_frame {
            let interval = now.saturating_duration_since(last);
            if interval < Duration::from_secs(1) {
                self.stats.frame = Some(interval);
            }
        }
        self.stats.last_frame = Some(now);
        self.now = now;
        self.on_session(|session| session.advance(now));
    }

    fn is_animating(&self) -> bool {
        self.slide.is_animating(self.now)
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.transition.is_some())
    }

    fn fail(&mut self, error: String) {
        log::warn!("{error}");
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

    fn window(&self, action: Action) -> Task<Message> {
        if action == Action::Close {
            return self.save_last().chain(chrome::perform(action));
        }
        chrome::perform(action)
    }

    fn title(&self) -> String {
        match &self.session {
            Some(session) => format!("{} · genie", session.reference.name),
            None => "genie".to_owned(),
        }
    }

    fn scale_factor(&self) -> f32 {
        f32::from(self.settings.scale) / 100.0
    }

    fn theme(&self) -> Theme {
        if self.settings.appearance.is_dark(self.system) {
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
            event::listen().filter_map(|event| mouse_button(&event)),
            window::events().filter_map(window_event),
            frames,
        ])
    }
}

fn window_event((_, event): (window::Id, window::Event)) -> Option<Message> {
    match event {
        window::Event::FileHovered(_) => Some(Message::FileHovered),
        window::Event::FilesHoveredLeft => Some(Message::FileHoverEnded),
        window::Event::CloseRequested => Some(Message::Window(Action::Close)),
        window::Event::FileDropped(path) => Some(Message::FileDropped(path)),
        window::Event::Opened { size, .. } | window::Event::Resized(size) => {
            Some(Message::WindowResized(size))
        }
        _ => None,
    }
}

fn mouse_button(event: &Event) -> Option<Message> {
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => {
            Some(Message::Command(Command::Back))
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
            Some(Message::Command(Command::Forward))
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
        .scale_factor(Genie::scale_factor)
        .theme(Genie::theme)
        .subscription(Genie::subscription)
        .run()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::signal::Summary;
    use genie_core::variant::{Call, Variant};
    use genie_core::{Feature, Names, Region, Strand};
    use genie_formats::fai::FaiRecord;
    use genie_formats::features::Kind;
    use genie_formats::tabix::Index;
    use genie_render::features::Mode as Rows;
    use genie_render::scene::Hue;
    use genie_render::signal::{Scale, Style};
    use iced::keyboard::{self, key::Named};
    use iced::theme::Mode;
    use iced::time::{Duration, Instant};
    use iced_test::{selector, simulator};

    use iced::{Point, Size};

    use super::tracks::NO_REFERENCE;
    use super::view::{DROP_HINT, SIGNAL_HINT, VARIANT_HINT, ZOOM_HINT};
    use super::{Genie, Lane, Message, Regions, Sweep, mouse_button, shortcut, window_event};
    use crate::export;
    use crate::menu;
    use crate::reading::{self, Frames};
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::settings::{Density, Setting};
    use crate::shortcuts::Command;
    use crate::theme::Appearance;
    use crate::tiles::Key;
    use crate::track::{
        Change, Content, FeatureFile, Format, Grouping, Loaded, Options, ReadFile, SignalFile,
        Source, Toggle, VariantFile,
    };

    fn source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from(format!("/data/{name}")),
            name: name.to_owned(),
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: Index::default(),
            }),
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
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        assert!(shows(&genie, VARIANT_HINT));
        let indexed = Format::Signal(SignalFile::BedGraph {
            index: Index::default(),
        });
        let _ = genie.update(Message::TrackOpened(Ok(signal_source(indexed))));
        assert!(shows(&genie, SIGNAL_HINT));
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
    fn back_and_forward_step_through_jumps_but_not_pans() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        go(&mut genie, "chr2:51-150");
        settle(&mut genie);
        let _ = genie.update(Message::Pan(-100.0));
        let _ = genie.update(Message::Command(Command::Back));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-200"));
        let _ = genie.update(Message::Command(Command::Forward));
        settle(&mut genie);
        assert!(shows(&genie, "chr2:61-160"));
        let _ = genie.update(Message::Command(Command::Forward));
        settle(&mut genie);
        assert!(shows(&genie, "chr2:61-160"));
    }

    #[test]
    fn dragging_on_the_ruler_zooms_to_the_range() {
        let mut genie = opened();
        let _ = genie.update(Message::Ruler(Sweep::Pressed(0.3)));
        let _ = genie.update(Message::Ruler(Sweep::Dragged(0.2)));
        let _ = genie.update(Message::Ruler(Sweep::Dragged(0.1)));
        let (start, end) = genie.sweep.unwrap();
        assert!((start - 300.0).abs() < 0.01 && (end - 100.0).abs() < 0.01);
        let _ = genie.update(Message::Ruler(Sweep::Released));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-300"));
        assert_eq!(genie.sweep, None);
        let _ = genie.update(Message::Command(Command::Back));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:1-1,000"));
    }

    #[test]
    fn short_ruler_drags_and_escape_keep_the_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Ruler(Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(Sweep::Dragged(0.101)));
        let _ = genie.update(Message::Ruler(Sweep::Released));
        let _ = genie.update(Message::Ruler(Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(Sweep::Dragged(0.5)));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert_eq!(genie.sweep, None);
        let _ = genie.update(Message::Ruler(Sweep::Released));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:1-1,000"));
    }

    #[test]
    fn feature_jumps_center_short_features_and_fit_wide_ones() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        let near = Ok(Some((0, Region::between(500, 520))));
        let _ = genie.update(Message::Jumped(0, true, near));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:461-561"));
        let wide = Ok(Some((1, Region::between(100, 400))));
        let _ = genie.update(Message::Jumped(0, true, wide));
        settle(&mut genie);
        assert!(shows(&genie, "chr2:101-400"));
        let _ = genie.update(Message::Command(Command::Back));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:461-561"));
    }

    #[test]
    fn missing_neighbors_say_so() {
        let mut genie = with_track();
        let _ = genie.update(Message::Jumped(0, true, Ok(None)));
        assert!(shows(&genie, "There is no next feature in genes.bed.gz."));
        let mut genie = with_variants();
        let _ = genie.update(Message::Jumped(0, false, Ok(None)));
        assert!(shows(&genie, "There is no previous variant in calls.vcf."));
    }

    #[test]
    fn feature_jumps_go_on_from_the_last_feature_until_the_view_moves() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        assert_eq!(genie.jump_from(), Some(150));
        let near = Ok(Some((0, Region::between(500, 520))));
        let _ = genie.update(Message::Jumped(0, true, near));
        settle(&mut genie);
        assert_eq!(genie.jump_from(), Some(500));
        let _ = genie.update(Message::Pan(-100.0));
        assert_eq!(genie.jump_from(), Some(520));
    }

    #[test]
    fn hovering_or_a_menu_points_at_a_track() {
        let mut genie = with_track();
        assert_eq!(genie.pointed, None);
        let _ = genie.update(Message::TrackHovered(0, Some(over(10.0, 10.0))));
        assert_eq!(genie.pointed, Some(0));
        genie.pointed = None;
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert_eq!(genie.pointed, Some(0));
    }

    #[test]
    fn feature_and_variant_menus_offer_jumps() {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("Next feature").unwrap();
        assert!(
            ui.into_messages()
                .any(|message| message == Message::Jump(0, true))
        );
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Previous variant"));
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(!shows(&genie, "Next feature"));
    }

    fn chosen(genie: &Genie) -> Option<u64> {
        genie.sidebar.as_ref().and_then(|sidebar| sidebar.chosen)
    }

    fn marks(genie: &Genie) -> Vec<genie_core::Mark> {
        genie.session.as_ref().unwrap().marks.all().to_vec()
    }

    #[test]
    fn adding_the_view_opens_the_region_list() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        assert!(genie.sidebar.is_some());
        let added = marks(&genie);
        assert_eq!(added.len(), 1);
        assert_eq!(
            (added[0].contig, added[0].span),
            (0, Region::between(100, 200))
        );
        assert_eq!(chosen(&genie), Some(added[0].id));
        assert!(shows(&genie, "Region 1"));
        go(&mut genie, "chr2:1-100");
        settle(&mut genie);
        assert!(shows(&genie, "100 bp"));
        assert!(shows(&genie, "chr1:101-200"));
        let _ = genie.update(Message::Regions(Regions::Toggle));
        settle(&mut genie);
        assert!(genie.sidebar.is_none());
        assert!(!shows(&genie, "Region 1"));
    }

    #[test]
    fn choosing_a_region_jumps_to_it() {
        let mut genie = opened();
        go(&mut genie, "chr2:51-150");
        let _ = genie.update(Message::Command(Command::AddRegion));
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        genie.sidebar.as_mut().unwrap().chosen = None;
        let _ = genie.update(Message::Regions(Regions::Choose(id)));
        settle(&mut genie);
        assert!(shows(&genie, "chr2:51-150"));
        assert_eq!(chosen(&genie), Some(id));
        let _ = genie.update(Message::Command(Command::Back));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn regions_can_be_renamed_and_removed() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        let _ = genie.update(Message::Regions(Regions::Edit(id)));
        let draft = Regions::Draft("promoter A".to_owned());
        let _ = genie.update(Message::Regions(draft));
        assert!(shows(&genie, "promoter A"));
        let _ = genie.update(Message::Regions(Regions::Rename));
        assert_eq!(marks(&genie)[0].name, "promoter A");
        assert!(shows(&genie, "promoter A"));
        let _ = genie.update(Message::Regions(Regions::Remove));
        assert_eq!(marks(&genie).len(), 0);
        assert_eq!(chosen(&genie), None);
    }

    #[test]
    fn double_clicking_a_region_starts_a_rename() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        let mut ui = simulator(genie.view());
        ui.click("Region 1").unwrap();
        ui.click("Region 1").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Regions(Regions::Choose(id))));
        assert!(messages.contains(&Message::Regions(Regions::Edit(id))));
    }

    #[test]
    fn the_sidebar_slides_in_and_out() {
        let later = |genie: &Genie| Message::Frame(genie.now + Duration::from_secs(1));
        let mut genie = opened();
        let _ = genie.update(Message::Regions(Regions::Toggle));
        assert!(genie.is_animating());
        assert!(genie.sidebar_width() < 1.0);
        let _ = genie.update(later(&genie));
        assert!(!genie.is_animating());
        assert!((genie.sidebar_width() - 220.0).abs() < 0.01);
        let _ = genie.update(Message::Regions(Regions::Toggle));
        assert!(genie.sidebar.is_none());
        assert!(genie.sidebar_width() > 219.0);
        let _ = genie.update(later(&genie));
        assert!(genie.sidebar_width() < 0.01);
        genie.settings.motion = false;
        let _ = genie.update(Message::Regions(Regions::Toggle));
        assert!(!genie.is_animating());
        assert!((genie.sidebar_width() - 220.0).abs() < 0.01);
    }

    #[test]
    fn an_empty_region_list_says_how_to_add_regions() {
        let mut genie = opened();
        let _ = genie.update(Message::Regions(Regions::Toggle));
        settle(&mut genie);
        let keys = crate::chrome::keys('D');
        assert!(shows(
            &genie,
            &format!("No regions. Push {keys} to add the view.")
        ));
    }

    #[test]
    fn mouse_side_buttons_step_through_history() {
        let press = |button| iced::Event::Mouse(iced::mouse::Event::ButtonPressed(button));
        assert_eq!(
            mouse_button(&press(iced::mouse::Button::Back)),
            Some(Message::Command(Command::Back))
        );
        assert_eq!(
            mouse_button(&press(iced::mouse::Button::Forward)),
            Some(Message::Command(Command::Forward))
        );
        assert_eq!(mouse_button(&press(iced::mouse::Button::Left)), None);
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

    #[test]
    fn the_gear_opens_settings_that_apply_at_once() {
        let mut genie = opened();
        let _ = genie.update(Message::ToggleSettings);
        assert!(shows(&genie, "UI scale"));
        assert!(shows(&genie, "100 %"));
        let _ = genie.update(Message::SettingChanged(Setting::Scale(1)));
        assert_eq!(genie.settings.scale, 110);
        assert!(shows(&genie, "110 %"));
        let density = Setting::Density(Density::Comfortable);
        let _ = genie.update(Message::SettingChanged(density));
        let _ = genie.update(Message::SettingChanged(Setting::Motion(false)));
        assert_eq!(genie.settings.density, Density::Comfortable);
        assert!(!genie.settings.motion);
        assert!(shows(&genie, "UI scale"));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "UI scale"));
    }

    #[test]
    fn settings_panel_sits_under_the_gear() {
        let mut genie = opened();
        let _ = genie.update(Message::ToggleSettings);
        let mut ui = simulator(genie.view());
        let gear = ui.find(selector::id("settings")).unwrap().bounds();
        let panel = ui.find(selector::id("settings-panel")).unwrap().bounds();
        let edge = gear.x + gear.width;
        assert!(
            (panel.x + panel.width - edge).abs() < 1.0,
            "{panel:?} {gear:?}"
        );
        assert!(panel.y >= gear.y + gear.height);
    }

    #[test]
    fn without_motion_keys_jump_at_once() {
        let mut genie = opened();
        genie.settings.motion = false;
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        assert!(genie.session.as_ref().unwrap().transition.is_none());
        assert!(shows(&genie, "chr1:251-750"));
    }

    #[test]
    fn comfortable_density_pads_tracks() {
        let mut genie = opened();
        genie.settings.density = Density::Comfortable;
        let mut ui = simulator(genie.view());
        let bounds = ui.find(selector::id("sequence")).unwrap().bounds();
        assert!((bounds.height - 28.0).abs() < 0.5, "{}", bounds.height);
    }

    #[test]
    fn scale_factor_follows_the_setting() {
        let mut genie = Genie::default();
        assert!((genie.scale_factor() - 1.0).abs() < f32::EPSILON);
        genie.settings.scale = 125;
        assert!((genie.scale_factor() - 1.25).abs() < f32::EPSILON);
    }

    fn settle(genie: &mut Genie) {
        let later = genie.now.max(Instant::now()) + Duration::from_secs(1);
        let _ = genie.update(Message::Frame(later));
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
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.0625)));
        let _ = genie.update(Message::Overview(Sweep::Dragged(0.5)));
        let _ = genie.update(Message::Overview(Sweep::Released));
        assert!(shows(&genie, "chr1:438-688"));
    }

    #[test]
    fn pressing_the_overview_outside_the_window_centers_there() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(4.0, 0.0));
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.75)));
        assert!(shows(&genie, "chr1:626-875"));
    }

    #[test]
    fn overview_drags_stop_at_release() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(4.0, 0.0));
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Overview(Sweep::Released));
        let _ = genie.update(Message::Overview(Sweep::Dragged(0.9)));
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
        assert_eq!(
            window_event((id, iced::window::Event::CloseRequested)),
            Some(Message::Window(crate::chrome::Action::Close))
        );
    }

    #[test]
    fn tracks_opened_after_a_restore_keep_their_moves() {
        let saved_track = |name: &str| crate::saved::SavedTrack {
            path: PathBuf::from(format!("/data/{name}")),
            mode: None,
            hue: Hue::Blue,
            height: None,
            options: crate::track::Options::default(),
            plot: crate::track::Plot::default(),
        };
        let saved = crate::saved::Saved {
            reference: PathBuf::from("ref.fa"),
            locus: None,
            reading: crate::reading::Reading::default(),
            tracks: vec![saved_track("a.bed.gz"), saved_track("b.bed.gz")],
        };
        let mut genie = Genie::default();
        let _ = genie.restore(saved);
        let _ = genie.update(Message::Opened(Ok(reference())));
        let _ = genie.update(Message::TrackOpened(Ok(source("b.bed.gz"))));
        let _ = genie.update(Message::TrackOpened(Ok(source("a.bed.gz"))));
        let names = |genie: &Genie| -> Vec<String> {
            let session = genie.session.as_ref().unwrap();
            session
                .tracks
                .iter()
                .map(|track| track.source.name.clone())
                .collect()
        };
        assert_eq!(names(&genie), ["a.bed.gz", "b.bed.gz"]);
        let b = genie.session.as_ref().unwrap().tracks[1].id;
        let _ = genie.update(Message::TrackChanged(b, Change::Up));
        let _ = genie.update(Message::TrackOpened(Ok(source("c.bed.gz"))));
        assert_eq!(names(&genie), ["b.bed.gz", "a.bed.gz", "c.bed.gz"]);
        assert!(genie.restoring.is_none());
    }

    #[test]
    fn welcome_offers_to_restore_the_last_session() {
        let last = crate::saved::Saved {
            reference: PathBuf::from("/data/hg38.fa"),
            locus: Some("chr1:1,000-2,000".to_owned()),
            reading: crate::reading::Reading::default(),
            tracks: vec![crate::saved::SavedTrack {
                path: PathBuf::from("/data/reads.bam"),
                mode: None,
                hue: Hue::Blue,
                height: None,
                options: crate::track::Options::default(),
                plot: crate::track::Plot::default(),
            }],
        };
        let mut genie = Genie {
            last: Some(last),
            ..Genie::default()
        };
        let mut ui = simulator(genie.view());
        assert!(ui.find("hg38.fa · chr1:1,000-2,000 · 1 track").is_ok());
        ui.click("Restore last session").unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        assert_eq!(genie.pending, [PathBuf::from("/data/reads.bam")]);
        assert!(genie.restoring.is_some());
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

    fn with_rows(count: usize) -> Genie {
        let mut genie = with_track();
        let region = Region::between(0, 1_000);
        let features = (0..count)
            .map(|index| {
                Feature::new(
                    format!("f{index}"),
                    Region::between(10, 20),
                    Strand::Forward,
                )
            })
            .collect();
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        track.accept(0, region, Ok(Arc::new(Loaded::features(region, features))));
        genie
    }

    fn first_height(genie: &Genie) -> Option<f32> {
        genie.session.as_ref().unwrap().tracks[0].height
    }

    fn reference_at(path: PathBuf) -> Arc<Reference> {
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 1_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        Arc::new(Reference::fasta(path, records).unwrap())
    }

    #[test]
    fn opening_a_session_restores_the_view_and_tracks() {
        let scratch = crate::scratch::Scratch::new("restore");
        let text = "[session]\nreference = ref.fa\nlocus = chr1:101-200\nframes = 3\n\
                    [track]\nfile = genes.bed.gz\ncolor = orange\nheight = 50\n";
        let file = scratch.file("work.genie", text);
        let folder = file.parent().unwrap().to_owned();
        let mut genie = Genie::default();
        let _ = genie.update(Message::Picked(Some(file.clone())));
        assert_eq!(genie.pending, [folder.join("genes.bed.gz")]);
        assert_eq!(genie.recent.paths()[0], file);
        let _ = genie.update(Message::Opened(Ok(reference_at(folder.join("ref.fa")))));
        assert!(shows(&genie, "chr1:101-200"));
        let source = Arc::new(Source {
            path: folder.join("genes.bed.gz"),
            name: "genes.bed.gz".to_owned(),
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: Index::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.reading.frames, Frames::Three);
        assert_eq!(session.tracks[0].hue, Hue::Orange);
        assert_eq!(session.tracks[0].height, Some(50.0));
    }

    #[test]
    fn the_saved_view_lists_the_reference_and_tracks() {
        let genie = with_track();
        let saved = genie.saved().unwrap();
        assert_eq!(saved.reference, PathBuf::from("ref.fa"));
        assert_eq!(saved.locus.as_deref(), Some("chr1:1-1,000"));
        let paths: Vec<PathBuf> = saved
            .tracks
            .iter()
            .map(|track| track.path.clone())
            .collect();
        assert_eq!(paths, [PathBuf::from("/data/genes.bed.gz")]);
    }

    #[test]
    fn saved_sessions_are_announced() {
        let scratch = crate::scratch::Scratch::new("save-session");
        let file = scratch.file("view", "");
        let mut genie = with_track();
        let _ = genie.update(Message::SessionPicked(Some(file.clone())));
        assert!(shows(&genie, "Saved view.genie."));
        let written = std::fs::read_to_string(file.with_file_name("view.genie")).unwrap();
        assert!(written.contains("file = /data/genes.bed.gz"), "{written}");
    }

    #[test]
    fn dragging_a_label_moves_the_track() {
        let mut genie = with_menu();
        let _ = genie.update(Message::MenuDismissed);
        let _ = genie.update(Message::Lane(1, Lane::Moved(-25.0)));
        assert!(simulator(genie.view()).find(selector::id("drop")).is_ok());
        let _ = genie.update(Message::Lane(1, Lane::Dropped));
        assert_eq!(ids(&genie), [1, 0]);
        assert!(simulator(genie.view()).find(selector::id("drop")).is_err());
    }

    #[test]
    fn short_label_drags_keep_the_order() {
        let mut genie = with_menu();
        let _ = genie.update(Message::Lane(0, Lane::Moved(5.0)));
        let _ = genie.update(Message::Lane(0, Lane::Dropped));
        assert_eq!(ids(&genie), [0, 1]);
        let _ = genie.update(Message::Lane(0, Lane::Moved(40.0)));
        let _ = genie.update(Message::Lane(0, Lane::Dropped));
        assert_eq!(ids(&genie), [1, 0]);
    }

    #[test]
    fn dragging_the_edge_cuts_rows_and_double_clicks_fit_them() {
        let mut genie = with_rows(3);
        let _ = genie.update(Message::Lane(0, Lane::Resized(-25.0)));
        let _ = genie.update(Message::Lane(0, Lane::Resized(-30.0)));
        let _ = genie.update(Message::Lane(0, Lane::Released));
        assert_eq!(first_height(&genie), Some(30.0));
        assert!(shows(&genie, "+2 rows"));
        let _ = genie.update(Message::Lane(0, Lane::Fit));
        assert_eq!(first_height(&genie), None);
        assert!(!shows(&genie, "+2 rows"));
    }

    #[test]
    fn lanes_have_the_height_the_app_expects() {
        for genie in [with_rows(3), with_reads(), with_variants(), with_signal()] {
            let session = genie.session.as_ref().unwrap();
            let expected = super::view::body_height(session, &session.tracks[0]) + 4.0;
            let mut ui = simulator(genie.view());
            let bounds = ui.find(selector::id("lane-0")).unwrap().bounds();
            assert!(
                (bounds.height - expected).abs() < 0.5,
                "{bounds:?} {expected}"
            );
        }
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
            mate: None,
            tags: genie_core::alignment::Tags::default(),
        }
    }

    fn add_reads(genie: &mut Genie, reads: Vec<genie_core::alignment::Alignment>) {
        let source = Arc::new(Source {
            path: PathBuf::from("/data/reads.bam"),
            name: "reads.bam".to_owned(),
            format: Format::Reads(ReadFile {
                index: genie_formats::bai::Index::default(),
                contigs: vec!["chr1".to_owned()],
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, Options::default());
        for read in reads {
            gather.add(read);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept(0, region, Ok(Arc::new(loaded)));
    }

    #[test]
    fn right_clicking_the_sequence_opens_its_menu() {
        let mut genie = opened();
        let _ = genie.update(Message::SequenceMenu(Point::new(100.0, 120.0)));
        for item in [
            "Forward strand",
            "Reverse strand",
            "No translation",
            "3 frames",
            "6 frames",
        ] {
            assert!(shows(&genie, item), "{item}");
        }
    }

    #[test]
    fn six_frames_grow_the_sequence_track() {
        let mut genie = opened();
        let _ = genie.update(Message::SequenceMenu(Point::new(100.0, 120.0)));
        let change = reading::Change::Frames(Frames::Six);
        let _ = genie.update(Message::SequenceChanged(change));
        assert_eq!(genie.sequence_menu, None);
        let mut ui = simulator(genie.view());
        let bounds = ui.find(selector::id("sequence")).unwrap().bounds();
        assert!((bounds.height - 104.0).abs() < 0.5, "{}", bounds.height);
    }

    #[test]
    fn sequence_menu_knows_its_height() {
        let mut genie = opened();
        let _ = genie.update(Message::SequenceMenu(Point::new(100.0, 120.0)));
        let mut ui = simulator(genie.view());
        let height = ui.find(selector::id("menu")).unwrap().bounds().height;
        let expected = reading::MENU_HEIGHT;
        assert!(height <= expected && expected - height < 1.0, "{height}");
    }

    #[test]
    fn f12_toggles_the_diagnostics_panel() {
        let mut genie = with_reads();
        let _ = genie.update(Message::Command(Command::Diagnostics));
        assert!(
            simulator(genie.view())
                .find(selector::id("diagnostics"))
                .is_ok()
        );
        let figures = genie.figures();
        assert!(figures.contains(&("reads", "1".to_owned())), "{figures:?}");
        assert!(figures.contains(&("tiles", "0 of 64".to_owned())));
        let _ = genie.update(Message::Command(Command::Diagnostics));
        assert!(
            simulator(genie.view())
                .find(selector::id("diagnostics"))
                .is_err()
        );
    }

    #[test]
    fn frames_report_their_interval() {
        let mut genie = opened();
        let now = Instant::now();
        let _ = genie.update(Message::Frame(now));
        let _ = genie.update(Message::Frame(now + Duration::from_millis(16)));
        assert!(genie.figures().contains(&("frame", "16.00 ms".to_owned())));
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
    fn looked_up_names_move_the_view_or_explain() {
        let mut genie = opened();
        let named = genie_core::Named {
            name: "chr2_450".to_owned(),
            contig: 1,
            span: Region::between(100, 200),
        };
        let _ = genie.update(Message::Looked(Some(named), String::new()));
        assert!(shows(&genie, "chr2:101-200"));
        let error = "The name x is not a locus or a feature.".to_owned();
        let _ = genie.update(Message::Looked(None, error.clone()));
        assert!(shows(&genie, &error));
    }

    #[test]
    fn bed_menus_turn_file_colors_off() {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Use file colors"));
        let _ = genie.update(Message::TrackChanged(0, Change::Colors));
        assert!(!genie.session.as_ref().unwrap().tracks[0].plot.colors);
    }

    #[test]
    fn hovering_a_paired_read_shows_its_mate() {
        use genie_core::alignment::{FIRST, MATE_REVERSE, Mate, PAIRED};
        let mut genie = opened();
        let paired = genie_core::alignment::Alignment {
            flags: PAIRED | MATE_REVERSE | FIRST,
            mate: Some(Mate {
                contig: 0,
                start: 300,
                insert: 330,
            }),
            ..read("pair1", 10)
        };
        add_reads(&mut genie, vec![paired]);
        let _ = genie.update(Message::TrackHovered(0, Some(over(20.0, 5.0))));
        assert!(shows(&genie, "Mate chr1:301 · insert 330 · FR · first"));
    }

    #[test]
    fn read_menus_group_by_phase_or_strand() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for item in ["Group by phase", "Group by strand"] {
            assert!(shows(&genie, item), "{item}");
        }
        let _ = genie.update(Message::TrackChanged(0, Change::Group(Grouping::Strand)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.options.group, Grouping::Strand);
    }

    #[test]
    fn read_menus_sort_at_the_clicked_base() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(20.5, 200.0)));
        assert!(shows(&genie, "Sort by base at 21"));
        let _ = genie.update(Message::TrackChanged(0, Change::Sort(20)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.order.as_ref().map(|(at, _)| *at), Some(20));
    }

    #[test]
    fn read_menus_turn_pair_colors_off_and_on() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Color unusual pairs"));
        let _ = genie.update(Message::TrackChanged(0, Change::Pairs));
        assert!(!genie.session.as_ref().unwrap().tracks[0].plot.pairs);
    }

    #[test]
    fn toggling_a_filter_reloads_the_reads() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackChanged(0, Change::Toggle(Toggle::Duplicates)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.options.duplicates);
        assert!(track.wanted.is_some());
    }

    fn with_gff() -> Genie {
        let mut genie = opened();
        let source = Arc::new(Source {
            path: PathBuf::from("/data/genes.gff3.gz"),
            name: "genes.gff3.gz".to_owned(),
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Gff,
                index: Index::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        genie
    }

    #[test]
    fn menus_know_their_height() {
        let tracks = [
            with_track(),
            with_gff(),
            with_reads(),
            with_variants(),
            with_signal(),
        ];
        for mut genie in tracks {
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

    fn site(start: u64, calls: Vec<Call>) -> Variant {
        Variant {
            id: format!("rs{start}"),
            span: Region::between(start, start + 1),
            reference: "A".to_owned(),
            alternates: vec!["G".to_owned()],
            quality: Some(50.0),
            filter: "PASS".to_owned(),
            info: "DP=12".to_owned(),
            calls,
        }
    }

    fn variant_source() -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from("/data/calls.vcf"),
            name: "calls.vcf".to_owned(),
            format: Format::Variants(VariantFile::Table {
                table: genie_formats::features::Table::new(Vec::new()),
                samples: vec!["NA1".to_owned(), "NA2".to_owned()],
            }),
        })
    }

    fn with_variants() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        let variants = vec![site(20, vec![Call::Het, Call::Alternate])];
        let loaded = Loaded::variants(region, variants, 2);
        track.accept(0, region, Ok(Arc::new(loaded)));
        genie
    }

    fn signal_source(format: Format) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from("/data/coverage.bedGraph"),
            name: "coverage.bedGraph".to_owned(),
            format,
        })
    }

    fn signal_table() -> Format {
        Format::Signal(SignalFile::BedGraphTable {
            table: genie_formats::table::Table::new(Vec::new()),
        })
    }

    fn with_signal() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(signal_source(signal_table()))));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.wanted = Some((0, region));
        let values = vec![
            Summary::single(Region::between(0, 100), 1.0),
            Summary::single(Region::between(100, 200), 4.0),
        ];
        track.accept(0, region, Ok(Arc::new(Loaded::signal(region, values, 1))));
        genie
    }

    #[test]
    fn signal_lanes_have_a_fixed_height() {
        let genie = with_signal();
        let session = genie.session.as_ref().unwrap();
        let height = super::view::body_height(session, &session.tracks[0]);
        assert!((height - 48.0).abs() < f32::EPSILON, "{height}");
    }

    #[test]
    fn hovering_a_signal_shows_its_value() {
        let mut genie = with_signal();
        let _ = genie.update(Message::TrackHovered(0, Some(over(150.0, 20.0))));
        assert!(shows(&genie, "Value 4"));
        assert!(shows(&genie, "chr1:101-200"));
        let _ = genie.update(Message::TrackHovered(0, Some(over(500.0, 20.0))));
        assert!(genie.hover.is_none());
    }

    #[test]
    fn signal_menus_offer_colors_but_no_rows() {
        let mut genie = with_signal();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Move up"));
        assert!(!shows(&genie, "Squished"));
        assert!(
            simulator(genie.view())
                .find(selector::id("swatches"))
                .is_ok()
        );
    }

    #[test]
    fn signal_menus_offer_bars_lines_and_a_shared_scale() {
        let mut genie = with_signal();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for item in ["Bars", "Line", "Shared scale"] {
            assert!(shows(&genie, item), "{item}");
        }
        let _ = genie.update(Message::TrackChanged(0, Change::Style(Style::Line)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.plot.style, Style::Line);
    }

    fn scale_of(genie: &Genie, index: usize) -> Option<Scale> {
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[index];
        let Some(Content::Signal(data)) = track.data(0).map(|data| &data.content) else {
            return None;
        };
        let columns = genie_render::signal::columns(session.viewport, &data.summaries);
        super::view::signal_scale(session, track, &columns)
    }

    #[test]
    fn linked_signal_tracks_share_one_scale() {
        let mut genie = with_signal();
        let other = Arc::new(Source {
            path: PathBuf::from("/data/other.bedGraph"),
            name: "other.bedGraph".to_owned(),
            format: signal_table(),
        });
        let _ = genie.update(Message::TrackOpened(Ok(other)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[1];
        track.wanted = Some((0, region));
        let values = vec![Summary::single(Region::between(300, 400), 10.0)];
        track.accept(0, region, Ok(Arc::new(Loaded::signal(region, values, 1))));
        let own = Scale { min: 0.0, max: 4.0 };
        assert_eq!(scale_of(&genie, 0), Some(own));
        for id in [0, 1] {
            let _ = genie.update(Message::TrackChanged(id, Change::Link));
        }
        let shared = Scale {
            min: 0.0,
            max: 10.0,
        };
        assert_eq!(scale_of(&genie, 0), Some(shared));
        assert_eq!(scale_of(&genie, 1), Some(shared));
    }

    #[test]
    fn variant_lanes_hold_a_row_per_sample() {
        let genie = with_variants();
        let session = genie.session.as_ref().unwrap();
        let height = super::view::body_height(session, &session.tracks[0]);
        assert!((height - 33.0).abs() < f32::EPSILON, "{height}");
    }

    #[test]
    fn variant_menus_offer_no_read_toggles() {
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Move up"));
        assert!(!shows(&genie, "Show duplicates"));
    }

    #[test]
    fn hovering_a_genotype_shows_the_sample() {
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackHovered(0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "rs20"));
        assert!(shows(&genie, "A > G · QUAL 50 · PASS"));
        assert!(shows(&genie, "DP=12"));
        assert!(!shows(&genie, "NA2 · homozygous alternate"));
        let _ = genie.update(Message::TrackHovered(0, Some(over(20.5, 28.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
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
        for mut other in [with_reads(), with_variants()] {
            let _ = other.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
            assert!(shows(&other, "Squished"));
            assert!(
                simulator(other.view())
                    .find(selector::id("swatches"))
                    .is_err()
            );
        }
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
        assert_eq!(genie.settings.appearance, Appearance::Light);
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
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/a.png")), false));
        let _ = genie.update(Message::NoticeExpired(notice_id(&genie)));
        assert!(genie.notice.is_none());
        let _ = genie.update(Message::Exported(
            Err("The disk is full.".to_owned()),
            false,
        ));
        let _ = genie.update(Message::NoticeExpired(notice_id(&genie)));
        assert!(shows(&genie, "The disk is full."));
    }

    #[test]
    fn old_timers_leave_newer_notices() {
        let mut genie = opened();
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/a.png")), false));
        let first = notice_id(&genie);
        let _ = genie.update(Message::Exported(Ok(PathBuf::from("/data/b.png")), false));
        let _ = genie.update(Message::NoticeExpired(first));
        assert!(shows(&genie, "Saved b.png."));
    }

    #[test]
    fn saved_exports_are_announced() {
        let mut genie = opened();
        let path = PathBuf::from("/data/chr1_1-1000.png");
        let _ = genie.update(Message::Exported(Ok(path), false));
        assert!(shows(&genie, "Saved chr1_1-1000.png."));
        let _ = genie.update(Message::Exported(
            Err("The disk is full.".to_owned()),
            false,
        ));
        assert!(shows(&genie, "The disk is full."));
    }

    #[test]
    fn cut_exports_say_so_until_the_next_notice() {
        let mut genie = opened();
        let path = PathBuf::from("/data/chr1_1-1000.png");
        let _ = genie.update(Message::Exported(Ok(path), true));
        let _ = genie.update(Message::NoticeExpired(notice_id(&genie)));
        assert!(shows(
            &genie,
            "Saved chr1_1-1000.png. The view is too tall. The image shows only the top part."
        ));
    }
}
