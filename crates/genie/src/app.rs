use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use genie_core::{History, Locus, Marks, Named, Names, Region, Span, Target, Viewport, Visit};
use genie_formats::features::Entry;
use genie_formats::fields::Rule;
use genie_formats::http::Url;
use iced::theme::Mode;
use iced::time::Instant;
use iced::{
    Animation, Event, Point, Size, Subscription, Task, Theme, event, keyboard, mouse, system,
    window,
};

use crate::card::{Card, Changes};
use crate::chrome::{self, Action};
use crate::export::Figure;
use crate::hub::Hub;
use crate::location::Location;
use crate::motion::{self, Transition};
use crate::port::{Order, Reply};
use crate::prepare::{Step, Work};
use crate::reading::{self, Reading};
use crate::recent::{self, Hubs, Recent};
use crate::reference::Reference;
use crate::saved::{Saved, SavedTrack};
use crate::scene::Pointer;
use crate::settings::{Setting, Settings};
use crate::shortcuts::{self, Command};
use crate::theme;
use crate::tiles::{Key, Tiles};
use crate::track::{self, Change, Loaded, Prepared, Source, Track};
use crate::{font, paths, timer, update};

mod address;
mod app_menu;
mod assemblies;
mod blat;
mod caching;
mod cells;
#[cfg(test)]
mod clips;
mod copy;
mod dmrs;
mod dotplot;
mod effects;
mod export;
mod figure;
mod filters;
mod find;
mod gnomad;
mod hubs;
mod isoforms;
mod lift;
mod made;
mod motifs;
mod navigation;
mod offline;
mod orders;
mod palette;
mod primers;
mod regions;
mod review;
mod samples;
mod sessions;
pub mod snapshot;
mod table;
mod track_list;
mod track_settings;
mod tracks;
mod ucsc;
mod undo;
mod updates;
mod view;
mod whole;

pub(crate) use assemblies::Assembling;
pub(crate) use blat::Blatting;
pub(crate) use cells::Celling;
pub(crate) use dmrs::Dmring;
pub(crate) use dotplot::Plotting;
pub(crate) use export::Figuring;
pub(crate) use gnomad::Gnomading;
pub(crate) use isoforms::Isoforming;
pub(crate) use lift::Lifting;
pub(crate) use made::{Making, additions};
pub(crate) use motifs::Motifing;
use orders::Answer;
pub(crate) use primers::Priming;
pub(crate) use review::Reviewing;
pub(crate) use samples::{Arranging, Sampling, active, arranging_words};
pub(crate) use table::Tabling;
pub(crate) use track_list::Listing;
use tracks::pick;
pub(crate) use updates::Updating;

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
    /// The menu of a folder of the tracks panel, and where it opens.
    folder_menu: Option<(String, Point)>,
    resize: Option<(u64, f32)>,
    moving: Option<(u64, f32)>,
    lane_hover: Option<u64>,
    scroll: f32,
    window: Size,
    hover: Option<Card>,
    pinned: Option<Card>,
    field: Option<Choice>,
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
    cache_size: Option<u64>,
    download: Option<Download>,
    busy: usize,
    answers: Vec<(Answer, Reply)>,
    looking: Option<Reply>,
    folder: Option<PathBuf>,
    renaming: Option<(u64, String)>,
    range_draft: Option<(String, String)>,
    track_draft: Option<crate::track_dialog::Draft>,
    palette: Option<crate::palette::State>,
    finder: Option<find::Finder>,
    table: Option<table::VariantTable>,
    /// The table of the DMRs of the methylation difference of a track.
    dmrs: Option<dmrs::DmrTable>,
    /// The table of the isoforms in view of a read track.
    isoforms: Option<isoforms::IsoformTable>,
    field_draft: Option<String>,
    hubs: Vec<Arc<Hub>>,
    hub_view: Option<hubs::View>,
    hub_adding: Vec<Location>,
    /// The hubs to read once the next reference opens, and whether to show each one.
    hubs_next: Vec<(Location, bool)>,
    known_hubs: Hubs,
    checks: update::Checks,
    release: update::State,
    effects: effects::Effects,
    assemblies: assemblies::Assemblies,
    blat: blat::Blat,
    /// The tracks panel at the left of the tracks, when it is open.
    track_list: Option<track_list::TrackList>,
    /// The gnomAD answers, and the gnomAD part of the open variant card.
    gnomad: gnomad::Lookups,
    /// The dialog of the restriction enzymes of a track made from the reference.
    enzymes: made::Enzymes,
    /// The dialog of the motifs of a motif file, when it is open.
    motif_dialog: Option<motifs::MotifDialog>,
    /// The dialog of primer design and its work.
    primers: Option<primers::PrimerDialog>,
    /// The review of the variants of a track.
    review: Option<review::Reviewer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popup {
    Settings,
    Genomes,
    Catalog,
    Help,
    App,
    AppRecent,
    /// The genie menu with its list of the tracks that genie makes from the reference.
    AppMade,
    Track(u64),
    Palette,
    Hub,
    Export,
    Assemblies,
    Blat,
    /// The dialog of the restriction enzymes.
    Enzymes,
    /// The dialog of the motifs of a motif file.
    Motifs,
    /// The dialog of primer design.
    Primers,
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
            folder_menu: None,
            resize: None,
            moving: None,
            lane_hover: None,
            scroll: 0.0,
            window: chrome::WINDOW,
            hover: None,
            pinned: None,
            field: None,
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
            cache_size: None,
            download: None,
            busy: 0,
            answers: Vec::new(),
            looking: None,
            folder: None,
            renaming: None,
            range_draft: None,
            track_draft: None,
            palette: None,
            finder: None,
            table: None,
            dmrs: None,
            isoforms: None,
            field_draft: None,
            hubs: Vec::new(),
            hub_view: None,
            hub_adding: Vec::new(),
            hubs_next: Vec::new(),
            known_hubs: Hubs::default(),
            checks: update::Checks::default(),
            release: update::State::default(),
            effects: effects::Effects::default(),
            assemblies: assemblies::Assemblies::default(),
            blat: blat::Blat::default(),
            track_list: None,
            gnomad: gnomad::Lookups::default(),
            enzymes: made::Enzymes::default(),
            motif_dialog: None,
            primers: None,
            review: None,
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
    /// A button next to the text: its label and its message.
    action: Option<(String, Message)>,
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
    preparing: Vec<Preparing>,
    /// The whole-genome view: the panels show the main chromosomes, each by its length.
    genome: bool,
    /// The sample table that groups, sorts and colors the samples of the tracks.
    samples: Option<samples::Samples>,
    /// The title and the parts of exported images.
    figure: Figure,
    /// The steps of the work on tracks and region marks, to undo and to redo.
    steps: undo::Steps,
    /// The dot plot of a PAF track, which shows in place of the tracks.
    plot: Option<Box<dotplot::DotPlot>>,
}

#[derive(Debug)]
struct Preparing {
    id: u64,
    location: Location,
    name: String,
    step: Step,
    share: f32,
    stop: Arc<AtomicBool>,
    saved: Option<SavedTrack>,
    reply: Option<(Reply, Option<String>)>,
}

impl Drop for Preparing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
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
            preparing: Vec::new(),
            genome: false,
            samples: None,
            figure: Figure::default(),
            steps: undo::Steps::default(),
            plot: None,
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
        let widths = self.widths(width);
        let mut left = 0.0;
        for (panel, width) in widths.iter().enumerate() {
            if x < left + width + PANEL_GAP || panel + 1 == widths.len() {
                return (panel, x - left);
            }
            left += width + PANEL_GAP;
        }
        (0, x)
    }

    fn motion(&mut self, panel: usize) -> Option<(&mut Viewport, &mut Option<Transition>)> {
        if self.genome {
            return None;
        }
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
        self.genome = false;
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
        if self.genome || self.panels.is_empty() {
            return;
        }
        self.panels.clear();
        for track in &mut self.tracks {
            track.extra.clear();
        }
        self.refit(width);
    }

    fn refit(&mut self, width: f32) {
        if self.genome {
            return self.fit_genome(width);
        }
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
        if self.genome {
            return self.fit_genome(width);
        }
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
            genome: self.genome,
            plot: self.plot.as_ref().and_then(|plot| plot.shown()),
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
        if self.genome {
            return;
        }
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

    /// Keeps the names of a track, when its lift did not change since genie read them.
    fn name(&mut self, (id, lift): (u64, u16), names: Arc<Names>) {
        if let Some(track) = self
            .tracks
            .iter_mut()
            .find(|track| track.id == id && track.options.lift == lift)
        {
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
        if self.genome {
            return "All".to_owned();
        }
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
    Prepare(Work),
    Preparing(u64, Preparation),
    TrackLoaded(u64, usize, usize, Region, Result<Arc<Loaded>, String>),
    NoticeDismissed,
    TrackMenu(u64, Point),
    TrackChanged(u64, Change),
    MenuDismissed,
    Scrolled(f32),
    WindowResized(Size),
    TrackHovered(u64, usize, Option<Pointer>),
    TrackClicked(u64, usize, Pointer),
    /// The changes of the variant of a card, with the number of the request.
    Effects(u64, Changes),
    Jump(u64, bool),
    Jumped(u64, bool, Result<Option<(usize, Region)>, String>),
    Regions(Regions),
    Lift(Lifting),
    Table(Tabling),
    Address(Address),
    Modifiers(keyboard::Modifiers),
    CardClosed,
    /// Shows the read of the pinned card and its mate side by side.
    ViewMate,
    /// The names of the features of a track, with the number of the lift of the track.
    NamesLoaded((u64, u16), Result<Arc<Names>, String>),
    Looked(Option<Named>, String),
    Figure(Figuring),
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
    CatalogMenu,
    AppMenu,
    RecentMenu(bool),
    TrackSettings(u64),
    TrackDraft(Drafting),
    Palette(Paletting),
    Catalog(usize),
    CatalogOpened(usize, Result<Arc<Source>, String>),
    Offline(Offline),
    CopyMenu(Option<u64>, Point),
    Copy(Copying),
    Port(Port),
    Rename(Renaming),
    Range(u64, Ranging),
    Panel(usize, Paneling),
    Filter(u64, Filtering),
    Field(Choice),
    Visit(String),
    Cache(Caching),
    Find(find::Finding),
    Hub(hubs::Hubbing),
    Update(updates::Updating),
    Samples(Sampling),
    Assemblies(Assembling),
    Blat(Blatting),
    TrackList(track_list::Listing),
    /// Shows, hides, renames or removes a folder of the tracks panel.
    Folder(String, track_list::Foldering),
    /// Opens the menu of a folder of the tracks panel at a place.
    FolderMenu(String, Point),
    /// The dot plot of a PAF track, in place of the tracks.
    Plot(Plotting),
    /// The gnomAD frequencies of the variant of the open card.
    Gnomad(Gnomading),
    /// The table of the DMRs of a methylation difference.
    Dmrs(Dmring),
    /// The table of the isoforms of a read track.
    Isoforms(Isoforming),
    /// The cell table of a single-cell track.
    Cells(Celling),
    /// Adds or changes a track that genie makes from the reference.
    Made(Making),
    /// Picks the motifs of a motif track, or changes its threshold.
    Motifs(Motifing),
    /// Designs primers around a target.
    Primers(Priming),
    /// Reviews the variants of a track.
    Review(Reviewing),
    /// Makes the dot of the pinned card the lead of the LocusZoom colors of its GWAS track.
    Lead,
    /// The sequence and the track of a GenBank or EMBL file.
    RecordsOpened(
        Location,
        Result<Arc<Reference>, String>,
        Result<Arc<Source>, String>,
    ),
    /// Shows the list of the tracks that genie makes from the reference beside the genie menu,
    /// or hides it.
    MadeMenu(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caching {
    Measured(u64),
    Clear,
    Cleared(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub track: u64,
    pub label: String,
    pub item: String,
    pub at: Point,
    /// True for a field of the sample table, false for a field of the file of the track.
    pub sample: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filtering {
    Add(Rule),
    Remove(usize),
    Label(Option<String>),
    Draft(String),
    Checked(String, bool),
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
    Prepare(Work, Option<String>, Reply),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Preparation {
    Progress(Step, f32),
    Done(Result<Prepared, String>),
    Opened(Result<Arc<Source>, String>),
    Stop,
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
    hub: bool,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Paletting {
    Query(String),
    Choose(usize),
    Run,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Drafting {
    Name(String),
    Height(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lane {
    Resized(f32),
    Released,
    Fit,
    Moved(f32),
    Dropped,
    Entered,
    Left,
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
            known_hubs: folder.as_deref().map(Hubs::load).unwrap_or_default(),
            checks: folder
                .as_deref()
                .map(update::Checks::load)
                .unwrap_or_default(),
            settings: folder.as_deref().map(Settings::load).unwrap_or_default(),
            blat: blat::Blat::keyed(folder.as_deref().map(blat::load_key).unwrap_or_default()),
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
        let check = if genie.checks.due(update::now()) {
            genie.check_updates(false)
        } else {
            Task::none()
        };
        (
            genie,
            Task::batch([system::theme().map(Message::SystemModeChanged), open, check]),
        )
    }

    /// Opens the files of the command line: the first reference, then the tracks. Without a
    /// reference, a GenBank or EMBL file gives the reference.
    fn launch(&mut self, files: Vec<Location>) -> Task<Message> {
        let (mut tracks, references): (Vec<Location>, Vec<Location>) =
            files.into_iter().partition(track::is_track);
        let records = tracks
            .iter()
            .position(|location| genie_formats::genbank::is_name(&location.name()));
        if let Some(reference) = references
            .into_iter()
            .next()
            .or_else(|| records.map(|at| tracks.remove(at)))
        {
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
                self.hubs_next.clear();
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
        self.forget_matches();
        self.session = Some(Session::new(reference, self.width));
        self.resume();
        let samples = self
            .restoring
            .as_ref()
            .and_then(|saved| saved.samples.clone());
        let mut tasks = vec![self.fetch(), self.remember(), self.reopen_hubs()];
        let pending = std::mem::take(&mut self.pending);
        self.waiting = pending.len();
        if self.waiting == 0 {
            self.restoring = None;
        }
        for location in pending {
            tasks.push(self.open(location));
        }
        if let Some(location) = samples {
            tasks.push(self.open_samples(location));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let started = Instant::now();
        let before = self.undo_point(&message);
        let task = self.handle(message);
        self.keep_step(before);
        self.refresh_table();
        self.refresh_dmrs();
        self.refresh_isoforms();
        let plot = self.refresh_plot();
        self.answer();
        self.stats.update = started.elapsed();
        Task::batch([task, plot])
    }

    /// Sends each message to the work that it asks for, with one arm for each kind of message.
    #[allow(clippy::too_many_lines)]
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
            Message::NamesLoaded(id, names) => self.names_loaded(id, names),
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
            Message::Prepare(work) => return self.prepare(work, None),
            Message::Preparing(id, event) => return self.preparing(id, event),
            Message::TrackLoaded(id, panel, contig, region, result) => {
                return self.track_loaded((id, panel), contig, region, result);
            }
            Message::NoticeDismissed => self.notice = None,
            Message::TrackMenu(id, at) => self.open_menu(id, at),
            Message::TrackChanged(id, change) => return self.change_track(id, change),
            Message::TrackHovered(id, panel, pointer) => return self.hovered(id, panel, pointer),
            Message::TrackClicked(id, panel, pointer) => return self.clicked(id, panel, pointer),
            Message::Effects(..) | Message::Gnomad(_) => return self.card_message(message),
            Message::Jump(id, forward) => return self.jump(id, forward),
            Message::Jumped(id, forward, result) => return self.jumped(id, forward, result),
            Message::Regions(action) => return self.regions(action),
            Message::Lift(lifting) => return self.lift(lifting),
            Message::Table(_)
            | Message::Plot(_)
            | Message::Dmrs(_)
            | Message::Isoforms(_)
            | Message::Cells(_) => {
                return self.pane_message(message);
            }
            Message::Address(action) => return self.address(action),
            Message::Modifiers(modifiers) => self.modifiers = modifiers,
            Message::CardClosed => self.pinned = None,
            Message::ViewMate => return self.view_mate(),
            Message::Filter(id, filtering) => return self.filter(id, filtering),
            Message::Field(choice) => self.choose_field(choice),
            Message::Visit(locus) => return self.show_locus(locus),
            Message::MenuDismissed => self.close_menus(),
            Message::Scrolled(offset) => self.scrolled(offset),
            Message::SequenceMenu(at) => self.open_sequence_menu(at),
            Message::SequenceChanged(change) => self.change_reading(change),
            Message::ToggleSettings => return self.toggle_settings(),
            Message::Cache(event) => return self.caching(event),
            Message::Find(finding) => return self.find(finding),
            Message::Hub(hubbing) => return self.hubbing(hubbing),
            Message::Assemblies(assembling) => return self.assembling(assembling),
            Message::Blat(blatting) => return self.blatting(blatting),
            Message::Made(making) => return self.making(making),
            Message::Motifs(motifing) => return self.motifing(motifing),
            Message::Primers(priming) => return self.priming(priming),
            Message::Review(reviewing) => return self.reviewing(reviewing),
            Message::Lead => self.make_lead(),
            Message::RecordsOpened(location, reference, source) => {
                return self.records_opened(&location, reference, source);
            }
            Message::MadeMenu(open) => self.show_made(open),
            Message::TrackList(_) | Message::Folder(..) | Message::FolderMenu(..) => {
                return self.panel_message(message);
            }
            Message::Update(updating) => return self.updating(updating),
            Message::Samples(sampling) => return self.sampling(sampling),
            Message::Lane(id, lane) => self.lane(id, lane),
            Message::RestoreLast => return self.restore_last(),
            Message::Strip(pointer) => self.strip = pointer,
            Message::Genome(id) => return self.open_genome(&id),
            Message::GenomeMenu => self.toggle(Popup::Genomes),
            Message::CatalogMenu => self.toggle(Popup::Catalog),
            Message::AppMenu => self.toggle_app_menu(),
            Message::RecentMenu(open) => self.show_recent(open),
            Message::TrackSettings(id) => self.open_track_settings(id),
            Message::TrackDraft(drafting) => self.draft_track(drafting),
            Message::Palette(paletting) => return self.palette_message(paletting),
            Message::Catalog(index) => return self.open_catalog(index),
            Message::CatalogOpened(index, result) => return self.catalog_opened(index, result),
            Message::Offline(offline) => return self.offline(offline),
            Message::CopyMenu(region, at) => self.open_copy_menu(region, at),
            Message::Copy(copying) => return self.copying(copying),
            Message::Port(event) => return self.port(event),
            Message::Rename(renaming) => return self.rename(renaming),
            Message::Range(id, ranging) => self.range(id, ranging),
            Message::Panel(panel, paneling) => return self.panel(panel, paneling),
            Message::SettingChanged(setting) => return self.change_setting(setting),
            Message::WindowResized(size) => self.window = size,
            Message::Pan(panel, dx) => return self.pan(panel, dx),
            Message::Zoom(panel, factor, anchor) => return self.zoom(panel, factor, anchor),
            Message::Resized(width) => return self.resize(width),
            Message::TileLoaded(at, key, result) => self.tile_loaded(at, key, result),
            Message::LocusEdited(text) => self.edit_locus(text),
            Message::LocusSubmitted => return self.submit_locus(),
            Message::SuggestionChosen(position) => return self.choose(position),
            Message::Command(command) => return self.command(command),
            Message::Frame(now) => self.frame(now),
            Message::Overview(sweep) => return self.sweep_overview(sweep),
            Message::Ruler(panel, sweep) => return self.sweep_ruler(panel, sweep),
            Message::Figure(figuring) => return self.figuring(figuring),
            Message::ExportPicked(path) => return self.export_to(path),
            Message::SessionPicked(path) => return self.session_picked(path),
            Message::Exported(result, cut) => return self.exported(result, cut),
            Message::NoticeExpired(id) => self.expire(id),
        }
        Task::none()
    }

    /// The messages that add to the open card of a variant: its changes on transcripts and its
    /// gnomAD frequencies.
    fn card_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Effects(generation, lines) => {
                self.effects_found(generation, lines);
                Task::none()
            }
            Message::Gnomad(gnomading) => self.gnomading(gnomading),
            _ => Task::none(),
        }
    }

    /// The messages of the variant table, of the dot plot and of the DMR table.
    fn pane_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Table(tabling) => self.tabling(tabling),
            Message::Plot(plotting) => self.plotting(plotting),
            Message::Dmrs(dmring) => self.dmring(dmring),
            Message::Isoforms(isoforming) => self.isoforming(isoforming),
            Message::Cells(celling) => self.celling(celling),
            _ => Task::none(),
        }
    }

    fn scrolled(&mut self, offset: f32) {
        self.scroll = offset;
        self.close_menus();
    }

    fn open_sequence_menu(&mut self, at: Point) {
        self.sequence_menu = Some(Point::new(at.x, at.y - self.scroll));
    }

    fn names_loaded(&mut self, key: (u64, u16), names: Result<Arc<Names>, String>) {
        let names = names.unwrap_or_default();
        self.on_session(|session| session.name(key, names));
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
        self.show_with(text, fades, None)
    }

    fn show_with(&mut self, text: String, fades: bool, action: Option<(String, Message)>) -> u64 {
        self.notices += 1;
        self.notice = Some(Notice {
            id: self.notices,
            text,
            fades,
            action,
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
    use std::sync::atomic::{AtomicBool, Ordering};

    use genie_core::samples::Table;
    use genie_core::signal::Summary;
    use genie_core::variant::{Annotations, Call, Structure, Sv, Variant};
    use genie_core::{Band, Feature, Names, Region, Stain, Strand};
    use genie_formats::fai::FaiRecord;
    use genie_formats::features::Kind;
    use genie_formats::tabix::Index;
    use genie_render::features::Mode as Rows;
    use genie_render::scene::{Hue, Paint};
    use genie_render::signal::{Scale, Style};
    use iced::keyboard::{self, key::Named};
    use iced::theme::Mode;
    use iced::time::{Duration, Instant};
    use iced_test::{selector, simulator};

    use iced::{Event, Point, Size, mouse};

    use super::tracks::{NO_REFERENCE, REMOTE_SESSION};
    use super::view::{DROP_HINT, SIGNAL_HINT, VARIANT_HINT, WELCOME, ZOOM_HINT};
    use super::{
        Address, Arranging, Caching, Celling, Copying, Drafting, Figuring, Filtering, Genie,
        Isoforming, Lane, Lifting, Message, Offline, Paletting, Paneling, Port, Preparation,
        Ranging, Regions, Renaming, Reviewing, Sampling, Sweep, Tabling, mouse_button, shortcut,
        window_event,
    };
    use crate::card::Changes;
    use crate::export;
    use crate::location::Location;
    use crate::menu;
    use crate::prepare::{self, Check, Step, Work};
    use crate::reading::{self, Frames};
    use crate::recent::Entry;
    use crate::reference::Reference;
    use crate::saved::SavedTrack;
    use crate::scene::Pointer;
    use crate::scratch::Scratch;
    use crate::settings::{Density, Setting};
    use crate::shortcuts::Command;
    use crate::theme::Appearance;
    use crate::tiles::Key;
    use crate::track::{
        Change, Content, FeatureFile, Format, Grouping, LinkFile, Loaded, Options, ReadFile,
        SegmentFile, SignalFile, Source, Toggle, VariantFile,
    };
    use genie_formats::fields::{Rule, Test};

    fn source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: name.to_owned(),
            data: None,
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

    /// Chains that move chr1 of another assembly 100 bases to the right on chr1.
    fn shift() -> Arc<genie_formats::chain::Chains> {
        let text = "chain 1 chr1 2000 + 0 900 chr1 1000 + 100 1000 1\n900\n";
        Arc::new(genie_formats::chain::Chains::read(std::io::Cursor::new(text)).unwrap())
    }

    fn shift_file() -> Location {
        Location::File(PathBuf::from("/data/shift.chain"))
    }

    fn with_lifted() -> Genie {
        let mut genie = with_track();
        let read = Lifting::Read(Some(0), shift_file(), Ok(shift()));
        let _ = genie.update(Message::Lift(read));
        genie
    }

    fn palette_shows(genie: &mut Genie, query: &str, words: &str) -> bool {
        let _ = genie.update(Message::Command(Command::Palette));
        let _ = genie.update(Message::Palette(Paletting::Query(query.to_owned())));
        let found = shows(genie, words);
        let _ = genie.update(Message::Command(Command::Dismiss));
        found
    }

    #[test]
    fn the_palette_lifts_tracks_and_regions() {
        let mut genie = with_track();
        assert!(palette_shows(&mut genie, "lift over", "Lift over…"));
        assert!(!palette_shows(
            &mut genie,
            "lift regions",
            "Lift the regions…"
        ));
        let _ = genie.update(Message::Command(Command::AddRegion));
        assert!(palette_shows(
            &mut genie,
            "lift regions",
            "Lift the regions…"
        ));
        let read = Lifting::Read(Some(0), shift_file(), Ok(shift()));
        let _ = genie.update(Message::Lift(read));
        assert!(palette_shows(&mut genie, "remove lift", "Remove lift"));
        assert!(!palette_shows(&mut genie, "next feature", "Next feature"));
    }

    #[test]
    fn tracks_lift_over_and_back_with_chain_files() {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "Lift over…"));
        assert!(shows(&genie, "Next feature"));
        let read = Lifting::Read(Some(0), shift_file(), Ok(shift()));
        let _ = genie.update(Message::Lift(read));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.lift.is_some());
        assert_eq!(track.contigs, [Some("chr1".to_owned()), None]);
        assert_eq!(track.options.lift, 1);
        assert_eq!(track.slot.wanted, Some((0, Region::between(0, 1_000))));
        assert_eq!(genie.saved().unwrap().tracks[0].lift, Some(shift_file()));
        assert_eq!(genie.saved().unwrap().tracks[0].options, Options::default());
        let names = Arc::new(genie_core::Names::new([genie_core::Named {
            name: "BRCA1".to_owned(),
            contig: 0,
            span: Region::between(100, 200),
        }]));
        let _ = genie.update(Message::NamesLoaded((0, 0), Ok(Arc::clone(&names))));
        assert!(genie.session.as_ref().unwrap().tracks[0].names.is_none());
        let _ = genie.update(Message::NamesLoaded((0, 1), Ok(names)));
        assert!(genie.session.as_ref().unwrap().tracks[0].names.is_some());
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "Remove lift"));
        assert!(!shows(&genie, "Next feature"));
        let _ = genie.update(Message::Jump(0, true));
        assert!(shows(
            &genie,
            "genie cannot go to the next feature of a lifted track."
        ));
        let _ = genie.update(Message::Lift(Lifting::Stop(0)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.lift.is_none() && track.names.is_none());
        assert_eq!(track.contigs, [None, None]);
        assert_eq!(track.options.lift, 2);
        assert_eq!(genie.saved().unwrap().tracks[0].lift, None);
    }

    #[test]
    fn sessions_lift_tracks_again_before_they_load() {
        let saved = with_lifted().saved().unwrap();
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        let _ = restored.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.lifting, Some(shift_file()));
        assert!(track.contigs.iter().all(Option::is_none));
        assert_eq!(track.slot.wanted, None);
        assert_eq!(restored.saved().unwrap().tracks[0].lift, Some(shift_file()));
        let read = Lifting::Read(Some(track.id), shift_file(), Ok(shift()));
        let _ = restored.update(Message::Lift(read));
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert!(track.lift.is_some() && track.lifting.is_none());
        assert_eq!(track.contigs, [Some("chr1".to_owned()), None]);
        let gone = Lifting::Read(Some(track.id), shift_file(), Err("gone".to_owned()));
        let _ = restored.update(Message::Lift(gone));
        assert!(shows(&restored, "gone"));
    }

    #[test]
    fn the_region_list_lifts_over_with_chain_files() {
        let mut genie = opened();
        for locus in ["chr1:101-200", "chr2:1-50"] {
            go(&mut genie, locus);
            settle(&mut genie);
            let _ = genie.update(Message::Command(Command::AddRegion));
            settle(&mut genie);
        }
        assert!(shows(&genie, "Lift…"));
        let elsewhere = "chain 1 chr9 2000 + 0 900 chr1 1000 + 100 1000 1\n900\n";
        let elsewhere = genie_formats::chain::Chains::read(std::io::Cursor::new(elsewhere));
        let read = Lifting::Read(None, shift_file(), Ok(Arc::new(elsewhere.unwrap())));
        let _ = genie.update(Message::Lift(read));
        assert!(shows(
            &genie,
            "The chain file shift.chain moves none of the regions. The list did not change."
        ));
        assert_eq!(marks(&genie).len(), 2);
        let read = Lifting::Read(None, shift_file(), Ok(shift()));
        let _ = genie.update(Message::Lift(read));
        let moved: Vec<(String, usize, Region)> = marks(&genie)
            .into_iter()
            .map(|mark| (mark.name, mark.contig, mark.span))
            .collect();
        assert_eq!(
            moved,
            [("Region 1".to_owned(), 0, Region::between(200, 300))]
        );
        assert!(shows(
            &genie,
            "Lifted 1 of 2 regions. 1 had no place on this assembly."
        ));
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

    const VIRUS: &str = "\
LOCUS       NC_1          40 bp    DNA     linear   VRL 01-JAN-2020
VERSION     NC_1.1
FEATURES             Location/Qualifiers
     CDS             3..20
                     /gene=\"abc\"
ORIGIN
        1 acgtacgtac gtacgtacgt acgtacgtac gtacgtacgt
//
";

    /// What the work of opening a GenBank file gives: its sequence and its track.
    fn records_opened(location: &Location) -> Message {
        let reference = crate::reference::open(location.clone(), None, &mut |_| {});
        let source = crate::track::open(location.clone());
        Message::RecordsOpened(
            location.clone(),
            reference.map(Arc::new),
            source.map(Arc::new),
        )
    }

    fn reference_name(genie: &Genie) -> String {
        genie.session.as_ref().unwrap().reference.name.clone()
    }

    #[test]
    fn genbank_files_open_their_sequence_and_their_track() {
        let scratch = Scratch::new("genbank-open");
        let gbk = Location::File(scratch.file("virus.gbk", VIRUS));
        let mut genie = Genie::default();
        let _ = genie.update(Message::Reopen(gbk.clone()));
        assert!(shows(&genie, "Opening virus.gbk…"));
        let _ = genie.update(records_opened(&gbk));
        assert_eq!(reference_name(&genie), "virus.gbk");
        assert_eq!(track_name(&genie), "virus.gbk");
        let _ = genie.update(records_opened(&gbk));
        assert!(shows(&genie, "The track virus.gbk is already open."));
        let fasta = scratch.file("virus.fa", format!(">NC_1.1\n{}\n", "ACGT".repeat(10)));
        let mut same = Genie::default();
        let reference = crate::reference::open(fasta, None, &mut |_| {}).unwrap();
        let _ = same.update(Message::Opened(Ok(Arc::new(reference))));
        let _ = same.update(records_opened(&gbk));
        assert_eq!(reference_name(&same), "virus.fa");
        assert_eq!(track_name(&same), "virus.gbk");
        let mut other = opened();
        let _ = other.update(records_opened(&gbk));
        assert_eq!(reference_name(&other), "virus.gbk");
        assert_eq!(track_name(&other), "virus.gbk");
        assert_eq!(other.busy, 0);
    }

    #[test]
    fn sessions_of_genbank_files_open_the_track_once() {
        let scratch = Scratch::new("genbank-session");
        let gbk = Location::File(scratch.file("virus.gbk", VIRUS));
        let mut genie = Genie::default();
        let _ = genie.update(records_opened(&gbk));
        let _ = genie.update(Message::TrackChanged(0, Change::Hue(Hue::Orange)));
        let folder = scratch.file("view.genie", "");
        let folder = folder.parent().unwrap();
        let text = genie.saved().unwrap().text(folder);
        let mut again = Genie::default();
        let _ = again.restore(crate::saved::Saved::parse(&text, folder).unwrap());
        let _ = again.update(records_opened(&gbk));
        assert_eq!(again.session.as_ref().unwrap().tracks.len(), 0);
        let _ = again.update(records_opened(&gbk));
        let tracks = &again.session.as_ref().unwrap().tracks;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].hue, Hue::Orange);
        assert!(again.restoring.is_none());
    }

    #[test]
    fn command_line_genbank_files_open_as_the_reference() {
        let mut genie = Genie::default();
        let reads = Location::File(PathBuf::from("/data/reads.bam"));
        let gbk = Location::File(PathBuf::from("/data/virus.gbk"));
        let _ = genie.launch(vec![reads.clone(), gbk]);
        assert!(shows(&genie, "Opening virus.gbk…"));
        assert_eq!(genie.pending, [reads]);
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
            data: None,
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
        assert!(!shows(&genie, VARIANT_HINT));
        let uncounted = Arc::new(Source {
            location: PathBuf::from("/data/calls.vcf.gz").into(),
            name: "calls.vcf.gz".to_owned(),
            data: None,
            format: Format::Variants(VariantFile::Indexed {
                index: Index::default(),
                samples: Vec::new(),
                infos: Vec::new(),
                layouts: Vec::new(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(uncounted)));
        assert!(shows(&genie, VARIANT_HINT));
        let indexed = Format::Signal(SignalFile::BedGraph {
            index: Index::default(),
        });
        let _ = genie.update(Message::TrackOpened(Ok(signal_source(indexed))));
        assert!(shows(&genie, SIGNAL_HINT));
    }

    /// A density that lands after a zoom to a narrow view gives the narrow view its features.
    #[test]
    fn a_density_that_lands_after_a_zoom_asks_for_the_features() {
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 50_000_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        let reference = Reference::fasta(PathBuf::from("big.fa"), records).unwrap();
        let mut genie = Genie::default();
        genie.settings.motion = false;
        let _ = genie.update(Message::Opened(Ok(Arc::new(reference))));
        let table = Arc::new(Source {
            location: PathBuf::from("/data/genes.bed").into(),
            name: "genes.bed".to_owned(),
            data: None,
            format: Format::Features(FeatureFile::Table {
                kind: Kind::Bed,
                table: genie_formats::features::Table::new([(
                    "chr1".to_owned(),
                    Region::between(20_000_100, 20_000_400),
                    Feature::new(
                        "geneA",
                        Region::between(20_000_100, 20_000_400),
                        Strand::Forward,
                    ),
                )]),
                names: Vec::new(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(table)));
        let wanted = |genie: &Genie| genie.session.as_ref().unwrap().tracks[0].slot(0).wanted;
        let (contig, wide) = wanted(&genie).unwrap();
        assert!(wide.len() > 10_000_000, "{wide:?}");
        go(&mut genie, "chr1:20,000,001-20,001,000");
        assert_eq!(wanted(&genie), Some((contig, wide)));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let density = Arc::new(Loaded::signal(wide, Vec::new(), 1_024));
        let _ = genie.update(Message::TrackLoaded(id, 0, contig, wide, Ok(density)));
        assert_eq!(
            wanted(&genie),
            Some((contig, Region::between(19_999_000, 20_002_000)))
        );
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
            samples: Vec::new(),
            fields: Vec::new(),
            effects: crate::card::Changes::default(),
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

    fn left_click(ui: &mut iced_test::Simulator<'_, Message>, at: Point) {
        ui.point_at(at);
        let _ = ui.simulate(vec![
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
    }

    /// The tracks panel lists the tracks: the eye of a row hides or shows its track, the field
    /// finds tracks by name, and a right-click on a row opens the track menu.
    #[test]
    fn the_tracks_panel_lists_tracks_with_an_eye_and_a_search() {
        use super::track_list::{self, Listing};

        let mut genie = opened();
        for name in ["a.bed.gz", "b.bed.gz"] {
            let _ = genie.update(Message::TrackOpened(Ok(source(name))));
        }
        let first = genie.session.as_ref().unwrap().tracks[0].id;
        let mut ui = simulator(genie.view());
        let button = ui.find(selector::id("tracks")).unwrap().bounds().center();
        left_click(&mut ui, button);
        let toggle = Message::TrackList(Listing::Toggle);
        assert!(ui.into_messages().any(|message| message == toggle));
        let _ = genie.update(toggle);
        assert!(shows(&genie, "2 tracks"));
        let mut ui = simulator(genie.view());
        let eye = ui.find(selector::id(format!("eye-{first}"))).unwrap();
        left_click(&mut ui, eye.bounds().center());
        let row = ui.find(selector::id(format!("row-{first}"))).unwrap();
        let row = row.bounds().center();
        right_click(&mut ui, row);
        let messages: Vec<Message> = ui.into_messages().collect();
        let hide = Message::TrackChanged(first, Change::Visible(false));
        assert!(messages.contains(&hide), "{messages:?}");
        assert!(
            messages.contains(&Message::TrackMenu(first, row)),
            "{messages:?}"
        );
        let _ = genie.update(hide);
        assert_eq!(panel_items(&genie), ["a.bed.gz hidden", "b.bed.gz"]);
        let _ = genie.update(Message::TrackList(Listing::Query(" B.BED ".to_owned())));
        assert_eq!(panel_items(&genie), ["b.bed.gz"]);
        let _ = genie.update(Message::Command(Command::TrackList));
        assert!(!shows(&genie, "2 tracks"));
        assert_eq!(
            track_list::items(genie.session.as_ref().unwrap(), None).len(),
            2
        );
    }

    /// The rows of the tracks panel as words: a folder with the number of its tracks, and a
    /// track in a folder after two spaces.
    fn panel_items(genie: &Genie) -> Vec<String> {
        use super::track_list::{self, Item};

        let session = genie.session.as_ref().unwrap();
        let mark = |on: bool, word: &str| {
            if on {
                format!(" {word}")
            } else {
                String::new()
            }
        };
        track_list::items(session, genie.track_list.as_ref())
            .into_iter()
            .map(|item| match item {
                Item::Folder {
                    name,
                    count,
                    hidden,
                    folded,
                } => format!(
                    "{name} ({count}){}{}",
                    mark(hidden, "hidden"),
                    mark(folded, "folded")
                ),
                Item::Track(row) => format!(
                    "{}{}{}",
                    if row.inside { "  " } else { "" },
                    row.name,
                    mark(row.hidden, "hidden")
                ),
            })
            .collect()
    }

    /// A drag of a row moves a loose track or a folder among the loose tracks and the folders,
    /// and a track of a folder among the tracks of its folder. Each drop is one step to undo.
    #[test]
    fn rows_of_the_panel_move_by_drag() {
        use super::track_list::{Foldering, Grab, Listing, ROW_STEP};

        let mut genie = opened();
        for name in ["a.bed.gz", "b.bed.gz", "c.bed.gz", "d.bed.gz"] {
            let _ = genie.update(Message::TrackOpened(Ok(source(name))));
        }
        let ids: Vec<u64> = genie
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        let _ = genie.update(Message::Command(Command::TrackList));
        for id in [ids[2], ids[3]] {
            let _ = genie.update(Message::Folder("F".to_owned(), Foldering::Add(id)));
        }
        let drag = |genie: &mut Genie, grab: Grab, rows: f32| {
            let _ = genie.update(Message::TrackList(Listing::Moved(grab, rows * ROW_STEP)));
            let _ = genie.update(Message::TrackList(Listing::Dropped));
        };
        drag(&mut genie, Grab::Track(ids[0]), 1.6);
        assert_eq!(
            panel_items(&genie),
            ["b.bed.gz", "a.bed.gz", "F (2)", "  c.bed.gz", "  d.bed.gz"]
        );
        drag(&mut genie, Grab::Folder("F".to_owned()), -2.2);
        assert_eq!(
            panel_items(&genie),
            ["F (2)", "  c.bed.gz", "  d.bed.gz", "b.bed.gz", "a.bed.gz"]
        );
        drag(&mut genie, Grab::Track(ids[3]), -1.6);
        assert_eq!(
            panel_items(&genie),
            ["F (2)", "  d.bed.gz", "  c.bed.gz", "b.bed.gz", "a.bed.gz"]
        );
        drag(&mut genie, Grab::Track(ids[1]), -2.6);
        assert_eq!(
            panel_items(&genie),
            ["b.bed.gz", "F (2)", "  d.bed.gz", "  c.bed.gz", "a.bed.gz"]
        );
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(
            panel_items(&genie),
            ["F (2)", "  d.bed.gz", "  c.bed.gz", "b.bed.gz", "a.bed.gz"]
        );
    }

    /// Folders group tracks in the panel. A track joins a folder at its end, the eye of a folder
    /// hides or shows all its tracks in one step, and a folder folds its rows.
    #[test]
    fn folders_group_tracks_in_the_panel() {
        use super::track_list::{Foldering, Listing};

        let mut genie = opened();
        for name in ["a.bed.gz", "b.bed.gz", "c.bed.gz"] {
            let _ = genie.update(Message::TrackOpened(Ok(source(name))));
        }
        let ids: Vec<u64> = genie
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        let _ = genie.update(Message::TrackMenu(ids[2], Point::ORIGIN));
        assert!(shows(&genie, "New folder…"));
        let _ = genie.update(Message::TrackList(Listing::Name(ids[2])));
        assert!(shows(&genie, "Name of the new folder"));
        let _ = genie.update(Message::TrackList(Listing::Draft("Signals".to_owned())));
        let _ = genie.update(Message::TrackList(Listing::Named));
        let _ = genie.update(Message::TrackList(Listing::Toggle));
        let _ = genie.update(Message::TrackMenu(ids[0], Point::ORIGIN));
        assert!(shows(&genie, "Signals"));
        let _ = genie.update(Message::MenuDismissed);
        let _ = genie.update(Message::TrackList(Listing::Toggle));
        let folder = |foldering| Message::Folder("Signals".to_owned(), foldering);
        let _ = genie.update(folder(Foldering::Add(ids[0])));
        assert_eq!(
            panel_items(&genie),
            ["b.bed.gz", "Signals (2)", "  c.bed.gz", "  a.bed.gz"]
        );
        let _ = genie.update(folder(Foldering::Visible(false)));
        assert_eq!(
            panel_items(&genie),
            [
                "b.bed.gz",
                "Signals (2) hidden",
                "  c.bed.gz hidden",
                "  a.bed.gz hidden"
            ]
        );
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(panel_items(&genie)[1], "Signals (2)");
        let _ = genie.update(Message::TrackList(Listing::Fold("Signals".to_owned())));
        assert_eq!(panel_items(&genie), ["b.bed.gz", "Signals (2) folded"]);
        let _ = genie.update(Message::TrackList(Listing::Fold("Signals".to_owned())));
        let _ = genie.update(folder(Foldering::Take(ids[2])));
        assert_eq!(
            panel_items(&genie),
            ["b.bed.gz", "Signals (1)", "  a.bed.gz", "c.bed.gz"]
        );
        let _ = genie.update(folder(Foldering::Rename("Coverage".to_owned())));
        assert_eq!(panel_items(&genie)[1], "Coverage (1)");
        let gone = Message::Folder("Coverage".to_owned(), Foldering::Remove);
        let _ = genie.update(gone);
        assert_eq!(panel_items(&genie), ["b.bed.gz", "a.bed.gz", "c.bed.gz"]);
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
        let _ = genie.update(Message::NamesLoaded((id, 0), Err("no names".to_owned())));
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

    /// A pinned read card has a button that shows the read and its mate side by side, in two
    /// panels of the width of the view: the mate can be far away or on another chromosome.
    #[test]
    fn the_mate_of_a_pinned_read_opens_in_a_second_panel() {
        use crate::card::{Card, Detail};
        use genie_core::alignment::{Alignment, MATE_UNMAPPED, Mate, Op, PAIRED, Tags};

        let mut genie = opened();
        go(&mut genie, "chr1:101-300");
        settle(&mut genie);
        let read = |flags| Alignment {
            name: "pair7".to_owned(),
            start: 150,
            flags,
            mapq: 60,
            cigar: vec![(Op::Match, 100)],
            bases: vec![b'A'; 100],
            qualities: Vec::new(),
            mate: Some(Mate {
                contig: 1,
                start: 400,
                insert: 0,
            }),
            tags: Tags::default(),
        };
        let pin = |genie: &mut Genie, read: Alignment| {
            genie.pinned = Some(Card {
                track: 0,
                contig: "chr1".to_owned(),
                samples: Vec::new(),
                detail: Detail::Read {
                    read,
                    mate: Some("chr2".to_owned()),
                    same: false,
                    insertion: None,
                },
                at: Point::ORIGIN,
                fields: Vec::new(),
                effects: Changes::default(),
            });
        };
        pin(&mut genie, read(PAIRED));
        let mut ui = simulator(genie.view());
        ui.click("View mate").unwrap();
        assert!(
            ui.into_messages()
                .any(|message| message == Message::ViewMate)
        );
        let _ = genie.update(Message::ViewMate);
        settle(&mut genie);
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.locus(), "chr1:101-300 chr2:301-500");
        assert!(genie.pinned.is_none());
        pin(&mut genie, read(PAIRED | MATE_UNMAPPED));
        assert!(!shows(&genie, "View mate"));
    }

    /// A spliced read and its mate show their starts: the panels leave out the introns of the
    /// read when they find its middle, as in RNA-seq reads.
    #[test]
    fn spliced_reads_and_their_mates_show_their_starts() {
        use crate::card::{Card, Detail};
        use genie_core::alignment::{Alignment, Mate, Op, PAIRED, Tags};

        let mut genie = opened();
        go(&mut genie, "chr1:101-300");
        settle(&mut genie);
        let read = Alignment {
            name: "spliced".to_owned(),
            start: 150,
            flags: PAIRED,
            mapq: 60,
            cigar: vec![(Op::Match, 50), (Op::Skip, 600), (Op::Match, 50)],
            bases: vec![b'A'; 100],
            qualities: Vec::new(),
            mate: Some(Mate {
                contig: 0,
                start: 700,
                insert: 0,
            }),
            tags: Tags::default(),
        };
        genie.pinned = Some(Card {
            track: 0,
            contig: "chr1".to_owned(),
            samples: Vec::new(),
            detail: Detail::Read {
                read,
                mate: Some("chr1".to_owned()),
                same: true,
                insertion: None,
            },
            at: Point::ORIGIN,
            fields: Vec::new(),
            effects: Changes::default(),
        });
        let _ = genie.update(Message::ViewMate);
        settle(&mut genie);
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.locus(), "chr1:101-300 chr1:651-850");
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
    fn the_catalog_adds_ucsc_tracks_with_their_names() {
        let mut genie = Genie::default();
        let mut human = Arc::try_unwrap(reference()).unwrap();
        human.genome = Some("hg38".to_owned());
        let _ = genie.update(Message::Opened(Ok(Arc::new(human))));
        let _ = genie.update(Message::CatalogMenu);
        assert!(shows(&genie, "MANE Select"));
        assert!(shows(&genie, "Conservation"));
        let entries = crate::catalog::entries("hg38");
        let index = entries
            .iter()
            .position(|entry| entry.name == "ClinVar")
            .unwrap();
        let busy = genie.busy;
        let _ = genie.update(Message::Catalog(index));
        assert!(!shows(&genie, "MANE Select"));
        assert_eq!(genie.busy, busy + 1);
        let url = genie_formats::http::Url::parse(&entries[index].url()).unwrap();
        let source = Arc::new(Source {
            location: Location::Url(url),
            name: "clinvarMain.bb".to_owned(),
            data: None,
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: Index::default(),
            }),
        });
        let _ = genie.update(Message::CatalogOpened(index, Ok(source)));
        assert_eq!(genie.busy, busy);
        assert_eq!(track_name(&genie), "ClinVar");
        let _ = genie.update(Message::CatalogMenu);
        assert!(shows(&genie, "Added"));
    }

    /// A collapsed track draws all its features in one row.
    #[test]
    fn tracks_with_too_much_to_draw_show_a_hint() {
        let mut genie = with_track();
        let region = Region::between(0, 1_000);
        let features: Vec<Feature> = (0..200_500_u64)
            .map(|index| {
                let start = (index % 500) * 2;
                Feature::new("", Region::between(start, start + 2), Strand::Unknown)
            })
            .collect();
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.mode = Some(Rows::Collapsed);
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
        assert!(shows(&genie, "There is too much to draw here. Zoom in."));
    }

    #[test]
    fn images_pack_only_the_features_in_view() {
        let mut genie = with_track();
        go(&mut genie, "chr1:481-620");
        settle(&mut genie);
        let region = Region::between(0, 1_000);
        let feature = |name: &str, start, end| {
            Feature::new(name, Region::between(start, end), Strand::Forward)
        };
        let features = vec![
            feature("A", 0, 100),
            feature("B", 50, 150),
            feature("C", 500, 600),
        ];
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
        let session = genie.session.as_ref().unwrap();
        let (_, live) = super::view::shapes(session, &session.tracks[0], false);
        let (_, image) = super::view::shapes(session, &session.tracks[0], true);
        assert!(
            image.height < live.height,
            "{} {}",
            image.height,
            live.height
        );
    }

    /// The JASPAR track of UCSC can have hundreds of sites on one place.
    #[test]
    fn feature_tracks_show_their_first_rows_and_a_note_of_the_others() {
        let mut genie = with_track();
        go(&mut genie, "chr1:481-620");
        settle(&mut genie);
        let region = Region::between(0, 1_000);
        let features = (0..150)
            .map(|_| Feature::new("site", Region::between(500, 510), Strand::Forward))
            .collect();
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.mode = Some(Rows::Expanded);
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.tracks[0].mode_at(0, 0), Rows::Squished);
        let (shapes, _) = super::view::shapes(session, &session.tracks[0], false);
        let rows = genie_render::features::MOST_ROWS as f32 * Rows::Squished.row_height();
        assert!(
            (shapes[0].height - rows - genie_render::features::NOTE_HEIGHT).abs() < 0.01,
            "{}",
            shapes[0].height
        );
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
    fn the_palette_finds_and_runs_actions() {
        let mut genie = with_reads();
        let _ = genie.update(Message::Command(Command::Palette));
        assert!(has_id(&genie, "palette"));
        let query = Message::Palette(Paletting::Query("phase group".to_owned()));
        let _ = genie.update(query);
        {
            let mut ui = simulator(genie.view());
            assert!(ui.find("Group by phase").is_ok());
            assert!(ui.find("Show duplicates").is_err());
        }
        let _ = genie.update(Message::Palette(Paletting::Run));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.options.group, Grouping::Phase);
        assert!(!has_id(&genie, "palette"));
    }

    #[test]
    fn the_palette_moves_its_choice_and_finds_recent_files() {
        let mut genie = opened();
        let file = Location::File(PathBuf::from("/data/hg38.2bit"));
        genie.recent.add(file.clone().into());
        let _ = genie.update(Message::Command(Command::Palette));
        let _ = genie.update(Message::Palette(Paletting::Query("zoom".to_owned())));
        let _ = genie.update(Message::Command(Command::NextSuggestion));
        assert_eq!(genie.palette.as_ref().map(|state| state.selected), Some(1));
        let _ = genie.update(Message::Command(Command::PreviousSuggestion));
        assert_eq!(genie.palette.as_ref().map(|state| state.selected), Some(0));
        let _ = genie.update(Message::Palette(Paletting::Query("hg38".to_owned())));
        let mut ui = simulator(genie.view());
        ui.click("Open hg38.2bit").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        let choose = Message::Palette(Paletting::Choose(0));
        assert!(messages.contains(&choose), "{messages:?}");
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!has_id(&genie, "palette"));
    }

    #[test]
    fn track_settings_open_from_the_menu_and_apply_at_once() {
        let mut genie = with_track();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("Track settings…").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.contains(&Message::TrackSettings(0)),
            "{messages:?}"
        );
        let _ = genie.update(Message::TrackSettings(0));
        let mut ui = simulator(genie.view());
        for line in [
            "Name", "Color", "Rows", "Expanded", "Squished", "Height", "Fit rows",
        ] {
            assert!(ui.find(line).is_ok(), "{line}");
        }
        ui.click("Squished").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        let squish = Message::TrackChanged(0, Change::Mode(Rows::Squished));
        assert!(messages.contains(&squish), "{messages:?}");
        let _ = genie.update(squish);
        let _ = genie.update(Message::TrackDraft(Drafting::Name("Genes".to_owned())));
        let _ = genie.update(Message::TrackDraft(Drafting::Height("120".to_owned())));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.mode, Some(Rows::Squished));
        assert_eq!(track.name, "Genes");
        assert_eq!(track.height, Some(120.0));
        assert!(has_id(&genie, "track-dialog"));
        let _ = genie.update(Message::TrackDraft(Drafting::Name(String::new())));
        let _ = genie.update(Message::TrackDraft(Drafting::Height(String::new())));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.name, track.source.name);
        assert_eq!(track.height, None);
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!has_id(&genie, "track-dialog"));
    }

    #[test]
    fn track_settings_show_the_settings_of_each_kind() {
        /// A genie with a track of one kind, and the lines of the settings of the kind.
        type Kind = (fn() -> Genie, &'static [&'static str]);
        let kinds: [Kind; 3] = [
            (
                with_reads,
                &[
                    "Show duplicates",
                    "Group by phase",
                    "Color unusual pairs",
                    "Color by modification",
                ],
            ),
            (with_signal, &["Bars", "Line", "Shared scale", "Auto range"]),
            (with_filtered, &["Filter"]),
        ];
        for (make, lines) in kinds {
            let mut genie = make();
            let _ = genie.update(Message::TrackSettings(0));
            let mut ui = simulator(genie.view());
            for line in lines {
                assert!(ui.find(*line).is_ok(), "{line}");
            }
        }
    }

    #[test]
    fn first_start_tips_show_until_closed() {
        let tips = crate::help::tips();
        assert!(!shows(&Genie::default(), &tips));
        let mut genie = opened();
        assert!(shows(&genie, &tips));
        let closed = click_id(&genie, "tips-close");
        let hide = Message::SettingChanged(Setting::Tips(false));
        assert!(closed.contains(&hide), "{closed:?}");
        let _ = genie.update(hide);
        assert!(!genie.settings.tips);
        assert!(!shows(&genie, &tips));
    }

    #[test]
    fn the_app_menu_lists_actions_with_their_keys() {
        use crate::chrome::keys;
        let mut genie = opened();
        let _ = genie.update(Message::AppMenu);
        let mut ui = simulator(genie.view());
        for line in [
            "Recent",
            &keys('O'),
            &keys('U'),
            &keys('S'),
            &keys('E'),
            "F1",
        ] {
            assert!(ui.find(line).is_ok(), "{line}");
        }
        let items = [
            ("Open file…", Message::Command(Command::Open)),
            ("Open URL…", Message::Command(Command::OpenUrl)),
            ("Open a track hub…", Message::Hub(super::hubs::Hubbing::Ask)),
            ("Save session", Message::Command(Command::SaveSession)),
            ("Export image…", Message::Command(Command::Export)),
            ("Keys and mouse…", Message::Command(Command::Help)),
            ("Settings", Message::ToggleSettings),
            (
                "Check for updates…",
                Message::Update(super::Updating::Check),
            ),
        ];
        for (label, _) in &items {
            ui.click(*label).unwrap();
        }
        let messages: Vec<Message> = ui.into_messages().collect();
        for (label, message) in items {
            assert!(messages.contains(&message), "{label} {messages:?}");
        }
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "Open file…"));
    }

    #[test]
    fn the_app_menu_lists_recent_files() {
        let mut genie = opened();
        let file = Location::File(PathBuf::from("/data/hg38.2bit"));
        genie.recent.add(file.clone().into());
        let _ = genie.update(Message::AppMenu);
        assert!(!shows(&genie, "hg38.2bit"));
        let _ = genie.update(Message::RecentMenu(true));
        assert!(shows(&genie, "hg38.2bit"));
        let mut ui = simulator(genie.view());
        ui.click("hg38.2bit").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Reopen(file)), "{messages:?}");
        let _ = genie.update(Message::RecentMenu(false));
        assert!(!shows(&genie, "hg38.2bit"));
        assert!(shows(&genie, "Open file…"));
    }

    #[test]
    fn the_help_dialog_lists_keys_and_mouse_actions() {
        use crate::chrome::keys;
        let mut genie = opened();
        assert!(!shows(&genie, "Keys and mouse"));
        let _ = genie.update(Message::Command(Command::Help));
        for line in [
            "Keys and mouse",
            "Drag on the ruler",
            "Zoom to a range",
            "Shift+drag on the ruler",
            &keys('D'),
        ] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(!shows(&genie, "Keys and mouse"));
        let _ = genie.update(Message::Command(Command::Help));
        let closed = click_id(&genie, "help-close");
        assert!(
            closed.contains(&Message::Command(Command::Help)),
            "{closed:?}"
        );
        let inside = click_id(&genie, "help-dialog");
        assert!(!inside.contains(&Message::MenuDismissed), "{inside:?}");
    }

    #[test]
    fn the_whole_command_shows_the_whole_chromosome() {
        let mut genie = opened();
        go(&mut genie, "chr2:51-150");
        settle(&mut genie);
        let _ = genie.update(Message::Command(Command::Whole));
        settle(&mut genie);
        let session = genie.session.as_ref().unwrap();
        let length = session.reference.assembly.contigs()[session.contig].length;
        let whole = format!("chr2:1-{}", genie_core::grouped(length));
        assert!(shows(&genie, &whole), "{whole}");
        let _ = genie.update(Message::Command(Command::Back));
        settle(&mut genie);
        assert!(shows(&genie, "chr2:51-150"));
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
        genie.session.as_mut().unwrap().tracks[0].filter.rules = vec![Rule {
            label: "Quality".to_owned(),
            test: Test::AtLeast(30.0),
        }];
        let _ = genie.update(Message::Jumped(0, true, Ok(None)));
        assert!(shows(
            &genie,
            "There is no next variant in calls.vcf that passes the filter."
        ));
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

    fn click_id(genie: &Genie, id: &str) -> Vec<Message> {
        let mut ui = simulator(genie.view());
        let at = ui
            .find(selector::id(id.to_owned()))
            .unwrap()
            .bounds()
            .center();
        ui.point_at(at);
        let _ = ui.simulate(vec![
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
        ui.into_messages().collect()
    }

    fn has_id(genie: &Genie, id: &str) -> bool {
        simulator(genie.view())
            .find(selector::id(id.to_owned()))
            .is_ok()
    }

    #[test]
    fn track_buttons_show_while_the_pointer_is_on_the_track() {
        let mut genie = with_track();
        assert!(!has_id(&genie, "more-0"));
        let _ = genie.update(Message::Lane(0, Lane::Entered));
        assert!(has_id(&genie, "more-0") && has_id(&genie, "remove-0"));
        let opened = click_id(&genie, "more-0");
        assert!(
            opened
                .iter()
                .any(|message| matches!(message, Message::TrackMenu(0, _))),
            "{opened:?}"
        );
        let removed = click_id(&genie, "remove-0");
        assert!(
            removed.contains(&Message::TrackChanged(0, Change::Remove)),
            "{removed:?}"
        );
        let _ = genie.update(Message::Lane(0, Lane::Left));
        assert!(!has_id(&genie, "more-0"));
    }

    #[test]
    fn moving_over_a_track_reports_it() {
        let genie = with_track();
        let mut ui = simulator(genie.view());
        let at = ui.find(selector::id("lane-0")).unwrap().bounds().center();
        ui.point_at(at);
        let _ = ui.simulate(vec![Event::Mouse(mouse::Event::CursorMoved {
            position: at,
        })]);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.contains(&Message::Lane(0, Lane::Entered)),
            "{messages:?}"
        );
    }

    #[test]
    fn menus_show_the_keys_of_their_items() {
        use crate::chrome::keys;
        let mut genie = with_track();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for keys in [keys('F'), keys('B')] {
            assert!(shows(&genie, &keys), "{keys}");
        }
        let _ = genie.update(Message::Command(Command::Dismiss));
        let _ = genie.update(Message::CopyMenu(None, Point::new(100.0, 50.0)));
        assert!(shows(&genie, &keys('C')));
        let _ = genie.update(Message::Command(Command::Dismiss));
        let _ = genie.update(Message::CopyMenu(Some(1), Point::new(100.0, 50.0)));
        assert!(shows(&genie, "Copy locus"));
        assert!(!shows(&genie, &keys('C')));
    }

    fn chosen(genie: &Genie) -> Option<u64> {
        genie.sidebar.as_ref().and_then(|sidebar| sidebar.chosen)
    }

    fn marks(genie: &Genie) -> Vec<genie_core::Mark> {
        genie.session.as_ref().unwrap().marks.all().to_vec()
    }

    /// Ctrl+Z undoes the work on tracks and region marks step by step, and Ctrl+Y does it again.
    /// A message that changes nothing is not a step, and new work drops the steps to redo.
    #[test]
    fn undo_and_redo_step_through_the_work_on_tracks() {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source("a.bed.gz"))));
        let _ = genie.update(Message::TrackOpened(Ok(source("b.bed.gz"))));
        let names = |genie: &Genie| -> Vec<String> {
            let tracks = &genie.session.as_ref().unwrap().tracks;
            tracks.iter().map(|track| track.name.clone()).collect()
        };
        let hue = |genie: &Genie| genie.session.as_ref().unwrap().tracks[0].hue;
        let (first, plain) = (genie.session.as_ref().unwrap().tracks[0].id, hue(&genie));
        let undo = || Message::Command(Command::Undo);
        let redo = || Message::Command(Command::Redo);
        let _ = genie.update(Message::TrackChanged(first, Change::Hue(Hue::Purple)));
        let _ = genie.update(Message::TrackChanged(first, Change::Hue(Hue::Purple)));
        let _ = genie.update(Message::TrackChanged(first, Change::Remove));
        assert_eq!(names(&genie), ["b.bed.gz"]);
        let _ = genie.update(undo());
        assert_eq!(names(&genie), ["a.bed.gz", "b.bed.gz"]);
        assert_eq!(hue(&genie), Hue::Purple);
        let _ = genie.update(undo());
        assert_eq!(hue(&genie), plain);
        let _ = genie.update(undo());
        assert_eq!(names(&genie), ["a.bed.gz"]);
        for _ in 0..3 {
            let _ = genie.update(redo());
        }
        assert_eq!(names(&genie), ["b.bed.gz"]);
        let _ = genie.update(redo());
        assert!(shows(&genie, "Nothing to redo."));
        let _ = genie.update(undo());
        let _ = genie.update(Message::TrackChanged(first, Change::Hue(Hue::Orange)));
        let _ = genie.update(redo());
        assert!(shows(&genie, "Nothing to redo."));
        assert_eq!(hue(&genie), Hue::Orange);
    }

    /// A hidden track leaves the view and loads nothing. Shown again, it loads and shows again.
    #[test]
    fn hidden_tracks_leave_the_view_and_load_nothing() {
        let mut genie = opened();
        for name in ["a.bed.gz", "b.bed.gz"] {
            let _ = genie.update(Message::TrackOpened(Ok(source(name))));
        }
        for track in &mut genie.session.as_mut().unwrap().tracks {
            track.contigs = vec![Some("1".to_owned()), None];
        }
        let first = genie.session.as_ref().unwrap().tracks[0].id;
        let wanted = |genie: &Genie, at: usize| {
            genie.session.as_ref().unwrap().tracks[at]
                .slot
                .wanted
                .is_some()
        };
        let _ = genie.update(Message::TrackChanged(first, Change::Visible(false)));
        go(&mut genie, "chr1:101-200");
        assert_eq!((wanted(&genie, 0), wanted(&genie, 1)), (false, true));
        assert!(!shows(&genie, "a.bed.gz"));
        assert!(shows(&genie, "b.bed.gz"));
        let _ = genie.update(Message::TrackChanged(first, Change::Visible(true)));
        assert!(wanted(&genie, 0));
        assert!(shows(&genie, "a.bed.gz"));
        let _ = genie.update(Message::Command(Command::Undo));
        assert!(genie.session.as_ref().unwrap().tracks[0].hidden);
    }

    /// A drag of the bottom edge of a lane sends many messages, and makes one step.
    #[test]
    fn a_drag_of_a_lane_is_one_step() {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source("a.bed.gz"))));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let height = |genie: &Genie| genie.session.as_ref().unwrap().tracks[0].height;
        for delta in [10.0, 20.0, 30.0] {
            let _ = genie.update(Message::Lane(id, Lane::Resized(delta)));
        }
        let _ = genie.update(Message::Lane(id, Lane::Released));
        assert!(height(&genie).is_some());
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(height(&genie), None);
        assert_eq!(genie.session.as_ref().unwrap().tracks.len(), 1);
    }

    #[test]
    fn undo_takes_back_a_region_mark() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        let _ = genie.update(Message::Command(Command::AddRegion));
        assert_eq!(marks(&genie).len(), 1);
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(marks(&genie).len(), 0);
        let _ = genie.update(Message::Command(Command::Redo));
        assert_eq!(marks(&genie).len(), 1);
        let _ = genie.update(Message::Command(Command::Undo));
        let _ = genie.update(Message::Command(Command::Undo));
        assert!(shows(&genie, "Nothing to undo."));
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
        assert!(shows(
            &genie,
            "Open a file from the web, or an NCBI assembly by its accession"
        ));
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
        let error =
            "The address data/genes.bed does not start with https://, http://, s3:// or gs://.";
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
    fn the_settings_panel_names_the_version() {
        let mut genie = opened();
        let _ = genie.update(Message::ToggleSettings);
        let version = format!("genie {}", env!("CARGO_PKG_VERSION"));
        assert!(shows(&genie, &version), "{version}");
        assert!(!shows(&genie, &format!("{version} · Apache-2.0")));
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
            hue: Hue::Blue,
            ..crate::saved::SavedTrack::plain(PathBuf::from(format!("/data/{name}")).into())
        };
        let saved = crate::saved::Saved {
            reference: PathBuf::from("ref.fa").into(),
            sizes: None,
            genome: None,
            locus: None,
            reading: crate::reading::Reading::default(),
            tracks: vec![saved_track("a.bed.gz"), saved_track("b.bed.gz")],
            regions: Vec::new(),
            hubs: Vec::new(),
            samples: None,
            figure: crate::export::Figure::default(),
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
            sizes: None,
            genome: None,
            locus: Some("chr1:1,000-2,000".to_owned()),
            reading: crate::reading::Reading::default(),
            tracks: vec![crate::saved::SavedTrack {
                hue: Hue::Blue,
                ..crate::saved::SavedTrack::plain(PathBuf::from("/data/reads.bam").into())
            }],
            regions: Vec::new(),
            hubs: Vec::new(),
            samples: None,
            figure: crate::export::Figure::default(),
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
            data: None,
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
        let makers: [fn() -> Genie; 4] = [|| with_rows(3), with_reads, with_variants, with_signal];
        for make in makers {
            let genie = make();
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

    /// A segment track of two samples with one segment each on chr1.
    fn with_segments() -> Genie {
        use genie_core::segment::Segment;
        use genie_formats::seg::Segments;
        let mut genie = opened();
        let segments = Segments {
            samples: vec!["tumor1".to_owned(), "tumor2".to_owned()],
            value: "seg.mean".to_owned(),
            ..Segments::default()
        };
        let source = Arc::new(Source {
            location: PathBuf::from("/data/cohort.seg").into(),
            name: "cohort.seg".to_owned(),
            data: None,
            format: Format::Segments(SegmentFile { segments }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let segment = |sample, start, end| Segment {
            span: Region::between(start, end),
            sample,
            value: 0.5,
            markers: None,
        };
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let found = vec![segment(0, 0, 400), segment(1, 100, 900)];
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::segments(region, found, 2))),
        );
        genie
    }

    /// A track of a table with a tabix index and two columns of numbers, without data in view.
    fn with_table() -> Genie {
        let scratch = crate::scratch::Scratch::new("app-table");
        let text = "#Chrom\tPos\tRef\tAlt\tRawScore\tPHRED\n1\t101\tT\tA\t0.7\t8.5\n";
        let mut writer = genie_formats::bgzf::Writer::new(Vec::new());
        std::io::Write::write_all(&mut writer, text.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let layout = genie_formats::tabix::Layout::generic([1, 2, 2]);
        let index =
            genie_formats::tabix::build(std::io::Cursor::new(&data), layout, false).unwrap();
        let path = scratch.file("scores.tsv.gz", &data);
        scratch.file("scores.tsv.gz.tbi", &index);
        let source = Arc::new(crate::track::open(path).unwrap());
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        genie
    }

    /// A Hi-C track of the test file of genie-formats, without data in view.
    fn with_contacts() -> Genie {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data/contacts.hic");
        let source = Arc::new(crate::track::open(path).unwrap());
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        genie
    }

    /// A Hi-C track with a small map on chr1: bins of 100 bases.
    fn with_contact_map() -> Genie {
        use genie_core::contact::ContactMap;
        let mut genie = with_contacts();
        let region = Region::between(0, 1_000);
        let map = ContactMap {
            bin: 100,
            contacts: vec![
                (2, 2, 10.0),
                (2, 5, 4.0),
                (5, 5, 8.0),
                (5, 7, 6.0),
                (7, 7, 9.0),
            ],
            ratio: false,
        };
        let contacts = crate::track::Contacts {
            map,
            step: 4,
            normalization: "KR".to_owned(),
        };
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let loaded = Loaded::contacts(region, contacts);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        genie
    }

    #[test]
    fn hic_tracks_show_the_contacts_of_a_viewpoint_under_the_map() {
        let height = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            super::view::body_height(session, &session.tracks[0])
        };
        let mut genie = with_contact_map();
        assert!((height(&genie) - 160.0).abs() < 0.5);
        let _ = genie.update(Message::TrackMenu(0, Point::new(550.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Virtual 4C from here").unwrap();
        let picked = ui.into_messages().any(|message| {
            matches!(
                message,
                Message::TrackChanged(0, Change::Viewpoint(Some(_)))
            )
        });
        assert!(picked);
        let _ = genie.update(Message::TrackChanged(0, Change::Viewpoint(Some(550))));
        assert!((height(&genie) - 209.0).abs() < 0.5);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(750.0, 190.0))));
        assert!(shows(&genie, "Contacts with the viewpoint 6"));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        let _ = genie.update(Message::TrackMenu(0, Point::new(550.0, 40.0)));
        assert!(shows(&genie, "Stop virtual 4C"));
        let saved = genie.saved().unwrap();
        assert_eq!(saved.tracks[0].plot.viewpoint, Some(550));
        let text = saved.text(std::path::Path::new("/data"));
        assert!(text.contains("viewpoint = 551"), "{text}");
        let _ = genie.update(Message::TrackChanged(0, Change::Viewpoint(None)));
        assert!((height(&genie) - 160.0).abs() < 0.5);
    }

    #[test]
    fn segment_tracks_show_a_row_per_sample_and_a_range_in_the_menu() {
        use genie_core::segment::Segment;
        use genie_formats::seg::Segments;
        let mut genie = opened();
        let segments = Segments {
            samples: vec!["tumor1".to_owned(), "tumor2".to_owned()],
            value: "seg.mean".to_owned(),
            ..Segments::default()
        };
        let source = Arc::new(Source {
            location: PathBuf::from("/data/cohort.seg").into(),
            name: "cohort.seg".to_owned(),
            data: None,
            format: Format::Segments(SegmentFile { segments }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let segment = |sample, start, end, value, markers| Segment {
            span: Region::between(start, end),
            sample,
            value,
            markers,
        };
        let found = vec![
            segment(0, 0, 400, 0.85, Some(12)),
            segment(1, 100, 900, -1.2, None),
        ];
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.contigs = vec![Some("chr1".to_owned()), None];
        track.slot.wanted = Some((0, region));
        let loaded = Loaded::segments(region, found, 2);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        assert_eq!(track.mode_at(0, 0), Rows::Expanded);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.0, 5.0))));
        for text in ["chr1:1-400", "400 bp · 12 markers", "seg.mean 0.85"] {
            assert!(shows(&genie, text), "{text}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        let _ = genie.update(Message::TrackMenu(0, Point::new(10.0, 10.0)));
        assert!(shows(&genie, "Default range (±1.5)"));
        let _ = genie.update(Message::Range(0, Ranging::Min("-1".to_owned())));
        let _ = genie.update(Message::Range(0, Ranging::Max("1".to_owned())));
        let _ = genie.update(Message::Range(0, Ranging::Fix));
        let range = genie.session.as_ref().unwrap().tracks[0]
            .plot
            .range
            .unwrap();
        assert_eq!((range.min, range.max), (-1.0, 1.0));
    }

    #[test]
    fn link_tracks_draw_arcs_and_their_cards_name_both_ends() {
        use genie_core::interaction::Interaction;
        let mut genie = opened();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/loops.bedpe").into(),
            name: "loops.bedpe".to_owned(),
            data: None,
            format: Format::Links(LinkFile::Table {
                bedpe: genie_formats::bedpe::Bedpe::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let link = Interaction {
            name: "loopA".to_owned(),
            value: Some(5.0),
            ..Interaction::new(Region::between(100, 120), Region::between(700, 720), None)
        };
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.contigs = vec![Some("chr1".to_owned()), None];
        track.slot.wanted = Some((0, region));
        let loaded = Loaded::links(region, vec![link]);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(410.0, 4.0))));
        for text in [
            "loopA",
            "chr1:101-720",
            "Ends 101-120 and 701-720",
            "Distance 600 bp",
            "Value 5",
        ] {
            assert!(shows(&genie, text), "{text}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(410.0, 40.0))));
        assert!(!shows(&genie, "loopA"));
    }

    /// A link track of a BEDPE file with one loop on chr1.
    fn with_links() -> Genie {
        use genie_core::interaction::Interaction;
        let mut genie = opened();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/loops.bedpe").into(),
            name: "loops.bedpe".to_owned(),
            data: None,
            format: Format::Links(LinkFile::Table {
                bedpe: genie_formats::bedpe::Bedpe::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let link = Interaction::new(Region::between(100, 120), Region::between(700, 720), None);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::links(region, vec![link]))),
        );
        genie
    }

    /// A junction track of a STAR file with two junctions on chr1: 101-300 with 12 reads and
    /// 401-800 with 4 reads.
    fn with_junctions() -> Genie {
        let scratch = crate::scratch::Scratch::new("app-junctions");
        let text = "chr1\t101\t300\t1\t1\t1\t12\t3\t40\nchr1\t401\t800\t2\t2\t0\t4\t0\t25\n";
        let source = Arc::new(crate::track::open(scratch.file("liver.SJ.out.tab", text)).unwrap());
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::clone(&source))));
        let region = Region::between(0, 1_000);
        let none = |_: Region| Err("There is no reference.".to_owned());
        let options = crate::track::Options::default();
        let loaded = crate::track::load(&source, "chr1", region, options, 1, true, none).unwrap();
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        genie
    }

    #[test]
    fn junction_tracks_draw_arcs_and_their_cards_show_the_fields_of_the_file() {
        let mut genie = with_junctions();
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.0, 4.0))));
        for text in [
            "Intron 200 bp",
            "Unique reads",
            "Multi-mapping reads",
            "GT/AG",
            "40 bp",
        ] {
            assert!(shows(&genie, text), "{text}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.0, 28.0))));
        assert!(!shows(&genie, "Intron 200 bp"));
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
        let _ = genie.update(Message::NamesLoaded((0, 0), Ok(Arc::new(names))));
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
        add_named_reads(genie, "reads.bam", reads);
    }

    fn add_named_reads(
        genie: &mut Genie,
        name: &str,
        reads: Vec<genie_core::alignment::Alignment>,
    ) {
        let _ = genie.update(Message::TrackOpened(Ok(read_source(name))));
        let region = Region::between(0, 1_000);
        let track = genie.session.as_mut().unwrap().tracks.last_mut().unwrap();
        track.slot.wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, Options::default());
        for read in reads {
            gather.add(read);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
    }

    #[test]
    fn read_cards_show_the_bases_of_the_insertion_under_the_pointer() {
        use genie_core::alignment::Op;
        let mut genie = opened();
        let mut bases = vec![b'A'; 20];
        bases.extend([b'C'; 15]);
        bases.extend([b'A'; 20]);
        let long = genie_core::alignment::Alignment {
            cigar: vec![(Op::Match, 20), (Op::Insertion, 15), (Op::Match, 20)],
            bases,
            ..read("long1", 0)
        };
        add_reads(&mut genie, vec![long]);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "Insertion of 15 bp after base 20"));
        let _ = genie.update(Message::TrackClicked(0, 0, over(20.5, 5.0)));
        let card = genie.pinned.as_ref().unwrap();
        assert_eq!(card.fields[0].label, "Inserted bases");
        assert_eq!(card.fields[0].value, "C".repeat(15));
        let _ = genie.update(Message::CardClosed);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(35.0, 5.0))));
        assert!(shows(&genie, "long1"));
        assert!(!shows(&genie, "Insertion of 15 bp after base 20"));
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

    fn spliced(name: &str) -> genie_core::alignment::Alignment {
        use genie_core::alignment::{Op, Tags};
        genie_core::alignment::Alignment {
            cigar: vec![(Op::Match, 10), (Op::Skip, 50), (Op::Match, 10)],
            bases: vec![b'A'; 20],
            tags: Tags {
                xs: Some(Strand::Forward),
                ..Tags::default()
            },
            ..read(name, 10)
        }
    }

    fn with_spliced() -> Genie {
        let mut genie = opened();
        add_reads(&mut genie, vec![spliced("one"), spliced("two")]);
        genie
    }

    fn with_sashimi() -> Genie {
        let mut genie = with_spliced();
        let _ = genie.update(Message::TrackChanged(0, Change::Sashimi));
        genie
    }

    #[test]
    fn hovering_a_junction_shows_its_reads() {
        let mut genie = with_spliced();
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(45.0, 5.0))));
        for line in [
            "Splice junction",
            "chr1:21-70",
            "2 reads · forward strand",
            "Intron 50 bp",
        ] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, 5.0))));
        assert!(!shows(&genie, "Splice junction"));
        assert!(!shows(&genie, "CIGAR 10M50N10M"));
        let below = genie_render::alignments::JUNCTION_HEIGHT + 5.0;
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, below))));
        assert!(shows(&genie, "CIGAR 10M50N10M"));
    }

    /// The labels and the arcs of the Sashimi plot of a track.
    fn sashimi(genie: &Genie, at: usize) -> (Vec<String>, usize, f32) {
        use genie_render::scene::Primitive;
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[at];
        let (shapes, _) = super::view::shapes(session, track, false);
        let super::view::Body::Scene(primitives, height) =
            super::view::body(session, track, 0, shapes[0], false)
        else {
            panic!("expected a Sashimi plot");
        };
        let words = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Label { content, .. } | Primitive::Text { content, .. } => {
                    Some(content.to_string())
                }
                _ => None,
            })
            .collect();
        let arcs = primitives
            .iter()
            .filter(|primitive| matches!(primitive, Primitive::Arc { .. }))
            .count();
        (words, arcs, height.max(shapes[0].height))
    }

    #[test]
    fn sashimi_plots_show_coverage_and_junction_counts() {
        use genie_render::sashimi::{ARCS, HEIGHT};
        let mut genie = with_spliced();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "Sashimi plot"));
        assert!(!shows(&genie, "Arcs ≥"));
        let _ = genie.update(Message::TrackChanged(0, Change::Sashimi));
        assert_eq!(
            sashimi(&genie, 0),
            (vec!["0-2".to_owned(), "2".to_owned()], 1, HEIGHT)
        );
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(45.0, ARCS - 24.0))));
        for line in ["Splice junction", "2 reads · forward strand"] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "Arcs ≥"));
        assert!(shows(&genie, "Shared scale"));
        let words = "Show the arcs with 5 or more reads";
        assert!(palette_shows(&mut genie, "arcs 5 more", words));
        let _ = genie.update(Message::TrackChanged(0, Change::JunctionReads(5)));
        assert_eq!(sashimi(&genie, 0), (vec!["0-2".to_owned()], 0, HEIGHT));
    }

    /// Long reads with the same splice junctions, within 5 bp, are one isoform: a row with the
    /// name of the transcript that has the same introns, or "novel", and the reads.
    #[test]
    fn isoforms_join_the_reads_of_one_chain_of_junctions() {
        use genie_core::alignment::{Alignment, Op};
        use genie_core::{Feature, Strand};

        let mut genie = opened();
        let spliced = |name: &str, start, cigar: &[(Op, u32)]| Alignment {
            cigar: cigar.to_vec(),
            ..read(name, start)
        };
        let (m, n) = (Op::Match, Op::Skip);
        add_reads(
            &mut genie,
            vec![
                spliced("a1", 60, &[(m, 40), (n, 200), (m, 100)]),
                spliced("a2", 70, &[(m, 30), (n, 200), (m, 50)]),
                spliced("a3", 52, &[(m, 50), (n, 198), (m, 60)]),
                spliced("b1", 60, &[(m, 40), (n, 200), (m, 50), (n, 70), (m, 30)]),
                spliced("b2", 80, &[(m, 20), (n, 200), (m, 50), (n, 70), (m, 30)]),
                spliced("c1", 450, &[(m, 50), (n, 100), (m, 50)]),
                spliced("u1", 700, &[(m, 50)]),
            ],
        );
        let source = Arc::new(Source {
            location: PathBuf::from("/data/genes.bed").into(),
            name: "genes.bed".to_owned(),
            data: None,
            format: Format::Features(FeatureFile::Table {
                kind: Kind::Bed,
                table: genie_formats::table::Table::new(Vec::new()),
                names: Vec::new(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let transcript = Feature::new("ENST1.2", Region::between(50, 450), Strand::Forward)
            .with_blocks(vec![Region::between(50, 100), Region::between(300, 450)])
            .with_label("TX1");
        let genes = genie.session.as_mut().unwrap().tracks.last_mut().unwrap();
        genes.slot.wanted = Some((0, region));
        let loaded = Loaded::features(region, vec![transcript]);
        genes.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("Isoforms").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::TrackChanged(0, Change::Isoforms)));
        let _ = genie.update(Message::TrackChanged(0, Change::Isoforms));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.5, 5.0))));
        assert!(shows(&genie, "TX1 ENST1.2 · 3 reads"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.5, 25.0))));
        assert!(
            shows(&genie, "TX1 novel 1 · 2 reads"),
            "a novel isoform takes the gene of a transcript with one of its junctions"
        );
        let session = genie.session.as_ref().unwrap();
        let Some(Content::Reads(pile)) = session.tracks[0].data(0).map(|data| &data.content) else {
            panic!("expected reads");
        };
        let shown = super::view::isoforms(session, 0, pile);
        assert_eq!(shown.features.len(), 2);
        assert_eq!(
            shown.others,
            "1 more isoform with 1 read · 1 read without a splice junction"
        );
        let _ = genie.update(Message::TrackChanged(0, Change::Sashimi));
        let plot = genie.session.as_ref().unwrap().tracks[0].plot;
        assert!(
            plot.sashimi && !plot.isoforms,
            "a Sashimi plot ends the isoforms"
        );
    }

    /// Adds a feature track with features on chr1 from 0 to 1,000.
    fn add_features(genie: &mut Genie, name: &str, features: Vec<genie_core::Feature>) {
        let source = Arc::new(Source {
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: name.to_owned(),
            data: None,
            format: Format::Features(FeatureFile::Table {
                kind: Kind::Bed,
                table: genie_formats::table::Table::new(Vec::new()),
                names: Vec::new(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let track = genie.session.as_mut().unwrap().tracks.last_mut().unwrap();
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
    }

    /// Isoform rows show the isoforms in view with 1 % or more of the spliced reads in view, after
    /// their junctions snap to the introns of the open transcripts within 20 bp. A transcript with
    /// an ID names a row before a transcript without one.
    #[test]
    fn isoform_rows_show_the_isoforms_in_view_with_enough_reads() {
        use genie_core::alignment::{Alignment, Op};
        use genie_core::{Feature, Strand};

        let mut genie = opened();
        let (m, n) = (Op::Match, Op::Skip);
        let chain = |name: &str, count: usize, start: u64, cigar: &[(Op, u32)]| {
            (0..count)
                .map(|index| Alignment {
                    cigar: cigar.to_vec(),
                    ..read(&format!("{name}{index}"), start)
                })
                .collect::<Vec<_>>()
        };
        let mut reads = chain("a", 200, 60, &[(m, 40), (n, 200), (m, 100)]);
        reads.extend(chain("s", 2, 60, &[(m, 52), (n, 188), (m, 100)]));
        let two = [(m, 40), (n, 200), (m, 50), (n, 70), (m, 30)];
        reads.extend(chain("c", 3, 60, &two));
        reads.extend(chain("d", 2, 450, &[(m, 50), (n, 100), (m, 50)]));
        reads.extend(chain("e", 4, 700, &[(m, 50), (n, 100), (m, 50)]));
        add_reads(&mut genie, reads);
        let blocks = vec![Region::between(50, 100), Region::between(300, 450)];
        let span = Region::between(50, 450);
        let plain = Feature::new("TX1", span, Strand::Forward).with_blocks(blocks.clone());
        add_features(&mut genie, "refseq.bed", vec![plain]);
        let labeled = Feature::new("ENST1.2", span, Strand::Forward)
            .with_blocks(blocks)
            .with_label("TX1");
        add_features(&mut genie, "gencode.bb", vec![labeled]);
        let _ = genie.update(Message::TrackChanged(0, Change::Isoforms));
        let shown = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            let Some(Content::Reads(pile)) = session.tracks[0].data(0).map(|data| &data.content)
            else {
                panic!("expected reads");
            };
            let shown = super::view::isoforms(session, 0, pile);
            let names: Vec<String> = shown
                .features
                .iter()
                .map(|feature| feature.name().to_owned())
                .collect();
            (names, shown.others)
        };
        assert_eq!(
            shown(&genie),
            (
                vec![
                    "TX1 ENST1.2 · 202 reads".to_owned(),
                    "novel 1 · 4 reads".to_owned(),
                    "TX1 novel 1 · 3 reads".to_owned(),
                ],
                "1 more isoform with 2 reads".to_owned()
            ),
            "2 reads with a junction 12 bp off join TX1, and 2 reads are under 1 % of 211"
        );
        go(&mut genie, "chr1:1-660");
        settle(&mut genie);
        assert_eq!(
            shown(&genie),
            (
                vec![
                    "TX1 ENST1.2 · 202 reads".to_owned(),
                    "TX1 novel 1 · 3 reads".to_owned(),
                ],
                "1 more isoform with 2 reads".to_owned()
            ),
            "the 4 reads after 700 are out of view"
        );
    }

    /// Adds a read track with reads on chr1 from 0 to 1,000 that load with some options.
    fn add_reads_with(
        genie: &mut Genie,
        reads: Vec<genie_core::alignment::Alignment>,
        options: Options,
    ) {
        let _ = genie.update(Message::TrackOpened(Ok(read_source("reads.bam"))));
        let region = Region::between(0, 1_000);
        let track = genie.session.as_mut().unwrap().tracks.last_mut().unwrap();
        track.options = options;
        track.slot.wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, options);
        for read in reads {
            gather.add(read);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
    }

    /// Long reads in groups by phase, and the transcript TX1 (ENST1.2) with the intron 100-300:
    /// 6 reads of TX1 (4 of haplotype 1 and 2 of haplotype 2), 3 reads of haplotype 2 with one
    /// more junction that no transcript has, 2 reads of haplotype 1 with a junction of no
    /// transcript, and 1 read without a haplotype.
    fn with_isoform_reads() -> Genie {
        use genie_core::alignment::{Alignment, Op};
        let mut genie = opened();
        let (m, n) = (Op::Match, Op::Skip);
        let mut reads = Vec::new();
        for (name, start, cigar, phases) in [
            (
                "a",
                60,
                vec![(m, 40), (n, 200), (m, 100)],
                vec![Some(1), Some(1), Some(1), Some(1), Some(2), Some(2)],
            ),
            (
                "b",
                60,
                vec![(m, 40), (n, 200), (m, 50), (n, 70), (m, 30)],
                vec![Some(2); 3],
            ),
            ("c", 450, vec![(m, 50), (n, 100), (m, 50)], vec![Some(1); 2]),
            ("d", 700, vec![(m, 50), (n, 100), (m, 50)], vec![None]),
        ] {
            for (index, phase) in phases.into_iter().enumerate() {
                let mut alignment = Alignment {
                    cigar: cigar.clone(),
                    ..read(&format!("{name}{index}"), start)
                };
                alignment.tags.phase = phase;
                reads.push(alignment);
            }
        }
        let options = Options {
            group: Grouping::Phase,
            ..Options::default()
        };
        add_reads_with(&mut genie, reads, options);
        let transcript = Feature::new("ENST1.2", Region::between(50, 450), Strand::Forward)
            .with_blocks(vec![Region::between(50, 100), Region::between(300, 450)])
            .with_label("TX1");
        add_features(&mut genie, "genes.bb", vec![transcript]);
        genie
    }

    #[test]
    fn isoform_tables_list_the_isoforms_in_view_with_the_reads_of_each_group() {
        let mut genie = with_isoform_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Isoform table").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Isoforms(Isoforming::Open(0))));
        assert!(palette_shows(
            &mut genie,
            "isoform table",
            "Show the isoform table"
        ));
        let _ = genie.update(Message::Isoforms(Isoforming::Open(0)));
        for words in [
            "Isoforms · reads.bam",
            "3 isoforms in view · 1 with fewer reads",
            "TX1 ENST1.2",
            "known",
            "chr1:61-400",
            "50.0 %",
            "140 bp",
            "TX1 novel 1",
            "new junction",
            "novel 1",
            "Haplotype 1",
            "No haplotype",
            "4 · 66.7 %",
            "2 · 40.0 %",
        ] {
            assert!(shows(&genie, words), "{words}");
        }
        let p = crate::card::p_value(genie_core::methylation::fisher([[0, 6], [3, 2]]));
        assert!(shows(&genie, &p), "the p of TX1 novel 1: {p}");
        assert!(!shows(&genie, "novel 2"));
        let _ = genie.update(Message::Isoforms(Isoforming::All));
        assert!(shows(&genie, "4 isoforms in view"));
        assert!(shows(&genie, "novel 2"));
    }

    #[test]
    fn isoform_tables_sort_go_to_the_isoforms_and_export_them() {
        let mut genie = with_isoform_reads();
        let _ = genie.update(Message::Isoforms(Isoforming::Open(0)));
        let _ = genie.update(Message::Isoforms(Isoforming::All));
        let _ = genie.update(Message::Isoforms(Isoforming::Sort(3)));
        assert!(shows(&genie, "Reads ▲"));
        let scratch = Scratch::new("isoform-table");
        let path = scratch.file("isoforms", "");
        let export = |genie: &mut Genie, extension: &str| {
            let file = path.with_extension(extension);
            let picked = Isoforming::ExportPicked(Some(file.clone()));
            let _ = genie.update(Message::Isoforms(picked));
            std::fs::read_to_string(file).unwrap()
        };
        let tsv = export(&mut genie, "tsv");
        let mut lines = tsv.lines();
        assert_eq!(
            lines.next(),
            Some(
                "Name\tKind\tChrom\tStart\tEnd\tStrand\tReads\tShare\tExons\tLength\t\
                 Haplotype 1 reads\tHaplotype 1 share\tHaplotype 2 reads\tHaplotype 2 share\t\
                 No haplotype reads\tNo haplotype share\tp"
            )
        );
        let names: Vec<&str> = lines.map(|line| line.split('\t').next().unwrap()).collect();
        assert_eq!(
            names,
            ["novel 2", "novel 1", "TX1 novel 1", "TX1 ENST1.2"],
            "the fewest reads first"
        );
        let bed = export(&mut genie, "bed");
        assert!(
            bed.contains("chr1\t60\t400\tENST1.2\t6\t+\t60\t60\t0\t2\t40,100,\t0,240,\n"),
            "{bed}"
        );
        let gtf = export(&mut genie, "gtf");
        for line in [
            "chr1\tgenie\ttranscript\t61\t400\t.\t+\t.\tgene_id \"TX1\"; transcript_id \
             \"ENST1.2\"; reads \"6\"; kind \"known\";",
            "chr1\tgenie\texon\t301\t400\t.\t+\t.\tgene_id \"TX1\"; transcript_id \"ENST1.2\"; \
             exon_number \"2\";",
            "chr1\tgenie\ttranscript\t451\t650\t.\t.\t.\tgene_id \"novel_1\"; transcript_id \
             \"novel_1\"; reads \"2\"; kind \"new junction\";",
        ] {
            assert!(gtf.lines().any(|known| known == line), "{line}\n{gtf}");
        }
        for _ in 0..2 {
            let _ = genie.update(Message::Isoforms(Isoforming::Sort(3)));
        }
        let _ = genie.update(Message::Isoforms(Isoforming::Go(0)));
        settle(&mut genie);
        assert!(
            shows(&genie, "chr1:27-434"),
            "TX1 ENST1.2 with a tenth of its length at each side"
        );
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert!(genie.isoforms.is_none());
    }

    /// A cell table of three cells with the columns `orig.ident`, `seurat_clusters` and
    /// `cell_type`.
    fn cell_table() -> Arc<genie_core::samples::Table> {
        let row = |barcode: &str, values: [&str; 3]| {
            (barcode.to_owned(), values.map(str::to_owned).to_vec())
        };
        Arc::new(genie_core::samples::Table::new(
            ["orig.ident", "seurat_clusters", "cell_type"]
                .map(str::to_owned)
                .to_vec(),
            vec![
                row("AAACCCAAGAAACACT-1", ["pbmc", "0", "CD4 T"]),
                row("AAACCCAAGAAACCAT-1", ["pbmc", "2", "B"]),
                row("AAACCCAAGAAACCCA-1", ["pbmc", "0", "CD4 T"]),
            ],
        ))
    }

    /// A read track with the cell table of `cell_table`: 2 reads of each CD4 T cell, 1 read of
    /// the B cell and 1 read of a barcode that is not a cell, each from 100 to 140.
    fn with_cell_reads() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(read_source("reads.bam"))));
        let location: Location = PathBuf::from("/data/meta.csv").into();
        let opened = Celling::Opened(0, location, Ok(cell_table()), None);
        let _ = genie.update(Message::Cells(opened));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let cells = track.cells.as_ref().map(|cells| Arc::clone(&cells.map));
        let mut gather = crate::track::Gather::new(region, 0, track.options).with_cells(cells);
        for (index, barcode) in [
            "AAACCCAAGAAACACT-1",
            "AAACCCAAGAAACACT-1",
            "AAACCCAAGAAACCCA-1",
            "AAACCCAAGAAACCCA-1",
            "AAACCCAAGAAACCAT-1",
            "TTTTTTTTTTTTTTTT-1",
        ]
        .into_iter()
        .enumerate()
        {
            let mut alignment = read(&format!("c{index}"), 100);
            alignment.tags.all.push((*b"CB", barcode.to_owned()));
            gather.add(alignment);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        genie
    }

    #[test]
    fn coverage_by_cell_type_shows_a_lane_for_each_cell_type() {
        use genie_render::cells::LANE_HEIGHT;
        use genie_render::scene::Primitive;
        let mut genie = with_cell_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Coverage by cell type").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::TrackChanged(0, Change::CellLanes)));
        let _ = genie.update(Message::TrackChanged(0, Change::CellLanes));
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[0];
        let (shapes, _) = super::view::shapes(session, track, false);
        let super::view::Body::Reads {
            reads: Some((primitives, height)),
            ..
        } = super::view::body(session, track, 0, shapes[0], false)
        else {
            panic!("expected the lanes of the cell types");
        };
        let texts: Vec<String> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            ["CD4 T · 2 cells", "B · 1 cell", "0-2.0"],
            "4 reads of 2 CD4 T cells are 2 per cell, and the read of no cell does not count"
        );
        assert!((height - 2.0 * LANE_HEIGHT).abs() < 0.5);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(120.5, 5.0))));
        for line in ["CD4 T", "Coverage 4.0", "2 cells · 2.0 per cell"] {
            assert!(shows(&genie, line), "{line}");
        }
        assert!(palette_shows(
            &mut genie,
            "coverage by cell type",
            "Coverage by cell type"
        ));
        let _ = genie.update(Message::TrackChanged(0, Change::Isoforms));
        let plot = genie.session.as_ref().unwrap().tracks[0].plot;
        assert!(plot.isoforms && !plot.cells, "isoforms end the lanes");
    }

    #[test]
    fn fragment_files_show_the_coverage_of_each_cell_type() {
        use genie_render::scene::Primitive;
        let scratch = Scratch::new("fragments");
        let text = "# id=pbmc\nchr1\t100\t300\tAAACCCAAGAAACACT-1\t1\n\
            chr1\t150\t250\tAAACCCAAGAAACCAT-1\t2\nchr1\t200\t400\tGGGGGGGGGGGGGGGG-1\t1\n";
        let path = scratch.file("pbmc_atac_fragments.tsv", text);
        let source = crate::track::open(path).unwrap();
        assert!(source.has_fragments());
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        let texts = |genie: &mut Genie| {
            genie.load_now();
            let session = genie.session.as_ref().unwrap();
            let track = &session.tracks[0];
            let (shapes, _) = super::view::shapes(session, track, false);
            let super::view::Body::Scene(primitives, _) =
                super::view::body(session, track, 0, shapes[0], false)
            else {
                panic!("expected the lanes of the fragments");
            };
            primitives
                .iter()
                .filter_map(|primitive| match primitive {
                    Primitive::Text { content, .. } => Some(content.to_string()),
                    _ => None,
                })
                .collect::<Vec<String>>()
        };
        assert_eq!(
            texts(&mut genie),
            ["Fragments", "0-3.0"],
            "one lane of all fragments without a cell table"
        );
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "Cell types…"));
        let _ = genie.update(Message::MenuDismissed);
        let location: Location = PathBuf::from("/data/meta.csv").into();
        let opened = Celling::Opened(0, location, Ok(cell_table()), None);
        let _ = genie.update(Message::Cells(opened));
        assert_eq!(
            texts(&mut genie),
            ["CD4 T · 2 cells", "B · 1 cell", "0-1.0"],
            "the fragment of a barcode that is not a cell does not count"
        );
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(160.5, 5.0))));
        assert!(
            !shows(&genie, "Coverage 1.0"),
            "the name of the track has a row of its own over the lanes"
        );
        let first = genie_render::cells::HEADER + 5.0;
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(160.5, first))));
        for line in ["CD4 T", "Coverage 1.0", "2 cells · 0.50 per cell"] {
            assert!(shows(&genie, line), "{line}");
        }
    }

    #[test]
    fn read_menus_open_a_cell_table_and_group_the_reads_by_cell_type() {
        let mut genie = opened();
        add_reads(&mut genie, vec![read("r1", 10)]);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Cell types…").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Cells(Celling::Pick(0))));
        let location: Location = PathBuf::from("/data/meta.csv").into();
        let opened = Celling::Opened(0, location.clone(), Ok(cell_table()), None);
        let _ = genie.update(Message::Cells(opened));
        let cells = |genie: &Genie| {
            let track = &genie.session.as_ref().unwrap().tracks[0];
            let column = track
                .cells
                .as_ref()
                .map(|cells| cells.column_name().to_owned());
            (column, track.options.cells, track.options.group)
        };
        assert_eq!(
            cells(&genie),
            (Some("cell_type".to_owned()), 1, Grouping::None)
        );
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        for words in [
            "Cell types by",
            "seurat_clusters",
            "cell_type",
            "Group by cell type",
        ] {
            assert!(shows(&genie, words), "{words}");
        }
        let _ = genie.update(Message::Cells(Celling::Column(0, 1)));
        assert_eq!(
            cells(&genie),
            (Some("seurat_clusters".to_owned()), 2, Grouping::None)
        );
        let _ = genie.update(Message::TrackChanged(0, Change::Group(Grouping::Cell)));
        assert_eq!(cells(&genie).2, Grouping::Cell);
        assert!(palette_shows(
            &mut genie,
            "cell types cell_type",
            "Cell types by cell_type"
        ));
        assert!(palette_shows(
            &mut genie,
            "remove cell table",
            "Remove the cell table"
        ));
        let named = Some("cell_type".to_owned());
        let session = Celling::Opened(0, location, Ok(cell_table()), named);
        let _ = genie.update(Message::Cells(session));
        assert_eq!(
            cells(&genie).0.as_deref(),
            Some("cell_type"),
            "a session gives the column by its name"
        );
        let _ = genie.update(Message::Cells(Celling::Remove(0)));
        assert_eq!(
            cells(&genie),
            (None, 4, Grouping::None),
            "the reads leave their groups by cell type"
        );
    }

    #[test]
    fn sashimi_plots_can_share_one_scale() {
        let mut genie = opened();
        add_reads(&mut genie, vec![spliced("one"), spliced("two")]);
        let deep = ["a", "b", "c", "d"].map(|name| read(name, 10)).to_vec();
        add_named_reads(&mut genie, "deep.bam", deep);
        for id in [0, 1] {
            let _ = genie.update(Message::TrackChanged(id, Change::Sashimi));
        }
        assert_eq!(sashimi(&genie, 0).0[0], "0-2");
        assert_eq!(sashimi(&genie, 1).0[0], "0-4");
        for id in [0, 1] {
            let _ = genie.update(Message::TrackChanged(id, Change::Link));
        }
        assert_eq!(sashimi(&genie, 0).0[0], "0-4");
        assert_eq!(sashimi(&genie, 1).0[0], "0-4");
    }

    #[test]
    fn junction_arcs_sit_above_the_reads() {
        use genie_render::alignments::{COVERAGE_HEIGHT, JUNCTION_HEIGHT};
        use genie_render::scene::Primitive;
        let mut genie = with_spliced();
        for (mode, rows) in [(Rows::Expanded, 2.0), (Rows::Collapsed, 0.0)] {
            genie.session.as_mut().unwrap().tracks[0].mode = Some(mode);
            let session = genie.session.as_ref().unwrap();
            let track = &session.tracks[0];
            let (shapes, _) = super::view::shapes(session, track, false);
            let lane = JUNCTION_HEIGHT + rows * 10.0;
            assert_eq!(shapes[0].height, COVERAGE_HEIGHT + 1.0 + lane, "{mode:?}");
            let super::view::Body::Reads {
                reads: Some((primitives, height)),
                ..
            } = super::view::body(session, track, 0, shapes[0], false)
            else {
                panic!("expected reads in {mode:?}");
            };
            assert_eq!(height, lane, "{mode:?}");
            let arcs = primitives
                .iter()
                .filter(|primitive| matches!(primitive, Primitive::Arc { .. }))
                .count();
            assert_eq!(arcs, 1, "{mode:?}");
            let top = primitives
                .iter()
                .filter_map(|primitive| match primitive {
                    Primitive::Rect { y, .. } => Some(*y),
                    _ => None,
                })
                .fold(f32::MAX, f32::min);
            assert!(top >= JUNCTION_HEIGHT, "{mode:?} {top}");
        }
    }

    fn methylated(name: &str) -> genie_core::alignment::Alignment {
        use genie_core::modification::{Call, Code, Modifications};
        let code = |code: &str| Code {
            base: b'C',
            code: code.to_owned(),
        };
        let call = |at, code, chance| Call { at, code, chance };
        let mut read = read(name, 10);
        read.tags.mods = Modifications {
            codes: vec![code("m"), code("h")],
            calls: vec![call(5, 0, 250), call(5, 1, 3), call(9, 0, 20)],
        };
        read
    }

    #[test]
    fn reads_show_modification_counts_and_colors() {
        use genie_render::scene::{Paint, Primitive};
        let mut genie = opened();
        add_reads(&mut genie, vec![methylated("meth1")]);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.0, 5.0))));
        assert!(shows(&genie, "Modified 5mC 1 of 2 · 5hmC 0 of 1"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Color by modification"));
        let _ = genie.update(Message::TrackChanged(0, Change::Mods));
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[0];
        assert!(track.plot.look.mods);
        let (shapes, _) = super::view::shapes(session, track, false);
        let super::view::Body::Reads {
            reads: Some((primitives, _)),
            ..
        } = super::view::body(session, track, 0, shapes[0], false)
        else {
            panic!("expected reads");
        };
        let lanes = genie_render::alignments::methylation_height(1);
        let marks: Vec<Paint> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    paint: paint @ Paint::Modified(_),
                    y,
                    ..
                } if *y >= lanes => Some(*paint),
                _ => None,
            })
            .collect();
        assert_eq!(marks, [Paint::Modified(253), Paint::Modified(20)]);
    }

    fn with_methylation() -> Genie {
        let mut genie = opened();
        add_reads(&mut genie, vec![methylated("meth1"), methylated("meth2")]);
        let _ = genie.update(Message::TrackChanged(0, Change::Mods));
        genie
    }

    /// With the reads grouped by phase, the methylation difference shows haplotype 1 against
    /// haplotype 2 at each site under the methylation lanes, with the test in the card.
    #[test]
    fn methylation_differences_compare_the_first_two_groups() {
        use genie_render::alignments::methylation_height;

        let phased = |name: String, phase: i64, chance: u8| {
            let mut read = methylated(&name);
            read.tags.phase = Some(phase);
            read.tags.mods.calls[0].chance = chance;
            read
        };
        let mut reads: Vec<_> = (0..8).map(|at| phased(format!("a{at}"), 1, 250)).collect();
        reads.extend((0..8).map(|at| phased(format!("b{at}"), 2, 10)));
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(read_source("reads.bam"))));
        let track = genie.session.as_mut().unwrap().tracks.last_mut().unwrap();
        track.options.group = Grouping::Phase;
        let region = Region::between(0, 1_000);
        track.slot.wanted = Some((0, region));
        let mut gather = crate::track::Gather::new(region, 0, track.options);
        for read in reads {
            gather.add(read);
        }
        let loaded = Loaded::reads(gather, vec![b'A'; 1_000], 0);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackChanged(0, Change::Mods));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Methylation difference"));
        let _ = genie.update(Message::TrackChanged(0, Change::Difference));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let Some(Content::Reads(pile)) = track.data(0).map(|data| &data.content) else {
            panic!("expected reads");
        };
        let (groups, differences) = track.methylation_difference(pile).unwrap();
        assert_eq!(groups, ("Haplotype 1".to_owned(), "Haplotype 2".to_owned()));
        let first = differences[0];
        assert!((first.points - 100.0).abs() < 1e-4, "{first:?}");
        assert!((first.p - 2.0 / 12_870.0).abs() < 1e-9, "{first:?}");
        let y = methylation_height(2) + 10.0;
        let x = first.position as f32 + 0.5;
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(x, y))));
        for line in [
            "5mC difference",
            "Haplotype 1: 8 of 8 reads · 100 %",
            "Haplotype 2: 0 of 8 reads · 0 %",
            "Difference +100 points · p = 1.6e-4",
        ] {
            assert!(shows(&genie, line), "{line}");
        }
    }

    fn read_source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: name.to_owned(),
            data: None,
            format: Format::Reads(ReadFile {
                index: crate::track::reads::Reads::Bam(genie_formats::bai::Index::default()),
                contigs: vec!["chr1".to_owned()],
                samples: Vec::new(),
            }),
        })
    }

    #[test]
    fn the_read_menu_picks_the_modification_of_the_lane() {
        let mut genie = with_methylation();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for item in ["Show 5mC", "Show 5hmC"] {
            assert!(shows(&genie, item), "{item}");
        }
        let _ = genie.update(Message::TrackChanged(0, Change::Modification(1)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.modification.as_deref(), Some("5hmC"));
        let saved = genie.saved().unwrap();
        assert_eq!(saved.tracks[0].modification.as_deref(), Some("5hmC"));
        let mut restored = opened();
        restored.restoring = Some(saved);
        restored.waiting = 1;
        add_reads(&mut restored, vec![methylated("meth1")]);
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.modification.as_deref(), Some("5hmC"));
        assert!(track.plot.look.mods);
    }

    #[test]
    fn methylation_lanes_sit_above_the_reads_and_give_site_cards() {
        use genie_render::alignments::methylation_height;
        let mut genie = opened();
        add_reads(&mut genie, vec![methylated("meth1"), methylated("meth2")]);
        let height = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            super::view::shapes(session, &session.tracks[0], false)
                .1
                .height
        };
        let plain = height(&genie);
        let _ = genie.update(Message::TrackChanged(0, Change::Mods));
        assert!((height(&genie) - plain - methylation_height(1)).abs() < 0.01);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.5, 5.0))));
        for line in ["5mC", "Modified 2 of 2 reads · 100 %", "5hmC 0 of 2 reads"] {
            assert!(shows(&genie, line), "{line}");
        }
        let below = methylation_height(1) + 5.0;
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.0, below))));
        assert!(shows(&genie, "CIGAR 40M"));
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
    fn read_menus_set_a_mapq_floor_and_show_more_kinds_of_reads() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(20.5, 200.0)));
        for text in [
            "MAPQ ≥",
            "Show secondary reads",
            "Show supplementary reads",
            "Show QC-failed reads",
        ] {
            assert!(shows(&genie, text), "{text}");
        }
        let mut ui = simulator(genie.view());
        ui.click("20").unwrap();
        let floor = Message::TrackChanged(0, Change::Mapq(20));
        assert!(ui.into_messages().any(|message| message == floor));
        let _ = genie.update(floor);
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.options.mapq, 20);
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
        assert!(!genie.session.as_ref().unwrap().tracks[0].plot.look.pairs);
    }

    #[test]
    fn read_menus_view_the_mates_of_a_pair_on_one_row() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("View as pairs").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::TrackChanged(0, Change::Toggle(Toggle::Mates))));
        let _ = genie.update(Message::TrackChanged(0, Change::Toggle(Toggle::Mates)));
        assert!(genie.session.as_ref().unwrap().tracks[0].options.mates);
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
            data: None,
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Gff,
                index: Index::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        genie
    }

    fn methyl_source() -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from("/data/calls.bedMethyl").into(),
            name: "calls.bedMethyl".to_owned(),
            data: None,
            format: Format::Signal(SignalFile::BedMethylTable {
                table: genie_formats::table::Table::new(Vec::new()),
                codes: vec!["5mC".to_owned(), "5hmC".to_owned()],
            }),
        })
    }

    fn methyl_named(name: &str) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: name.to_owned(),
            data: None,
            format: Format::Signal(SignalFile::BedMethylTable {
                table: genie_formats::table::Table::new(Vec::new()),
                codes: vec!["5mC".to_owned()],
            }),
        })
    }

    /// Two folders of bedMethyl tracks compare as two groups: the counts of the tracks of each
    /// folder add up at each site, also in views of more than one base for a pixel.
    #[test]
    fn bedmethyl_folders_compare_as_two_groups() {
        use super::track_list::Foldering;
        use genie_core::methylation::Site;
        use genie_render::signal::Operation;

        let mut genie = opened();
        for name in ["t1.bedMethyl", "t2.bedMethyl", "n1.bedMethyl"] {
            let _ = genie.update(Message::TrackOpened(Ok(methyl_named(name))));
        }
        let ids: Vec<u64> = genie
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        for (id, folder) in [(ids[0], "Tumor"), (ids[1], "Tumor"), (ids[2], "Normal")] {
            let _ = genie.update(Message::Folder(folder.to_owned(), Foldering::Add(id)));
        }
        let region = Region::between(0, 1_000);
        for (id, modified, total) in [(ids[0], 9, 10), (ids[1], 6, 10), (ids[2], 2, 20)] {
            let tracks = &mut genie.session.as_mut().unwrap().tracks;
            let track = tracks.iter_mut().find(|track| track.id == id).unwrap();
            track.slot.wanted = Some((0, region));
            let sites = vec![Site {
                position: 20,
                modified,
                total,
            }];
            let loaded = Loaded::methylation(region, sites, 4);
            track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        }
        let _ = genie.update(Message::TrackChanged(ids[0], Change::Compare(Some(ids[2]))));
        let _ = genie.update(Message::TrackMenu(ids[0], Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Methylation difference"));
        let _ = genie.update(Message::MenuDismissed);
        let methylation = Change::Operation(Operation::Methylation);
        let _ = genie.update(Message::TrackChanged(ids[0], methylation));
        let session = genie.session.as_ref().unwrap();
        let tumor = session
            .tracks
            .iter()
            .find(|track| track.id == ids[0])
            .unwrap();
        let (groups, differences) = super::view::group_difference(session, tumor, 0).unwrap();
        assert_eq!(groups, ("Tumor".to_owned(), "Normal".to_owned()));
        assert!((differences[0].points - 65.0).abs() < 1e-4);
        let p = crate::card::p_value(differences[0].p);
        let _ = genie.update(Message::TrackHovered(ids[0], 0, Some(over(20.5, 10.0))));
        for line in [
            "5mC difference".to_owned(),
            "Tumor: 15 of 20 reads · 75 %".to_owned(),
            "Normal: 2 of 20 reads · 10 %".to_owned(),
            format!("Difference +65 points · p = {p}"),
        ] {
            assert!(shows(&genie, &line), "{line}");
        }
    }

    fn with_bedmethyl() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(methyl_source())));
        genie
    }

    #[test]
    fn bedmethyl_menus_pick_the_modification_and_join_strands() {
        let mut genie = with_bedmethyl();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        for item in ["Show 5mC", "Show 5hmC", "Join CpG strands"] {
            assert!(shows(&genie, item), "{item}");
        }
        let _ = genie.update(Message::TrackChanged(0, Change::Modification(1)));
        let _ = genie.update(Message::TrackChanged(0, Change::Toggle(Toggle::Join)));
        let methyl = genie.session.as_ref().unwrap().tracks[0].options.methyl;
        assert_eq!(
            methyl,
            crate::track::Methyl {
                code: 1,
                separate: true
            }
        );
        let saved = genie.saved().unwrap();
        let mut restored = opened();
        restored.restoring = Some(saved);
        restored.waiting = 1;
        let _ = restored.update(Message::TrackOpened(Ok(methyl_source())));
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.options.methyl, methyl);
        assert_eq!(track.shown_modification().as_deref(), Some("5hmC"));
    }

    #[test]
    fn bedmethyl_cards_count_the_reads_of_a_site() {
        use genie_core::methylation::Site;
        let mut genie = with_bedmethyl();
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let sites = vec![Site {
            position: 20,
            modified: 20,
            total: 30,
        }];
        let loaded = Loaded::methylation(region, sites, 1);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 10.0))));
        for line in ["5mC", "Modified 20 of 30 reads · 67 %"] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(30.5, 10.0))));
        assert!(!shows(&genie, "Modified 20 of 30 reads · 67 %"));
    }

    #[test]
    fn menus_know_their_height() {
        let tracks = vec![
            with_track(),
            with_gff(),
            with_reads(),
            with_methylation(),
            with_variants(),
            with_signal(),
            with_two_signals(),
            with_bedmethyl(),
            with_filtered(),
            with_lifted(),
            with_sashimi(),
            with_junctions(),
            with_links(),
            with_segments(),
            with_contacts(),
            with_table(),
            with_family(),
            with_fractions(),
            with_phased_reads(),
        ];
        for mut genie in tracks {
            let _ = genie.update(Message::WindowResized(Size::new(1024.0, 768.0)));
            let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
            let session = genie.session.as_ref().unwrap();
            let sections = super::view::links(session, &session.tracks[0]);
            let full = menu::height(&session.tracks[0], &sections);
            let expected = full.min(menu::room(genie.window));
            let mut ui = simulator(genie.view());
            let bounds = ui.find(selector::id("menu")).unwrap().bounds();
            let height = bounds.height;
            assert!(
                height <= expected && expected - height < 1.0,
                "{height} {expected}"
            );
            // A menu that scrolls is wider by its scrollbar.
            let width = if session.tracks[0].filter.is_empty() {
                menu::WIDTH
            } else {
                menu::FILTER_WIDTH
            };
            let tall = full > menu::room(genie.window);
            assert_eq!(bounds.width > width + 1.0, tall, "{bounds:?}");
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
            annotations: Annotations::default(),
            structure: None,
            calls,
        }
    }

    fn variant_source() -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from("/data/calls.vcf").into(),
            name: "calls.vcf".to_owned(),
            data: None,
            format: Format::Variants(VariantFile::Table {
                table: genie_formats::features::Table::new(Vec::new()),
                samples: vec!["NA1".to_owned(), "NA2".to_owned()],
                infos: vec![("DP".to_owned(), "Read depth".to_owned())],
                fractions: crate::track::Memo::default(),
                phased: crate::track::Memo::default(),
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

    /// A variant track with the B-allele frequency lane of NA2: a dot at rs20, at 0.5 of 30
    /// reads.
    fn with_fractions() -> Genie {
        use genie_core::variant::Fraction;
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackChanged(0, Change::Baf(Some(1))));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let dot = Fraction {
            position: 20,
            value: 0.5,
            depth: Some(30),
            call: Call::Het,
        };
        let variants = vec![site(20, vec![Call::Het, Call::Alternate])];
        let loaded = Loaded {
            options: track.options,
            fractions: Some(Arc::from([dot])),
            ..Loaded::variants(region, variants, 2)
        };
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        genie
    }

    /// The lane of the B-allele frequency of a sample goes between the sites and the rows of
    /// the samples, and sessions keep its sample.
    #[test]
    fn variant_tracks_show_the_b_allele_frequency_of_a_sample() {
        let height = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            super::view::body_height(session, &session.tracks[0])
        };
        let mut genie = with_variants();
        let plain = height(&genie);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("B-allele frequency").unwrap();
        let on = Message::TrackChanged(0, Change::Baf(Some(0)));
        assert!(ui.into_messages().any(|message| message == on));
        let _ = genie.update(on);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        assert!(shows(&genie, "NA2"));
        assert!(palette_shows(
            &mut genie,
            "allele NA2",
            "Show the B-allele frequency of NA2"
        ));
        let mut genie = with_fractions();
        assert!((height(&genie) - plain - genie_render::fractions::HEIGHT).abs() < 0.01);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 32.0))));
        for line in [
            "B-allele frequency",
            "Fraction 0.50 · 30 reads",
            "NA2 · heterozygous",
        ] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 68.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
        let text = genie.saved().unwrap().text(std::path::Path::new("/data"));
        assert!(text.contains("baf = NA2"), "{text}");
        let base = std::path::Path::new("/data");
        let mut restored = opened();
        restored.restoring = Some(crate::saved::Saved::parse(&text, base).unwrap());
        restored.waiting = 1;
        let _ = restored.update(Message::TrackOpened(Ok(variant_source())));
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.options.baf, Some(1));
        let _ = genie.update(Message::TrackChanged(0, Change::Baf(None)));
        assert_eq!(genie.session.as_ref().unwrap().tracks[0].options.baf, None);
    }

    /// A read track whose reads take haplotypes from the phased SNVs of NA1 of calls.vcf.
    fn with_phased_reads() -> Genie {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let phase = crate::track::Phase {
            track: 1,
            sample: 0,
        };
        let _ = genie.update(Message::TrackChanged(0, Change::Phase(Some(phase))));
        genie
    }

    /// The menu of a read track lists the variant tracks under "Phase by", and the samples of
    /// the one in use. Sessions keep the variant track and the sample, and the phase stops when
    /// the variant track goes.
    #[test]
    fn read_menus_phase_the_reads_by_the_snvs_of_a_variant_track() {
        use crate::track::Phase;
        let window = Size::new(1024.0, 1200.0);
        let settings = iced::Settings::default;
        let mut genie = with_reads();
        let _ = genie.update(Message::WindowResized(window));
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        // The items under the heading, the first at 0: the variant track names are also the
        // labels of their lanes.
        let click_item = |genie: &Genie, place: f32| -> Vec<Message> {
            let mut ui = iced_test::Simulator::with_size(settings(), window, genie.view());
            let heading = ui.find("Phase by").unwrap().bounds();
            let y = heading.y + 18.0 + place * 22.6 + 10.0;
            ui.point_at(Point::new(heading.x + 10.0, y));
            let _ = ui.simulate(simulator::click());
            ui.into_messages().collect()
        };
        let phase = Phase {
            track: 1,
            sample: 0,
        };
        let on = Message::TrackChanged(0, Change::Phase(Some(phase)));
        let messages = click_item(&genie, 0.0);
        assert!(messages.contains(&on), "{messages:?}");
        let _ = genie.update(on);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let second = Phase { sample: 1, ..phase };
        let second = Message::TrackChanged(0, Change::Phase(Some(second)));
        let messages = click_item(&genie, 2.0);
        assert!(messages.contains(&second), "{messages:?}");
        let _ = genie.update(second);
        assert!(palette_shows(
            &mut genie,
            "stop phas",
            "Stop phasing the reads"
        ));
        let base = std::path::Path::new("/data");
        let text = genie.saved().unwrap().text(base);
        assert!(text.contains("phase = 2 NA2"), "{text}");
        let mut restored = opened();
        restored.restoring = Some(crate::saved::Saved::parse(&text, base).unwrap());
        restored.waiting = 2;
        let _ = restored.update(Message::TrackOpened(Ok(variant_source())));
        let _ = restored.update(Message::TrackOpened(Ok(read_source("reads.bam"))));
        let session = restored.session.as_ref().unwrap();
        let reads = session.tracks.iter().find(|track| track.source.has_reads());
        let variants = session
            .tracks
            .iter()
            .find(|track| track.source.has_variants());
        let expected = Phase {
            track: variants.unwrap().id,
            sample: 1,
        };
        assert_eq!(reads.unwrap().options.phase, Some(expected));
        let _ = genie.update(Message::TrackChanged(1, Change::Remove));
        assert_eq!(
            genie.session.as_ref().unwrap().tracks[0].options.phase,
            None
        );
    }

    /// The review bar goes to a variant and marks it with keys and a note. The mark shows over
    /// the site, in the card, in the session and in a TSV file, and undo takes it back.
    #[test]
    fn the_review_bar_marks_variants_with_keys_and_notes() {
        use genie_core::review::Verdict;
        let height = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            super::view::body_height(session, &session.tracks[0])
        };
        let reviews = |genie: &Genie| genie.session.as_ref().unwrap().tracks[0].reviews.len();
        let mut genie = with_variants();
        genie.session.as_mut().unwrap().tracks[0].contigs[0] = Some("chr1".to_owned());
        let plain = height(&genie);
        assert!(palette_shows(&mut genie, "review", "Review the variants"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Review variants…").unwrap();
        let open = Message::Review(Reviewing::Open(0));
        assert!(ui.into_messages().any(|message| message == open));
        let _ = genie.update(open);
        assert!(shows(&genie, "Finding a variant…"));
        let found = Ok(Some((0, Region::between(20, 21))));
        let _ = genie.update(Message::Jumped(0, true, found));
        assert!(shows(&genie, "chr1:21 A > G rs20"));
        let _ = genie.update(Message::Command(Command::Mark(Verdict::False)));
        assert_eq!(reviews(&genie), 1);
        let strip = genie_render::variants::REVIEW_HEIGHT;
        assert!((height(&genie) - plain - strip).abs() < 0.01);
        let _ = genie.update(Message::Review(Reviewing::Note("strand bias".to_owned())));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 9.0))));
        assert!(shows(&genie, "Review note"));
        let text = genie.saved().unwrap().text(std::path::Path::new("/data"));
        let line = "review = false chr1 21 A G rs20 strand bias";
        assert!(text.contains(line), "{text}");
        let scratch = crate::scratch::Scratch::new("review-export");
        let path = scratch.file("calls review.tsv", "");
        let picked = Reviewing::ExportPicked(Some(path.clone()));
        let _ = genie.update(Message::Review(picked));
        let exported = std::fs::read_to_string(&path).unwrap();
        let row = "chr1\t21\trs20\tA\tG\tfalse\tstrand bias";
        assert!(exported.contains(row), "{exported}");
        let _ = genie.update(Message::Command(Command::Undo));
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(reviews(&genie), 0);
        let _ = genie.update(Message::Review(Reviewing::Close));
        let _ = genie.update(Message::Command(Command::Mark(Verdict::True)));
        assert_eq!(reviews(&genie), 0);
    }

    /// LD in the track menu adds the r² of the pairs of variants in view under the sites. A
    /// group of the samples gives its own LD, and sessions keep the choice.
    #[test]
    fn variant_tracks_show_the_ld_of_their_variants() {
        let height = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            super::view::body_height(session, &session.tracks[0])
        };
        let mut genie = with_sample_table();
        let plain = height(&genie);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("LD").unwrap();
        let on = Message::TrackChanged(0, Change::Ld(Some(0)));
        assert!(ui.into_messages().any(|message| message == on));
        let _ = genie.update(on);
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let variants = vec![
            site(20, vec![Call::Het, Call::Alternate]),
            site(30, vec![Call::Het, Call::Alternate]),
            site(40, vec![Call::Alternate, Call::Het]),
        ];
        let loaded = Loaded {
            options: track.options,
            ..Loaded::variants(region, variants, 2)
        };
        let loaded = crate::track::with_ld(loaded, None);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        assert!((height(&genie) - plain - genie_render::ld::HEIGHT).abs() < 0.01);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(25.5, 15.0))));
        for line in ["LD", "rs20 · rs30", "r² 1.00"] {
            assert!(shows(&genie, line), "{line}");
        }
        arrange(&mut genie, Arranging::Group("pop".to_owned()));
        let _ = genie.update(Message::TrackChanged(0, Change::Ld(Some(2))));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.ld_group.as_deref(), Some("EUR"));
        assert_eq!(track.ld_samples().as_deref(), Some(&[0][..]));
        let text = genie.saved().unwrap().text(std::path::Path::new("/data"));
        assert!(text.contains("ld = EUR"), "{text}");
    }

    /// With LD on in a variant track, GWAS dots take the LocusZoom colors of their r² with the
    /// top dot, or with a dot that its card makes the lead.
    #[test]
    fn gwas_dots_take_the_r2_with_the_lead() {
        let scratch = crate::scratch::Scratch::new("app-locuszoom");
        let text = "chromosome\tbase_pair_location\tbeta\tp_value\trsid\n\
                    chr1\t500\t0.1\t5e-9\trs1\nchr1\t600\t-0.02\t0.2\trs2\n";
        let mut genie = opened();
        let source = crate::track::open(scratch.file("height.tsv", text)).unwrap();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let region = Region::between(0, 1_000);
        let session = genie.session.as_mut().unwrap();
        let gwas = &mut session.tracks[0];
        gwas.slot.wanted = Some((0, region));
        let options = crate::track::Options::default();
        let none = |_| Err(String::new());
        let loaded = crate::track::load(&gwas.source, "chr1", region, options, 1, true, none);
        gwas.accept_at(0, 0, region, Ok(Arc::new(loaded.unwrap())));
        let id = session.tracks[1].id;
        let _ = genie.update(Message::TrackChanged(id, Change::Ld(Some(0))));
        let calls = &mut genie.session.as_mut().unwrap().tracks[1];
        calls.slot.wanted = Some((0, region));
        let variants = vec![
            site(499, vec![Call::Het, Call::Alternate]),
            site(599, vec![Call::Het, Call::Alternate]),
        ];
        let loaded = Loaded {
            options: calls.options,
            ..Loaded::variants(region, variants, 2)
        };
        let loaded = crate::track::with_ld(loaded, None);
        calls.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let zoom = |genie: &Genie| {
            let session = genie.session.as_ref().unwrap();
            let gwas = &session.tracks[0];
            let Content::Associations(found) = &gwas.data(0).unwrap().content else {
                panic!("expected associations");
            };
            super::view::locus_zoom(session, gwas, 0, found)
        };
        let (r2, lead) = zoom(&genie).unwrap();
        assert_eq!((lead, r2[1]), (0, Some(1.0)));
        let _ = genie.update(Message::TrackClicked(0, 0, over(599.5, 91.0)));
        let _ = genie.update(Message::Lead);
        assert_eq!(zoom(&genie).unwrap().1, 1);
    }

    /// A variant track with rs20 (depth 12) and rs700 (depth 40) loaded on chr1.
    fn with_variant_rows() -> Genie {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let mut deep = site(700, vec![Call::Alternate, Call::Alternate]);
        deep.info = "DP=40".to_owned();
        let variants = vec![site(20, vec![Call::Het, Call::Alternate]), deep];
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::variants(region, variants, 2))),
        );
        genie
    }

    fn top(genie: &Genie, words: &str) -> f32 {
        simulator(genie.view()).find(words).unwrap().bounds().y
    }

    #[test]
    fn variant_tables_list_sort_and_find_the_variants_in_view() {
        let mut genie = with_variant_rows();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Variant table").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Table(Tabling::Open(0))));
        let _ = genie.update(Message::Table(Tabling::Open(0)));
        for words in [
            "Variants · calls.vcf",
            "2 variants in view",
            "Pos",
            "Hom alt",
            "Quality",
            "rs20",
            "rs700",
        ] {
            assert!(shows(&genie, words), "{words}");
        }
        assert!(top(&genie, "rs20") < top(&genie, "rs700"));
        let _ = genie.update(Message::Table(Tabling::Sort(1)));
        let _ = genie.update(Message::Table(Tabling::Sort(1)));
        assert!(shows(&genie, "Pos ▼"));
        assert!(top(&genie, "rs700") < top(&genie, "rs20"));
        let _ = genie.update(Message::Table(Tabling::Search("RS700".to_owned())));
        assert!(shows(&genie, "1 variant in view"));
        assert!(!shows(&genie, "rs20"));
        let _ = genie.update(Message::Table(Tabling::Search(String::new())));
        go(&mut genie, "chr1:501-600");
        settle(&mut genie);
        assert!(shows(&genie, "No variants in view."));
        let _ = genie.update(Message::Table(Tabling::Go(0)));
        settle(&mut genie);
        assert!(shows(&genie, "chr1:1-100"));
        assert!(shows(&genie, "1 variant in view"));
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert!(genie.table.is_none());
        assert!(!shows(&genie, "Variants · calls.vcf"));
    }

    #[test]
    fn variant_tables_export_tab_separated_text() {
        let mut genie = with_variant_rows();
        let _ = genie.update(Message::Table(Tabling::Open(0)));
        let _ = genie.update(Message::Table(Tabling::Sort(1)));
        let _ = genie.update(Message::Table(Tabling::Sort(1)));
        let scratch = Scratch::new("variant-table");
        let path = scratch.file("calls", "");
        let _ = genie.update(Message::Table(Tabling::ExportPicked(Some(path.clone()))));
        let text = std::fs::read_to_string(path.with_extension("tsv")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].starts_with("Chrom\tPos\tID\tRef\tAlt\tHet\tHom alt\tQuality"),
            "{text}"
        );
        assert!(
            lines[1].starts_with("chr1\t701\trs700\tA\tG\t0\t2\t50"),
            "{text}"
        );
        assert!(
            lines[2].starts_with("chr1\t21\trs20\tA\tG\t1\t1\t50"),
            "{text}"
        );
        assert!(shows(&genie, "Saved calls.tsv."));
    }

    fn signal_source(format: Format) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from("/data/coverage.bedGraph").into(),
            name: "coverage.bedGraph".to_owned(),
            data: None,
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
            data: None,
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

    /// Two signal tracks: coverage.bedGraph (1 on 1-100, 4 on 101-200) compared with
    /// other.bedGraph (3 on 1-100, 1 on 101-200).
    fn with_compared() -> Genie {
        let mut genie = with_signal();
        let _ = genie.update(Message::TrackOpened(Ok(other_signal())));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[1];
        track.slot.wanted = Some((0, region));
        let values = vec![
            Summary::single(Region::between(0, 100), 3.0),
            Summary::single(Region::between(100, 200), 1.0),
        ];
        let loaded = Loaded::signal(region, values, 1);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackChanged(0, Change::Compare(Some(1))));
        genie
    }

    #[test]
    fn signal_tracks_compare_with_another_signal_track() {
        use genie_render::signal::Operation;
        let mut genie = with_compared();
        let math = genie.session.as_ref().unwrap().tracks[0].plot.math;
        let log2 = crate::track::Math {
            other: 1,
            operation: Operation::Log2Ratio,
        };
        assert_eq!(math, Some(log2));
        assert!(shows(&genie, "log2 ratio to other.bedGraph"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(150.0, 20.0))));
        assert!(shows(&genie, "Log2 ratio 1.322"));
        let _ = genie.update(Message::TrackChanged(
            0,
            Change::Operation(Operation::Difference),
        ));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(50.0, 20.0))));
        assert!(shows(&genie, "Difference -2"));
        assert!(shows(&genie, "difference to other.bedGraph"));
        let _ = genie.update(Message::TrackChanged(0, Change::Compare(None)));
        assert_eq!(genie.session.as_ref().unwrap().tracks[0].plot.math, None);
        let _ = genie.update(Message::TrackChanged(0, Change::Compare(Some(1))));
        let _ = genie.update(Message::TrackChanged(1, Change::Remove));
        assert_eq!(genie.session.as_ref().unwrap().tracks[0].plot.math, None);
    }

    #[test]
    fn signal_menus_offer_a_comparison_and_its_operations() {
        let at = Point::new(100.0, 200.0);
        let mut genie = with_two_signals();
        let _ = genie.update(Message::TrackMenu(0, at));
        assert!(shows(&genie, "Compare with"));
        assert!(shows(&genie, "Overlay on"));
        let mut genie = with_compared();
        let _ = genie.update(Message::TrackMenu(0, at));
        for item in ["Log2 ratio", "Ratio", "Difference", "Stop comparing"] {
            assert!(shows(&genie, item), "{item}");
        }
        assert!(!shows(&genie, "Overlay on"));
        let _ = genie.update(Message::TrackMenu(1, at));
        assert!(!shows(&genie, "Overlay on"));
    }

    #[test]
    fn sessions_keep_comparisons_whatever_opens_first() {
        use genie_render::signal::Operation;
        let mut genie = with_compared();
        let _ = genie.update(Message::TrackChanged(
            0,
            Change::Operation(Operation::Ratio),
        ));
        let saved = genie.saved().unwrap();
        let text = saved.text(std::path::Path::new("/data"));
        assert!(
            text.contains("compare = 2") && text.contains("math = ratio"),
            "{text}"
        );
        let saved = crate::saved::Saved::parse(&text, std::path::Path::new("/data")).unwrap();
        let comparisons: Vec<_> = saved.tracks.iter().map(|track| track.compare).collect();
        assert_eq!(comparisons, [Some((1, Operation::Ratio)), None]);
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
        let ratio = crate::track::Math {
            other: named("other.bedGraph").id,
            operation: Operation::Ratio,
        };
        assert_eq!(named("coverage.bedGraph").plot.math, Some(ratio));
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
            data: None,
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
            data: None,
            format: Format::Variants(VariantFile::Table {
                table: genie_formats::features::Table::new(Vec::new()),
                samples,
                infos: Vec::new(),
                fractions: crate::track::Memo::default(),
                phased: crate::track::Memo::default(),
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
    fn hovering_a_gwas_dot_shows_its_row() {
        let scratch = crate::scratch::Scratch::new("app-gwas");
        let text = "chromosome\tbase_pair_location\tbeta\tp_value\trsid\n\
                    chr1\t500\t0.1\t5e-9\trs1\nchr1\t600\t-0.02\t0.2\trs2\n";
        let path = scratch.file("height.tsv", text);
        let mut genie = opened();
        let source = crate::track::open(path).unwrap();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let options = crate::track::Options::default();
        let loaded = crate::track::load(&track.source, "chr1", region, options, 1, true, |_| {
            Err(String::new())
        })
        .unwrap();
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(499.5, 4.0))));
        for line in ["rs1", "-log10 p 8.301 · p 5.0e-9", "beta", "0.1"] {
            assert!(shows(&genie, line), "{line}");
        }
    }

    #[test]
    fn hovering_a_contact_map_shows_the_pair_of_bins() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data/contacts.hic");
        let mut genie = opened();
        let source = crate::track::open(path).unwrap();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let map = genie_core::contact::ContactMap {
            bin: 100,
            contacts: vec![(2, 4, 20.0)],
            ratio: false,
        };
        let contacts = crate::track::Contacts {
            map,
            step: 4,
            normalization: "KR".to_owned(),
        };
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::contacts(region, contacts))),
        );
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(350.0, 30.0))));
        for line in [
            "Contact",
            "Bins 201-300 and 401-500",
            "Distance 200 bp · 100 bp bins",
            "Contacts 20",
            "Normalization KR",
        ] {
            assert!(shows(&genie, line), "{line}");
        }
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        let _ = genie.update(Message::TrackMenu(0, Point::new(10.0, 10.0)));
        for line in [
            "No normalization",
            "KR normalization",
            "SCALE normalization",
            "Observed/expected",
        ] {
            assert!(shows(&genie, line), "{line}");
        }
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::TrackChanged(id, Change::Normalization(1)));
        let _ = genie.update(Message::TrackChanged(id, Change::Ratio));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let showing = crate::track::Showing {
            normalization: Some(1),
            ratio: true,
        };
        assert_eq!(track.options.contacts, showing);
        assert_eq!(track.normalization().as_deref(), Some("KR"));
    }

    #[test]
    fn breakends_draw_arcs_above_the_sites() {
        let mut genie = opened();
        let _ = genie.update(Message::TrackOpened(Ok(variant_source())));
        let region = Region::between(0, 1_000);
        let mut breakend = site(100, vec![Call::Het, Call::Alternate]);
        breakend.id = "bnd_1".to_owned();
        breakend.structure = Some(Structure {
            kind: Sv::Breakend,
            length: None,
            mate: Some(("chr1".to_owned(), 300)),
            around: [None, None],
            precise: true,
        });
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let variants = vec![site(20, vec![Call::Het, Call::Alternate]), breakend];
        let loaded = Loaded::variants(region, variants, 2);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let band = genie_render::variants::BREAKEND_HEIGHT;
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(200.5, 3.0))));
        assert!(shows(&genie, "bnd_1"));
        assert!(shows(&genie, "Breakend · other end chr1:301"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, band + 5.0))));
        assert!(shows(&genie, "rs20"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, band + 28.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
    }

    #[test]
    fn hovering_a_genotype_shows_the_sample() {
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "rs20"));
        assert!(shows(&genie, "A > G"));
        assert!(shows(&genie, "Quality"));
        assert!(shows(&genie, "PASS"));
        assert!(shows(&genie, "Read depth"));
        assert!(shows(&genie, "12"));
        assert!(!shows(&genie, "NA2 · homozygous alternate"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 28.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
    }

    fn sample_table() -> Arc<Table> {
        let row = |name: &str, pop: &str, age: &str| {
            (name.to_owned(), vec![pop.to_owned(), age.to_owned()])
        };
        Arc::new(Table::new(
            vec!["pop".to_owned(), "age".to_owned()],
            vec![
                row("NA1", "EUR", "31"),
                row("NA2", "AFR", "45"),
                row("NA9", "EAS", "20"),
            ],
        ))
    }

    fn with_sample_table() -> Genie {
        let mut genie = with_variants();
        let location = Location::File(PathBuf::from("/data/groups.tsv"));
        let opened = Sampling::Opened(location, Ok(sample_table()));
        let _ = genie.update(Message::Samples(opened));
        genie
    }

    fn arrange(genie: &mut Genie, arranging: Arranging) {
        let _ = genie.update(Message::Samples(Sampling::Arrange(0, arranging)));
    }

    #[test]
    fn sample_tables_say_how_many_samples_of_the_tracks_they_know() {
        let genie = with_sample_table();
        assert!(shows(
            &genie,
            "The sample table groups.tsv has 3 samples and 2 fields. \
             It has 2 of the 2 samples of the open tracks."
        ));
        let session = genie.session.as_ref().unwrap();
        assert!(session.samples.is_some());
        assert_eq!(session.tracks[0].rows.groups, []);
        let mut failed = with_variants();
        let location = Location::File(PathBuf::from("/data/x.tsv"));
        let error = "The sample table x.tsv is not valid.".to_owned();
        let _ = failed.update(Message::Samples(Sampling::Opened(location, Err(error))));
        assert!(shows(&failed, "The sample table x.tsv is not valid."));
        assert!(failed.session.as_ref().unwrap().samples.is_none());
    }

    #[test]
    fn fields_group_sort_and_color_the_rows_of_a_track() {
        let mut genie = with_sample_table();
        arrange(&mut genie, Arranging::Group("pop".to_owned()));
        let rows = |genie: &Genie| Arc::clone(&genie.session.as_ref().unwrap().tracks[0].rows);
        assert_eq!(rows(&genie).members, [vec![1], vec![0]]);
        assert_eq!(
            rows(&genie).groups,
            [
                (0, "AFR · 1 sample".to_owned()),
                (1, "EUR · 1 sample".to_owned())
            ]
        );
        let session = genie.session.as_ref().unwrap();
        let height = super::view::body_height(session, &session.tracks[0]);
        assert!((height - 61.0).abs() < f32::EPSILON, "{height}");
        arrange(&mut genie, Arranging::Color("age".to_owned()));
        assert_eq!(rows(&genie).strip, [Paint::Level(100), Paint::Level(0)]);
        arrange(&mut genie, Arranging::Group("pop".to_owned()));
        assert_eq!(rows(&genie).groups, []);
        assert_eq!(rows(&genie).members, [vec![0], vec![1]]);
        arrange(&mut genie, Arranging::Sort("age".to_owned()));
        assert_eq!(rows(&genie).members, [vec![0], vec![1]]);
        let _ = genie.update(Message::Samples(Sampling::Close));
        assert!(genie.session.as_ref().unwrap().samples.is_none());
        assert_eq!(rows(&genie).strip, []);
        let _ = genie.update(Message::Samples(Sampling::Opened(
            Location::File(PathBuf::from("/data/groups.tsv")),
            Ok(sample_table()),
        )));
        assert_eq!(rows(&genie).strip, [Paint::Level(0), Paint::Level(100)]);
    }

    #[test]
    fn the_palette_and_the_track_menu_arrange_samples() {
        let mut genie = with_sample_table();
        let _ = genie.update(Message::Command(Command::Palette));
        let query = Message::Palette(Paletting::Query("group samples pop".to_owned()));
        let _ = genie.update(query);
        assert!(shows(&genie, "Group samples by pop"));
        let _ = genie.update(Message::Palette(Paletting::Run));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.choice.group.as_deref(), Some("pop"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        ui.click("Group samples by pop").unwrap();
        for message in ui.into_messages().collect::<Vec<_>>() {
            let _ = genie.update(message);
        }
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.choice.group, None);
        let _ = genie.update(Message::Command(Command::Palette));
        let query = Message::Palette(Paletting::Query("close sample".to_owned()));
        let _ = genie.update(query);
        assert!(shows(&genie, "Close the sample table"));
        let _ = genie.update(Message::Palette(Paletting::Run));
        assert!(genie.session.as_ref().unwrap().samples.is_none());
    }

    #[test]
    fn genotype_cards_list_the_fields_of_their_sample() {
        let mut genie = with_sample_table();
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 28.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
        assert!(shows(&genie, "AFR"));
        assert!(shows(&genie, "45"));
        let _ = genie.update(Message::TrackClicked(0, 0, over(20.5, 28.0)));
        let text = genie.copy_text().unwrap();
        assert!(text.contains("pop: AFR\nage: 45\nQuality: 50"), "{text}");
        let field = super::Choice {
            track: 0,
            label: "pop".to_owned(),
            item: "AFR".to_owned(),
            at: Point::new(10.0, 10.0),
            sample: true,
        };
        let _ = genie.update(Message::Field(field));
        assert!(shows(&genie, "Sort by this field"));
        assert!(!shows(&genie, "Keep only AFR"));
        let mut ui = simulator(genie.view());
        ui.click("Group by this field").unwrap();
        for message in ui.into_messages().collect::<Vec<_>>() {
            let _ = genie.update(message);
        }
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.choice.group.as_deref(), Some("pop"));
        let _ = genie.update(Message::CardClosed);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "rs20"));
        assert!(!shows(&genie, "45"));
    }

    #[test]
    fn sessions_reopen_the_sample_table_and_the_fields_of_tracks() {
        let mut genie = with_sample_table();
        arrange(&mut genie, Arranging::Group("pop".to_owned()));
        let saved = genie.saved().unwrap();
        let table = Location::File(PathBuf::from("/data/groups.tsv"));
        assert_eq!(saved.samples.as_ref(), Some(&table));
        assert_eq!(saved.tracks[0].choice.group.as_deref(), Some("pop"));
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        assert_eq!(restored.busy, 2);
        let _ = restored.update(Message::TrackOpened(Ok(variant_source())));
        let rows = |genie: &Genie| Arc::clone(&genie.session.as_ref().unwrap().tracks[0].rows);
        assert_eq!(rows(&restored).groups, []);
        let opened = Sampling::Opened(table, Ok(sample_table()));
        let _ = restored.update(Message::Samples(opened));
        assert_eq!(rows(&restored).groups.len(), 2);
        assert_eq!(restored.busy, 0);
    }

    /// A family track (kid, dad and mum) with a de novo variant at 20, a consistent one at 300
    /// and a Mendelian error at 700, and the pedigree of the family as the sample table.
    fn with_family() -> Genie {
        let mut genie = opened();
        let names = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        };
        let source = Arc::new(Source {
            location: PathBuf::from("/data/family.vcf").into(),
            name: "family.vcf".to_owned(),
            data: None,
            format: Format::Variants(VariantFile::Table {
                table: genie_formats::features::Table::new(Vec::new()),
                samples: names(&["kid", "dad", "mum"]),
                infos: Vec::new(),
                fractions: crate::track::Memo::default(),
                phased: crate::track::Memo::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let region = Region::between(0, 1_000);
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        let variants = vec![
            site(20, vec![Call::Het, Call::Reference, Call::Reference]),
            site(300, vec![Call::Het, Call::Het, Call::Reference]),
            site(700, vec![Call::Reference, Call::Alternate, Call::Het]),
        ];
        let loaded = Loaded::variants(region, variants, 3);
        track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        let table = Table::new(
            names(&["Family", "Father", "Mother"]),
            vec![
                ("kid".to_owned(), names(&["F1", "dad", "mum"])),
                ("dad".to_owned(), names(&["F1"])),
                ("mum".to_owned(), names(&["F1"])),
            ],
        );
        let location = Location::File(PathBuf::from("/data/family.ped"));
        let opened = Sampling::Opened(location, Ok(Arc::new(table)));
        let _ = genie.update(Message::Samples(opened));
        genie
    }

    /// The names of the variants that a track shows after its filter.
    fn shown_ids(genie: &Genie) -> Vec<String> {
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let data = track
            .slot
            .shown
            .as_ref()
            .or(track.slot.loaded.as_ref().map(|(_, data)| data));
        let Some(Content::Variants(calls)) = data.map(|data| &data.content) else {
            return Vec::new();
        };
        calls
            .variants
            .iter()
            .map(|variant| variant.id.clone())
            .collect()
    }

    #[test]
    fn family_tracks_check_the_calls_of_each_child() {
        let mut genie = with_family();
        assert_eq!(genie.session.as_ref().unwrap().tracks[0].trios.len(), 1);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "Inheritance"));
        assert!(shows(&genie, "Mendelian error, De novo"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 18.0))));
        assert!(shows(&genie, "kid · heterozygous"));
        assert!(shows(
            &genie,
            "De novo: father homozygous reference, mother homozygous reference"
        ));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(700.5, 18.0))));
        assert!(shows(
            &genie,
            "Mendelian error: father homozygous alternate, mother heterozygous"
        ));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(700.5, 28.0))));
        assert!(shows(&genie, "dad · homozygous alternate"));
        assert!(!shows(&genie, "Trio"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut ui = simulator(genie.view());
        assert!(ui.find("Only Mendelian errors").is_ok());
        ui.click("Only de novo variants").unwrap();
        for message in ui.into_messages().collect::<Vec<_>>() {
            let _ = genie.update(message);
        }
        assert_eq!(shown_ids(&genie), ["rs20"]);
    }

    #[test]
    fn inheritance_filters_keep_the_variants_of_trios() {
        let mut genie = with_family();
        assert_eq!(shown_ids(&genie), ["rs20", "rs300", "rs700"]);
        let keep = |word: &str| Rule {
            label: "Inheritance".to_owned(),
            test: Test::Keep(word.to_owned()),
        };
        let _ = genie.update(Message::Filter(0, Filtering::Add(keep("Mendelian error"))));
        assert_eq!(shown_ids(&genie), ["rs20", "rs700"]);
        let _ = genie.update(Message::Filter(0, Filtering::Remove(0)));
        let _ = genie.update(Message::Filter(0, Filtering::Add(keep("De novo"))));
        assert_eq!(shown_ids(&genie), ["rs20"]);
        let _ = genie.update(Message::Samples(Sampling::Close));
        assert_eq!(shown_ids(&genie), Vec::<String>::new());
    }

    #[test]
    fn children_with_calls_that_break_inheritance_have_a_frame() {
        let genie = with_family();
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[0];
        let (shapes, _) = super::view::shapes(session, track, false);
        let shape = shapes.into_iter().next().unwrap();
        let super::view::Body::Scene(primitives, _) =
            super::view::body(session, track, 0, shape, false)
        else {
            panic!("no scene");
        };
        let frames = primitives
            .iter()
            .filter(|primitive| {
                matches!(
                    primitive,
                    genie_render::scene::Primitive::Rect {
                        paint: Paint::Text,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(frames, 2 * 4);
    }

    #[test]
    fn genotype_rows_under_group_bands_show_their_sample() {
        let mut genie = with_sample_table();
        arrange(&mut genie, Arranging::Group("pop".to_owned()));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 32.0))));
        assert!(shows(&genie, "NA2 · homozygous alternate"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 56.0))));
        assert!(shows(&genie, "NA1 · heterozygous"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 45.0))));
        assert!(!shows(&genie, "NA1 · heterozygous"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert!(shows(&genie, "AFR · AF 1 · 2 alleles"));
        assert!(shows(&genie, "EUR · AF 0.5 · 2 alleles"));
    }

    #[test]
    fn variant_cards_show_the_changes_on_the_open_transcripts() {
        let mut genie = with_variants();
        let _ = genie.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        let region = Region::between(0, 1_000);
        let exon = Region::between(10, 40);
        let transcript = Feature::new("GENE1", exon, Strand::Forward)
            .with_blocks(vec![exon])
            .with_coding(Some(Region::between(13, 37)));
        let enhancer = Feature::new("ENHANCER", Region::between(0, 100), Strand::Forward);
        let features = vec![transcript.clone(), enhancer];
        let genes = &mut genie.session.as_mut().unwrap().tracks[1];
        genes.slot.wanted = Some((0, region));
        genes.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.5, 5.0))));
        assert_eq!(genie.effects.transcripts, [transcript]);
        let generation = genie.effects.generation;
        let line = "GENE1 p.Lys3Arg · c.8A>G".to_owned();
        let changes = Changes {
            lines: vec![line.clone()],
            others: 2,
        };
        let _ = genie.update(Message::Effects(generation, changes.clone()));
        assert!(shows(&genie, &line));
        assert!(shows(&genie, "+2 more transcripts"));
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.7, 6.0))));
        assert_eq!(genie.effects.generation, generation);
        assert!(shows(&genie, &line));
        let _ = genie.update(Message::TrackClicked(0, 0, over(20.5, 5.0)));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        assert_eq!(genie.pinned.as_ref().unwrap().effects, changes);
        assert!(genie.copy_text().unwrap().contains(&line));
        let _ = genie.update(Message::Effects(generation - 1, Changes::default()));
        assert_eq!(genie.pinned.as_ref().unwrap().effects, changes);
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
    fn read_cards_list_fields_and_their_filters_load_the_reads_again() {
        let mut genie = with_reads();
        let _ = genie.update(Message::TrackClicked(0, 0, over(20.0, 5.0)));
        assert!(shows(&genie, "MAPQ"));
        let choice = super::Choice {
            track: 0,
            label: "MAPQ".to_owned(),
            item: "60".to_owned(),
            at: Point::new(10.0, 10.0),
            sample: false,
        };
        let _ = genie.update(Message::Field(choice));
        assert!(shows(&genie, "Keep only 60"));
        assert!(shows(&genie, "At least"));
        assert!(!shows(&genie, "Label with this field"));
        assert!(!shows(&genie, "Group by this tag"));
        let tag = super::Choice {
            track: 0,
            label: "NM".to_owned(),
            item: "1".to_owned(),
            at: Point::new(10.0, 10.0),
            sample: false,
        };
        let _ = genie.update(Message::Field(tag));
        assert!(shows(&genie, "Group by this tag"));
        let mut ui = simulator(genie.view());
        ui.click("Color by this tag").unwrap();
        let color = Message::TrackChanged(0, Change::Color(*b"NM"));
        assert!(ui.into_messages().any(|message| message == color));
        let _ = genie.update(color);
        assert_eq!(
            genie.session.as_ref().unwrap().tracks[0].options.color,
            Some(*b"NM")
        );
        assert!(!shows(&genie, "Group by this tag"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(20.5, 40.0)));
        assert!(shows(&genie, "Color by NM"));
        let _ = genie.update(Message::MenuDismissed);
        let rule = Rule {
            label: "MAPQ".to_owned(),
            test: Test::AtLeast(61.0),
        };
        let _ = genie.update(Message::Filter(0, Filtering::Add(rule)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.filter.rules.len(), 1);
        assert!(track.slot.loaded.is_none());
    }

    #[test]
    fn split_read_cards_list_the_other_parts_and_go_there() {
        let mut genie = opened();
        let mut split = read("split7", 10);
        split.cigar = vec![
            (genie_core::alignment::Op::Match, 30),
            (genie_core::alignment::Op::SoftClip, 10),
        ];
        split.tags.parts = vec![genie_core::alignment::Part {
            contig: "chr2".to_owned(),
            start: 99,
            reverse: true,
            mapq: 60,
        }];
        add_reads(&mut genie, vec![split]);
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(20.0, 5.0))));
        assert!(shows(&genie, "Other part"));
        assert!(shows(&genie, "chr2:100 - · MAPQ 60"));
        let _ = genie.update(Message::TrackClicked(0, 0, over(20.0, 5.0)));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        let text = genie.copy_text().unwrap();
        assert!(text.ends_with("Other part: chr2:100 - · MAPQ 60"), "{text}");
        press(&mut genie, "chr2:100 - · MAPQ 60");
        settle(&mut genie);
        let locus = genie.session.as_ref().unwrap().locus();
        assert!(locus.starts_with("chr2:"), "{locus}");
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

    fn with_fields() -> Genie {
        let mut genie = with_track();
        let region = Region::between(0, 1_000);
        let values: Vec<String> = (1..=8).map(|index| format!("value{index}")).collect();
        let feature = Feature::new("BRCA1", Region::between(10, 20), Strand::Reverse)
            .with_extra(values.join("\t"));
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

    fn with_two_features() -> Genie {
        let mut genie = with_track();
        accept_two(&mut genie);
        genie
    }

    fn accept_two(genie: &mut Genie) {
        let region = Region::between(0, 1_000);
        let features = vec![
            Feature::new("BRCA1", Region::between(10, 20), Strand::Reverse).with_extra("keep\t0.5"),
            Feature::new("BRCA2", Region::between(40, 50), Strand::Forward)
                .with_extra("other\t0.01"),
        ];
        let track = &mut genie.session.as_mut().unwrap().tracks[0];
        track.slot.wanted = Some((0, region));
        track.accept_at(
            0,
            0,
            region,
            Ok(Arc::new(Loaded::features(region, features))),
        );
    }

    fn shown_names(genie: &Genie) -> Vec<String> {
        let track = &genie.session.as_ref().unwrap().tracks[0];
        match &track.data(0).unwrap().content {
            Content::Features(annotation) => annotation
                .features
                .iter()
                .map(|feature| feature.name().to_owned())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn field_rule(label: &str, test: Test) -> Rule {
        Rule {
            label: label.to_owned(),
            test,
        }
    }

    #[test]
    fn filters_keep_matching_features_and_labels_use_a_field() {
        let mut genie = with_two_features();
        let keep = field_rule("Column 13", Test::Keep("keep".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(keep)));
        assert_eq!(shown_names(&genie), ["BRCA1"]);
        let least = field_rule("Column 14", Test::AtLeast(0.1));
        let _ = genie.update(Message::Filter(0, Filtering::Add(least)));
        let _ = genie.update(Message::Filter(0, Filtering::Remove(0)));
        assert_eq!(shown_names(&genie), ["BRCA1"]);
        let _ = genie.update(Message::Filter(0, Filtering::Remove(0)));
        assert_eq!(shown_names(&genie), ["BRCA1", "BRCA2"]);
        let label = Filtering::Label(Some("Column 13".to_owned()));
        let _ = genie.update(Message::Filter(0, label));
        assert_eq!(shown_names(&genie), ["keep", "other"]);
        let _ = genie.update(Message::Filter(0, Filtering::Label(None)));
        assert_eq!(shown_names(&genie), ["BRCA1", "BRCA2"]);
    }

    fn press(genie: &mut Genie, words: &str) {
        let mut ui = simulator(genie.view());
        ui.click(words).unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
    }

    fn with_filtered() -> Genie {
        let mut genie = with_two_features();
        let least = field_rule("Column 14", Test::AtLeast(0.1));
        let _ = genie.update(Message::Filter(0, Filtering::Add(least)));
        let label = Filtering::Label(Some("Column 13".to_owned()));
        let _ = genie.update(Message::Filter(0, label));
        genie
    }

    #[test]
    fn pinned_card_values_set_filters_and_labels() {
        let mut genie = with_two_features();
        let _ = genie.update(Message::TrackClicked(0, 0, over(15.0, 5.0)));
        press(&mut genie, "keep");
        assert!(shows(&genie, "Keep only keep"));
        assert!(!shows(&genie, "At least keep"));
        press(&mut genie, "Keep only keep");
        assert_eq!(shown_names(&genie), ["BRCA1"]);
        press(&mut genie, "0.5");
        assert!(shows(&genie, "At most"));
        press(&mut genie, "Label with this field");
        assert_eq!(shown_names(&genie), ["0.5"]);
        press(&mut genie, "0.5");
        let _ = genie.update(Message::MenuDismissed);
        assert!(!shows(&genie, "At most"));
    }

    #[test]
    fn zoomed_out_bigbed_densities_load_again_with_their_filter() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../genie-formats/tests/data/ccres.bb");
        let mut genie = opened();
        let source = crate::track::open(path).unwrap();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        let region = Region::between(0, 1_000);
        let accept = |genie: &mut Genie| {
            let kept = crate::track::Kept {
                kept: vec![Region::between(500, 900)],
                all: vec![Region::between(100, 400), Region::between(500, 900)],
            };
            let track = &mut genie.session.as_mut().unwrap().tracks[0];
            track.slot.wanted = Some((0, region));
            let density = Loaded::kept_density(region, kept, 1);
            track.accept_at(0, 0, region, Ok(Arc::new(density)));
        };
        accept(&mut genie);
        let rule = field_rule("Class", Test::Keep("CTCF-only".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(rule)));
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert!(track.slot.loaded.is_none());
        assert!(track.slot.wanted.is_some());
        accept(&mut genie);
        assert!(shows(&genie, "1 of 2"));
    }

    #[test]
    fn names_that_the_filter_hides_get_a_note() {
        let mut genie = with_two_features();
        let named = genie_core::Named {
            name: "TP53".to_owned(),
            contig: 0,
            span: Region::between(100, 200),
        };
        assert_eq!(genie.filtered_for(&named), [] as [u64; 0]);
        let keep = field_rule("Column 13", Test::Keep("keep".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(keep)));
        assert_eq!(genie.filtered_for(&named), [0]);
        let elsewhere = genie_core::Named { contig: 1, ..named };
        assert_eq!(genie.filtered_for(&elsewhere), [] as [u64; 0]);
        let checked = Filtering::Checked("BRCA1".to_owned(), false);
        let _ = genie.update(Message::Filter(0, checked));
        assert!(!shows(
            &genie,
            "BRCA1 is hidden by the filter of genes.bed.gz."
        ));
        let checked = Filtering::Checked("TP53".to_owned(), true);
        let _ = genie.update(Message::Filter(0, checked));
        assert!(shows(
            &genie,
            "TP53 is hidden by the filter of genes.bed.gz."
        ));
    }

    #[test]
    fn typed_numbers_set_at_least_and_at_most_rules() {
        let rules = |genie: &Genie| {
            genie.session.as_ref().unwrap().tracks[0]
                .filter
                .rules
                .clone()
        };
        let mut genie = with_two_features();
        let _ = genie.update(Message::TrackClicked(0, 0, over(15.0, 5.0)));
        press(&mut genie, "0.5");
        assert_eq!(genie.field_draft.as_deref(), Some("0.5"));
        let _ = genie.update(Message::Filter(0, Filtering::Draft("0.25".to_owned())));
        assert!(shows(&genie, "At least"));
        press(&mut genie, "At least");
        assert_eq!(
            rules(&genie),
            [field_rule("Column 14", Test::AtLeast(0.25))]
        );
        assert_eq!(shown_names(&genie), ["BRCA1"]);
        press(&mut genie, "0.5");
        let _ = genie.update(Message::Filter(0, Filtering::Draft("a lot".to_owned())));
        press(&mut genie, "At most");
        assert_eq!(rules(&genie).len(), 1);
        let _ = genie.update(Message::Filter(0, Filtering::Draft("0.75".to_owned())));
        press(&mut genie, "At most");
        assert_eq!(
            rules(&genie)[1],
            field_rule("Column 14", Test::AtMost(0.75))
        );
    }

    #[test]
    fn track_menus_with_rules_are_wider() {
        let width = |genie: &Genie| {
            simulator(genie.view())
                .find(selector::id("menu"))
                .unwrap()
                .bounds()
                .width
        };
        let mut plain = with_two_features();
        let _ = plain.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        let mut filtered = with_filtered();
        let _ = filtered.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(width(&filtered) > width(&plain) + 50.0);
    }

    #[test]
    fn the_track_menu_lists_rules_and_removes_them() {
        let mut genie = with_filtered();
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(shows(&genie, "Column 14 ≥ 0.1"));
        press(&mut genie, "Label with name");
        assert_eq!(shown_names(&genie), ["BRCA1"]);
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 200.0)));
        assert!(!shows(&genie, "Label with name"));
        let mut ui = simulator(genie.view());
        ui.click(selector::id("rule-0")).unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        assert_eq!(shown_names(&genie), ["BRCA1", "BRCA2"]);
    }

    #[test]
    fn chips_at_the_right_edge_stay_clear_of_the_scrollbar() {
        let room = |genie: &Genie, words: &str| {
            let mut ui = simulator(genie.view());
            let lane = ui.find(selector::id("lane-0")).unwrap().bounds();
            let chip = ui.find(words).unwrap().bounds();
            lane.x + lane.width - (chip.x + chip.width)
        };
        let mut rows = with_rows(3);
        let _ = rows.update(Message::Lane(0, Lane::Resized(-25.0)));
        let _ = rows.update(Message::Lane(0, Lane::Resized(-30.0)));
        let _ = rows.update(Message::Lane(0, Lane::Released));
        let mut filtered = with_two_features();
        let keep = field_rule("Column 13", Test::Keep("keep".to_owned()));
        let _ = filtered.update(Message::Filter(0, Filtering::Add(keep)));
        for (genie, words) in [(&rows, "+2 rows"), (&filtered, "1 of 2")] {
            let room = room(genie, words);
            assert!(
                room >= genie_render::scene::LABEL_INSET - 0.5,
                "{words}: {room}"
            );
        }
    }

    #[test]
    fn filtered_tracks_show_a_count_chip() {
        let mut genie = with_two_features();
        assert!(!shows(&genie, "1 of 2"));
        let keep = field_rule("Column 13", Test::Keep("keep".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(keep)));
        assert!(shows(&genie, "1 of 2"));
        press(&mut genie, "1 of 2");
        assert!(shows(&genie, "Column 13 = keep"));
        let session = genie.session.as_ref().unwrap();
        assert!(
            simulator(genie.export_view(session))
                .find("1 of 2")
                .is_err()
        );
    }

    fn variant_count(genie: &Genie) -> usize {
        let track = &genie.session.as_ref().unwrap().tracks[0];
        match &track.data(0).unwrap().content {
            Content::Variants(calls) => calls.variants.len(),
            _ => 0,
        }
    }

    #[test]
    fn variant_tracks_filter_by_quality_and_filter() {
        let mut genie = with_variants();
        let high = field_rule("Quality", Test::AtLeast(60.0));
        let _ = genie.update(Message::Filter(0, Filtering::Add(high)));
        assert_eq!(variant_count(&genie), 0);
        let _ = genie.update(Message::Filter(0, Filtering::Remove(0)));
        let pass = field_rule("Filter", Test::Keep("PASS".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(pass)));
        assert_eq!(variant_count(&genie), 1);
    }

    #[test]
    fn sessions_keep_track_filters() {
        let genie = with_filtered();
        let saved = genie.saved().unwrap();
        let filter = saved.tracks[0].filter.clone();
        assert_eq!(filter.rules, [field_rule("Column 14", Test::AtLeast(0.1))]);
        assert_eq!(filter.label.as_deref(), Some("Column 13"));
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        let _ = restored.update(Message::TrackOpened(Ok(source("genes.bed.gz"))));
        let track = &restored.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.filter, filter);
    }

    #[test]
    fn new_data_follows_the_filter() {
        let mut genie = with_track();
        let hide = field_rule("Column 13", Test::Hide("keep".to_owned()));
        let _ = genie.update(Message::Filter(0, Filtering::Add(hide)));
        accept_two(&mut genie);
        assert_eq!(shown_names(&genie), ["BRCA2"]);
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let (_, all) = track.slot.loaded.as_ref().unwrap();
        assert!(
            matches!(&all.content, Content::Features(annotation) if annotation.features.len() == 2)
        );
    }

    #[test]
    fn hover_cards_show_six_fields_and_pinned_cards_show_all() {
        let mut genie = with_fields();
        let _ = genie.update(Message::TrackHovered(0, 0, Some(over(15.0, 5.0))));
        assert!(shows(&genie, "Column 13"));
        assert!(shows(&genie, "value6"));
        assert!(!shows(&genie, "value7"));
        assert!(shows(&genie, "+2 fields: click to see all"));
        let _ = genie.update(Message::TrackClicked(0, 0, over(15.0, 5.0)));
        let _ = genie.update(Message::TrackHovered(0, 0, None));
        assert!(shows(&genie, "value8"));
        assert!(!shows(&genie, "+2 fields: click to see all"));
        let text = genie.copy_text().unwrap();
        assert_eq!(text.lines().last(), Some("Column 20: value8"));
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
    fn exports_ask_for_a_title_and_the_parts_of_the_figure() {
        let mut genie = with_track();
        let _ = genie.update(Message::Command(Command::Export));
        for text in [
            "Export image",
            "Title",
            "Scale bar",
            "Region bands",
            "Export…",
        ] {
            assert!(shows(&genie, text), "{text}");
        }
        let _ = genie.update(Message::Figure(Figuring::Title("TP53 promoter".to_owned())));
        let _ = genie.update(Message::Figure(Figuring::Scale));
        let _ = genie.update(Message::Figure(Figuring::Bands));
        let figure = &genie.session.as_ref().unwrap().figure;
        assert_eq!(
            figure,
            &crate::export::Figure {
                title: "TP53 promoter".to_owned(),
                scale: false,
                bands: false,
            }
        );
        let _ = genie.update(Message::Figure(Figuring::Export));
        assert!(!shows(&genie, "Export image"));
        let saved = genie.saved().unwrap();
        assert_eq!(saved.figure.title, "TP53 promoter");
        let mut restored = Genie::default();
        let _ = restored.restore(saved);
        let _ = restored.update(Message::Opened(Ok(reference())));
        assert!(!restored.session.as_ref().unwrap().figure.scale);
    }

    /// A genie with a track and a region in view that exports a figure with a title.
    fn titled() -> Genie {
        let mut genie = with_track();
        let session = genie.session.as_mut().unwrap();
        let exon = Some("Exon 2".to_owned());
        session.marks.add(0, Region::between(100, 300), exon);
        session.figure.title = " TP53 promoter ".to_owned();
        genie
    }

    #[test]
    fn figures_show_the_title_the_scale_bar_and_the_regions_in_view() {
        let genie = titled();
        let session = genie.session.as_ref().unwrap();
        let svg = genie.figure(session);
        for text in [">TP53 promoter</tspan>", ">Exon 2</text>", ">100 bp</text>"] {
            assert!(svg.contains(text), "{text}\n{svg}");
        }
        let mut ui = simulator(genie.export_view(session));
        assert!(ui.find("TP53 promoter").is_ok());
        assert!(ui.find(selector::id("figure-bands")).is_ok());
        assert!(ui.find(selector::id("scale-bar")).is_ok());
    }

    #[test]
    fn figures_leave_out_the_parts_that_are_off() {
        let mut genie = titled();
        let session = genie.session.as_mut().unwrap();
        session.figure = crate::export::Figure {
            title: String::new(),
            scale: false,
            bands: false,
        };
        let session = genie.session.as_ref().unwrap();
        let svg = genie.figure(session);
        for text in ["TP53 promoter", ">Exon 2</text>", ">100 bp</text>"] {
            assert!(!svg.contains(text), "{text}\n{svg}");
        }
        let mut ui = simulator(genie.export_view(session));
        assert!(ui.find(selector::id("figure-bands")).is_err());
        assert!(ui.find(selector::id("scale-bar")).is_err());
    }

    #[test]
    fn figures_with_panels_show_a_scale_bar_for_each_panel() {
        let mut genie = with_track();
        go(&mut genie, "chr1:101-200 chr1:401-900");
        let session = genie.session.as_ref().unwrap();
        let svg = genie.figure(session);
        for text in [">10 bp</text>", ">50 bp</text>"] {
            assert!(svg.contains(text), "{text}\n{svg}");
        }
        let mut ui = simulator(genie.export_view(session));
        assert!(ui.find(selector::id("scale-bar-1")).is_ok());
    }

    /// The number of pixels of a row of an image, from `from` to `to`, that are much darker or
    /// lighter than the top left pixel.
    fn inked(image: &export::Image, y: f32, (from, to): (u32, u32)) -> usize {
        let pixel = |x: u32, y: u32| image.rgba[((y * image.width + x) * 4) as usize];
        let background = pixel(4, 4);
        let y = (2.0 * y) as u32;
        (from..to)
            .filter(|&x| pixel(x, y).abs_diff(background) > 60)
            .count()
    }

    #[test]
    fn scale_bars_show_at_the_end_of_the_caption_and_of_each_header() {
        use super::export::CAPTION_HEIGHT;
        use super::view::HEADER_HEIGHT;

        let mut genie = with_track();
        let session = genie.session.as_ref().unwrap();
        let image = export::render(genie.export_view(session), genie.width, &genie.theme());
        let middle = CAPTION_HEIGHT / 2.0;
        assert!(inked(&image.unwrap(), middle, (1_790, 1_980)) > 150);
        go(&mut genie, "chr1:101-200 chr1:401-900");
        let session = genie.session.as_ref().unwrap();
        let image = export::render(genie.export_view(session), genie.width, &genie.theme());
        let middle = CAPTION_HEIGHT + 1.0 + HEADER_HEIGHT / 2.0;
        assert!(inked(&image.unwrap(), middle, (1_890, 1_980)) > 80);
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

    fn preparing_genie(name: &str) -> (Genie, Scratch, Work) {
        let scratch = Scratch::new(name);
        let cache = scratch.file("marker", "").parent().unwrap().join("cache");
        let data =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../genie-formats/tests/data/reads.bam");
        let reads = scratch.file("reads.bam", std::fs::read(data).unwrap());
        let Ok(Check::Needs(work)) = prepare::check(&reads.into(), Some(&cache)) else {
            panic!("expected work");
        };
        let mut genie = opened();
        genie.cache = Some(cache);
        (genie, scratch, work)
    }

    fn preparing_id(genie: &Genie) -> u64 {
        genie.session.as_ref().unwrap().preparing[0].id
    }

    #[test]
    fn files_that_need_an_index_show_progress_until_stopped() {
        let (mut genie, _scratch, work) = preparing_genie("app-prepare-stop");
        let _ = genie.update(Message::Prepare(work));
        let id = preparing_id(&genie);
        let stop = Arc::clone(&genie.session.as_ref().unwrap().preparing[0].stop);
        let progress = Preparation::Progress(Step::Indexing, 0.45);
        let _ = genie.update(Message::Preparing(id, progress));
        assert!(shows(&genie, "Indexing reads.bam… 45 %"));
        let _ = genie.update(Message::Preparing(id, Preparation::Stop));
        assert!(genie.session.as_ref().unwrap().preparing.is_empty());
        assert!(stop.load(Ordering::Relaxed));
    }

    #[test]
    fn prepared_files_become_tracks_with_their_saved_settings() {
        let (mut genie, _scratch, work) = preparing_genie("app-prepare-done");
        let location = work.location.clone();
        let cache = genie.cache.clone().unwrap();
        let prepared = prepare::run(&work, &cache, &mut |_, _| {}, &AtomicBool::new(false));
        let mut saved = genie.saved().unwrap();
        let mut track = SavedTrack::plain(location.clone());
        track.name = Some("Tumor".to_owned());
        saved.tracks = vec![track];
        genie.restoring = Some(saved);
        genie.waiting = 1;
        let _ = genie.update(Message::Prepare(work));
        assert!(genie.restoring.is_none());
        let id = preparing_id(&genie);
        let source = crate::track::open_with(location, Some(prepared.unwrap())).map(Arc::new);
        let _ = genie.update(Message::Preparing(id, Preparation::Opened(source)));
        let session = genie.session.as_ref().unwrap();
        assert!(session.preparing.is_empty());
        assert_eq!(session.tracks.len(), 1);
        assert_eq!(session.tracks[0].name, "Tumor");
    }

    #[test]
    fn sessions_keep_files_that_are_being_prepared() {
        let (mut genie, _scratch, work) = preparing_genie("app-prepare-saved");
        let location = work.location.clone();
        let _ = genie.update(Message::Prepare(work));
        let saved = genie.saved().unwrap();
        let locations: Vec<&Location> = saved.tracks.iter().map(|track| &track.location).collect();
        assert_eq!(locations, [&location]);
    }

    #[test]
    fn the_settings_show_the_cache_and_clear_it() {
        let (mut genie, _scratch, work) = preparing_genie("app-cache");
        let _ = genie.update(Message::ToggleSettings);
        let _ = genie.update(Message::Cache(Caching::Measured(2_048)));
        assert!(shows(&genie, "2 KB"));
        let _ = genie.update(Message::Cache(Caching::Cleared(2_048)));
        assert!(shows(&genie, "Removed 2 KB of indexes and copies."));
        let _ = genie.update(Message::Cache(Caching::Measured(0)));
        let _ = genie.update(Message::Prepare(work));
        let _ = genie.update(Message::Cache(Caching::Clear));
        assert_eq!(genie.cache_size, Some(0));
    }

    #[test]
    fn failed_preparation_says_why() {
        let (mut genie, _scratch, work) = preparing_genie("app-prepare-fail");
        let _ = genie.update(Message::Prepare(work));
        let id = preparing_id(&genie);
        let error = "genie cannot prepare reads.bam. The disk is full.".to_owned();
        let _ = genie.update(Message::Preparing(id, Preparation::Done(Err(error))));
        assert!(genie.session.as_ref().unwrap().preparing.is_empty());
        assert!(shows(
            &genie,
            "genie cannot prepare reads.bam. The disk is full."
        ));
    }
}
