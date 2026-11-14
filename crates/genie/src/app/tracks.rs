use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::SystemTime;

use genie_core::methylation::Site;
use genie_core::{Region, Viewport};
use genie_render::features::Mode;
use genie_render::{alignments, features, signal, variants};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::time::Instant;
use iced::widget::operation;
use iced::{Point, Task};

use crate::card::{Card, Detail};
use crate::genome;
use crate::location::Location;
use crate::paths;
use crate::port::Reply;
use crate::prepare::{self, Check, Step, Work};
use crate::reading;
use crate::reference;
use crate::saved::{self, Saved, SavedTrack};
use crate::scene::Pointer;
use crate::tiles::{Key, TILING};
use crate::track::{self, Change, Content, Pile, Source, Track};

use super::orders::Answer;
use super::view::{body_height, signal_columns, signal_scale};
use super::{
    Filtering, Genie, Lane, Message, Opening, Preparation, Preparing, Ranging, Renaming, Status,
};

pub(super) const TRACK_NAME: &str = "track-name";
const BAD_RANGE: &str = "Type two numbers. The maximum must be larger than the minimum.";
const MIN_HEIGHT: f32 = 10.0;
const GRAB: f32 = 8.0;
pub(super) const NO_REFERENCE: &str = "Open a reference sequence before you add tracks.";
const NO_CACHE: &str = "genie cannot find a cache folder for the genome files.";
pub(super) const REMOTE_SESSION: &str =
    "genie cannot open session files from the web. Copy the file to this computer, then open it.";

impl Genie {
    pub(super) fn track_opened(&mut self, result: Result<Arc<Source>, String>) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let task = match result {
            Ok(source) => self.add_track(source),
            Err(error) => {
                self.fail(error);
                Task::none()
            }
        };
        self.track_settled();
        task
    }

    fn track_settled(&mut self) {
        if self.restoring.is_some() {
            self.waiting = self.waiting.saturating_sub(1);
            if self.waiting == 0 {
                self.restoring = None;
            }
        }
    }

    pub(super) fn prepare(
        &mut self,
        work: Work,
        reply: Option<(Reply, Option<String>)>,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let saved = self.restoring.as_ref().and_then(|saved| {
            saved
                .tracks
                .iter()
                .find(|track| track.location == work.location)
                .cloned()
        });
        self.track_settled();
        let (Some(session), Some(cache)) = (&mut self.session, self.cache.clone()) else {
            if let Some((reply, _)) = reply {
                reply.send(NO_REFERENCE);
            }
            return Task::none();
        };
        if session
            .preparing
            .iter()
            .any(|item| item.location == work.location)
        {
            let text = format!("genie is already preparing {}.", work.name);
            if let Some((reply, _)) = reply {
                reply.send(text.clone());
            }
            return self.inform(text);
        }
        let id = self.next_track;
        self.next_track += 1;
        let stop = Arc::new(AtomicBool::new(false));
        session.preparing.push(Preparing {
            id,
            location: work.location.clone(),
            name: work.name.clone(),
            step: Step::Indexing,
            share: 0.0,
            stop: Arc::clone(&stop),
            saved,
            reply,
        });
        let events = iced::stream::channel(16, async move |mut output| {
            let (sender, mut receiver) = mpsc::unbounded();
            thread::spawn(move || {
                let progress = sender.clone();
                let mut report = move |step, share| {
                    let _ = progress.unbounded_send(Preparation::Progress(step, share));
                };
                let result = prepare::run(&work, &cache, &mut report, &stop);
                let _ = sender.unbounded_send(Preparation::Done(result));
            });
            while let Some(event) = receiver.next().await {
                let _ = output.send(event).await;
            }
        });
        Task::run(events, move |event| Message::Preparing(id, event))
    }

    pub(super) fn preparing(&mut self, id: u64, event: Preparation) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let Some(at) = session.preparing.iter().position(|item| item.id == id) else {
            return Task::none();
        };
        match event {
            Preparation::Progress(step, share) => {
                let item = &mut session.preparing[at];
                (item.step, item.share) = (step, share);
                Task::none()
            }
            Preparation::Done(Ok(prepared)) => {
                let location = session.preparing[at].location.clone();
                Task::perform(
                    async move { track::open_with(location, Some(prepared)).map(Arc::new) },
                    move |result| Message::Preparing(id, Preparation::Opened(result)),
                )
            }
            Preparation::Opened(Ok(source)) => {
                let item = session.preparing.remove(at);
                let task = self.add_track(Arc::clone(&source));
                let track = self.session.as_mut().and_then(|session| {
                    session
                        .tracks
                        .iter_mut()
                        .find(|track| track.source == source)
                });
                if let Some(track) = track {
                    if let Some(found) = &item.saved {
                        apply_saved(track, found);
                    }
                    if let Some((_, Some(name))) = &item.reply {
                        track.name.clone_from(name);
                    }
                }
                if let Some((reply, _)) = &item.reply {
                    self.answers.push((Answer::Done, reply.clone()));
                }
                task
            }
            Preparation::Done(Err(error)) | Preparation::Opened(Err(error)) => {
                let item = session.preparing.remove(at);
                if let Some((reply, _)) = &item.reply {
                    reply.send(error.clone());
                }
                if error != prepare::STOPPED {
                    self.fail(error);
                }
                Task::none()
            }
            Preparation::Stop => {
                let item = session.preparing.remove(at);
                if let Some((reply, _)) = &item.reply {
                    reply.send(prepare::STOPPED);
                }
                Task::none()
            }
        }
    }

    pub(super) fn tile_loaded(
        &mut self,
        generation: u64,
        key: Key,
        result: Result<Arc<[u8]>, String>,
    ) {
        if generation == self.generation {
            match result {
                Ok(bases) => self.tiles.insert(key, bases),
                Err(_) => self.tiles.cancel(key),
            }
        }
    }

    pub(super) fn lane(&mut self, id: u64, lane: Lane) {
        if let Lane::Dropped = lane {
            let target = self.drop_target();
            self.moving = None;
            if let (Some((from, gap)), Some(session)) = (target, &mut self.session) {
                let track = session.tracks.remove(from);
                session
                    .tracks
                    .insert(if gap > from { gap - 1 } else { gap }, track);
            }
            return;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(position) = session.tracks.iter().position(|track| track.id == id) else {
            return;
        };
        match lane {
            Lane::Resized(delta) => {
                let from = match self.resize {
                    Some((dragged, from)) if dragged == id => from,
                    _ => body_height(session, &session.tracks[position]),
                };
                self.resize = Some((id, from));
                session.tracks[position].height = Some((from + delta).max(MIN_HEIGHT));
            }
            Lane::Released => self.resize = None,
            Lane::Fit => session.tracks[position].height = None,
            Lane::Moved(delta) => self.moving = Some((id, delta)),
            Lane::Dropped => {}
        }
    }

    pub(super) fn drop_target(&self) -> Option<(usize, usize)> {
        let (id, delta) = self.moving?;
        let session = self.session.as_ref()?;
        let padding = self.settings.density.padding();
        let heights: Vec<f32> = session
            .tracks
            .iter()
            .map(|track| {
                if track.over.is_some() {
                    0.0
                } else {
                    body_height(session, track) + 2.0 * padding + 1.0
                }
            })
            .collect();
        let from = session.tracks.iter().position(|track| track.id == id)?;
        let pointer = heights[..from].iter().sum::<f32>() + GRAB + delta;
        let mut top = 0.0;
        for (gap, height) in heights.iter().enumerate() {
            if pointer < top + height / 2.0 {
                return Some((from, gap));
            }
            top += height;
        }
        Some((from, heights.len()))
    }

    pub(super) fn filter(&mut self, id: u64, filtering: Filtering) {
        self.close_menus();
        self.on_session(|session| {
            let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) else {
                return;
            };
            let filter = &mut track.filter;
            match filtering {
                Filtering::Add(rule) if !filter.rules.contains(&rule) => filter.rules.push(rule),
                Filtering::Remove(index) if index < filter.rules.len() => {
                    filter.rules.remove(index);
                }
                Filtering::Label(label) => filter.label = label,
                Filtering::Add(_) | Filtering::Remove(_) => {}
            }
            track.refilter();
        });
    }

    pub(super) fn close_menus(&mut self) {
        self.menu = None;
        self.field = None;
        self.sequence_menu = None;
        self.copy_menu = None;
        self.popup = None;
        self.address = None;
        self.range_draft = None;
    }

    pub(super) fn open_menu(&mut self, id: u64, at: Point) {
        self.pointed = Some(id);
        self.menu = Some((id, Point::new(at.x, at.y - self.scroll)));
        self.range_draft = self
            .shown_range(id)
            .map(|scale| (signal::number(scale.min), signal::number(scale.max)));
    }

    fn shown_range(&self, id: u64) -> Option<signal::Scale> {
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        if let Some(range) = track.plot.range {
            return Some(range);
        }
        let Content::Signal(data) = &track.data(session.contig)?.content else {
            return None;
        };
        let columns = signal_columns(track, session.viewport, &data.summaries);
        signal_scale(session, track, 0, &columns)
    }

    pub(super) fn range(&mut self, id: u64, ranging: Ranging) {
        let fixed = match ranging {
            Ranging::Min(text) => {
                if let Some((min, _)) = &mut self.range_draft {
                    *min = text;
                }
                return;
            }
            Ranging::Max(text) => {
                if let Some((_, max)) = &mut self.range_draft {
                    *max = text;
                }
                return;
            }
            Ranging::Auto => None,
            Ranging::Fix => {
                let number = |text: &str| {
                    text.trim()
                        .replace(',', "")
                        .parse::<f32>()
                        .ok()
                        .filter(|value| value.is_finite())
                };
                let draft = self.range_draft.as_ref();
                let parsed = draft.map(|(min, max)| (number(min), number(max)));
                let Some((Some(min), Some(max))) = parsed.filter(|pair| match pair {
                    (Some(min), Some(max)) => max > min,
                    _ => false,
                }) else {
                    self.warn(BAD_RANGE.to_owned());
                    return;
                };
                Some(signal::Scale { min, max })
            }
        };
        self.close_menus();
        self.on_session(|session| {
            if let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) {
                track.plot.range = fixed;
                if fixed.is_some() {
                    track.plot.linked = false;
                }
            }
        });
    }

    pub(super) fn hovered(&mut self, id: u64, panel: usize, pointer: Option<Pointer>) {
        if pointer.is_some() {
            self.pointed = Some(id);
        }
        self.hover = pointer.and_then(|pointer| self.card(id, panel, pointer));
    }

    pub(super) fn change_reading(&mut self, change: reading::Change) {
        self.sequence_menu = None;
        self.on_session(|session| session.reading.change(change));
    }

    pub(super) fn card(&self, id: u64, panel: usize, pointer: Pointer) -> Option<Card> {
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        let (contig, viewport) = session.view(panel);
        let slot = track.slot(panel);
        let data = slot.data(contig)?;
        let mode = track.mode_at(panel, contig);
        let (x, y) = (pointer.local.x, pointer.local.y);
        let detail = match &data.content {
            Content::Features(annotation) => {
                let index =
                    features::hit(viewport, &annotation.features, &annotation.rows, mode, x, y)?;
                Detail::Feature(annotation.features[index].clone())
            }
            Content::Reads(pile) => {
                read_detail(track, panel, contig, pile, viewport, mode, (x, y))?
            }
            Content::Variants(calls) => {
                let (index, line) = variants::hit(viewport, &calls.variants, mode, x, y)?;
                let variant = calls.variants[index].clone();
                let sample = line.and_then(|line| {
                    let names = track.source.samples();
                    let size = variants::group(variant.calls.len());
                    let first = line * size;
                    let last = (first + size).min(variant.calls.len());
                    let call = variants::strongest(variant.calls.get(first..last)?);
                    let label = if last - first == 1 {
                        names.get(first)?.clone()
                    } else {
                        let (from, to) = (names.get(first)?, names.get(last - 1)?);
                        format!("{from} to {to} ({} samples)", last - first)
                    };
                    Some((label, call))
                });
                Detail::Variant { variant, sample }
            }
            Content::Signal(data) => {
                if let Some(detail) = site_detail(track, viewport, &data.sites, x) {
                    detail
                } else {
                    let columns = signal_columns(track, viewport, &data.summaries);
                    let column = signal::hit(&columns, x)?;
                    Detail::Signal {
                        span: column.span,
                        bin: column.bin,
                    }
                }
            }
        };
        let fields = match &detail {
            Detail::Feature(feature) => track::feature_fields(&track.source, feature),
            Detail::Variant { variant, .. } => track::variant_fields(&track.source, variant),
            Detail::Read { .. }
            | Detail::Junction(_)
            | Detail::Methylation { .. }
            | Detail::Signal { .. } => Vec::new(),
        };
        Some(Card {
            track: id,
            contig: session.reference.assembly.contigs()[contig].name.clone(),
            detail,
            at: Point::new(pointer.at.x, pointer.at.y - self.scroll),
            fields,
        })
    }

    pub(super) fn change_track(&mut self, id: u64, change: Change) -> Task<Message> {
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

    pub(super) fn fetch(&mut self) -> Task<Message> {
        Task::batch([self.fetch_bases(), self.fetch_features()])
    }

    fn fetch_features(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let frames: Vec<(usize, usize, Viewport)> = (0..session.count())
            .map(|panel| {
                let (contig, viewport) = session.frame(panel);
                (panel, contig, viewport)
            })
            .collect();
        let mut tasks = Vec::new();
        for (panel, contig, viewport) in frames {
            tasks.extend(Self::fetch_panel(session, panel, contig, viewport));
        }
        Task::batch(tasks)
    }

    fn fetch_panel(
        session: &mut super::Session,
        panel: usize,
        contig: usize,
        viewport: Viewport,
    ) -> Vec<Task<Message>> {
        let visible = viewport.visible();
        let length = viewport.length();
        let wanted = visible.expand(visible.len(), length);
        let step = track::step(viewport.scale());
        let detail = track::detailed(visible);
        let reference = &session.reference;
        session
            .tracks
            .iter_mut()
            .filter(|track| track.needs_at(panel, contig, visible, step))
            .filter_map(|track| {
                let name = track.contigs[contig].clone()?;
                track.slot_mut(panel).wanted = Some((contig, wanted));
                let (id, source, options) = (track.id, Arc::clone(&track.source), track.options);
                let reference = Arc::clone(reference);
                Some(Task::perform(
                    async move {
                        let started = Instant::now();
                        let bases =
                            |window: Region| reference.fetch(contig, window.clamp_to(length));
                        let result =
                            track::load(&source, &name, wanted, options, step, detail, bases);
                        match &result {
                            Ok(loaded) => log::debug!(
                                "Loaded {} items of {} at {name}:{}-{} in {:.0?}",
                                loaded.count(),
                                source.name,
                                wanted.start() + 1,
                                wanted.end(),
                                started.elapsed()
                            ),
                            Err(error) => log::warn!("{}: {error}", source.name),
                        }
                        result.map(Arc::new)
                    },
                    move |result| Message::TrackLoaded(id, panel, contig, wanted, result),
                ))
            })
            .collect()
    }

    fn fetch_bases(&mut self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let generation = self.generation;
        let mut keys = Vec::new();
        for panel in 0..session.count() {
            let (contig, viewport) = session.frame(panel);
            if viewport.scale() > 1.0 {
                continue;
            }
            let visible = viewport.visible();
            let wanted = visible.expand(visible.len(), viewport.length());
            keys.extend(self.tiles.request(contig, wanted));
        }
        Task::batch(keys.into_iter().map(|key| {
            let reference = Arc::clone(&session.reference);
            Task::perform(
                async move {
                    reference
                        .fetch(key.contig, TILING.span(key.index))
                        .map(Arc::from)
                },
                move |result| Message::TileLoaded(generation, key, result),
            )
        }))
    }

    pub(super) fn open_genome(&mut self, id: &str) -> Task<Message> {
        self.close_menus();
        let Some(genome) = genome::find(id) else {
            return Task::none();
        };
        let Some(cache) = self.cache.clone() else {
            self.fail(NO_CACHE.to_owned());
            return Task::none();
        };
        let folder = genome.folder(&cache);
        if self.session.is_none() {
            self.status = Status::Opening(genome.label.clone());
        }
        self.pending = vec![Location::File(genome.genes_in(&folder))];
        self.busy += 1;
        Task::perform(
            async move { genome::open(&genome, &folder, SystemTime::now()).map(Arc::new) },
            Message::Opened,
        )
    }

    pub(super) fn open(&mut self, location: Location) -> Task<Message> {
        if let Some(path) = location.path()
            && saved::is_session(path)
        {
            return match Saved::load(path) {
                Ok(saved) => {
                    self.recent.add(location.clone().into());
                    let remember = self.remember();
                    Task::batch([self.restore(saved), remember])
                }
                Err(error) => {
                    self.fail(error);
                    Task::none()
                }
            };
        }
        if location.is_remote() && saved::is_session(Path::new(&location.name())) {
            self.fail(REMOTE_SESSION.to_owned());
            return Task::none();
        }
        if track::is_track(&location) {
            if self.session.is_none() {
                self.status = Status::Failed(NO_REFERENCE.to_owned());
                return Task::none();
            }
            self.busy += 1;
            let cache = self.cache.clone();
            return Task::perform(
                async move { opening(location, cache.as_deref()) },
                std::convert::identity,
            );
        }
        let name = location.name();
        if self.session.is_none() {
            self.status = Status::Opening(name.clone());
        }
        self.busy += 1;
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            let started = Instant::now();
            let mut report = |share| {
                let _ = sender.unbounded_send(Opening::Progress(share));
            };
            let result = reference::open(location, paths::cache_dir().as_deref(), &mut report);
            if let Ok(reference) = &result {
                let contigs = reference.assembly.contigs().len();
                log::info!(
                    "Opened {} with {contigs} contigs in {:.0?}",
                    reference.name,
                    started.elapsed()
                );
            }
            let _ = sender.unbounded_send(Opening::Done(result.map(Arc::new)));
        });
        Task::run(receiver, move |event| match event {
            Opening::Progress(share) => Message::Indexing(name.clone(), (share * 100.0) as u8),
            Opening::Done(result) => Message::Opened(result),
        })
    }

    pub(super) fn rename(&mut self, renaming: Renaming) -> Task<Message> {
        match renaming {
            Renaming::Start(id) => {
                self.close_menus();
                let name = self
                    .session
                    .as_ref()
                    .and_then(|session| session.tracks.iter().find(|track| track.id == id))
                    .map(|track| track.name.clone());
                if let Some(name) = name {
                    self.renaming = Some((id, name));
                    return Task::batch([
                        operation::focus(TRACK_NAME),
                        operation::select_all(TRACK_NAME),
                    ]);
                }
            }
            Renaming::Draft(text) => {
                if let Some((_, draft)) = &mut self.renaming {
                    *draft = text;
                }
            }
            Renaming::Done => {
                let Some((id, draft)) = self.renaming.take() else {
                    return Task::none();
                };
                let clean: String = draft
                    .chars()
                    .map(|letter| if letter.is_control() { ' ' } else { letter })
                    .collect();
                let source = self
                    .session
                    .as_ref()
                    .and_then(|session| session.tracks.iter().find(|track| track.id == id))
                    .map(|track| Arc::clone(&track.source));
                let Some(source) = source else {
                    return Task::none();
                };
                let name = match clean.trim() {
                    "" => self.track_name(&source),
                    name => name.to_owned(),
                };
                self.on_session(|session| {
                    if let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) {
                        track.name = name;
                    }
                });
            }
            Renaming::Cancel => self.renaming = None,
        }
        Task::none()
    }

    pub(super) fn track_name(&self, source: &Source) -> String {
        let genes = self
            .current_genome()
            .zip(self.cache.as_deref())
            .filter(|(genome, cache)| {
                source.location == Location::File(genome.genes_in(&genome.folder(cache)))
            });
        genes.map_or_else(|| source.name.clone(), |(genome, _)| genome.track)
    }

    pub(super) fn add_track(&mut self, source: Arc<Source>) -> Task<Message> {
        let name = self.track_name(&source);
        let Some(session) = &mut self.session else {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        };
        if session.tracks.iter().any(|track| track.source == source) {
            return self.inform(format!("The track {name} is already open."));
        }
        self.notice = None;
        let id = self.next_track;
        self.next_track += 1;
        let mut track = Track::new(id, Arc::clone(&source), &session.reference.assembly);
        track.name = name;
        if let Some(saved) = &self.restoring {
            let saved_at = |location: &Location| {
                saved
                    .tracks
                    .iter()
                    .position(|known| known.location == *location)
            };
            if let Some(found) = saved_at(&source.location).map(|at| &saved.tracks[at]) {
                apply_saved(&mut track, found);
            }
            session.tracks.push(track);
            session
                .tracks
                .sort_by_key(|track| saved_at(&track.source.location).unwrap_or(usize::MAX));
            let links: Vec<(u64, u64)> = session
                .tracks
                .iter()
                .filter(|track| track.over.is_none())
                .filter_map(|track| {
                    let base = saved.tracks[saved_at(&track.source.location)?].over?;
                    let location = &saved.tracks.get(base)?.location;
                    let base = session
                        .tracks
                        .iter()
                        .find(|other| other.source.location == *location)?;
                    Some((track.id, base.id))
                })
                .collect();
            for (member, base) in links {
                track::change(&mut session.tracks, member, Change::Overlay(base));
            }
        } else {
            session.tracks.push(track);
        }
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
}

fn read_detail(
    track: &Track,
    panel: usize,
    contig: usize,
    pile: &Pile,
    viewport: Viewport,
    mode: Mode,
    (x, y): (f32, f32),
) -> Option<Detail> {
    let (keys, lanes) = track.methylation_lanes(pile);
    let above = alignments::methylation_height(lanes.len());
    if y < above {
        let (index, site) = alignments::methylation_hit(viewport, &lanes, x, y)?;
        return Some(Detail::Methylation {
            name: track.methylation_name(pile)?,
            position: site.position,
            group: lanes[index].0.clone(),
            counts: pile.methylation.at(keys[index], site.position),
        });
    }
    let y = y - above;
    if y < pile.arc_height() {
        let index = alignments::arc_hit(viewport, &pile.junctions, x, y)?;
        return Some(Detail::Junction(pile.junctions[index]));
    }
    let slot = track.slot(panel);
    let (rows, groups, y) = (slot.rows(pile), &pile.groups, y - pile.arc_height());
    let index = alignments::hit(viewport, &pile.reads, rows, groups, mode, x, y)?;
    let read = pile.reads[index].alignment.clone();
    let names = track.source.contigs();
    let own = track.contigs[contig]
        .as_ref()
        .and_then(|name| names.iter().position(|known| known == name));
    let mate = read.mate.and_then(|mate| names.get(mate.contig).cloned());
    let same = read.mate.is_some_and(|mate| Some(mate.contig) == own);
    Some(Detail::Read { read, mate, same })
}

fn site_detail(track: &Track, viewport: Viewport, sites: &[Site], x: f32) -> Option<Detail> {
    let site = alignments::site_at(viewport, sites, x)?;
    let name = track.shown_modification()?;
    Some(Detail::Methylation {
        name: name.clone(),
        position: site.position,
        group: None,
        counts: vec![(name, site.modified, site.total)],
    })
}

fn apply_saved(track: &mut Track, found: &SavedTrack) {
    if let Some(name) = &found.name {
        track.name.clone_from(name);
    }
    track.mode = found.mode;
    track.hue = found.hue;
    track.height = found.height;
    track.options = found.options;
    track.plot = found.plot;
    track.filter = found.filter.clone();
    track.set_modification(found.modification.clone());
}

pub(super) fn opening(location: Location, cache: Option<&Path>) -> Message {
    match prepare::check(&location, cache) {
        Ok(Check::Ready) => Message::TrackOpened(track::open(location).map(Arc::new)),
        Ok(Check::Prepared(prepared)) => {
            Message::TrackOpened(track::open_with(location, Some(prepared)).map(Arc::new))
        }
        Ok(Check::Needs(work)) => Message::Prepare(work),
        Err(error) => Message::TrackOpened(Err(error)),
    }
}

pub(super) async fn pick(tracks: bool) -> Option<PathBuf> {
    let references = ["fa", "fasta", "fna", "fas", "gz", "bgz", "2bit"];
    let dialog = if tracks {
        rfd::AsyncFileDialog::new()
            .set_title("Open a track, a reference sequence or a session")
            .add_filter(
                "Tracks",
                &[
                    "gz",
                    "bgz",
                    "bed",
                    "gff",
                    "gff3",
                    "gtf",
                    "vcf",
                    "bam",
                    "bw",
                    "bigwig",
                    "bb",
                    "bigbed",
                    "bedgraph",
                    "bdg",
                    "bedmethyl",
                ],
            )
            .add_filter("Reference sequences", &references)
    } else {
        rfd::AsyncFileDialog::new()
            .set_title("Open a reference sequence or a session")
            .add_filter("Reference sequences", &references)
    }
    .add_filter("Sessions", &[saved::EXTENSION]);
    dialog.pick_file().await.map(|file| file.path().to_owned())
}
