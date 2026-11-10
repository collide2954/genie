use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use genie_core::{History, Locus, Marks, Named, Names, Region, Span, Target, Viewport, Visit};
use genie_formats::features::Entry;
use genie_formats::http::Url;
use iced::theme::Mode;
use iced::time::Instant;
use iced::{
    Animation, Event, Point, Size, Subscription, Task, Theme, event, keyboard, mouse, system,
    window,
};

use crate::card::Card;
use crate::chrome::{self, Action};
use crate::location::Location;
use crate::motion::{self, Transition};
use crate::port::{Order, Reply};
use crate::reading::{self, Reading};
use crate::recent::{self, Recent};
use crate::reference::Reference;
use crate::saved::Saved;
use crate::scene::Pointer;
use crate::settings::{Setting, Settings};
use crate::shortcuts::{self, Command};
use crate::theme;
use crate::tiles::{Key, Tiles};
use crate::track::{self, Change, Loaded, Source, Track};
use crate::{font, paths, timer};

mod address;
mod copy;
mod export;
mod figure;
mod navigation;
mod offline;
mod orders;
mod regions;
mod sessions;
pub mod snapshot;
mod tracks;
mod view;

use orders::Answer;
use tracks::pick;

const INITIAL_WIDTH: f32 = 1_000.0;
const FADE: Duration = Duration::from_secs(4);
const LAST: &str = "last.genie";

#[derive(Debug)]
pub struct Genie {
    settings: Settings,
    popup: Option<Popup>,
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
    copy_menu: Option<(Option<u64>, Point)>,
    resize: Option<(u64, f32)>,
    moving: Option<(u64, f32)>,
    scroll: f32,
    window: Size,
    hover: Option<Card>,
    pinned: Option<Card>,
    pending: Vec<Location>,
    sweep: Option<(usize, f64, f64)>,
    pointed: Option<u64>,
    anchor: Option<(Visit, u64)>,
    sidebar: Option<Sidebar>,
    slide: Animation<bool>,
    now: Instant,
    modifiers: keyboard::Modifiers,
    address: Option<AddressField>,
    strip: Option<Pointer>,
    cache: Option<PathBuf>,
    download: Option<Download>,
    busy: usize,
    answers: Vec<(Answer, Reply)>,
    looking: Option<Reply>,
    folder: Option<PathBuf>,
    renaming: Option<(u64, String)>,
    range_draft: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popup {
    Settings,
    Genomes,
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
            popup: None,
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
            copy_menu: None,
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
            modifiers: keyboard::Modifiers::default(),
            address: None,
            strip: None,
            cache: paths::cache_dir(),
            download: None,
            busy: 0,
            answers: Vec::new(),
            looking: None,
            folder: None,
            renaming: None,
            range_draft: None,
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

const MAX_PANELS: usize = 4;
const PANEL_GAP: f32 = 1.0;

#[derive(Debug, Clone)]
struct Panel {
    contig: usize,
    viewport: Viewport,
    transition: Option<Transition>,
}

impl Panel {
    fn destination(&self) -> Viewport {
        self.transition
            .as_ref()
            .map_or(self.viewport, Transition::target)
    }
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
    panels: Vec<Panel>,
}

fn panel_width(width: f32, count: usize) -> f32 {
    let gaps = PANEL_GAP * count.saturating_sub(1) as f32;
    ((width - gaps) / count.max(1) as f32).max(1.0)
}

fn cannot_open(name: &str, reason: &str) -> String {
    if reason.starts_with("genie cannot open") {
        reason.to_owned()
    } else {
        format!("genie cannot open {name}. {reason}")
    }
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
            panels: Vec::new(),
        }
    }

    fn count(&self) -> usize {
        1 + self.panels.len()
    }

    fn frame(&self, panel: usize) -> (usize, Viewport) {
        match panel
            .checked_sub(1)
            .and_then(|index| self.panels.get(index))
        {
            Some(extra) => (extra.contig, extra.destination()),
            None => (self.contig, self.destination()),
        }
    }

    fn locate(&self, x: f32, width: f32) -> (usize, f32) {
        let count = self.count();
        let step = panel_width(width, count) + PANEL_GAP;
        let panel = ((x / step).max(0.0) as usize).min(count - 1);
        (panel, x - panel as f32 * step)
    }

    fn motion(&mut self, panel: usize) -> Option<(&mut Viewport, &mut Option<Transition>)> {
        if panel == 0 {
            return Some((&mut self.viewport, &mut self.transition));
        }
        let extra = self.panels.get_mut(panel - 1)?;
        Some((&mut extra.viewport, &mut extra.transition))
    }

    fn shift(&mut self, panel: usize, change: impl FnOnce(Viewport) -> Viewport) {
        if let Some((viewport, transition)) = self.motion(panel) {
            *transition = None;
            *viewport = change(*viewport);
        }
    }

    fn steer(&mut self, panel: usize, change: impl FnOnce(Viewport) -> Viewport, motion: bool) {
        if let Some((viewport, transition)) = self.motion(panel) {
            let to = change(transition.as_ref().map_or(*viewport, Transition::target));
            if motion {
                *transition = Some(Transition::new(*viewport, to, Instant::now()));
            } else {
                *transition = None;
                *viewport = to;
            }
        }
    }

    fn is_moving(&self) -> bool {
        self.transition.is_some() || self.panels.iter().any(|extra| extra.transition.is_some())
    }

    fn view(&self, panel: usize) -> (usize, Viewport) {
        match panel
            .checked_sub(1)
            .and_then(|index| self.panels.get(index))
        {
            Some(extra) => (extra.contig, extra.viewport),
            None => (self.contig, self.viewport),
        }
    }

    fn only(&mut self, panel: usize, width: f32) {
        if let Some((contig, visible)) = panel
            .checked_sub(1)
            .and_then(|index| self.panels.get(index))
            .map(|extra| (extra.contig, extra.destination().visible()))
        {
            let length = self.reference.assembly.contigs()[contig].length;
            self.transition = None;
            self.contig = contig;
            self.viewport = self.viewport.retarget(length).jump(Span::Region(visible));
            for track in &mut self.tracks {
                if panel <= track.extra.len() {
                    track.slot = track.extra.remove(panel - 1);
                }
            }
        }
        self.join(width);
    }

    fn close(&mut self, panel: usize, width: f32) {
        if panel == 0 {
            if !self.panels.is_empty() {
                let next = self.panels.remove(0);
                self.transition = None;
                self.contig = next.contig;
                self.viewport = next.destination();
                for track in &mut self.tracks {
                    track.slot = if track.extra.is_empty() {
                        crate::track::Slot::default()
                    } else {
                        track.extra.remove(0)
                    };
                }
            }
        } else if panel <= self.panels.len() {
            self.panels.remove(panel - 1);
            for track in &mut self.tracks {
                if panel <= track.extra.len() {
                    track.extra.remove(panel - 1);
                }
            }
        }
        self.refit(width);
    }

    fn split(&mut self, targets: &[Target], width: f32) {
        let width = f64::from(panel_width(width, targets.len()));
        let lengths: Vec<u64> = targets
            .iter()
            .map(|target| self.reference.assembly.contigs()[target.contig].length)
            .collect();
        self.transition = None;
        self.contig = targets[0].contig;
        self.viewport = self
            .viewport
            .retarget(lengths[0])
            .resize(width)
            .jump(targets[0].span);
        self.panels = targets[1..]
            .iter()
            .zip(&lengths[1..])
            .map(|(target, length)| Panel {
                contig: target.contig,
                viewport: Viewport::new(*length, width).jump(target.span),
                transition: None,
            })
            .collect();
        for track in &mut self.tracks {
            track.extra.clear();
        }
    }

    fn join(&mut self, width: f32) {
        if self.panels.is_empty() {
            return;
        }
        self.panels.clear();
        for track in &mut self.tracks {
            track.extra.clear();
        }
        self.refit(width);
    }

    fn refit(&mut self, width: f32) {
        let width = f64::from(panel_width(width, self.count()));
        self.transition = None;
        let visible = self.viewport.visible();
        self.viewport = self.viewport.resize(width).show(visible);
        for panel in &mut self.panels {
            let visible = panel.destination().visible();
            panel.transition = None;
            panel.viewport = panel.viewport.resize(width).show(visible);
        }
    }

    fn resize(&mut self, width: f32) {
        let width = f64::from(panel_width(width, self.count()));
        self.transition = None;
        self.viewport = self.viewport.resize(width);
        for panel in &mut self.panels {
            panel.viewport = panel.destination().resize(width);
            panel.transition = None;
        }
    }

    fn visit(&self) -> Visit {
        Visit {
            contig: self.contig,
            region: self.destination().visible(),
        }
    }

    fn show(&mut self, target: Target, motion: bool) {
        if target.contig == self.contig {
            let to = self.destination().jump(target.span);
            if motion {
                self.glide(to);
            } else {
                self.transition = None;
                self.viewport = to;
            }
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
        for panel in 0..self.count() {
            if let Some((viewport, transition)) = self.motion(panel)
                && let Some(running) = transition.as_ref()
            {
                *viewport = running.at(now);
                if running.is_done(now) {
                    *transition = None;
                }
            }
        }
    }

    fn accept(
        &mut self,
        (id, panel): (u64, usize),
        contig: usize,
        region: Region,
        result: Result<Arc<Loaded>, String>,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == id) {
            track.accept_at(panel, contig, region, result);
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

    fn locus_at(&self, panel: usize) -> String {
        let (contig, viewport) = self.view(panel);
        Locus {
            name: self.reference.assembly.contigs()[contig].name.clone(),
            span: Span::Region(viewport.visible()),
        }
        .to_string()
    }

    fn locus(&self) -> String {
        (0..self.count())
            .map(|panel| self.locus_at(panel))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    CycleAppearance,
    SystemModeChanged(Mode),
    Window(chrome::Action),
    Open,
    Picked(Option<PathBuf>),
    Reopen(Location),
    Opened(Result<Arc<Reference>, String>),
    Pan(usize, f32),
    Zoom(usize, f32, f32),
    Resized(f32),
    TileLoaded(u64, Key, Result<Arc<[u8]>, String>),
    LocusEdited(String),
    LocusSubmitted,
    Command(Command),
    Frame(Instant),
    Overview(Sweep),
    Ruler(usize, Sweep),
    FileHovered,
    FileHoverEnded,
    FileDropped(PathBuf),
    SuggestionChosen(usize),
    TrackOpened(Result<Arc<Source>, String>),
    TrackLoaded(u64, usize, usize, Region, Result<Arc<Loaded>, String>),
    NoticeDismissed,
    TrackMenu(u64, Point),
    TrackChanged(u64, Change),
    MenuDismissed,
    Scrolled(f32),
    WindowResized(Size),
    TrackHovered(u64, usize, Option<Pointer>),
    TrackClicked(u64, usize, Pointer),
    Jump(u64, bool),
    Jumped(u64, bool, Result<Option<(usize, Region)>, String>),
    Regions(Regions),
    Address(Address),
    Modifiers(keyboard::Modifiers),
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
    Strip(Option<Pointer>),
    Genome(String),
    GenomeMenu,
    Offline(Offline),
    CopyMenu(Option<u64>, Point),
    Copy(Copying),
    Port(Port),
    Rename(Renaming),
    Range(u64, Ranging),
    Panel(usize, Paneling),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paneling {
    Only,
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ranging {
    Min(String),
    Max(String),
    Fix,
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Renaming {
    Start(u64),
    Draft(String),
    Done,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Port {
    Order(Order, Reply),
    Loaded(Result<Arc<Source>, String>, Option<String>, Reply),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Copying {
    Locus(Option<u64>),
    Sequence(Option<u64>),
    Fetched(Result<(String, u64), String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Offline {
    Start,
    Progress(u64, u64),
    Done(Result<(), String>),
    Stop,
    Remove,
    Online(String, Result<Arc<Reference>, String>),
}

#[derive(Debug)]
struct Download {
    genome: String,
    done: u64,
    size: u64,
    stop: Arc<AtomicBool>,
}

impl Download {
    fn percent(&self) -> u64 {
        (self.done * 100).checked_div(self.size).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Address {
    Edit(String),
    Submit,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct AddressField {
    text: String,
    error: Option<String>,
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
    Import,
    ImportPicked(Option<PathBuf>),
    Imported(Result<Vec<Entry>, String>),
    Export,
    ExportPicked(Option<PathBuf>),
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
        let files = std::env::args_os()
            .skip(1)
            .map(|argument| match argument.to_str() {
                Some(text) if Url::is_url(text) => {
                    Location::parse(text).unwrap_or_else(|_| Location::File(PathBuf::from(text)))
                }
                _ => Location::File(PathBuf::from(argument)),
            })
            .collect();
        let open = genie.launch(files);
        (
            genie,
            Task::batch([system::theme().map(Message::SystemModeChanged), open]),
        )
    }

    fn launch(&mut self, files: Vec<Location>) -> Task<Message> {
        let (tracks, references): (Vec<Location>, Vec<Location>) =
            files.into_iter().partition(track::is_track);
        if let Some(reference) = references.into_iter().next() {
            self.pending = tracks;
            return self.open(reference);
        }
        let tasks: Vec<Task<Message>> = tracks
            .into_iter()
            .map(|location| self.open(location))
            .collect();
        Task::batch(tasks)
    }

    fn opened(&mut self, result: Result<Arc<Reference>, String>) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
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
        self.recent.add(match &reference.genome {
            Some(id) => recent::Entry::Genome(id.clone()),
            None => reference.location.clone().into(),
        });
        (self.hover, self.pinned) = (None, None);
        (self.anchor, self.sweep) = (None, None);
        if let Some(sidebar) = &mut self.sidebar {
            *sidebar = Sidebar::default();
        }
        self.session = Some(Session::new(reference, self.width));
        self.resume();
        let mut tasks = vec![self.fetch(), self.remember()];
        let pending = std::mem::take(&mut self.pending);
        self.waiting = pending.len();
        if self.waiting == 0 {
            self.restoring = None;
        }
        for location in pending {
            tasks.push(self.open(location));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let started = Instant::now();
        let task = self.handle(message);
        self.answer();
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
            Message::Picked(Some(path)) => return self.open(Location::File(path)),
            Message::Reopen(location) => return self.open(location),
            Message::Picked(None) => {}
            Message::NamesLoaded(id, names) => {
                let names = names.unwrap_or_default();
                self.on_session(|session| session.name(id, names));
            }
            Message::Looked(found, error) => return self.looked(found, error),
            Message::FileHovered => self.hovering = true,
            Message::FileHoverEnded => self.hovering = false,
            Message::FileDropped(path) => {
                self.hovering = false;
                return self.open(Location::File(path));
            }
            Message::Opened(result) => return self.opened(result),
            Message::Indexing(name, percent) => self.indexing = Some((name, percent)),
            Message::TrackOpened(result) => return self.track_opened(result),
            Message::TrackLoaded(id, panel, contig, region, result) => {
                self.on_session(|session| session.accept((id, panel), contig, region, result));
            }
            Message::NoticeDismissed => self.notice = None,
            Message::TrackMenu(id, at) => self.open_menu(id, at),
            Message::TrackChanged(id, change) => return self.change_track(id, change),
            Message::TrackHovered(id, panel, pointer) => self.hovered(id, panel, pointer),
            Message::TrackClicked(id, panel, pointer) => {
                self.pinned = self.card(id, panel, pointer);
            }
            Message::Jump(id, forward) => return self.jump(id, forward),
            Message::Jumped(id, forward, result) => return self.jumped(id, forward, result),
            Message::Regions(action) => return self.regions(action),
            Message::Address(action) => return self.address(action),
            Message::Modifiers(modifiers) => self.modifiers = modifiers,
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
            Message::ToggleSettings => self.toggle(Popup::Settings),
            Message::Lane(id, lane) => self.lane(id, lane),
            Message::RestoreLast => return self.restore_last(),
            Message::Strip(pointer) => self.strip = pointer,
            Message::Genome(id) => return self.open_genome(&id),
            Message::GenomeMenu => self.toggle(Popup::Genomes),
            Message::Offline(offline) => return self.offline(offline),
            Message::CopyMenu(region, at) => self.open_copy_menu(region, at),
            Message::Copy(copying) => return self.copying(copying),
            Message::Port(event) => return self.port(event),
            Message::Rename(renaming) => return self.rename(renaming),
            Message::Range(id, ranging) => self.range(id, ranging),
            Message::Panel(panel, paneling) => return self.panel(panel, paneling),
            Message::SettingChanged(setting) => return self.change_setting(setting),
            Message::WindowResized(size) => self.window = size,
            Message::Pan(panel, dx) => {
                self.drop_draft();
                return self.navigate(panel, |viewport| viewport.pan(f64::from(dx)));
            }
            Message::Zoom(panel, factor, anchor) => {
                self.drop_draft();
                return self.navigate(panel, |viewport| {
                    viewport.zoom(f64::from(factor), f64::from(anchor))
                });
            }
            Message::Resized(width) => return self.resize(width),
            Message::TileLoaded(generation, key, result) => {
                self.tile_loaded(generation, key, result);
            }
            Message::LocusEdited(text) => self.edit_locus(text),
            Message::LocusSubmitted => return self.submit_locus(),
            Message::SuggestionChosen(position) => return self.choose(position),
            Message::Command(command) => return self.command(command),
            Message::Frame(now) => self.frame(now),
            Message::Overview(sweep) => return self.sweep_overview(sweep),
            Message::Ruler(panel, sweep) => return self.sweep_ruler(panel, sweep),
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
        self.slide.is_animating(self.now) || self.session.as_ref().is_some_and(Session::is_moving)
    }

    fn toggle(&mut self, popup: Popup) {
        let open = self.popup != Some(popup);
        self.close_menus();
        self.popup = open.then_some(popup);
    }

    fn band_tip(&self) -> Option<String> {
        let (x, session) = (self.strip?.local.x, self.session.as_ref()?);
        let viewport = session.viewport;
        if viewport.width() <= 0.0 {
            return None;
        }
        let position = (f64::from(x) / viewport.width() * viewport.length() as f64) as u64;
        let band = session
            .reference
            .bands(session.contig)
            .iter()
            .find(|band| band.span.start() <= position && position < band.span.end())
            .filter(|band| !band.name.is_empty())?;
        let contig = &session.reference.assembly.contigs()[session.contig].name;
        Some(if band.name.starts_with(['p', 'q']) {
            format!(
                "{}{}",
                contig.strip_prefix("chr").unwrap_or(contig),
                band.name
            )
        } else {
            format!("{contig} {}", band.name)
        })
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
            self.port_events(),
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
        keyboard::Event::ModifiersChanged(modifiers) => Some(Message::Modifiers(modifiers)),
        keyboard::Event::KeyReleased { .. } => None,
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
    use genie_core::{Band, Feature, Names, Region, Stain, Strand};
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

    use super::tracks::{NO_REFERENCE, REMOTE_SESSION};
    use super::view::{DROP_HINT, SIGNAL_HINT, VARIANT_HINT, WELCOME, ZOOM_HINT};
    use super::{
        Address, Copying, Genie, Lane, Message, Offline, Paneling, Port, Ranging, Regions,
        Renaming, Sweep, mouse_button, shortcut, window_event,
    };
    use crate::export;
    use crate::location::Location;
    use crate::menu;
    use crate::reading::{self, Frames};
    use crate::recent::Entry;
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
            location: PathBuf::from(format!("/data/{name}")).into(),
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
        let track = Location::File(PathBuf::from("/data/genes.bed.gz"));
        let _ = genie.launch(vec![track.clone(), PathBuf::from("/data/ref.fa").into()]);
        assert!(shows(&genie, "Opening ref.fa…"));
        assert_eq!(genie.pending, [track]);
        let _ = genie.update(Message::Opened(Ok(reference())));
        assert_eq!(genie.pending, [] as [Location; 0]);
    }

    #[test]
    fn indexing_progress_shows_while_opening() {
        let mut genie = Genie::default();
        let _ = genie.launch(vec![PathBuf::from("/data/genome.fa.gz").into()]);
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
        let _ = genie.launch(vec![PathBuf::from("/data/genes.bed.gz").into()]);
        assert!(shows(&genie, NO_REFERENCE));
    }

    #[test]
    fn choosing_a_genome_opens_it_with_its_gene_track() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Genome("hg38".to_owned()));
        assert!(shows(&genie, "Opening hg38…"));
        let folder = genie.cache.clone().unwrap().join("genomes").join("hg38");
        assert_eq!(
            genie.pending,
            [Location::File(folder.join("RefSeq Select.bed"))]
        );
        let mut other = Genie::default();
        let _ = other.update(Message::Genome("nope".to_owned()));
        assert_eq!(other.pending, Vec::<Location>::new());
    }

    #[test]
    fn the_gene_track_of_a_genome_shows_the_track_name() {
        let mut genie = Genie::default();
        let folder = genie.cache.clone().unwrap().join("genomes").join("hg38");
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        let genes = Source {
            location: Location::File(folder.join("RefSeq Select.bed")),
            name: "RefSeq Select.bed".to_owned(),
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: Index::default(),
            }),
        };
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(genes))));
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        assert!(shows(&genie, "RefSeq Select"));
        assert!(!shows(&genie, "RefSeq Select.bed"));
        assert!(shows(&genie, "genes.bed.gz"));
    }

    #[test]
    fn the_genome_menu_switches_genomes() {
        let mut genie = opened();
        assert!(shows(&genie, "ref.fa"));
        let _ = genie.update(Message::GenomeMenu);
        let mut ui = simulator(genie.view());
        ui.click("mm10 · Mouse").unwrap();
        ui.click("Open reference…").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::Genome("mm10".to_owned()), Message::Open]
        );
        let _ = genie.update(Message::MenuDismissed);
        assert!(!shows(&genie, "mm10 · Mouse"));
        let _ = genie.update(Message::GenomeMenu);
        let _ = genie.update(Message::GenomeMenu);
        assert!(!shows(&genie, "mm10 · Mouse"));
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        assert!(shows(&genie, "hg38"));
        assert!(!shows(&genie, "ref.fa"));
    }

    #[test]
    fn offline_copies_download_with_progress_and_can_go_away() {
        let scratch = crate::scratch::Scratch::new("offline");
        let cache = scratch.file("marker", "").parent().unwrap().to_owned();
        let folder = cache.join("genomes").join("hg38");
        let mut genie = Genie {
            cache: Some(cache),
            ..Genie::default()
        };
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        let _ = genie.update(Message::GenomeMenu);
        assert!(shows(&genie, "Use offline (835 MB)"));
        let _ = genie.update(Message::Offline(Offline::Start));
        let _ = genie.update(Message::Offline(Offline::Progress(400, 1_000)));
        assert!(shows(&genie, "Downloading hg38… 40 %"));
        let _ = genie.update(Message::GenomeMenu);
        assert!(shows(&genie, "Stop download"));
        let _ = genie.update(Message::Offline(Offline::Stop));
        let stop = &genie.download.as_ref().unwrap().stop;
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
        let stopped = Err(crate::genome::STOPPED.to_owned());
        let _ = genie.update(Message::Offline(Offline::Done(stopped)));
        assert!(shows(&genie, crate::genome::STOPPED));
        let _ = genie.update(Message::Offline(Offline::Start));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("sequence.2bit"), b"2bit").unwrap();
        let _ = genie.update(Message::Offline(Offline::Done(Ok(()))));
        assert!(shows(&genie, "hg38 is now available offline."));
        let _ = genie.update(Message::GenomeMenu);
        assert!(shows(&genie, "Remove offline copy"));
        let _ = genie.update(Message::Offline(Offline::Remove));
        assert!(!folder.join("sequence.2bit").exists());
        assert!(shows(&genie, "Removed the offline copy of hg38."));
    }

    fn reading_offline(name: &str) -> (Genie, PathBuf, crate::scratch::Scratch) {
        let scratch = crate::scratch::Scratch::new(name);
        let cache = scratch.file("marker", "").parent().unwrap().to_owned();
        let folder = cache.join("genomes").join("hg38");
        std::fs::create_dir_all(&folder).unwrap();
        let copy = folder.join("sequence.2bit");
        std::fs::write(&copy, b"2bit").unwrap();
        let mut genie = Genie {
            cache: Some(cache),
            ..Genie::default()
        };
        let reference = human(Location::File(copy.clone()), reference());
        let _ = genie.update(Message::Opened(Ok(reference)));
        (genie, copy, scratch)
    }

    fn human(location: Location, reference: Arc<Reference>) -> Arc<Reference> {
        let mut human = Arc::try_unwrap(reference).unwrap();
        human.genome = Some("hg38".to_owned());
        human.location = location;
        Arc::new(human)
    }

    fn ucsc() -> Location {
        Location::parse("https://hgdownload.soe.ucsc.edu/goldenPath/hg38/bigZips/hg38.2bit")
            .unwrap()
    }

    #[test]
    fn offline_copies_in_use_go_away_once_the_genome_reads_from_ucsc() {
        let (mut genie, copy, _scratch) = reading_offline("offline-in-use");
        let _ = genie.update(Message::GenomeMenu);
        assert!(shows(&genie, "Remove offline copy"));
        let _ = genie.update(Message::Offline(Offline::Remove));
        assert!(copy.exists());
        let away = Err("The server answered 503 Service Unavailable.".to_owned());
        let _ = genie.update(Message::Offline(Offline::Online("hg38".to_owned(), away)));
        assert!(copy.exists());
        assert!(shows(
            &genie,
            "genie cannot read hg38 from UCSC, so it keeps the offline copy. \
             The server answered 503 Service Unavailable."
        ));
        let remote = Ok(human(ucsc(), reference()));
        let _ = genie.update(Message::Offline(Offline::Online("hg38".to_owned(), remote)));
        assert!(!copy.exists());
        assert!(shows(&genie, "Removed the offline copy of hg38."));
        assert_eq!(genie.session.as_ref().unwrap().reference.location, ucsc());
        assert_eq!(genie.restoring, None);
    }

    #[test]
    fn a_changed_genome_on_ucsc_opens_again_after_the_removal() {
        let (mut genie, copy, _scratch) = reading_offline("offline-changed");
        let _ = genie.update(Message::Offline(Offline::Remove));
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 900,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        let changed = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap());
        let remote = Ok(human(ucsc(), changed));
        let _ = genie.update(Message::Offline(Offline::Online("hg38".to_owned(), remote)));
        assert!(!copy.exists());
        assert!(genie.restoring.is_some());
    }

    #[test]
    fn genome_sessions_open_the_genome_and_find_its_gene_track() {
        let scratch = crate::scratch::Scratch::new("genome-session");
        let text = "[session]\n\
                    reference = https://hgdownload.soe.ucsc.edu/goldenPath/hg38/bigZips/hg38.2bit\n\
                    genome = hg38\nlocus = chr1:101-200\n\
                    [track]\nfile = /other/cache/genomes/hg38/RefSeq Select.bed\n\
                    [track]\nfile = genes.bed.gz\n";
        let file = scratch.file("work.genie", text);
        let folder = file.parent().unwrap().to_owned();
        let mut genie = Genie::default();
        let cache = genie.cache.clone().unwrap();
        let _ = genie.update(Message::Picked(Some(file)));
        assert!(shows(&genie, "Opening hg38…"));
        assert_eq!(
            genie.pending,
            [
                Location::File(cache.join("genomes/hg38/RefSeq Select.bed")),
                Location::File(folder.join("genes.bed.gz")),
            ]
        );
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn sessions_open_only_from_this_computer() {
        let mut genie = Genie::default();
        let location = Location::parse("http://127.0.0.1:9/view.genie").unwrap();
        let _ = genie.update(Message::Reopen(location));
        assert!(shows(&genie, REMOTE_SESSION));
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
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.slot.wanted, Some((0, Region::between(0, 1_000))));
    }

    #[test]
    fn loaded_features_fill_the_track() {
        let mut genie = with_track();
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        let region = Region::between(0, 1_000);
        let feature = Feature::new("a", Region::between(10, 20), Strand::Forward);
        let loaded = Arc::new(Loaded::features(region, vec![feature; 12]));
        let _ = genie.update(Message::TrackLoaded(0, 0, 0, region, Ok(loaded)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.data(0).unwrap().count(), 12);
        assert_eq!(track.mode_at(0, 0), Rows::Squished);
    }

    #[test]
    fn load_errors_show_inside_the_track() {
        let mut genie = with_track();
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        let error = "The strand is not +, - or ..".to_owned();
        let region = Region::between(0, 1_000);
        let _ = genie.update(Message::TrackLoaded(0, 0, 0, region, Err(error.clone())));
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
    fn command_c_copies_the_pinned_card_or_else_the_locus() {
        let mut genie = opened();
        assert_eq!(genie.copy_text().as_deref(), Some("chr1:1-1,000"));
        let _ = genie.update(Message::Command(Command::Copy));
        assert!(shows(&genie, "Copied chr1:1-1,000."));
        let feature = Feature::new("geneA", Region::between(9, 20), Strand::Forward);
        genie.pinned = Some(crate::card::Card {
            track: 1,
            contig: "chr1".to_owned(),
            detail: crate::card::Detail::Feature(feature),
            at: Point::ORIGIN,
        });
        assert_eq!(
            genie.copy_text().as_deref(),
            Some("geneA\nchr1:10-20\n11 bp · forward strand")
        );
        let _ = genie.update(Message::Command(Command::Copy));
        assert!(shows(&genie, "Copied the card text."));
        assert_eq!(Genie::default().copy_text(), None);
    }

    #[test]
    fn the_copy_menu_copies_the_locus_or_the_bases() {
        let mut genie = opened();
        let _ = genie.update(Message::CopyMenu(None, Point::new(300.0, 60.0)));
        assert!(shows(&genie, "Copy sequence"));
        let _ = genie.update(Message::Copy(Copying::Locus(None)));
        assert!(!shows(&genie, "Copy sequence"));
        assert!(shows(&genie, "Copied chr1:1-1,000."));
        let fasta = Ok((">chr1:1-4\nACGT\n".to_owned(), 4));
        let _ = genie.update(Message::Copy(Copying::Fetched(fasta)));
        assert!(shows(&genie, "Copied 4 bp as FASTA."));
        let failed = Err("The file is gone.".to_owned());
        let _ = genie.update(Message::Copy(Copying::Fetched(failed)));
        assert!(shows(
            &genie,
            "genie cannot read the sequence. The file is gone."
        ));
    }

    #[test]
    fn the_copy_menu_knows_its_height() {
        let mut genie = opened();
        let _ = genie.update(Message::CopyMenu(None, Point::new(300.0, 60.0)));
        let mut ui = simulator(genie.view());
        let height = ui.find(selector::id("menu")).unwrap().bounds().height;
        let expected = super::copy::MENU_HEIGHT;
        assert!(height <= expected && expected - height < 1.0, "{height}");
    }

    #[test]
    fn regions_copy_their_own_locus() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        settle(&mut genie);
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        go(&mut genie, "chr2:1-50");
        settle(&mut genie);
        let _ = genie.update(Message::Copy(Copying::Locus(Some(id))));
        assert!(shows(&genie, "Copied chr1:101-200."));
    }

    #[test]
    fn long_views_are_too_long_to_copy() {
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 2_000_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        let big = Reference::fasta(PathBuf::from("big.fa"), records).unwrap();
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(Arc::new(big))));
        let _ = genie.update(Message::Copy(Copying::Sequence(None)));
        assert!(shows(
            &genie,
            "genie copies up to 1 Mb of sequence. Zoom in, or choose a smaller region."
        ));
    }

    fn right_click(ui: &mut iced_test::Simulator<'_, Message>, at: Point) {
        ui.point_at(at);
        let _ = ui.simulate(vec![
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Right,
            )),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Right,
            )),
        ]);
    }

    #[test]
    fn right_clicks_on_the_ruler_and_on_regions_open_the_copy_menu() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        let mut ui = simulator(genie.view());
        let ruler = ui.find(selector::id("ruler")).unwrap().bounds().center();
        right_click(&mut ui, ruler);
        let row = ui.find("Region 1").unwrap().bounds().center();
        right_click(&mut ui, row);
        let menus: Vec<Message> = ui
            .into_messages()
            .filter(|message| matches!(message, Message::CopyMenu(..)))
            .collect();
        assert_eq!(
            menus,
            [
                Message::CopyMenu(None, ruler),
                Message::CopyMenu(Some(id), row)
            ]
        );
    }

    fn ask(genie: &mut Genie, line: &str) -> std::sync::mpsc::Receiver<String> {
        let order = crate::port::parse(line).unwrap();
        let (reply, answer) = crate::port::Reply::new();
        let _ = genie.update(Message::Port(Port::Order(order, reply)));
        answer
    }

    fn told(answer: &std::sync::mpsc::Receiver<String>) -> Option<String> {
        answer.try_recv().ok()
    }

    fn load_tiles(genie: &mut Genie) {
        for key in genie.tiles.waiting() {
            let bases: Arc<[u8]> = vec![b'A'; 4_096].into();
            let _ = genie.update(Message::TileLoaded(genie.generation, key, Ok(bases)));
        }
    }

    #[test]
    fn orders_wait_until_genie_is_idle() {
        let mut genie = opened();
        let answer = ask(&mut genie, "goto chr1:101-200");
        assert_eq!(told(&answer), None);
        settle(&mut genie);
        load_tiles(&mut genie);
        assert_eq!(told(&answer).as_deref(), Some("OK"));
        assert!(shows(&genie, "chr1:101-200"));
        let wrong = ask(&mut genie, "goto chr9:1-10");
        assert!(told(&wrong).is_some_and(|text| text != "OK"));
        assert_eq!(genie.draft, None);
        let both = ask(&mut genie, "goto chr1:101-200 chr2:1-50");
        settle(&mut genie);
        load_tiles(&mut genie);
        assert_eq!(told(&both).as_deref(), Some("OK"));
        let locus = genie.session.as_ref().unwrap().locus();
        assert_eq!(locus, "chr1:101-200 chr2:1-50");
        let many = ask(&mut genie, "goto chr1 chr1 chr1 chr1 chr1");
        assert_eq!(told(&many).as_deref(), Some("genie shows up to 4 loci."));
        assert_eq!(genie.draft, None);
        let alone = ask(&mut Genie::default(), "goto chr1");
        assert_eq!(
            told(&alone).as_deref(),
            Some("Open a genome or a reference first.")
        );
    }

    #[test]
    fn orders_wait_until_every_panel_stops() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200 chr2:1-100");
        settle(&mut genie);
        load_tiles(&mut genie);
        let _ = genie.update(Message::Command(Command::Pan(-1.0)));
        load_tiles(&mut genie);
        genie.session.as_mut().unwrap().transition = None;
        assert!(
            genie.session.as_ref().unwrap().panels[0]
                .transition
                .is_some()
        );
        let (reply, answer) = crate::port::Reply::new();
        genie.answers.push((super::Answer::Done, reply));
        let _ = genie.update(Message::Modifiers(keyboard::Modifiers::default()));
        assert_eq!(told(&answer), None);
        settle(&mut genie);
        assert_eq!(told(&answer).as_deref(), Some("OK"));
    }

    #[test]
    fn orders_add_regions_set_rows_and_start_again() {
        let mut genie = with_track();
        let answer = ask(&mut genie, "region chr1 101 200 promoter");
        assert_eq!(told(&answer).as_deref(), Some("OK"));
        assert_eq!(
            (marks(&genie)[0].span, marks(&genie)[0].name.as_str()),
            (Region::between(100, 200), "promoter")
        );
        let lost = ask(&mut genie, "region chrQ 1 10");
        assert_eq!(
            told(&lost).as_deref(),
            Some("The reference has no contig chrQ.")
        );
        assert_eq!(
            told(&ask(&mut genie, "squish genes.bed.gz")).as_deref(),
            Some("OK")
        );
        assert_eq!(
            genie.session.as_ref().unwrap().tracks[0].mode,
            Some(Rows::Squished)
        );
        let other = ask(&mut genie, "collapse other.bed");
        assert_eq!(
            told(&other).as_deref(),
            Some("There is no track other.bed.")
        );
        assert_eq!(told(&ask(&mut genie, "new")).as_deref(), Some("OK"));
        assert!(genie.session.as_ref().unwrap().tracks.is_empty());
        assert_eq!(marks(&genie), [] as [genie_core::Mark; 0]);
    }

    #[test]
    fn snapshot_orders_save_the_view_in_the_folder() {
        let scratch = crate::scratch::Scratch::new("port-snapshot");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        let mut genie = opened();
        load_tiles(&mut genie);
        assert_eq!(
            told(&ask(&mut genie, "snapshot")).as_deref(),
            Some("Give a folder with snapshotDirectory first.")
        );
        let line = format!("snapshotDirectory \"{}\"", folder.display());
        assert_eq!(told(&ask(&mut genie, &line)).as_deref(), Some("OK"));
        assert_eq!(
            told(&ask(&mut genie, "snapshot first.png")).as_deref(),
            Some("OK")
        );
        assert!(folder.join("first.png").is_file());
        assert_eq!(told(&ask(&mut genie, "snapshot")).as_deref(), Some("OK"));
        assert!(folder.join("chr1_1-1000.png").is_file());
    }

    #[test]
    fn load_orders_add_a_named_track_and_wait_for_its_names() {
        let mut genie = opened();
        load_tiles(&mut genie);
        let (reply, answer) = crate::port::Reply::new();
        let name = Some("Tumor".to_owned());
        let loaded = Port::Loaded(Ok(source("genes.bed.gz")), name, reply);
        let _ = genie.update(Message::Port(loaded));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.name, "Tumor");
        assert_eq!(told(&answer), None);
        let id = track.id;
        let _ = genie.update(Message::NamesLoaded(id, Err("no names".to_owned())));
        assert_eq!(told(&answer).as_deref(), Some("OK"));
        let (reply, answer) = crate::port::Reply::new();
        let failed = Port::Loaded(Err("The file is gone.".to_owned()), None, reply);
        let _ = genie.update(Message::Port(failed));
        assert_eq!(told(&answer).as_deref(), Some("The file is gone."));
        assert_eq!(
            told(&ask(&mut genie, "load /data/ref.fa")).as_deref(),
            Some("ref.fa is not a track. To open a reference, use genome.")
        );
    }

    #[test]
    fn genome_orders_answer_once_the_genome_is_open() {
        let mut genie = Genie::default();
        let answer = ask(&mut genie, "genome hg38");
        assert_eq!(told(&answer), None);
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        assert_eq!(told(&answer), None);
        let _ = genie.update(Message::TrackOpened(Err("No genes.".to_owned())));
        load_tiles(&mut genie);
        assert_eq!(told(&answer).as_deref(), Some("OK"));
        let missing = ask(&mut genie, "genome /data/missing.fa");
        let _ = genie.update(Message::Opened(Err("The file is gone.".to_owned())));
        load_tiles(&mut genie);
        assert_eq!(
            told(&missing).as_deref(),
            Some("genie cannot open /data/missing.fa. The file is gone.")
        );
        let named = ask(&mut genie, "genome /data/gone.fa");
        let error = "genie cannot open gone.fa. The file is gone.";
        let _ = genie.update(Message::Opened(Err(error.to_owned())));
        load_tiles(&mut genie);
        assert_eq!(told(&named).as_deref(), Some(error));
    }

    fn track_name(genie: &Genie) -> String {
        genie.session.as_ref().unwrap().tracks[0].name.clone()
    }

    fn rename(genie: &mut Genie, id: u64, text: &str) {
        let _ = genie.update(Message::Rename(Renaming::Start(id)));
        let _ = genie.update(Message::Rename(Renaming::Draft(text.to_owned())));
        let _ = genie.update(Message::Rename(Renaming::Done));
    }

    #[test]
    fn tracks_take_a_new_name() {
        let mut genie = with_track();
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::Rename(Renaming::Start(id)));
        assert!(
            simulator(genie.view())
                .find(selector::id("track-name"))
                .is_ok()
        );
        let _ = genie.update(Message::Rename(Renaming::Draft("Tumor\tA ".to_owned())));
        let _ = genie.update(Message::Rename(Renaming::Done));
        assert_eq!(track_name(&genie), "Tumor A");
        assert!(shows(&genie, "Tumor A"));
        rename(&mut genie, id, "  ");
        assert_eq!(track_name(&genie), "genes.bed.gz");
        let _ = genie.update(Message::Rename(Renaming::Start(id)));
        let _ = genie.update(Message::Rename(Renaming::Draft("Other".to_owned())));
        let _ = genie.update(Message::Rename(Renaming::Cancel));
        assert_eq!(track_name(&genie), "genes.bed.gz");
        assert!(
            simulator(genie.view())
                .find(selector::id("track-name"))
                .is_err()
        );
    }

    #[test]
    fn double_clicks_and_the_menu_start_a_rename_and_escape_ends_it() {
        let mut genie = with_track();
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let mut ui = simulator(genie.view());
        let label = ui.find("genes.bed.gz").unwrap().bounds().center();
        ui.point_at(label);
        let left = iced::mouse::Button::Left;
        for _ in 0..2 {
            let _ = ui.simulate([
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(left)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(left)),
            ]);
        }
        let started: Vec<Message> = ui
            .into_messages()
            .filter(|message| matches!(message, Message::Rename(_)))
            .collect();
        assert_eq!(started, [Message::Rename(Renaming::Start(id))]);
        let _ = genie.update(Message::TrackMenu(id, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("Rename").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Rename(Renaming::Start(id))));
        let _ = genie.update(Message::Rename(Renaming::Start(id)));
        let mut ui = simulator(genie.view());
        let escape = keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            modified_key: keyboard::Key::Named(Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        };
        let _ = ui.simulate([iced::Event::Keyboard(escape)]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Rename(Renaming::Cancel)));
    }

    #[test]
    fn sessions_keep_new_track_names() {
        let mut genie = with_track();
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        rename(&mut genie, id, "Tumor");
        let saved = genie.saved().unwrap();
        assert_eq!(saved.tracks[0].name.as_deref(), Some("Tumor"));
        let mut other = opened();
        other.restoring = Some(saved);
        other.waiting = 1;
        let _ = other.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        assert_eq!(track_name(&other), "Tumor");
        rename(&mut genie, id, "");
        assert_eq!(genie.saved().unwrap().tracks[0].name, None);
    }

    #[test]
    fn a_port_in_use_shows_a_notice() {
        let mut genie = Genie::default();
        let failed = Port::Failed("The address is in use.".to_owned());
        let _ = genie.update(Message::Port(failed));
        assert!(shows(
            &genie,
            "genie cannot take commands on port 60151. The address is in use."
        ));
    }

    #[test]
    fn several_loci_open_side_by_side_panels() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200 chr2:1-50");
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.count(), 2);
        let (contig, viewport) = session.frame(1);
        assert_eq!((contig, viewport.visible()), (1, Region::between(0, 50)));
        assert!(
            (viewport.width() - 499.5).abs() < 0.01,
            "{}",
            viewport.width()
        );
        assert_eq!(session.locus(), "chr1:101-200 chr2:1-50");
        assert!(shows(&genie, "chr1:101-200 chr2:1-50"));
        go(&mut genie, "chr2:11-20");
        settle(&mut genie);
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.count(), 1);
        assert!((session.viewport.width() - 1_000.0).abs() < 0.01);
        go(&mut genie, "chr1 chr1 chr1 chr1 chr1");
        assert_eq!(
            genie.locus_error.as_deref(),
            Some("genie shows up to 4 loci.")
        );
        go(&mut genie, "chr1 chrZ");
        assert_eq!(
            genie.locus_error.as_deref(),
            Some("genie cannot find chrZ.")
        );
        assert_eq!(genie.session.as_ref().unwrap().count(), 1);
    }

    #[test]
    fn loci_that_fail_keep_every_panel() {
        let mut genie = opened();
        let locus = |genie: &Genie| genie.session.as_ref().unwrap().locus();
        go(&mut genie, "chr1:101-200 chr2:1-50");
        go(&mut genie, "chrZ:1-10");
        assert_eq!(locus(&genie), "chr1:101-200 chr2:1-50");
        go(&mut genie, "chr1:5,000-6,000");
        assert_eq!(locus(&genie), "chr1:101-200 chr2:1-50");
        let found = genie_core::Named {
            name: "geneA".to_owned(),
            contig: 1,
            span: Region::between(100, 300),
        };
        let _ = genie.update(Message::Looked(Some(found), String::new()));
        settle(&mut genie);
        assert_eq!(locus(&genie), "chr2:101-300");
    }

    #[test]
    fn panel_headers_show_one_locus_or_close_a_panel() {
        let mut genie = opened();
        let locus = |genie: &Genie| genie.session.as_ref().unwrap().locus();
        go(&mut genie, "chr1:101-200 chr2:1-50 chr1:501-600");
        let _ = genie.update(Message::Panel(2, Paneling::Close));
        assert_eq!(locus(&genie), "chr1:101-200 chr2:1-50");
        let _ = genie.update(Message::Panel(1, Paneling::Only));
        assert_eq!(locus(&genie), "chr2:1-50");
        go(&mut genie, "chr1:101-200 chr2:1-50");
        let _ = genie.update(Message::Panel(0, Paneling::Close));
        assert_eq!(locus(&genie), "chr2:1-50");
        assert!((genie.session.as_ref().unwrap().viewport.width() - 1_000.0).abs() < 0.01);
        go(&mut genie, "chr1:101-200 chr2:1-50");
        let mut ui = simulator(genie.view());
        assert!(ui.find(selector::id("overview")).is_err());
        assert!(ui.find(selector::id("ruler-1")).is_ok());
        ui.click("chr2:1-50").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Panel(1, Paneling::Only)));
    }

    #[test]
    fn panels_load_their_own_track_data() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200 chr1:801-900");
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let (contig, region) = track.slot(1).wanted.unwrap();
        assert_eq!(contig, 0);
        assert!(region.start() <= 800 && region.end() >= 900, "{region:?}");
        let id = track.id;
        let loaded = Arc::new(Loaded::features(region, Vec::new()));
        let _ = genie.update(Message::TrackLoaded(id, 1, 0, region, Ok(loaded)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.slot(1).data(0).is_some());
        assert!(track.data(0).is_none());
    }

    #[test]
    fn pointer_moves_change_one_panel_and_keys_change_all() {
        let mut genie = opened();
        let locus = |genie: &Genie| genie.session.as_ref().unwrap().locus();
        go(&mut genie, "chr1:101-200 chr2:1-100");
        let _ = genie.update(Message::Pan(1, -499.5));
        assert_eq!(locus(&genie), "chr1:101-200 chr2:101-200");
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        assert_eq!(locus(&genie), "chr1:101-150 chr2:101-200");
        let _ = genie.update(Message::Command(Command::Pan(-1.0)));
        settle(&mut genie);
        assert_eq!(locus(&genie), "chr1:151-200 chr2:201-300");
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        settle(&mut genie);
        assert!(
            locus(&genie).ends_with(" chr2:226-275"),
            "{}",
            locus(&genie)
        );
        let _ = genie.update(Message::Pan(3, -100.0));
        assert!(locus(&genie).ends_with(" chr2:226-275"));
    }

    #[test]
    fn image_captions_name_the_genome() {
        let mut genie = Genie::default();
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        let session = genie.session.as_ref().unwrap();
        assert!(simulator(genie.export_view(session)).find("hg38").is_ok());
        assert!(genie.figure(session).contains(">hg38</tspan>"));
    }

    #[test]
    fn exports_draw_every_panel() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200 chr2:1-50");
        let session = genie.session.as_ref().unwrap();
        let mut ui = simulator(genie.export_view(session));
        assert!(ui.find("chr1:101-200").is_ok());
        assert!(ui.find("chr2:1-50").is_ok());
        assert!(ui.find("chr1:101-200 chr2:1-50").is_err());
        assert!(ui.find(selector::id("ruler-1")).is_ok());
    }

    #[test]
    fn keys_and_ruler_drags_glide_every_panel() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200 chr2:1-100");
        let _ = genie.update(Message::Command(Command::Pan(-1.0)));
        assert!(genie.is_animating());
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.view(1).1.visible(), Region::between(0, 100));
        assert_eq!(session.frame(1).1.visible(), Region::between(100, 200));
        settle(&mut genie);
        assert!(!genie.is_animating());
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.locus(), "chr1:201-300 chr2:101-200");
        let _ = genie.update(Message::Ruler(1, Sweep::Pressed(0.2)));
        let _ = genie.update(Message::Ruler(1, Sweep::Dragged(0.4)));
        let _ = genie.update(Message::Ruler(1, Sweep::Released));
        assert!(genie.is_animating());
        let _ = genie.update(Message::Pan(1, 0.0));
        assert!(!genie.is_animating());
    }

    #[test]
    fn cards_read_the_panel_under_the_pointer() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200 chr1:801-900");
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let (id, (_, region)) = (track.id, track.slot(1).wanted.unwrap());
        let feature = Feature::new("TP53", Region::between(850, 860), Strand::Forward);
        let loaded = Arc::new(Loaded::features(region, vec![feature]));
        let _ = genie.update(Message::TrackLoaded(id, 1, 0, region, Ok(loaded)));
        let x = 55.0 * 499.5 / 100.0;
        let _ = genie.update(Message::TrackHovered(id, 0, Some(over(x, 5.0))));
        assert!(genie.hover.is_none());
        let _ = genie.update(Message::TrackHovered(id, 1, Some(over(x, 5.0))));
        assert!(shows(&genie, "TP53"));
        assert!(shows(&genie, "chr1:851-860"));
        let _ = genie.update(Message::TrackClicked(id, 1, over(x, 5.0)));
        assert!(genie.pinned.is_some());
        let mut ui = simulator(genie.view());
        let label = ui.find("genes.bed.gz").unwrap().bounds().center();
        let position = Point::new(1_000.0, label.y);
        ui.point_at(position);
        let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position,
        })]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::TrackHovered(_, 1, Some(_)))),
            "{messages:?}"
        );
    }

    #[test]
    fn ruler_drags_zoom_their_own_panel() {
        let mut genie = opened();
        go(&mut genie, "chr1:1-1000 chr2:1-500");
        let _ = genie.update(Message::Ruler(1, Sweep::Pressed(0.2)));
        let _ = genie.update(Message::Ruler(1, Sweep::Dragged(0.4)));
        assert_eq!(genie.sweep.map(|(panel, ..)| panel), Some(1));
        let _ = genie.update(Message::Ruler(1, Sweep::Released));
        settle(&mut genie);
        let locus = genie.session.as_ref().unwrap().locus();
        assert_eq!(locus, "chr1:1-1,000 chr2:101-200");
        let mut ui = simulator(genie.view());
        let ruler = ui.find(selector::id("ruler-1")).unwrap().bounds().center();
        ui.point_at(ruler);
        let left = iced::mouse::Button::Left;
        let _ = ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(left))]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::Ruler(1, Sweep::Pressed(_)))),
            "{messages:?}"
        );
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
        let _ = genie.update(Message::Pan(0, -100.0));
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
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.3)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.2)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.1)));
        let (_, start, end) = genie.sweep.unwrap();
        assert!((start - 300.0).abs() < 0.01 && (end - 100.0).abs() < 0.01);
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
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
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.101)));
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.5)));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert_eq!(genie.sweep, None);
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
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
        let _ = genie.update(Message::Pan(0, -100.0));
        assert_eq!(genie.jump_from(), Some(520));
    }

    #[test]
    fn hovering_or_a_menu_points_at_a_track() {
        let mut genie = with_track();
        assert_eq!(genie.pointed, None);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(10.0, 10.0))));
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

    fn bed_entry(name: &str, contig: &str, start: u64, end: u64) -> genie_formats::features::Entry {
        genie_formats::features::Entry {
            name: name.to_owned(),
            contig: contig.to_owned(),
            span: Region::between(start, end),
        }
    }

    #[test]
    fn imported_regions_join_the_list() {
        let mut genie = opened();
        let entries = vec![
            bed_entry("promoter", "chr1", 10, 20),
            bed_entry("", "2", 5, 9),
            bed_entry("lost", "chr9", 0, 5),
            bed_entry("past the end", "chr1", 2_000, 3_000),
        ];
        let _ = genie.update(Message::Regions(Regions::Imported(Ok(entries))));
        settle(&mut genie);
        let found: Vec<(String, usize, Region)> = marks(&genie)
            .into_iter()
            .map(|mark| (mark.name, mark.contig, mark.span))
            .collect();
        assert_eq!(
            found,
            [
                ("promoter".to_owned(), 0, Region::between(10, 20)),
                ("Region 1".to_owned(), 1, Region::between(5, 9)),
            ]
        );
        assert!(genie.sidebar.is_some());
        let notice = "Imported 2 regions. 2 regions are not on a contig of the reference.";
        assert!(shows(&genie, notice));
        let _ = genie.update(Message::Regions(Regions::Imported(Err("bad".to_owned()))));
        assert!(shows(&genie, "bad"));
    }

    #[test]
    fn exported_regions_are_bed() {
        let scratch = crate::scratch::Scratch::new("regions");
        let marker = scratch.file("marker", "");
        let mut genie = opened();
        go(&mut genie, "chr2:51-150");
        let _ = genie.update(Message::Command(Command::AddRegion));
        go(&mut genie, "chr1:101-200");
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let path = marker.with_file_name("mine");
        let _ = genie.update(Message::Regions(Regions::ExportPicked(Some(path.clone()))));
        let saved = std::fs::read_to_string(path.with_file_name("mine.bed")).unwrap();
        assert_eq!(saved, "chr1\t100\t200\tRegion 2\nchr2\t50\t150\tRegion 1\n");
        assert!(shows(&genie, "Saved mine.bed."));
    }

    #[test]
    fn region_files_are_read_as_bed() {
        let scratch = crate::scratch::Scratch::new("region-files");
        let good = scratch.file("good.bed", "chr1\t10\t20\tA\n");
        let entries = super::regions::read(&good).unwrap();
        assert_eq!(entries, [bed_entry("A", "chr1", 10, 20)]);
        let bad = scratch.file("bad.bed", "chr1\tx\t20\n");
        let error = super::regions::read(&bad).unwrap_err();
        assert!(
            error.starts_with("The file bad.bed is not valid."),
            "{error}"
        );
    }

    #[test]
    fn shift_drags_on_the_ruler_add_a_region() {
        let mut genie = opened();
        let _ = genie.update(Message::Modifiers(iced::keyboard::Modifiers::SHIFT));
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.3)));
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
        settle(&mut genie);
        let added = marks(&genie);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].span, Region::between(100, 300));
        assert_eq!(chosen(&genie), Some(added[0].id));
        assert!(shows(&genie, "chr1:1-1,000"));
        let _ = genie.update(Message::Modifiers(iced::keyboard::Modifiers::empty()));
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.3)));
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
        settle(&mut genie);
        assert_eq!(marks(&genie).len(), 1);
        assert!(shows(&genie, "chr1:101-300"));
    }

    #[test]
    fn region_bands_sit_under_the_ruler_and_fit_on_a_click() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        let _ = genie.update(Message::Command(Command::AddRegion));
        let _ = genie.update(Message::Regions(Regions::Toggle));
        go(&mut genie, "chr1:1-1000");
        settle(&mut genie);
        let id = marks(&genie)[0].id;
        let band = || iced_test::selector::id(format!("band-{id}"));
        let mut ui = simulator(genie.view());
        let bounds = ui.find(band()).unwrap().bounds();
        assert!((bounds.x - 100.0).abs() < 0.5 && (bounds.width - 100.0).abs() < 0.5);
        assert!((bounds.height - 6.0).abs() < 0.5);
        ui.click(band()).unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Regions(Regions::Choose(id))));
        go(&mut genie, "chr2");
        settle(&mut genie);
        assert!(simulator(genie.view()).find(band()).is_err());
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
    fn the_address_field_opens_urls() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Command(Command::OpenUrl));
        assert!(shows(&genie, "Open a file from the web"));
        let text = "http://127.0.0.1:9/data/hg38.2bit".to_owned();
        let _ = genie.update(Message::Address(Address::Edit(text)));
        let _ = genie.update(Message::Address(Address::Submit));
        assert_eq!(genie.address, None);
        assert!(shows(&genie, "Opening hg38.2bit…"));
    }

    #[test]
    fn bad_addresses_stay_with_an_error() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Command(Command::OpenUrl));
        let text = "data/genes.bed".to_owned();
        let _ = genie.update(Message::Address(Address::Edit(text)));
        let _ = genie.update(Message::Address(Address::Submit));
        let error = "The address data/genes.bed does not start with https:// or http://.";
        assert!(shows(&genie, error));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert_eq!(genie.address, None);
        let _ = genie.update(Message::Command(Command::OpenUrl));
        let _ = genie.update(Message::MenuDismissed);
        assert_eq!(genie.address, None);
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
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
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
    fn without_motion_jumps_land_at_once() {
        let mut genie = opened();
        genie.settings.motion = false;
        go(&mut genie, "chr1:101-200");
        assert!(!genie.is_animating());
        assert!(shows(&genie, "chr1:101-200"));
        let _ = genie.update(Message::Command(Command::Back));
        assert!(!genie.is_animating());
        assert!(shows(&genie, "chr1:1-1,000"));
    }

    #[test]
    fn a_new_reference_forgets_the_region_choice_and_jump_anchor() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::AddRegion));
        genie.anchor = genie.session.as_ref().map(|session| (session.visit(), 10));
        let _ = genie.update(Message::Opened(Ok(reference())));
        assert_eq!(chosen(&genie), None);
        assert_eq!(genie.anchor, None);
        assert_eq!(marks(&genie).len(), 0);
        assert!(genie.sidebar.is_some());
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
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        let _ = genie.update(Message::Pan(0, -100.0));
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
            Some(Message::Modifiers(keyboard::Modifiers::SHIFT))
        );
    }

    #[test]
    fn navigating_drops_an_unsubmitted_draft() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr1:5".to_owned()));
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
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
        let _ = genie.update(Message::Zoom(0, 2.0, 0.0));
        assert!(shows(&genie, "chr1:1-500"));
        let _ = genie.update(Message::Pan(0, -100.0));
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
        assert!(ui.find(WELCOME).is_ok());
    }

    #[test]
    fn dragging_the_overview_window_moves_the_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(0, 4.0, 0.0));
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.0625)));
        let _ = genie.update(Message::Overview(Sweep::Dragged(0.5)));
        let _ = genie.update(Message::Overview(Sweep::Released));
        assert!(shows(&genie, "chr1:438-688"));
    }

    #[test]
    fn pressing_the_overview_outside_the_window_centers_there() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(0, 4.0, 0.0));
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.75)));
        assert!(shows(&genie, "chr1:626-875"));
    }

    #[test]
    fn overview_drags_stop_at_release() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(0, 4.0, 0.0));
        let _ = genie.update(Message::Overview(Sweep::Pressed(0.1)));
        let _ = genie.update(Message::Overview(Sweep::Released));
        let _ = genie.update(Message::Overview(Sweep::Dragged(0.9)));
        assert!(shows(&genie, "chr1:1-250"));
    }

    fn banded() -> Genie {
        let band = |start, end, name: &str| {
            (
                "chr1".to_owned(),
                Band {
                    span: Region::between(start, end),
                    name: name.to_owned(),
                    stain: Stain::Gneg,
                },
            )
        };
        let rows = [
            band(0, 500, "p11"),
            band(500, 1_000, "q11"),
            band(1_000, 1_000, ""),
        ];
        let banded = Arc::try_unwrap(reference()).unwrap().with_bands(&rows);
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(Arc::new(banded))));
        genie
    }

    fn point_at_strip(genie: &mut Genie, local: f32, x: f32) -> Point {
        let at = Point::new(x, crate::chrome::BAR_HEIGHT + 6.0);
        let local = Point::new(local, 6.0);
        let _ = genie.update(Message::Strip(Some(Pointer { local, at })));
        at
    }

    #[test]
    fn hovering_the_overview_names_the_band() {
        let mut genie = banded();
        let width = genie.session.as_ref().unwrap().viewport.width() as f32;
        point_at_strip(&mut genie, width * 0.75, width * 0.75);
        assert_eq!(genie.band_tip().as_deref(), Some("1q11"));
        point_at_strip(&mut genie, width * 0.1, width * 0.1);
        assert_eq!(genie.band_tip().as_deref(), Some("1p11"));
        let _ = genie.update(Message::Strip(None));
        assert_eq!(genie.band_tip(), None);
        let _ = genie.update(Message::Opened(Ok(reference())));
        point_at_strip(&mut genie, width * 0.75, width * 0.75);
        assert_eq!(genie.band_tip(), None);
    }

    #[test]
    fn the_band_name_shows_under_the_strip_beside_the_pointer() {
        let mut genie = banded();
        let width = genie.session.as_ref().unwrap().viewport.width() as f32;
        let at = point_at_strip(&mut genie, width * 0.75, width * 0.75);
        {
            let mut ui = simulator(genie.view());
            let strip = ui.find(selector::id("overview")).unwrap().bounds();
            let name = ui.find("1q11").unwrap().bounds();
            assert!(name.y >= strip.y + strip.height, "{name:?} {strip:?}");
            assert!(name.x > at.x, "{name:?} {at:?}");
        }
        let _ = genie.update(Message::Strip(None));
        assert!(!shows(&genie, "1q11"));
    }

    #[test]
    fn near_the_right_edge_the_band_name_turns_left() {
        let mut genie = banded();
        let width = genie.session.as_ref().unwrap().viewport.width() as f32;
        let window = genie.window;
        let at = point_at_strip(&mut genie, width - 2.0, window.width - 4.0);
        let mut ui =
            iced_test::Simulator::with_size(iced::Settings::default(), window, genie.view());
        let name = ui.find("1q11").unwrap().bounds();
        assert!(name.x + name.width < at.x, "{name:?} {at:?}");
    }

    fn with_recent(entries: &[&str]) -> Genie {
        let mut genie = Genie::default();
        for entry in entries.iter().rev() {
            genie.recent.add(Location::parse(entry).unwrap().into());
        }
        genie
    }

    #[test]
    fn welcome_lists_recent_files_and_addresses() {
        let genie = with_recent(&[
            "/data/hg38.2bit",
            "/data/demo.fa",
            "https://example.org/data/remote.2bit",
        ]);
        let mut ui = simulator(genie.view());
        assert!(ui.find("hg38.2bit").is_ok());
        assert!(ui.find("demo.fa").is_ok());
        assert!(ui.find("remote.2bit").is_ok());
        assert!(ui.find("example.org").is_ok());
    }

    #[test]
    fn the_start_view_lists_genomes_to_open() {
        let genie = with_recent(&["/data/demo.fa"]);
        let mut ui = simulator(genie.view());
        assert!(ui.find(WELCOME).is_ok());
        for text in [
            "Genomes",
            "Human",
            "GRCh38",
            "T2T-CHM13v2.0",
            "Zebrafish",
            "sacCer3",
        ] {
            assert!(ui.find(text).is_ok(), "{text}");
        }
        assert!(ui.find("demo.fa").is_ok());
        ui.click("hg38").unwrap();
        ui.click("T2T").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [
                Message::Genome("hg38".to_owned()),
                Message::Genome("hs1".to_owned())
            ]
        );
    }

    #[test]
    fn clicking_a_recent_entry_opens_it() {
        let address = "https://example.org/data/remote.2bit";
        let genie = with_recent(&["/data/hg38.2bit", address]);
        let mut ui = simulator(genie.view());
        ui.click("hg38.2bit").unwrap();
        ui.click("remote.2bit").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [
                Message::Reopen(Location::File(PathBuf::from("/data/hg38.2bit"))),
                Message::Reopen(Location::parse(address).unwrap()),
            ]
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
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: None,
            over: None,
            mode: None,
            hue: Hue::Blue,
            height: None,
            options: crate::track::Options::default(),
            plot: crate::track::Plot::default(),
        };
        let saved = crate::saved::Saved {
            reference: PathBuf::from("ref.fa").into(),
            genome: None,
            locus: None,
            reading: crate::reading::Reading::default(),
            tracks: vec![saved_track("a.bed.gz"), saved_track("b.bed.gz")],
            regions: Vec::new(),
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
    fn sessions_keep_the_regions() {
        let mut genie = opened();
        go(&mut genie, "chr2:51-150");
        let _ = genie.update(Message::Command(Command::AddRegion));
        settle(&mut genie);
        let saved = genie.saved().unwrap();
        let region = crate::saved::SavedRegion {
            name: "Region 1".to_owned(),
            locus: "chr2:51-150".to_owned(),
        };
        assert_eq!(saved.regions, [region]);
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        let back: Vec<(String, usize, Region)> = marks(&restored)
            .into_iter()
            .map(|mark| (mark.name, mark.contig, mark.span))
            .collect();
        assert_eq!(back, [("Region 1".to_owned(), 1, Region::between(50, 150))]);
    }

    #[test]
    fn sessions_keep_every_panel() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200 chr2:51-150");
        let saved = genie.saved().unwrap();
        assert_eq!(saved.locus.as_deref(), Some("chr1:101-200 chr2:51-150"));
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        let session = restored.session.as_ref().unwrap();
        assert_eq!(session.locus(), "chr1:101-200 chr2:51-150");
        assert!((session.viewport.width() - 499.5).abs() < 0.01);
    }

    #[test]
    fn welcome_offers_to_restore_the_last_session() {
        let last = crate::saved::Saved {
            reference: PathBuf::from("/data/hg38.fa").into(),
            genome: None,
            locus: Some("chr1:1,000-2,000".to_owned()),
            reading: crate::reading::Reading::default(),
            tracks: vec![crate::saved::SavedTrack {
                location: PathBuf::from("/data/reads.bam").into(),
                name: None,
                over: None,
                mode: None,
                hue: Hue::Blue,
                height: None,
                options: crate::track::Options::default(),
                plot: crate::track::Plot::default(),
            }],
            regions: Vec::new(),
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
        let reads = Location::File(PathBuf::from("/data/reads.bam"));
        assert_eq!(genie.pending, [reads]);
        assert!(genie.restoring.is_some());
        let mut genome = genie.restoring.clone().unwrap();
        genome.genome = Some("hg38".to_owned());
        genome.tracks.clear();
        let genie = Genie {
            last: Some(genome),
            ..Genie::default()
        };
        assert!(shows(&genie, "hg38 · chr1:1,000-2,000"));
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
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
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
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
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
        assert_eq!(genie.pending, [Location::File(folder.join("genes.bed.gz"))]);
        assert_eq!(genie.recent.entries()[0], Location::File(file).into());
        let _ = genie.update(Message::Opened(Ok(reference_at(folder.join("ref.fa")))));
        assert!(shows(&genie, "chr1:101-200"));
        let source = Arc::new(Source {
            location: folder.join("genes.bed.gz").into(),
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
        assert_eq!(saved.reference, Location::File(PathBuf::from("ref.fa")));
        assert_eq!(saved.locus.as_deref(), Some("chr1:1-1,000"));
        let locations: Vec<Location> = saved
            .tracks
            .iter()
            .map(|track| track.location.clone())
            .collect();
        assert_eq!(
            locations,
            [Location::File(PathBuf::from("/data/genes.bed.gz"))]
        );
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, 5.0))));
        assert!(shows(&genie, "BRCA1"));
        assert!(shows(&genie, "chr1:11-20"));
        assert!(shows(&genie, "10 bp · reverse strand"));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, 5.0))));
        let _ = genie.update(Message::Pan(0, -40.0));
        assert!(genie.hover.is_none());
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, 5.0))));
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
            location: PathBuf::from("/data/reads.bam").into(),
            name: "reads.bam".to_owned(),
            format: Format::Reads(ReadFile {
                index: genie_formats::bai::Index::default(),
                contigs: vec!["chr1".to_owned()],
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, Options::default());
        for read in reads {
            gather.add(read);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.0, 5.0))));
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
        let _ = genie.update(Message::TrackChanged(0, Change::Sort(0, 20)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.slot.order.as_ref().map(|(at, _)| *at), Some(20));
    }

    #[test]
    fn read_menus_sort_the_panel_under_the_pointer() {
        let mut genie = with_reads();
        go(&mut genie, "chr1:1-100 chr1:501-600");
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot_mut(1).wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, Options::default());
        gather.add(read("read9", 510));
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(1, 0, region, Ok(Arc::new(loaded)));
        let x = 500.5 + 20.0 * 4.995 + 2.5;
        let _ = genie.update(Message::TrackMenu(0, Point::new(x, 200.0)));
        assert!(shows(&genie, "Sort by base at 521"));
        let _ = genie.update(Message::TrackChanged(0, Change::Sort(1, 520)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.slot(1).order.as_ref().map(|(at, _)| *at), Some(520));
        assert!(track.slot.order.is_none());
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
        assert!(track.slot.wanted.is_some());
    }

    fn with_gff() -> Genie {
        let mut genie = opened();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/genes.gff3.gz").into(),
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
            with_two_signals(),
        ];
        for mut genie in tracks {
            let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
            let session = genie.session.as_ref().unwrap();
            let links = super::view::links(session, &session.tracks[0]).1.len();
            let expected = menu::height(&session.tracks[0], links);
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
            location: PathBuf::from("/data/calls.vcf").into(),
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
        track.slot.wanted = Some((0, region));
        let variants = vec![site(20, vec![Call::Het, Call::Alternate])];
        let loaded = Loaded::variants(region, variants, 2);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        genie
    }

    fn signal_source(format: Format) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from("/data/coverage.bedGraph").into(),
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
        track.slot.wanted = Some((0, region));
        let values = vec![
            Summary::single(Region::between(0, 100), 1.0),
            Summary::single(Region::between(100, 200), 4.0),
        ];
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::signal(region, values, 1))),
        );
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(150.0, 20.0))));
        assert!(shows(&genie, "Value 4"));
        assert!(shows(&genie, "chr1:101-200"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(500.0, 20.0))));
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
        super::view::signal_scale(session, track, 0, &columns)
    }

    fn other_signal() -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from("/data/other.bedGraph").into(),
            name: "other.bedGraph".to_owned(),
            format: signal_table(),
        })
    }

    fn with_two_signals() -> Genie {
        let mut genie = with_signal();
        let _ = genie.update(Message::TrackOpened(Ok(other_signal())));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[1];
        track.slot.wanted = Some((0, region));
        let values = vec![Summary::single(Region::between(300, 400), 10.0)];
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::signal(region, values, 1))),
        );
        genie
    }

    fn overlays(genie: &Genie) -> Vec<(Option<u64>, Hue)> {
        let session = genie.session.as_ref().unwrap();
        session
            .tracks
            .iter()
            .map(|track| (track.over, track.hue))
            .collect()
    }

    #[test]
    fn figures_draw_every_panel() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200 chr1:801-900");
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let (id, (_, region)) = (track.id, track.slot(1).wanted.unwrap());
        let feature = Feature::new("TP53", Region::between(850, 860), Strand::Forward);
        let loaded = Arc::new(Loaded::features(region, vec![feature]));
        let _ = genie.update(Message::TrackLoaded(id, 1, 0, region, Ok(loaded)));
        let svg = genie.figure(genie.session.as_ref().unwrap());
        for text in [
            ">chr1:101-200</text>",
            ">chr1:801-900</text>",
            ">TP53</text>",
        ] {
            assert!(svg.contains(text), "{text}\n{svg}");
        }
        assert!(!svg.contains("chr1:101-200 chr1:801-900"), "{svg}");
    }

    #[test]
    fn figures_hold_the_caption_the_ruler_and_the_tracks_as_svg() {
        let mut genie = with_two_signals();
        let _ = genie.update(Message::TrackChanged(1, Change::Overlay(0)));
        let svg = genie.figure(genie.session.as_ref().unwrap());
        assert!(svg.starts_with("<svg"), "{svg}");
        for text in [
            ">ref.fa</tspan>",
            ">chr1:1-1,000</tspan>",
            ">Sequence</text>",
            ">coverage.bedGraph</text>",
            ">other.bedGraph</text>",
            ">0-10</text>",
        ] {
            assert!(svg.contains(text), "{text}\n{svg}");
        }
        assert!(svg.contains("clip-path=\"url(#"), "{svg}");
        assert_eq!(svg.matches("<svg").count(), 1);
    }

    #[test]
    fn exports_save_svg_for_svg_names() {
        let scratch = crate::scratch::Scratch::new("export-svg");
        let path = scratch.file("marker", "").with_file_name("view.svg");
        let mut genie = with_signal();
        let _ = genie.update(Message::ExportPicked(Some(path.clone())));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("<svg"), "{text}");
        assert!(shows(&genie, "Saved view.svg."));
    }

    #[test]
    fn snapshot_orders_save_svg_for_svg_names() {
        let scratch = crate::scratch::Scratch::new("port-svg");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        let mut genie = opened();
        load_tiles(&mut genie);
        let line = format!("snapshotDirectory \"{}\"", folder.display());
        assert_eq!(told(&ask(&mut genie, &line)).as_deref(), Some("OK"));
        assert_eq!(
            told(&ask(&mut genie, "snapshot view.svg")).as_deref(),
            Some("OK")
        );
        let text = std::fs::read_to_string(folder.join("view.svg")).unwrap();
        assert!(text.starts_with("<svg"), "{text}");
    }

    #[test]
    fn signal_tracks_overlay_in_one_lane_and_separate_again() {
        let mut genie = with_two_signals();
        let at = Point::new(100.0, 200.0);
        let _ = genie.update(Message::TrackMenu(1, at));
        assert!(shows(&genie, "Overlay on"));
        let _ = genie.update(Message::TrackChanged(1, Change::Overlay(0)));
        assert_eq!(overlays(&genie), [(None, Hue::Blue), (Some(0), Hue::Green)]);
        {
            let mut ui = simulator(genie.view());
            assert!(ui.find(selector::id("lane-1")).is_err());
            assert!(ui.find("other.bedGraph").is_ok());
        }
        assert_eq!(
            scale_of(&genie, 0),
            Some(Scale {
                min: 0.0,
                max: 10.0
            })
        );
        let _ = genie.update(Message::TrackMenu(0, at));
        assert!(shows(&genie, "Separate"));
        assert!(!shows(&genie, "Overlay on"));
        let _ = genie.update(Message::TrackChanged(1, Change::Separate));
        assert_eq!(overlays(&genie)[1].0, None);
        assert!(simulator(genie.view()).find(selector::id("lane-1")).is_ok());
        let _ = genie.update(Message::TrackChanged(1, Change::Overlay(0)));
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert_eq!(overlays(&genie), [(None, Hue::Green)]);
    }

    #[test]
    fn sessions_keep_overlays_whatever_opens_first() {
        let mut genie = with_two_signals();
        let _ = genie.update(Message::TrackChanged(1, Change::Overlay(0)));
        let saved = genie.saved().unwrap();
        let links: Vec<Option<usize>> = saved.tracks.iter().map(|track| track.over).collect();
        assert_eq!(links, [None, Some(0)]);
        let mut other = opened();
        other.restoring = Some(saved);
        other.waiting = 2;
        let _ = other.update(Message::TrackOpened(Ok(other_signal())));
        let _ = other.update(Message::TrackOpened(Ok(signal_source(signal_table()))));
        let session = other.session.as_ref().unwrap();
        let named = |name: &str| {
            session
                .tracks
                .iter()
                .find(|track| track.source.name == name)
                .unwrap()
        };
        assert_eq!(
            named("other.bedGraph").over,
            Some(named("coverage.bedGraph").id)
        );
    }

    #[test]
    fn signal_menus_fix_the_range_or_go_back_to_auto() {
        let mut genie = with_signal();
        let at = Point::new(100.0, 200.0);
        let _ = genie.update(Message::TrackChanged(0, Change::Link));
        let _ = genie.update(Message::TrackMenu(0, at));
        assert_eq!(genie.range_draft, Some(("0".to_owned(), "4".to_owned())));
        assert!(shows(&genie, "Auto range"));
        let _ = genie.update(Message::Range(0, Ranging::Max("50".to_owned())));
        let _ = genie.update(Message::Range(0, Ranging::Fix));
        let fixed = Scale {
            min: 0.0,
            max: 50.0,
        };
        assert_eq!(scale_of(&genie, 0), Some(fixed));
        assert!(!genie.session.as_ref().unwrap().tracks[0].plot.linked);
        assert!(!shows(&genie, "Auto range"));
        let _ = genie.update(Message::TrackMenu(0, at));
        assert_eq!(genie.range_draft, Some(("0".to_owned(), "50".to_owned())));
        let _ = genie.update(Message::Range(0, Ranging::Min("60".to_owned())));
        let _ = genie.update(Message::Range(0, Ranging::Fix));
        assert!(shows(
            &genie,
            "Type two numbers. The maximum must be larger than the minimum."
        ));
        assert_eq!(scale_of(&genie, 0), Some(fixed));
        let _ = genie.update(Message::Range(0, Ranging::Auto));
        assert_eq!(scale_of(&genie, 0), Some(Scale { min: 0.0, max: 4.0 }));
        let _ = genie.update(Message::TrackMenu(0, at));
        let _ = genie.update(Message::Range(0, Ranging::Fix));
        let _ = genie.update(Message::TrackChanged(0, Change::Link));
        let plot = genie.session.as_ref().unwrap().tracks[0].plot;
        assert!(plot.linked);
        assert_eq!(plot.range, None);
    }

    #[test]
    fn linked_signal_tracks_share_one_scale() {
        let mut genie = with_signal();
        let other = Arc::new(Source {
            location: PathBuf::from("/data/other.bedGraph").into(),
            name: "other.bedGraph".to_owned(),
            format: signal_table(),
        });
        let _ = genie.update(Message::TrackOpened(Ok(other)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[1];
        track.slot.wanted = Some((0, region));
        let values = vec![Summary::single(Region::between(300, 400), 10.0)];
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::signal(region, values, 1))),
        );
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
    fn big_cohorts_show_groups_of_samples() {
        let mut genie = opened();
        let samples: Vec<String> = (1..=3_202).map(|index| format!("S{index}")).collect();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/cohort.vcf").into(),
            name: "cohort.vcf".to_owned(),
            format: Format::Variants(VariantFile::Table {
                table: genie_formats::features::Table::new(Vec::new()),
                samples,
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let mut calls = vec![Call::Reference; 3_202];
        calls[25] = Call::Het;
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let loaded = Loaded::variants(region, vec![site(20, calls)], 3_202);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let session = genie.session.as_ref().unwrap();
        let height = super::view::body_height(session, &session.tracks[0]);
        assert!(height < 900.0, "{height}");
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 13.0 + 7.5))));
        assert!(shows(&genie, "S23 to S33 (11 samples) · heterozygous"));
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "rs20"));
        assert!(shows(&genie, "A > G · QUAL 50 · PASS"));
        assert!(shows(&genie, "DP=12"));
        assert!(!shows(&genie, "NA2 · homozygous alternate"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 28.0))));
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.0, 5.0))));
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
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(500.0, 5.0))));
        assert!(genie.hover.is_none());
    }

    #[test]
    fn clicking_pins_the_card_until_dismissed() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackClicked(0, 0, over(15.0, 5.0)));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        assert!(shows(&genie, "chr1:11-20"));
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "chr1:11-20"));
    }

    #[test]
    fn removing_a_track_drops_its_cards() {
        let mut genie = with_features();
        let _ = genie.update(Message::TrackClicked(0, 0, over(15.0, 5.0)));
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
        assert_eq!(
            opened().recent.entries(),
            [Entry::Location(Location::File(PathBuf::from("ref.fa")))]
        );
        let mut genie = Genie::default();
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        assert_eq!(genie.recent.entries(), [Entry::Genome("hg38".to_owned())]);
    }

    #[test]
    fn recent_genomes_open_from_the_start_view() {
        let mut genie = Genie::default();
        genie.recent.add(Entry::Genome("mm10".to_owned()));
        let mut ui = simulator(genie.view());
        ui.click("Mouse GRCm38").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::Genome("mm10".to_owned())]
        );
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
        assert!(ui.find(WELCOME).is_ok());
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
