use iced::widget::operation;
use iced::{Element, Task};

use crate::chrome::keys;
use crate::genome;
use crate::menu::{GROUPS, MODES, STYLES, TOGGLES};
use crate::palette::{self, Action, State};
use crate::recent::Entry;
use crate::shortcuts::Command;
use crate::track::{Change, Toggle, Track};

use super::{Genie, Message, Paletting, Popup, Ranging, Regions, Renaming};

fn track_actions(track: &Track) -> Vec<Action> {
    let (id, name) = (track.id, track.name.as_str());
    let change = move |change| Message::TrackChanged(id, change);
    let mut actions = vec![Action::new(
        "Track settings…",
        name,
        Message::TrackSettings(id),
    )];
    let source = &track.source;
    if !source.has_signal() {
        actions.extend(
            MODES.map(|(label, mode)| Action::new(label, name, change(Change::Mode(mode)))),
        );
    }
    if source.has_reads() {
        actions.extend(
            TOGGLES.map(|(label, toggle)| Action::new(label, name, change(Change::Toggle(toggle)))),
        );
        actions.push(Action::new(
            "Color unusual pairs",
            name,
            change(Change::Pairs),
        ));
        actions.push(Action::new(
            "Color by modification",
            name,
            change(Change::Mods),
        ));
        actions
            .extend(GROUPS.map(|(label, grouping)| {
                Action::new(label, name, change(Change::Group(grouping)))
            }));
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
    if source.can_jump() {
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
        let mut actions = vec![
            Action::new("Open file…", keys('O'), Message::Command(Command::Open)),
            Action::new("Open URL…", keys('U'), Message::Command(Command::OpenUrl)),
            Action::new("Keys and mouse", "F1", Message::Command(Command::Help)),
            Action::new("Settings", "", Message::ToggleSettings),
            Action::new("Change the theme", "", Message::CycleAppearance),
        ];
        if let Some(session) = &self.session {
            let command = Message::Command;
            actions.extend([
                Action::new("Save session", keys('S'), command(Command::SaveSession)),
                Action::new("Export image…", keys('E'), command(Command::Export)),
                Action::new("Zoom in", "+", command(Command::Zoom(2.0))),
                Action::new("Zoom out", "-", command(Command::Zoom(0.5))),
                Action::new("Show the whole chromosome", "", command(Command::Whole)),
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
                Action::new("Copy the locus", keys('C'), command(Command::Copy)),
            ]);
            let catalog = session
                .reference
                .genome
                .as_deref()
                .is_some_and(|genome| !crate::catalog::entries(genome).is_empty());
            if catalog {
                actions.push(Action::new("Add a UCSC track", "", Message::CatalogMenu));
            }
            for track in &session.tracks {
                actions.extend(track_actions(track));
            }
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
