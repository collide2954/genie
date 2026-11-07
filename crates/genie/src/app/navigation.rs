use std::sync::Arc;

use genie_core::{Named, Span, Target, Viewport};
use iced::Task;
use iced::widget::operation;

use crate::chrome;
use crate::shortcuts::Command;
use crate::track::{self, Source};

use super::{Genie, Message, SUGGESTIONS, Suggestion};

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
                Task::none()
            }
            Command::Export => self.export(),
            Command::SaveSession => self.save_session(),
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

    pub(super) fn submit(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let text = self.draft.take().unwrap_or_else(|| session.locus());
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
            Ok(target) => self.go_to(target),
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

    fn go_to(&mut self, target: Target) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        if target.contig == session.contig {
            session.glide(session.destination().jump(target.span));
        } else {
            let length = session.reference.assembly.contigs()[target.contig].length;
            session.transition = None;
            session.contig = target.contig;
            session.viewport = session.viewport.retarget(length).jump(target.span);
        }
        self.locus_error = None;
        self.fetch()
    }

    pub(super) fn looked(&mut self, found: Option<Named>, error: String) -> Task<Message> {
        let Some(named) = found else {
            self.locus_error = Some(error);
            return Task::none();
        };
        self.draft = None;
        self.go_to(Target {
            contig: named.contig,
            span: Span::Region(named.span),
        })
    }

    pub(super) fn navigate(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        self.hover = None;
        if let Some(session) = &mut self.session {
            session.transition = None;
            session.viewport = change(session.viewport);
        }
        self.fetch()
    }

    fn glide(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        if !self.settings.motion {
            return self.navigate(change);
        }
        self.hover = None;
        if let Some(session) = &mut self.session {
            session.glide(change(session.destination()));
        }
        self.fetch()
    }
}
