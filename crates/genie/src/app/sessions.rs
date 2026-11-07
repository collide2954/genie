use std::path::PathBuf;

use iced::Task;

use crate::paths::{self, file_name};
use crate::saved::{self, Saved, SavedTrack};
use crate::settings::Setting;

use super::{Genie, LAST, Message};

impl Genie {
    pub(super) fn restore(&mut self, saved: Saved) -> Task<Message> {
        self.pending = saved
            .tracks
            .iter()
            .map(|track| track.path.clone())
            .collect();
        let reference = saved.reference.clone();
        self.restoring = Some(saved);
        self.open(reference)
    }

    pub(super) fn resume(&mut self) {
        let (Some(saved), Some(session)) = (&self.restoring, &mut self.session) else {
            return;
        };
        if saved.reference != session.reference.path {
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
    }

    pub(super) fn saved(&self) -> Option<Saved> {
        let session = self.session.as_ref()?;
        Some(Saved {
            reference: session.reference.path.clone(),
            locus: Some(session.locus()),
            reading: session.reading,
            tracks: session
                .tracks
                .iter()
                .map(|track| SavedTrack {
                    path: track.source.path.clone(),
                    mode: track.mode,
                    hue: track.hue,
                    height: track.height,
                    options: track.options,
                    plot: track.plot,
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
                self.recent.add(path.clone());
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
