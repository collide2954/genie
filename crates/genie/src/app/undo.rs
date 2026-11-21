//! Undo and redo of the work on tracks and region marks. A step keeps the settings that a person
//! can change on each track, with its file, and the region marks. Jumps of the view have their
//! own history: Back and Forward.

use std::sync::Arc;

use genie_core::Marks;
use genie_core::review::Reviews;
use genie_render::scene::Hue;
use iced::Task;

use crate::location::Location;
use crate::shortcuts::Command;
use genie_core::samples::Choice;
use genie_render::features::Mode;

use crate::track::{Declared, Filter, Lift, Options, Plot, Source, Track};

use super::{Genie, Message, Session};

/// The most steps that genie keeps.
const STEPS: usize = 100;

/// The settings of a track that a person can change, and its file.
#[derive(Debug, Clone)]
struct Kept {
    id: u64,
    source: Arc<Source>,
    name: String,
    over: Option<u64>,
    hidden: bool,
    folder: Option<String>,
    hue: Hue,
    mode: Option<Mode>,
    options: Options,
    plot: Plot,
    height: Option<f32>,
    filter: Filter,
    modification: Option<String>,
    declared: Option<Declared>,
    choice: Choice,
    lift: Option<Arc<Lift>>,
    lifting: Option<Location>,
    reviews: Arc<Reviews>,
    ld_group: Option<String>,
    lead: Option<u64>,
}

impl PartialEq for Kept {
    fn eq(&self, other: &Self) -> bool {
        let lifts = match (&self.lift, &other.lift) {
            (Some(one), Some(two)) => Arc::ptr_eq(one, two),
            (one, two) => one.is_none() && two.is_none(),
        };
        lifts
            && Arc::ptr_eq(&self.source, &other.source)
            && (Arc::ptr_eq(&self.reviews, &other.reviews) || self.reviews == other.reviews)
            && (&self.ld_group, self.lead) == (&other.ld_group, other.lead)
            && (
                self.id,
                &self.name,
                self.over,
                self.hidden,
                self.hue,
                self.mode,
            ) == (
                other.id,
                &other.name,
                other.over,
                other.hidden,
                other.hue,
                other.mode,
            )
            && (self.options, &self.plot, self.height, &self.filter)
                == (other.options, &other.plot, other.height, &other.filter)
            && (
                &self.modification,
                &self.declared,
                &self.choice,
                &self.lifting,
                &self.folder,
            ) == (
                &other.modification,
                &other.declared,
                &other.choice,
                &other.lifting,
                &other.folder,
            )
    }
}

impl Kept {
    fn of(track: &Track) -> Self {
        Self {
            id: track.id,
            source: Arc::clone(&track.source),
            name: track.name.clone(),
            over: track.over,
            hidden: track.hidden,
            folder: track.folder.clone(),
            hue: track.hue,
            mode: track.mode,
            options: track.options,
            plot: track.plot,
            height: track.height,
            filter: track.filter.clone(),
            modification: track.modification.clone(),
            declared: track.declared.clone(),
            choice: track.choice.clone(),
            lift: track.lift.clone(),
            lifting: track.lifting.clone(),
            reviews: Arc::clone(&track.reviews),
            ld_group: track.ld_group.clone(),
            lead: track.lead,
        }
    }

    /// Gives a track these settings, and makes it load its data again.
    fn apply(self, track: &mut Track, session: &Session) {
        if !Arc::ptr_eq(&track.source, &self.source) {
            track.source = Arc::clone(&self.source);
            track.drop_data();
        }
        let counter = track.options.lift;
        let lifted = match (&track.lift, &self.lift) {
            (Some(one), Some(two)) => !Arc::ptr_eq(one, two),
            (one, two) => one.is_some() != two.is_some(),
        };
        track.name = self.name;
        track.over = self.over;
        track.hidden = self.hidden;
        track.folder = self.folder;
        track.hue = self.hue;
        track.mode = self.mode;
        track.options = self.options;
        track.options.lift = counter;
        track.plot = self.plot;
        track.height = self.height;
        track.filter = self.filter;
        track.modification = self.modification;
        track.declared = self.declared;
        track.choice = self.choice;
        if lifted {
            track.set_lift(self.lift, &session.reference.assembly);
        }
        track.lifting = self.lifting;
        track.reviews = self.reviews;
        track.ld_group = self.ld_group;
        track.lead = self.lead;
        track.arrange(session.samples.as_ref().map(|samples| &*samples.table));
        track.forget_kept();
        track.forget();
        track.refilter();
        if track.hidden {
            track.drop_data();
        }
    }
}

/// The settings of all tracks and the region marks at one time.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Step {
    tracks: Vec<Kept>,
    marks: Marks,
}

/// The steps to undo, the last one last, and the steps to redo.
#[derive(Debug, Default)]
pub(super) struct Steps {
    done: Vec<Step>,
    undone: Vec<Step>,
    /// The gesture of the last step, which the next messages of the gesture join.
    gesture: Option<Gesture>,
}

/// Many messages that make one step: a track and what changes in it.
type Gesture = (u64, u8);

/// The gesture of a message: typing in a field of the track dialog, or a drag of the bottom
/// edge of a lane.
fn gesture(message: &Message) -> Option<Gesture> {
    match message {
        Message::TrackDraft(super::Drafting::Name(_)) => Some((0, 0)),
        Message::TrackDraft(super::Drafting::Height(_)) => Some((0, 1)),
        Message::Lane(id, super::Lane::Resized(_)) => Some((*id, 2)),
        Message::Review(super::Reviewing::Note(_)) => Some((0, 3)),
        _ => None,
    }
}

impl Session {
    pub(super) fn step(&self) -> Step {
        Step {
            tracks: self.tracks.iter().map(Kept::of).collect(),
            marks: self.marks.clone(),
        }
    }
}

/// True for the messages of a person that can change tracks or region marks. The results of
/// work that genie does in the background, such as a chain file that it read, are not steps.
fn records(message: &Message) -> bool {
    match message {
        Message::Command(command) => !matches!(command, Command::Undo | Command::Redo),
        Message::TrackOpened(result) | Message::CatalogOpened(_, result) => result.is_ok(),
        Message::Lift(lifting) => !matches!(lifting, super::Lifting::Read(..)),
        Message::TrackChanged(..)
        | Message::Range(..)
        | Message::Filter(..)
        | Message::Field(..)
        | Message::Rename(..)
        | Message::TrackDraft(..)
        | Message::Samples(..)
        | Message::Regions(..)
        | Message::Hub(..)
        | Message::Lane(..)
        | Message::Palette(..)
        | Message::Port(..)
        | Message::Find(..)
        | Message::Table(..)
        | Message::TrackList(..)
        | Message::Folder(..)
        | Message::Made(..)
        | Message::Motifs(..)
        | Message::Review(..)
        | Message::Lead => true,
        _ => false,
    }
}

impl Genie {
    /// The step before a message that can change tracks or region marks, and the gesture of
    /// the message. Tracks that a session opens again are not steps.
    pub(super) fn undo_point(&self, message: &Message) -> Option<(Step, Option<Gesture>)> {
        if self.restoring.is_some() || !records(message) {
            return None;
        }
        let step = self.session.as_ref().map(Session::step)?;
        Some((step, gesture(message)))
    }

    /// Keeps the step before a message when the message changed something, unless the message
    /// goes on with the gesture of the last step. New work drops the steps to redo.
    pub(super) fn keep_step(&mut self, before: Option<(Step, Option<Gesture>)>) {
        let (Some((before, gesture)), Some(session)) = (before, &mut self.session) else {
            return;
        };
        let same = session.step() == before;
        let steps = &mut session.steps;
        if same {
            if gesture.is_none() {
                steps.gesture = None;
            }
            return;
        }
        if gesture.is_some() && steps.gesture == gesture {
            return;
        }
        steps.gesture = gesture;
        steps.done.push(before);
        if steps.done.len() > STEPS {
            steps.done.remove(0);
        }
        steps.undone.clear();
    }

    /// Goes back one step, or forward one step again.
    pub(super) fn undo(&mut self, again: bool) -> Task<Message> {
        let popped = self.session.as_mut().map(|session| {
            let now = session.step();
            let steps = &mut session.steps;
            let (from, to) = if again {
                (&mut steps.undone, &mut steps.done)
            } else {
                (&mut steps.done, &mut steps.undone)
            };
            let step = from.pop()?;
            to.push(now);
            Some(step)
        });
        match popped {
            Some(Some(step)) => self.apply_step(step),
            Some(None) if again => self.inform("Nothing to redo.".to_owned()),
            Some(None) => self.inform("Nothing to undo.".to_owned()),
            None => Task::none(),
        }
    }

    fn apply_step(&mut self, step: Step) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let mut old = std::mem::take(&mut session.tracks);
        let mut added = Vec::new();
        let mut tracks = Vec::with_capacity(step.tracks.len());
        for kept in step.tracks {
            let mut track = if let Some(at) = old.iter().position(|track| track.id == kept.id) {
                old.swap_remove(at)
            } else {
                added.push(kept.id);
                Track::new(
                    kept.id,
                    Arc::clone(&kept.source),
                    &session.reference.assembly,
                )
            };
            if Kept::of(&track) != kept {
                kept.apply(&mut track, session);
            }
            tracks.push(track);
        }
        session.tracks = tracks;
        session.marks = step.marks;
        (self.hover, self.pinned) = (None, None);
        if let Some(sidebar) = &mut self.sidebar {
            (sidebar.chosen, sidebar.editing) = (None, None);
        }
        let names = added.into_iter().map(|id| self.find_names(id));
        let names: Vec<Task<Message>> = names.collect();
        Task::batch([self.fetch_features()].into_iter().chain(names))
    }
}
