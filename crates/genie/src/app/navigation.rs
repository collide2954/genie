use std::sync::Arc;

use genie_core::{Named, Region, Span, Target, Viewport};
use iced::Task;
use iced::widget::operation;

use crate::chrome;
use crate::shortcuts::Command;
use crate::track::{self, Source};

use super::{Genie, MAX_PANELS, Message, Paneling, Regions, Session, Suggestion, Sweep};

const SUGGESTIONS: usize = 8;
const MIN_SWEEP: f64 = 3.0;

fn resolve(session: &Session, text: &str) -> Option<Target> {
    session.reference.assembly.resolve(text).ok().or_else(|| {
        session
            .tracks
            .iter()
            .filter_map(|track| track.names.as_deref())
            .find_map(|names| names.find(text))
            .map(|named| Target {
                contig: named.contig,
                span: Span::Region(named.span),
            })
    })
}

impl Genie {
    pub(super) fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Pan(share) => {
                self.drop_draft();
                self.glide(|viewport| viewport.pan(f64::from(share) * viewport.width()))
            }
            Command::Zoom(factor) => {
                self.drop_draft();
                self.glide(|viewport| viewport.zoom(f64::from(factor), viewport.width() / 2.0))
            }
            Command::Back => self.step_history(false),
            Command::Forward => self.step_history(true),
            Command::AddRegion => self.regions(Regions::Add),
            Command::OpenUrl => self.ask_address(),
            Command::NextFeature | Command::PreviousFeature => {
                let forward = command == Command::NextFeature;
                self.pointed
                    .map_or_else(Task::none, |id| self.jump(id, forward))
            }
            Command::FocusLocus => Task::batch([
                operation::focus(chrome::LOCUS),
                operation::select_all(chrome::LOCUS),
            ]),
            Command::NextSuggestion => {
                self.step_highlight(true);
                Task::none()
            }
            Command::PreviousSuggestion => {
                self.step_highlight(false);
                Task::none()
            }
            Command::Dismiss => {
                self.close_menus();
                self.notice = None;
                self.pinned = None;
                self.sweep = None;
                Task::none()
            }
            Command::Export => self.export(),
            Command::SaveSession => self.save_session(),
            Command::Copy => self.copy(),
            Command::Diagnostics => {
                if cfg!(debug_assertions) {
                    self.diagnostics = !self.diagnostics;
                }
                Task::none()
            }
        }
    }

    pub(super) fn suggestions(&self) -> Vec<Suggestion> {
        let (Some(session), Some(draft)) = (&self.session, &self.draft) else {
            return Vec::new();
        };
        if draft.contains(':') {
            return Vec::new();
        }
        let mut list: Vec<Suggestion> = session
            .reference
            .assembly
            .suggest(draft, SUGGESTIONS)
            .into_iter()
            .map(Suggestion::Contig)
            .collect();
        let named = session
            .tracks
            .iter()
            .filter_map(|track| track.names.as_deref())
            .flat_map(|names| names.search(draft, SUGGESTIONS));
        for named in named {
            if list.len() == SUGGESTIONS {
                break;
            }
            let suggestion = Suggestion::Feature(named.clone());
            if !list.contains(&suggestion) {
                list.push(suggestion);
            }
        }
        list
    }

    fn step_highlight(&mut self, forward: bool) {
        let count = self.suggestions().len();
        if count == 0 {
            return;
        }
        self.highlight = Some(match (self.highlight, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(position), true) => (position + 1) % count,
            (Some(position), false) => (position + count - 1) % count,
        });
    }

    pub(super) fn choose(&mut self, position: usize) -> Task<Message> {
        let chosen = self.suggestions().into_iter().nth(position);
        let Some(session) = &self.session else {
            return Task::none();
        };
        if let Some(suggestion) = chosen {
            self.draft = Some(match suggestion {
                Suggestion::Contig(contig) => {
                    session.reference.assembly.contigs()[contig].name.clone()
                }
                Suggestion::Feature(named) => session.locus_of(&named),
            });
        }
        self.highlight = None;
        self.submit()
    }

    pub(super) fn drop_draft(&mut self) {
        self.draft = None;
        self.locus_error = None;
        self.highlight = None;
    }

    pub(super) fn edit_locus(&mut self, text: String) {
        self.draft = Some(text);
        self.locus_error = None;
        self.highlight = None;
    }

    pub(super) fn show_locus(&mut self, locus: String) -> Task<Message> {
        self.draft = Some(locus);
        self.submit()
    }

    pub(super) fn submit_locus(&mut self) -> Task<Message> {
        match self.highlight {
            Some(position) => self.choose(position),
            None => self.submit(),
        }
    }

    pub(super) fn panel(&mut self, panel: usize, paneling: Paneling) -> Task<Message> {
        let width = self.width;
        (self.hover, self.pinned) = (None, None);
        self.on_session(|session| match paneling {
            Paneling::Only => session.only(panel, width),
            Paneling::Close => session.close(panel, width),
        });
        self.fetch()
    }

    pub(super) fn resize(&mut self, width: f32) -> Task<Message> {
        self.width = width;
        self.hover = None;
        self.on_session(|session| session.resize(width));
        self.fetch()
    }

    fn submit_many(&mut self, text: String) -> Task<Message> {
        let width = self.width;
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let parts: Vec<&str> = text.split_whitespace().collect();
        let error = if parts.len() > MAX_PANELS {
            format!("genie shows up to {MAX_PANELS} loci.")
        } else {
            let targets: Result<Vec<Target>, String> = parts
                .iter()
                .map(|part| {
                    resolve(session, part).ok_or_else(|| format!("genie cannot find {part}."))
                })
                .collect();
            match targets {
                Ok(targets) => {
                    session.split(&targets, width);
                    (self.hover, self.pinned) = (None, None);
                    self.locus_error = None;
                    return self.fetch();
                }
                Err(error) => error,
            }
        };
        self.draft = Some(text);
        self.locus_error = Some(error);
        Task::none()
    }

    pub(super) fn submit(&mut self) -> Task<Message> {
        let width = self.width;
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let text = self.draft.take().unwrap_or_else(|| session.locus());
        if text.split_whitespace().nth(1).is_some() {
            return self.submit_many(text);
        }
        let resolved = session.reference.assembly.resolve(&text).or_else(|error| {
            session
                .tracks
                .iter()
                .filter_map(|track| track.names.as_deref())
                .find_map(|names| names.find(&text))
                .map(|named| Target {
                    contig: named.contig,
                    span: Span::Region(named.span),
                })
                .ok_or(error)
        });
        match resolved {
            Ok(target) => {
                session.join(width);
                self.go_to(target)
            }
            Err(error) => {
                let sources: Vec<Arc<Source>> = session
                    .tracks
                    .iter()
                    .filter(|track| track.source.has_lookup())
                    .map(|track| Arc::clone(&track.source))
                    .collect();
                self.draft = Some(text.clone());
                if sources.is_empty() {
                    self.locus_error = Some(error.to_string());
                    return Task::none();
                }
                let (reference, message) = (Arc::clone(&session.reference), error.to_string());
                Task::perform(
                    async move { track::lookup(&sources, &text, &reference.assembly) },
                    move |found| Message::Looked(found, message.clone()),
                )
            }
        }
    }

    pub(super) fn go_to(&mut self, target: Target) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let before = session.visit();
        session.show(target, self.settings.motion);
        if session.visit() != before {
            session.history.record(before);
        }
        self.locus_error = None;
        self.fetch()
    }

    fn step_history(&mut self, forward: bool) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let current = session.visit();
        let visit = if forward {
            session.history.forward(current)
        } else {
            session.history.back(current)
        };
        let Some(visit) = visit else {
            return Task::none();
        };
        session.show(
            Target {
                contig: visit.contig,
                span: Span::Region(visit.region),
            },
            self.settings.motion,
        );
        self.drop_draft();
        self.hover = None;
        self.fetch()
    }

    pub(super) fn sweep_overview(&mut self, sweep: Sweep) -> Task<Message> {
        match sweep {
            Sweep::Pressed(share) => {
                self.drop_draft();
                self.on_session(|session| session.grab(share));
            }
            Sweep::Dragged(share) => self.on_session(|session| session.slide(share)),
            Sweep::Released => {
                self.on_session(|session| session.grip = None);
                return Task::none();
            }
        }
        self.fetch()
    }

    pub(super) fn sweep_ruler(&mut self, panel: usize, sweep: Sweep) -> Task<Message> {
        let Some(session) = self
            .session
            .as_ref()
            .filter(|session| panel < session.count())
        else {
            return Task::none();
        };
        let (contig, viewport) = session.view(panel);
        let at = |share: f32| viewport.to_bp(f64::from(share) * viewport.width());
        match sweep {
            Sweep::Pressed(share) => {
                self.drop_draft();
                self.sweep = Some((panel, at(share), at(share)));
            }
            Sweep::Dragged(share) => {
                if let Some((swept, start, _)) = self.sweep
                    && swept == panel
                {
                    self.sweep = Some((panel, start, at(share)));
                }
            }
            Sweep::Released => {
                let Some((swept, start, end)) = self.sweep.take() else {
                    return Task::none();
                };
                if swept != panel || (viewport.to_px(end) - viewport.to_px(start)).abs() < MIN_SWEEP
                {
                    return Task::none();
                }
                let length = viewport.length() as f64;
                let low = start.min(end).clamp(0.0, length).round() as u64;
                let high = start.max(end).clamp(0.0, length).round() as u64;
                let region = Region::between(low, high.max(low + 1));
                if self.modifiers.shift() {
                    return self.mark_region(contig, region);
                }
                let span = Span::Region(region);
                if panel > 0 {
                    let motion = self.settings.motion;
                    self.on_session(|session| {
                        session.steer(panel, |viewport| viewport.jump(span), motion);
                    });
                    return self.fetch();
                }
                return self.go_to(Target { contig, span });
            }
        }
        Task::none()
    }

    pub(super) fn jump_from(&self) -> Option<u64> {
        let session = self.session.as_ref()?;
        Some(match self.anchor {
            Some((visit, start)) if visit == session.visit() => start,
            _ => session.destination().center().max(0.0) as u64,
        })
    }

    pub(super) fn jump(&mut self, id: u64, forward: bool) -> Task<Message> {
        self.close_menus();
        let Some(position) = self.jump_from() else {
            return Task::none();
        };
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == id) else {
            return Task::none();
        };
        if !track.source.can_jump() {
            return Task::none();
        }
        let source = Arc::clone(&track.source);
        let names = track.contigs.clone();
        let (reference, contig) = (Arc::clone(&session.reference), session.contig);
        Task::perform(
            async move {
                track::neighbor(
                    &source,
                    &names,
                    &reference.assembly,
                    contig,
                    position,
                    forward,
                )
            },
            move |result| Message::Jumped(id, forward, result),
        )
    }

    pub(super) fn jumped(
        &mut self,
        id: u64,
        forward: bool,
        result: Result<Option<(usize, Region)>, String>,
    ) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        match result {
            Ok(Some((contig, span))) => {
                let span_bp = span.len() as f64;
                let target = if span_bp <= session.destination().span() {
                    Span::Position(span.start() + span.len() / 2)
                } else {
                    Span::Region(span)
                };
                let task = self.go_to(Target {
                    contig,
                    span: target,
                });
                self.anchor = self
                    .session
                    .as_ref()
                    .map(|session| (session.visit(), span.start()));
                task
            }
            Ok(None) => {
                let Some(track) = session.tracks.iter().find(|track| track.id == id) else {
                    return Task::none();
                };
                let noun = if track.source.has_variants() {
                    "variant"
                } else {
                    "feature"
                };
                let side = if forward { "next" } else { "previous" };
                let text = format!("There is no {side} {noun} in {}.", track.name);
                self.inform(text)
            }
            Err(error) => {
                self.warn(error);
                Task::none()
            }
        }
    }

    pub(super) fn looked(&mut self, found: Option<Named>, error: String) -> Task<Message> {
        let order = self.looking.take();
        let Some(named) = found else {
            match order {
                Some(reply) => {
                    self.draft = None;
                    reply.send(error);
                }
                None => self.locus_error = Some(error),
            }
            return Task::none();
        };
        if let Some(reply) = order {
            self.answers.push((super::Answer::Done, reply));
        }
        self.draft = None;
        let width = self.width;
        self.on_session(|session| session.join(width));
        self.go_to(Target {
            contig: named.contig,
            span: Span::Region(named.span),
        })
    }

    pub(super) fn navigate(
        &mut self,
        panel: usize,
        change: impl FnOnce(Viewport) -> Viewport,
    ) -> Task<Message> {
        self.hover = None;
        self.on_session(|session| session.shift(panel, change));
        self.fetch()
    }

    pub(super) fn pan(&mut self, panel: usize, dx: f32) -> Task<Message> {
        self.drop_draft();
        self.navigate(panel, |viewport| viewport.pan(f64::from(dx)))
    }

    pub(super) fn zoom(&mut self, panel: usize, factor: f32, anchor: f32) -> Task<Message> {
        self.drop_draft();
        self.navigate(panel, |viewport| {
            viewport.zoom(f64::from(factor), f64::from(anchor))
        })
    }

    fn glide(&mut self, change: impl Fn(Viewport) -> Viewport) -> Task<Message> {
        self.hover = None;
        let motion = self.settings.motion;
        self.on_session(|session| {
            for panel in 0..session.count() {
                session.steer(panel, &change, motion);
            }
        });
        self.fetch()
    }
}
