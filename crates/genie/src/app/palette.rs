use iced::widget::operation;
use iced::{Element, Task};

use crate::chrome::{self, keys};
use crate::genome;
use crate::menu::{GROUPS, JUNCTION_FLOORS, MAPQ_FLOORS, MODES, STYLES, TOGGLES};
use crate::palette::{self, Action, State};
use crate::recent::Entry;
use crate::shortcuts::Command;
use crate::track::{Change, Toggle, Track};

use super::hubs::Hubbing;
use super::{
    Arranging, Assembling, Blatting, Genie, Lifting, Message, Paletting, Popup, Ranging, Regions,
    Renaming, Sampling, Session, Tabling, Updating, arranging_words,
};

/// The actions that need no open genome: open files, hubs and assemblies, keys, settings and
/// updates.
fn general_actions() -> Vec<Action> {
    vec![
        Action::new("Open file…", keys('O'), Message::Command(Command::Open)),
        Action::new("Open URL…", keys('U'), Message::Command(Command::OpenUrl)),
        Action::new("Open a track hub…", "", Message::Hub(Hubbing::Ask)),
        Action::new(
            "Open an NCBI assembly…",
            "",
            Message::Assemblies(Assembling::Open),
        ),
        Action::new("Keys and mouse", "F1", Message::Command(Command::Help)),
        Action::new("Settings", "", Message::ToggleSettings),
        Action::new("Change the theme", "", Message::CycleAppearance),
        Action::new("Check for updates", "", Message::Update(Updating::Check)),
    ]
}

fn read_actions(track: &Track) -> Vec<Action> {
    let (id, name) = (track.id, track.name.as_str());
    let change = move |change| Message::TrackChanged(id, change);
    let mut actions: Vec<Action> = TOGGLES
        .map(|(label, toggle)| Action::new(label, name, change(Change::Toggle(toggle))))
        .into();
    actions.extend(MAPQ_FLOORS.map(|(label, floor)| {
        let words = if floor == 0 {
            "Show reads of any MAPQ".to_owned()
        } else {
            format!("Hide reads under MAPQ {label}")
        };
        Action::new(words, name, change(Change::Mapq(floor)))
    }));
    actions.push(Action::new(
        "Color unusual pairs",
        name,
        change(Change::Pairs),
    ));
    actions.push(Action::new(
        "View as pairs",
        name,
        change(Change::Toggle(Toggle::Mates)),
    ));
    actions.push(Action::new(
        "Color by modification",
        name,
        change(Change::Mods),
    ));
    actions.extend(
        GROUPS.map(|(label, grouping)| Action::new(label, name, change(Change::Group(grouping)))),
    );
    actions.push(Action::new("Sashimi plot", name, change(Change::Sashimi)));
    if track.plot.sashimi {
        actions.extend(JUNCTION_FLOORS.map(|(label, floor)| {
            let words = format!("Show the arcs with {label} or more reads");
            Action::new(words, name, change(Change::JunctionReads(floor)))
        }));
        actions.push(Action::new("Shared scale", name, change(Change::Link)));
    }
    actions
}

/// The actions that group, sort and color the samples of a track by the fields of a table.
fn sample_actions(track: &Track, fields: &[String]) -> Vec<Action> {
    if track.source.samples().is_empty() {
        return Vec::new();
    }
    let (id, name) = (track.id, track.name.as_str());
    fields
        .iter()
        .flat_map(|field| {
            [
                Arranging::Group(field.clone()),
                Arranging::Sort(field.clone()),
                Arranging::Color(field.clone()),
            ]
        })
        .map(|arranging| {
            let words = arranging_words(&arranging);
            Action::new(
                words,
                name,
                Message::Samples(Sampling::Arrange(id, arranging)),
            )
        })
        .collect()
}

/// The actions of the tracks of a session and of its sample table.
fn tracks_actions(session: &Session) -> Vec<Action> {
    let mut actions = Vec::new();
    let fields = session
        .samples
        .as_ref()
        .map_or(&[][..], |samples| samples.table.columns.as_slice());
    if let Some(samples) = &session.samples {
        actions.push(Action::new(
            "Close the sample table",
            samples.location.name(),
            Message::Samples(Sampling::Close),
        ));
    }
    for track in &session.tracks {
        actions.extend(track_actions(track, fields));
    }
    actions
}

fn track_actions(track: &Track, fields: &[String]) -> Vec<Action> {
    let (id, name) = (track.id, track.name.as_str());
    let change = move |change| Message::TrackChanged(id, change);
    let mut actions = vec![Action::new(
        "Track settings…",
        name,
        Message::TrackSettings(id),
    )];
    actions.extend(sample_actions(track, fields));
    let source = &track.source;
    if source.has_variants() {
        actions.push(Action::new(
            "Show the variant table",
            name,
            Message::Table(Tabling::Open(id)),
        ));
    }
    if !source.has_signal() && !source.has_links() && !source.has_junctions() {
        actions.extend(
            MODES.map(|(label, mode)| Action::new(label, name, change(Change::Mode(mode)))),
        );
    }
    if source.has_reads() {
        actions.extend(read_actions(track));
    }
    if source.has_signal() {
        actions.extend(
            STYLES.map(|(label, style)| Action::new(label, name, change(Change::Style(style)))),
        );
        actions.push(Action::new("Shared scale", name, change(Change::Link)));
        actions.push(Action::new(
            "Auto range",
            name,
            Message::Range(id, Ranging::Auto),
        ));
    }
    if source.has_segments() {
        actions.push(Action::new(
            "Default range (±1.5)",
            name,
            Message::Range(id, Ranging::Auto),
        ));
    }
    if source.is_bedmethyl() {
        actions.push(Action::new(
            "Join CpG strands",
            name,
            change(Change::Toggle(Toggle::Join)),
        ));
    }
    if source.has_reads() || source.is_bedmethyl() {
        for (index, modification) in track.modification_names().into_iter().enumerate() {
            actions.push(Action::new(
                format!("Show {modification}"),
                name,
                change(Change::Modification(index)),
            ));
        }
    }
    if source.has_colors() {
        actions.push(Action::new("Use file colors", name, change(Change::Colors)));
    }
    if !source.has_reads() && !source.has_contacts() {
        let (words, lifting) = if track.lift.is_some() {
            ("Remove lift", Lifting::Stop(id))
        } else {
            ("Lift over…", Lifting::Pick(Some(id)))
        };
        actions.push(Action::new(words, name, Message::Lift(lifting)));
    }
    if source.can_jump() && track.lift.is_none() {
        let noun = if source.has_variants() {
            "variant"
        } else {
            "feature"
        };
        actions.push(Action::new(
            format!("Next {noun}"),
            name,
            Message::Jump(id, true),
        ));
        actions.push(Action::new(
            format!("Previous {noun}"),
            name,
            Message::Jump(id, false),
        ));
    }
    actions.extend([
        Action::new("Rename", name, Message::Rename(Renaming::Start(id))),
        Action::new("Move up", name, change(Change::Up)),
        Action::new("Move down", name, change(Change::Down)),
        Action::new("Remove", name, change(Change::Remove)),
    ]);
    actions
}

impl Genie {
    pub(super) fn open_palette(&mut self) -> Task<Message> {
        self.close_menus();
        self.popup = Some(Popup::Palette);
        self.palette = Some(State::default());
        operation::focus(palette::INPUT)
    }

    pub(super) fn palette_message(&mut self, paletting: Paletting) -> Task<Message> {
        match paletting {
            Paletting::Query(query) => {
                if let Some(state) = &mut self.palette {
                    state.query = query;
                    state.selected = 0;
                }
                Task::none()
            }
            Paletting::Choose(index) => self.run_palette(index),
            Paletting::Run => {
                let index = self.palette.as_ref().map_or(0, |state| state.selected);
                self.run_palette(index)
            }
        }
    }

    pub(super) fn step_palette(&mut self, forward: bool) -> bool {
        let Some(state) = &self.palette else {
            return false;
        };
        let count = palette::found(&self.palette_actions(), &state.query).len();
        if let Some(state) = &mut self.palette
            && count > 0
        {
            state.selected = if forward {
                (state.selected + 1).min(count - 1)
            } else {
                state.selected.saturating_sub(1)
            };
        }
        true
    }

    fn run_palette(&mut self, index: usize) -> Task<Message> {
        let query = self
            .palette
            .as_ref()
            .map(|state| state.query.clone())
            .unwrap_or_default();
        let actions = self.palette_actions();
        let message = palette::found(&actions, &query)
            .get(index)
            .map(|action| action.message.clone());
        self.close_menus();
        message.map_or_else(Task::none, |message| self.update(message))
    }

    fn palette_actions(&self) -> Vec<Action> {
        let mut actions = general_actions();
        if let Some(session) = &self.session {
            let command = Message::Command;
            actions.extend([
                Action::new("Save session", keys('S'), command(Command::SaveSession)),
                Action::new("Export image…", keys('E'), command(Command::Export)),
                Action::new("Zoom in", "+", command(Command::Zoom(2.0))),
                Action::new("Zoom out", "-", command(Command::Zoom(0.5))),
                Action::new("Show the whole chromosome", "", command(Command::Whole)),
                Action::new("Show the whole genome", "", command(Command::Genome)),
                Action::new("Go back", "", command(Command::Back)),
                Action::new("Go forward", "", command(Command::Forward)),
                Action::new(
                    "Go to a locus or a name",
                    keys('L'),
                    command(Command::FocusLocus),
                ),
                Action::new(
                    "Add the view to the regions",
                    keys('D'),
                    command(Command::AddRegion),
                ),
                Action::new("Show the regions", "", Message::Regions(Regions::Toggle)),
                Action::new(
                    "Show the tracks",
                    chrome::shift_keys('T'),
                    command(Command::TrackList),
                ),
                Action::new("Copy the locus", keys('C'), command(Command::Copy)),
                Action::new("Undo", keys('Z'), command(Command::Undo)),
                Action::new("Redo", keys('Y'), command(Command::Redo)),
                Action::new(
                    "Find a sequence…",
                    chrome::shift_keys('F'),
                    command(Command::Find),
                ),
                Action::new("BLAT a sequence…", "", Message::Blat(Blatting::Open(None))),
            ]);
            if !session.marks.is_empty() {
                actions.push(Action::new(
                    "Lift the regions…",
                    "",
                    Message::Lift(Lifting::Pick(None)),
                ));
            }
            let catalog = session
                .reference
                .genome
                .as_deref()
                .is_some_and(|genome| !crate::catalog::entries(genome).is_empty());
            if catalog {
                actions.push(Action::new("Add a UCSC track", "", Message::CatalogMenu));
            }
            for (index, hub) in self.hubs.iter().enumerate() {
                actions.push(Action::new(
                    format!("Show the hub {}", hub.short),
                    hub.location.place(),
                    Message::Hub(Hubbing::Show(index)),
                ));
            }
            actions.extend(tracks_actions(session));
        }
        let recent_hubs = self
            .known_hubs
            .entries()
            .iter()
            .filter(|(location, _)| self.hubs.iter().all(|hub| hub.location != *location));
        for (location, name) in recent_hubs {
            let name = if name.is_empty() {
                location.name()
            } else {
                name.clone()
            };
            actions.push(Action::new(
                format!("Open the hub {name}"),
                format!("Recent · {}", location.place()),
                Message::Hub(Hubbing::Open(location.clone())),
            ));
        }
        for entry in self.recent.entries() {
            match entry {
                Entry::Location(location) => actions.push(Action::new(
                    format!("Open {}", location.name()),
                    format!("Recent · {}", location.place()),
                    Message::Reopen(location.clone()),
                )),
                Entry::Genome(id) => {
                    if let Some(genome) = genome::find(id) {
                        actions.push(Action::new(
                            format!("Open {}", genome.label),
                            format!("Recent · {} {}", genome.species, genome.assembly),
                            Message::Genome(genome.id),
                        ));
                    }
                }
            }
        }
        actions
    }

    pub(super) fn palette_view(&self) -> Option<Element<'_, Message>> {
        let state = self.palette.as_ref()?;
        let actions = self.palette_actions();
        let found = palette::found(&actions, &state.query);
        Some(palette::view(&state.query, &found, state.selected))
    }
}
