use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use genie_core::{Locus, Region, Span};
use iced::Task;

use crate::genome::{self, Genome};
use crate::location::Location;
use crate::paths::{self, file_name};
use crate::saved::{self, Saved, SavedRegion, SavedTrack};
use crate::settings::Setting;

use super::{Genie, LAST, Message};

pub(super) fn local(
    location: &Location,
    genome: Option<&Genome>,
    folder: Option<&Path>,
) -> Location {
    if let (Some(genome), Some(folder), Location::File(path)) = (genome, folder, location) {
        let genes = genome.genes_in(folder);
        let parent = path.parent().and_then(Path::file_name);
        if path.file_name() == genes.file_name() && parent == Some(OsStr::new(&genome.id)) {
            return Location::File(genes);
        }
    }
    location.clone()
}

impl Genie {
    pub(super) fn restore(&mut self, saved: Saved) -> Task<Message> {
        let genome = saved.genome.as_deref().and_then(genome::find);
        let folder = genome
            .as_ref()
            .zip(self.cache.as_ref())
            .map(|(genome, cache)| genome.folder(cache));
        let tracks = saved
            .tracks
            .iter()
            .map(|track| local(&track.location, genome.as_ref(), folder.as_deref()))
            .collect();
        let task = match &genome {
            Some(genome) => self.open_genome(&genome.id),
            None => self.open(saved.reference.clone()),
        };
        self.pending = tracks;
        self.restoring = Some(saved);
        task
    }

    pub(super) fn resume(&mut self) {
        let (Some(saved), Some(session)) = (&self.restoring, &mut self.session) else {
            return;
        };
        let same = match (&saved.genome, &session.reference.genome) {
            (Some(wanted), Some(open)) => wanted == open,
            _ => saved.reference == session.reference.location,
        };
        if !same {
            self.restoring = None;
            return;
        }
        session.reading = saved.reading;
        let assembly = &session.reference.assembly;
        if let Some(target) = saved
            .locus
            .as_deref()
            .and_then(|locus| assembly.resolve(locus).ok())
        {
            let length = assembly.contigs()[target.contig].length;
            session.contig = target.contig;
            session.viewport = session.viewport.retarget(length).jump(target.span);
        }
        for region in &saved.regions {
            let Ok(target) = assembly.resolve(&region.locus) else {
                continue;
            };
            let length = assembly.contigs()[target.contig].length;
            let span = match target.span {
                Span::Region(span) => span.clamp_to(length),
                Span::Position(position) => Region::between(position, position + 1),
                Span::Whole => Region::between(0, length),
            };
            session
                .marks
                .add(target.contig, span, Some(region.name.clone()));
        }
    }

    pub(super) fn saved(&self) -> Option<Saved> {
        let session = self.session.as_ref()?;
        let contigs = session.reference.assembly.contigs();
        Some(Saved {
            reference: session.reference.location.clone(),
            genome: session.reference.genome.clone(),
            locus: Some(session.locus()),
            reading: session.reading,
            tracks: session
                .tracks
                .iter()
                .map(|track| SavedTrack {
                    location: track.source.location.clone(),
                    name: (track.name != self.track_name(&track.source))
                        .then(|| track.name.clone()),
                    over: track
                        .over
                        .and_then(|base| session.tracks.iter().position(|other| other.id == base)),
                    mode: track.mode,
                    hue: track.hue,
                    height: track.height,
                    options: track.options,
                    plot: track.plot,
                })
                .collect(),
            regions: session
                .marks
                .all()
                .iter()
                .map(|mark| SavedRegion {
                    name: mark.name.clone(),
                    locus: Locus {
                        name: contigs[mark.contig].name.clone(),
                        span: Span::Region(mark.span),
                    }
                    .to_string(),
                })
                .collect(),
        })
    }

    pub(super) fn save_session(&self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let stem = session
            .reference
            .name
            .split('.')
            .next()
            .unwrap_or("session");
        Task::perform(
            save_session_as(format!("{stem}.{}", saved::EXTENSION)),
            Message::SessionPicked,
        )
    }

    pub(super) fn session_picked(&mut self, path: Option<PathBuf>) -> Task<Message> {
        let (Some(path), Some(saved)) = (path, self.saved()) else {
            return Task::none();
        };
        let path = if saved::is_session(&path) {
            path
        } else {
            let mut text = path.into_os_string();
            text.push(".");
            text.push(saved::EXTENSION);
            PathBuf::from(text)
        };
        match saved.save(&path) {
            Ok(()) => {
                self.recent.add(path.clone().into());
                let remember = self.remember();
                Task::batch([
                    self.inform(format!("Saved {}.", file_name(&path))),
                    remember,
                ])
            }
            Err(error) => {
                self.warn(format!("The session could not be saved. {error}"));
                Task::none()
            }
        }
    }

    pub(super) fn restore_last(&mut self) -> Task<Message> {
        match self.last.clone() {
            Some(last) => self.restore(last),
            None => Task::none(),
        }
    }

    pub(super) fn change_setting(&mut self, setting: Setting) -> Task<Message> {
        self.settings = self.settings.apply(setting);
        self.save_settings()
    }

    pub(super) fn save_last(&self) -> Task<Message> {
        let (Some(folder), Some(saved)) = (paths::config_dir(), self.saved()) else {
            return Task::none();
        };
        Task::future(async move {
            let _ = std::fs::create_dir_all(&folder);
            let _ = saved.save(&folder.join(LAST));
        })
        .discard()
    }

    pub(super) fn save_settings(&self) -> Task<Message> {
        let Some(folder) = paths::config_dir() else {
            return Task::none();
        };
        let settings = self.settings;
        Task::future(async move {
            let _ = settings.save(&folder);
        })
        .discard()
    }

    pub(super) fn remember(&self) -> Task<Message> {
        let Some(folder) = paths::config_dir() else {
            return Task::none();
        };
        let recent = self.recent.clone();
        Task::future(async move {
            let _ = recent.save(&folder);
        })
        .discard()
    }
}

async fn save_session_as(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Save session")
        .set_file_name(name)
        .add_filter("Sessions", &[saved::EXTENSION])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}
