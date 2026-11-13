use std::path::PathBuf;

use iced::Task;

use crate::location::Location;
use crate::prepare;
use crate::settings::{self, Store};

use super::{Caching, Genie, Message, Popup};

impl Genie {
    pub(super) fn toggle_settings(&mut self) -> Task<Message> {
        self.toggle(Popup::Settings);
        if self.popup == Some(Popup::Settings) {
            self.measure()
        } else {
            Task::none()
        }
    }

    pub(super) fn caching(&mut self, event: Caching) -> Task<Message> {
        match event {
            Caching::Measured(size) => {
                self.cache_size = Some(size);
                Task::none()
            }
            Caching::Clear => {
                let Some(cache) = self.cache.clone().filter(|_| !self.is_preparing()) else {
                    return Task::none();
                };
                let keep: Vec<PathBuf> = self
                    .session
                    .iter()
                    .flat_map(|session| &session.tracks)
                    .filter_map(|track| match track.source.data() {
                        Location::File(path) => Some(path.clone()),
                        Location::Url(_) => None,
                    })
                    .collect();
                self.cache_size = None;
                Task::perform(async move { prepare::clear(&cache, &keep) }, |freed| {
                    Message::Cache(Caching::Cleared(freed))
                })
            }
            Caching::Cleared(freed) => {
                let size = settings::size_text(freed);
                let notice = self.inform(format!("Removed {size} of indexes and copies."));
                Task::batch([notice, self.measure()])
            }
        }
    }

    pub(super) fn store(&self) -> Store {
        Store {
            size: self.cache_size,
            can_clear: self.cache.is_some()
                && !self.is_preparing()
                && self.cache_size.is_some_and(|size| size > 0),
        }
    }

    fn measure(&self) -> Task<Message> {
        let Some(cache) = self.cache.clone() else {
            return Task::none();
        };
        Task::perform(async move { prepare::usage(&cache) }, |size| {
            Message::Cache(Caching::Measured(size))
        })
    }

    fn is_preparing(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.preparing.is_empty())
    }
}
