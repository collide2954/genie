use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::{Element, Task};

use crate::genome::{self, Genome};
use crate::location::Location;
use crate::menu;
use crate::reference::Reference;

use super::{Download, Genie, Message, Offline};

impl Genie {
    pub(super) fn current_genome(&self) -> Option<Genome> {
        let session = self.session.as_ref()?;
        genome::find(session.reference.genome.as_deref()?)
    }

    pub(super) fn offline(&mut self, offline: Offline) -> Task<Message> {
        match offline {
            Offline::Start => return self.start_download(),
            Offline::Progress(done, size) => {
                if let Some(download) = &mut self.download {
                    (download.done, download.size) = (done, size);
                }
            }
            Offline::Done(result) => {
                let label = self
                    .download
                    .take()
                    .and_then(|download| genome::find(&download.genome))
                    .map(|genome| genome.label)
                    .unwrap_or_default();
                return match result {
                    Ok(()) => self.inform(format!("{label} is now available offline.")),
                    Err(error) if error == genome::STOPPED => self.inform(error),
                    Err(error) => {
                        self.warn(format!("The download of {label} failed. {error}"));
                        Task::none()
                    }
                };
            }
            Offline::Stop => {
                self.close_menus();
                if let Some(download) = &self.download {
                    download.stop.store(true, Ordering::Relaxed);
                }
            }
            Offline::Remove => {
                self.close_menus();
                let (Some(genome), Some(cache)) = (self.current_genome(), self.cache.clone())
                else {
                    return Task::none();
                };
                let folder = genome.folder(&cache);
                if !self.reads(&folder) {
                    return self.remove_copy(&genome, &folder);
                }
                let id = genome.id.clone();
                return Task::perform(
                    async move { genome::online(&genome, &folder).map(Arc::new) },
                    move |result| Message::Offline(Offline::Online(id.clone(), result)),
                );
            }
            Offline::Online(id, result) => return self.online(&id, result),
        }
        Task::none()
    }

    fn reads(&self, folder: &Path) -> bool {
        let copy = Location::File(folder.join(genome::SEQUENCE));
        self.session
            .as_ref()
            .is_some_and(|session| session.reference.location == copy)
    }

    fn online(&mut self, id: &str, result: Result<Arc<Reference>, String>) -> Task<Message> {
        let (Some(genome), Some(cache)) = (genome::find(id), self.cache.clone()) else {
            return Task::none();
        };
        let reference = match result {
            Ok(reference) => reference,
            Err(error) => {
                self.warn(format!(
                    "genie cannot read {} from UCSC, so it keeps the offline copy. {error}",
                    genome.label
                ));
                return Task::none();
            }
        };
        let folder = genome.folder(&cache);
        let mut again = None;
        if self.reads(&folder)
            && let Some(session) = &mut self.session
        {
            if session.reference.assembly == reference.assembly {
                session.reference = reference;
            } else {
                again = self.saved();
            }
        }
        let removed = self.remove_copy(&genome, &folder);
        match again {
            Some(saved) => Task::batch([removed, self.restore(saved)]),
            None => removed,
        }
    }

    fn remove_copy(&mut self, genome: &Genome, folder: &Path) -> Task<Message> {
        match genome::remove(folder) {
            Ok(()) => self.inform(format!("Removed the offline copy of {}.", genome.label)),
            Err(error) => {
                self.warn(format!("The offline copy could not be removed. {error}"));
                Task::none()
            }
        }
    }

    fn start_download(&mut self) -> Task<Message> {
        self.close_menus();
        let (Some(genome), Some(cache)) = (self.current_genome(), self.cache.clone()) else {
            return Task::none();
        };
        if self.download.is_some() {
            return Task::none();
        }
        let folder = genome.folder(&cache);
        let stop = Arc::new(AtomicBool::new(false));
        self.download = Some(Download {
            genome: genome.id.clone(),
            done: 0,
            size: 0,
            stop: Arc::clone(&stop),
        });
        let events = iced::stream::channel(16, async move |mut output| {
            let (sender, mut receiver) = mpsc::unbounded();
            thread::spawn(move || {
                let report = sender.clone();
                let result = genome::download(&genome, &folder, &stop, |done, size| {
                    let _ = report.unbounded_send(Offline::Progress(done, size));
                });
                let _ = sender.unbounded_send(Offline::Done(result));
            });
            while let Some(event) = receiver.next().await {
                let _ = output.send(event).await;
            }
        });
        Task::run(events, Message::Offline)
    }

    pub(super) fn download_text(&self) -> Option<String> {
        let download = self.download.as_ref()?;
        let label = genome::find(&download.genome)?.label;
        Some(format!("Downloading {label}… {} %", download.percent()))
    }

    pub(super) fn offline_items(&self) -> Vec<Element<'_, Message>> {
        let (Some(genome), Some(cache)) = (self.current_genome(), self.cache.as_ref()) else {
            return Vec::new();
        };
        let folder = genome.folder(cache);
        let item = |label: String, offline: Offline, enabled: bool| {
            menu::item(label, Message::Offline(offline), false, enabled)
        };
        match &self.download {
            Some(download) if download.genome == genome.id => vec![
                item(
                    format!("Downloading… {} %", download.percent()),
                    Offline::Stop,
                    false,
                ),
                item("Stop download".to_owned(), Offline::Stop, true),
            ],
            Some(_) => vec![item(
                format!("Use offline ({} MB)", genome.megabytes),
                Offline::Start,
                false,
            )],
            None if genome::offline(&folder) => vec![item(
                "Remove offline copy".to_owned(),
                Offline::Remove,
                true,
            )],
            None => vec![item(
                format!("Use offline ({} MB)", genome.megabytes),
                Offline::Start,
                true,
            )],
        }
    }
}
