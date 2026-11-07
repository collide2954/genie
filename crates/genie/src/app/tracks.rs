use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use genie_core::Region;
use genie_render::{alignments, features, signal, variants};
use iced::futures::channel::mpsc;
use iced::time::Instant;
use iced::{Point, Task};

use crate::card::{Card, Detail};
use crate::paths::{self, file_name};
use crate::reading;
use crate::reference;
use crate::saved::{self, Saved};
use crate::scene::Pointer;
use crate::tiles::{Key, TILING};
use crate::track::{self, Change, Content, Source, Track};

use super::view::{body_height, signal_columns};
use super::{Genie, Lane, Message, Opening, Status};

const MIN_HEIGHT: f32 = 10.0;
const GRAB: f32 = 8.0;
pub(super) const NO_REFERENCE: &str = "Open a reference sequence before you add tracks.";

impl Genie {
    pub(super) fn track_opened(&mut self, result: Result<Arc<Source>, String>) -> Task<Message> {
        let task = match result {
            Ok(source) => self.add_track(source),
            Err(error) => {
                self.fail(error);
                Task::none()
            }
        };
        if self.restoring.is_some() {
            self.waiting = self.waiting.saturating_sub(1);
            if self.waiting == 0 {
                self.restoring = None;
            }
        }
        task
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
            .map(|track| body_height(session, track) + 2.0 * padding + 1.0)
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

    pub(super) fn close_menus(&mut self) {
        self.menu = None;
        self.sequence_menu = None;
        self.settings_open = false;
    }

    pub(super) fn change_reading(&mut self, change: reading::Change) {
        self.sequence_menu = None;
        self.on_session(|session| session.reading.change(change));
    }

    pub(super) fn card(&self, id: u64, pointer: Pointer) -> Option<Card> {
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
                let rows = track.rows(pile);
                let groups = &pile.groups;
                let index = alignments::hit(viewport, &pile.reads, rows, groups, mode, x, y)?;
                let read = pile.reads[index].alignment.clone();
                let names = track.source.contigs();
                let own = track.contigs[session.contig]
                    .as_ref()
                    .and_then(|name| names.iter().position(|known| known == name));
                let mate = read.mate.and_then(|mate| names.get(mate.contig).cloned());
                let same = read.mate.is_some_and(|mate| Some(mate.contig) == own);
                Detail::Read { read, mate, same }
            }
            Content::Variants(calls) => {
                let (index, sample) = variants::hit(viewport, &calls.variants, mode, x, y)?;
                let variant = calls.variants[index].clone();
                let sample = sample.and_then(|sample| {
                    let name = track.source.samples().get(sample)?.clone();
                    Some((name, variant.calls[sample]))
                });
                Detail::Variant { variant, sample }
            }
            Content::Signal(data) => {
                let column = signal::hit(&signal_columns(track, viewport, &data.summaries), x)?;
                Detail::Signal {
                    span: column.span,
                    bin: column.bin,
                }
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
        let viewport = session.destination();
        let visible = viewport.visible();
        let contig = session.contig;
        let length = viewport.length();
        let wanted = visible.expand(visible.len(), length);
        let step = track::step(viewport.scale());
        let detail = track::detailed(visible);
        let reference = &session.reference;
        let tasks: Vec<Task<Message>> = session
            .tracks
            .iter_mut()
            .filter(|track| track.needs(contig, visible, step))
            .filter_map(|track| {
                let name = track.contigs[contig].clone()?;
                track.wanted = Some((contig, wanted));
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

    pub(super) fn open(&mut self, path: PathBuf) -> Task<Message> {
        if saved::is_session(&path) {
            return match Saved::load(&path) {
                Ok(saved) => {
                    self.recent.add(path);
                    let remember = self.remember();
                    Task::batch([self.restore(saved), remember])
                }
                Err(error) => {
                    self.fail(error);
                    Task::none()
                }
            };
        }
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
            let started = Instant::now();
            let mut report = |share| {
                let _ = sender.unbounded_send(Opening::Progress(share));
            };
            let result = reference::open(path, paths::cache_dir().as_deref(), &mut report);
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
        let mut track = Track::new(id, Arc::clone(&source), &session.reference.assembly);
        if let Some(saved) = &self.restoring {
            if let Some(found) = saved.tracks.iter().find(|known| known.path == source.path) {
                track.mode = found.mode;
                track.hue = found.hue;
                track.height = found.height;
                track.options = found.options;
                track.plot = found.plot;
            }
            session.tracks.push(track);
            session.tracks.sort_by_key(|track| {
                saved
                    .tracks
                    .iter()
                    .position(|known| known.path == track.source.path)
                    .unwrap_or(usize::MAX)
            });
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

pub(super) async fn pick(tracks: bool) -> Option<PathBuf> {
    let references = ["fa", "fasta", "fna", "fas", "gz", "bgz", "2bit"];
    let dialog = if tracks {
        rfd::AsyncFileDialog::new()
            .set_title("Open a track, a reference sequence or a session")
            .add_filter(
                "Tracks",
                &[
                    "gz", "bgz", "bed", "gff", "gff3", "gtf", "vcf", "bam", "bw", "bigwig", "bb",
                    "bigbed", "bedgraph", "bdg",
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
