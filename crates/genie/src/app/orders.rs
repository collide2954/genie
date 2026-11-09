use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use genie_core::{Marks, Region};
use genie_render::features::Mode;
use iced::futures::channel::mpsc::{self, UnboundedSender};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{Subscription, Task};

use crate::export;
use crate::genome;
use crate::location::Location;
use crate::port::{self, Order, Reply};
use crate::track::{self, Source};

use super::{Genie, Message, Port, Status};

const ANSWER_TIME: Duration = Duration::from_secs(600);
const NO_REFERENCE: &str = "Open a genome or a reference first.";
const NO_FOLDER: &str = "Give a folder with snapshotDirectory first.";

#[derive(Debug)]
pub(super) enum Answer {
    Done,
    Snapshot(Option<String>),
    Genome(String),
}

impl Genie {
    pub(super) fn port(&mut self, event: Port) -> Task<Message> {
        match event {
            Port::Order(order, reply) => self.order(order, reply),
            Port::Loaded(result, name, reply) => self.loaded(result, name, reply),
            Port::Failed(error) => {
                self.warn(format!(
                    "genie cannot take commands on port {}. {error}",
                    port::PORT
                ));
                Task::none()
            }
        }
    }

    fn order(&mut self, order: Order, reply: Reply) -> Task<Message> {
        let free = matches!(
            order,
            Order::Echo | Order::Genome(_) | Order::Folder(_) | Order::Ignored
        );
        if !free && self.session.is_none() {
            reply.send(NO_REFERENCE);
            return Task::none();
        }
        match order {
            Order::Echo => reply.send("echo"),
            Order::Ignored => reply.send("OK"),
            Order::Folder(folder) => {
                self.folder = Some(folder);
                reply.send("OK");
            }
            Order::New => {
                self.clear();
                reply.send("OK");
            }
            Order::Rows(mode, name) => reply.send(self.rows(mode, name.as_deref())),
            Order::Region { contig, span, name } => reply.send(self.region(&contig, span, name)),
            Order::Snapshot(name) => self.answers.push((Answer::Snapshot(name), reply)),
            Order::Goto(locus) => return self.goto(locus, reply),
            Order::Load { file, name } => return self.load(&file, name, reply),
            Order::Genome(text) => return self.genome(text, reply),
        }
        Task::none()
    }

    fn goto(&mut self, locus: String, reply: Reply) -> Task<Message> {
        self.draft = Some(locus);
        self.locus_error = None;
        let task = self.submit();
        if let Some(error) = self.locus_error.take() {
            self.draft = None;
            reply.send(error);
        } else if self.draft.is_some() {
            self.looking = Some(reply);
        } else {
            self.answers.push((Answer::Done, reply));
        }
        task
    }

    fn load(&mut self, file: &str, name: Option<String>, reply: Reply) -> Task<Message> {
        let location = match Location::parse(file) {
            Ok(location) => location,
            Err(error) => {
                reply.send(error);
                return Task::none();
            }
        };
        if !track::is_track(&location) {
            reply.send(format!(
                "{} is not a track. To open a reference, use genome.",
                location.name()
            ));
            return Task::none();
        }
        self.busy += 1;
        Task::perform(
            async move { track::open(location).map(Arc::new) },
            move |result| Message::Port(Port::Loaded(result, name.clone(), reply.clone())),
        )
    }

    fn loaded(
        &mut self,
        result: Result<Arc<Source>, String>,
        name: Option<String>,
        reply: Reply,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let source = match result {
            Ok(source) => source,
            Err(error) => {
                reply.send(error);
                return Task::none();
            }
        };
        let task = self.add_track(Arc::clone(&source));
        if let (Some(name), Some(session)) = (name, &mut self.session)
            && let Some(track) = session
                .tracks
                .iter_mut()
                .find(|track| track.source == source)
        {
            track.name = name;
        }
        self.answers.push((Answer::Done, reply));
        task
    }

    fn genome(&mut self, text: String, reply: Reply) -> Task<Message> {
        let task = if let Some(genome) = genome::find(&text) {
            self.open_genome(&genome.id)
        } else {
            match Location::parse(&text) {
                Ok(location) if track::is_track(&location) => {
                    reply.send(format!(
                        "{} is a track. To add a track, use load.",
                        location.name()
                    ));
                    return Task::none();
                }
                Ok(location) => self.open(location),
                Err(error) => {
                    reply.send(error);
                    return Task::none();
                }
            }
        };
        self.answers.push((Answer::Genome(text), reply));
        task
    }

    fn clear(&mut self) {
        if let Some(session) = &mut self.session {
            session.tracks.clear();
            session.marks = Marks::default();
        }
        (self.hover, self.pinned) = (None, None);
        self.close_menus();
    }

    fn rows(&mut self, mode: Mode, name: Option<&str>) -> String {
        let Some(session) = &mut self.session else {
            return NO_REFERENCE.to_owned();
        };
        let mut found = false;
        for track in &mut session.tracks {
            let named = name.is_none_or(|name| {
                track.name.eq_ignore_ascii_case(name)
                    || track.source.name.eq_ignore_ascii_case(name)
            });
            if named && !track.source.has_signal() {
                track.mode = Some(mode);
                found = true;
            }
        }
        match name {
            Some(name) if !found => format!("There is no track {name}."),
            _ => "OK".to_owned(),
        }
    }

    fn region(&mut self, contig: &str, span: Region, name: Option<String>) -> String {
        let Some(session) = &mut self.session else {
            return NO_REFERENCE.to_owned();
        };
        let assembly = &session.reference.assembly;
        let Some(index) = assembly.find(contig) else {
            return format!("The reference has no contig {contig}.");
        };
        let span = span.clamp_to(assembly.contigs()[index].length);
        if span.is_empty() {
            return format!("The region is after the end of {contig}.");
        }
        session.marks.add(index, span, name);
        "OK".to_owned()
    }

    pub(super) fn answer(&mut self) {
        if self.answers.is_empty() || !self.settled() {
            return;
        }
        for (answer, reply) in std::mem::take(&mut self.answers) {
            reply.send(match answer {
                Answer::Done => "OK".to_owned(),
                Answer::Snapshot(name) => self.snapshot(name.as_deref()),
                Answer::Genome(text) => self.opened_genome(&text),
            });
        }
    }

    fn settled(&self) -> bool {
        self.busy == 0
            && self.looking.is_none()
            && self.tiles.waiting().is_empty()
            && self.session.as_ref().is_none_or(|session| {
                session.transition.is_none()
                    && session.tracks.iter().all(|track| {
                        track.slot.wanted.is_none()
                            && (track.names.is_some() || track.source.has_reads())
                    })
            })
    }

    fn snapshot(&self, name: Option<&str>) -> String {
        let Some(folder) = &self.folder else {
            return NO_FOLDER.to_owned();
        };
        let Some(session) = &self.session else {
            return NO_REFERENCE.to_owned();
        };
        let contig = &session.reference.assembly.contigs()[session.contig].name;
        let file = name
            .and_then(|name| Path::new(name).file_name())
            .map_or_else(
                || PathBuf::from(export::file_name(contig, session.viewport.visible())),
                PathBuf::from,
            );
        let svg = super::export::is_svg(&file);
        match self.save(folder.join(file), svg) {
            Ok(()) => "OK".to_owned(),
            Err(error) => error,
        }
    }

    fn opened_genome(&self, text: &str) -> String {
        let open = self.session.as_ref().is_some_and(|session| {
            session.reference.genome.as_deref() == Some(text)
                || Location::parse(text)
                    .is_ok_and(|location| session.reference.location == location)
        });
        if open {
            return "OK".to_owned();
        }
        let reason = match &self.status {
            Status::Failed(error) => Some(error.clone()),
            _ => self.notice.as_ref().map(|notice| notice.text.clone()),
        };
        match reason {
            Some(reason) => format!("genie cannot open {text}. {reason}"),
            None => format!("genie cannot open {text}."),
        }
    }

    pub(super) fn port_events(&self) -> Subscription<Message> {
        if self.settings.port {
            Subscription::run(events)
        } else {
            Subscription::none()
        }
    }
}

fn events() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async move |mut output| {
        let (sender, mut receiver) = mpsc::unbounded();
        thread::spawn(move || serve(&sender));
        while let Some(message) = receiver.next().await {
            if output.send(message).await.is_err() {
                break;
            }
        }
    })
}

fn serve(sender: &UnboundedSender<Message>) {
    let listener = match port::listen(port::PORT) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = sender.unbounded_send(Message::Port(Port::Failed(error.to_string())));
            return;
        }
    };
    let asker = sender.clone();
    let ask = Arc::new(move |line: String| {
        let order = match port::parse(&line) {
            Ok(Order::Echo) => return Some("echo".to_owned()),
            Ok(order) => order,
            Err(error) => return Some(error),
        };
        let (reply, answer) = Reply::new();
        asker
            .unbounded_send(Message::Port(Port::Order(order, reply)))
            .ok()?;
        answer.recv_timeout(ANSWER_TIME).ok()
    });
    port::serve(&listener, &ask, || !sender.is_closed());
}
