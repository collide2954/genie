use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::{Element, Task};

use crate::genome::{self, Genome};
use crate::location::Location;
use crate::menu;

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
                return match genome::remove(&genome.folder(&cache)) {
                    Ok(()) => self.inform(format!("Removed the offline copy of {}.", genome.label)),
                    Err(error) => {
                        self.warn(format!("The offline copy could not be removed. {error}"));
                        Task::none()
                    }
                };
            }
        }
        Task::none()
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
            None if genome::offline(&folder) => {
                let local = Location::File(folder.join(genome::SEQUENCE));
                let used = self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.reference.location == local);
                if used {
                    vec![item(
                        "Offline copy in use".to_owned(),
                        Offline::Remove,
                        false,
                    )]
                } else {
                    vec![item(
                        "Remove offline copy".to_owned(),
                        Offline::Remove,
                        true,
                    )]
                }
            }
            None => vec![item(
                format!("Use offline ({} MB)", genome.megabytes),
                Offline::Start,
                true,
            )],
        }
    }
}
