//! Tracks that genie makes from the reference: the actions that add them, the GC window of a
//! track, and the dialog of the restriction enzymes.

use std::sync::Arc;

use genie_core::enzyme::{self, Enzyme};
use genie_render::features::Mode;
use iced::widget::{
    Column, Row, button, center, column, container, opaque, operation, row, space, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::icon::{self, Icon};
use crate::track::{self, Change, Made};
use crate::{font, style};

use super::tracks::NO_REFERENCE;
use super::{Genie, Message, Popup, Status};

const FIELD: &str = "enzyme-site";
/// The name of a depth ratio track of a session until the session gives its own.
const RATIO_NAME: &str = "Depth ratio";
/// The most width of the dialog: six enzymes in a row, each with its site beside its name.
const WIDTH: f32 = 860.0;
const COLUMNS: usize = 6;
const ABOUT: &str =
    "Pick enzymes, or type a site with ^ at the cut. The track shows where they cut the reference.";
const NONE_PICKED: &str = "Pick an enzyme or type a site.";

#[derive(Debug, Clone, PartialEq)]
pub enum Making {
    /// Adds a track made from the reference.
    Add(Made),
    /// Shows the GC content of a track in windows of a size.
    Window(u64, u64),
    /// Opens the dialog of the enzymes for a new track, or for the track with this ID.
    Enzymes(Option<u64>),
    /// Picks a common enzyme at a place in the list, or drops it.
    Tick(usize),
    /// The text of the field of a typed site.
    Typed(String),
    /// Adds the typed site to the enzymes of the dialog.
    Type,
    /// Drops the enzyme at a place in the enzymes of the dialog.
    Drop(usize),
    /// Shows the cuts of the enzymes of the dialog.
    Show,
}

/// The tracks that genie makes from the reference, each with the message that adds it.
pub(crate) fn additions() -> [(&'static str, Message); 4] {
    [
        (
            "GC %",
            Message::Made(Making::Add(Made::Gc(track::made::WINDOW))),
        ),
        ("CpG islands", Message::Made(Making::Add(Made::Islands))),
        ("Repeats", Message::Made(Making::Add(Made::Repeats))),
        ("Restriction sites…", Message::Made(Making::Enzymes(None))),
    ]
}

/// The dialog of the enzymes: the track that it changes, if any, the enzymes in the order of
/// the picks, the typed site and the error of the last step.
#[derive(Debug, Default)]
pub(super) struct Enzymes {
    track: Option<u64>,
    chosen: Vec<Enzyme>,
    typed: String,
    error: Option<String>,
}

impl Genie {
    pub(super) fn making(&mut self, making: Making) -> Task<Message> {
        let state = &mut self.enzymes;
        match making {
            Making::Add(made) => return self.add_made(made),
            Making::Window(id, window) => return self.remake(id, Made::Gc(window)),
            Making::Enzymes(track) => return self.open_enzymes(track),
            Making::Tick(index) => {
                if let Some(picked) = enzyme::common().into_iter().nth(index) {
                    match state.chosen.iter().position(|known| *known == picked) {
                        Some(at) => {
                            state.chosen.remove(at);
                        }
                        None => state.chosen.push(picked),
                    }
                }
                state.error = None;
            }
            Making::Typed(text) => {
                state.typed = text;
                state.error = None;
            }
            Making::Type => {
                state.add_typed();
            }
            Making::Drop(at) => {
                if at < state.chosen.len() {
                    state.chosen.remove(at);
                }
            }
            Making::Show => return self.show_sites(),
        }
        Task::none()
    }

    /// Adds a track made from the reference. A new track of repeats is squished: its runs never
    /// overlap, and a label for each one says nothing new.
    pub(super) fn add_made(&mut self, made: Made) -> Task<Message> {
        self.close_menus();
        let Some(session) = &self.session else {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        };
        let squished = made == Made::Repeats && self.restoring.is_none();
        let source = Arc::new(track::make(made, Arc::clone(&session.reference)));
        let task = self.add_track(&source);
        if squished {
            self.on_session(|session| {
                if let Some(track) = session
                    .tracks
                    .iter_mut()
                    .find(|track| track.source == source)
                {
                    track.mode = Some(Mode::Squished);
                }
            });
        }
        task
    }

    /// Adds the track of the ratio of the depth of a read track over the depth of another one,
    /// after the first one, or removes it when it is open.
    pub(super) fn compare_depths(&mut self, first: u64, second: u64) -> Task<Message> {
        self.close_menus();
        let Some(session) = &self.session else {
            return Task::none();
        };
        let open = session
            .tracks
            .iter()
            .find(|track| track.options.depths == Some((first, second)));
        if let Some(id) = open.map(|track| track.id) {
            return self.change_track(id, Change::Remove);
        }
        let name = |id: u64| {
            let track = session.tracks.iter().find(|track| track.id == id)?;
            Some(track.name.clone())
        };
        let (Some(top), Some(bottom)) = (name(first), name(second)) else {
            return Task::none();
        };
        let key = format!("{} {}", track::ratio::KEY, self.next_track);
        let name = format!("{top} / {bottom}");
        let source = Arc::new(track::depth_ratio(&session.reference.assembly, &name, key));
        let task = self.add_track(&source);
        self.on_session(|session| {
            let Some(at) = session
                .tracks
                .iter()
                .position(|track| track.source == source)
            else {
                return;
            };
            let mut ratio = session.tracks.remove(at);
            ratio.options.depths = Some((first, second));
            let after = session.tracks.iter().position(|track| track.id == first);
            let place = after.map_or(session.tracks.len(), |place| place + 1);
            session.tracks.insert(place, ratio);
        });
        Task::batch([task, self.fetch()])
    }

    /// Opens a track made from the reference that a session names.
    pub(super) fn open_made(&mut self, key: &str) -> Task<Message> {
        if key.starts_with(track::ratio::KEY) {
            let task = match &self.session {
                Some(session) => {
                    let assembly = &session.reference.assembly;
                    let source = track::depth_ratio(assembly, RATIO_NAME, key.to_owned());
                    self.add_track(&Arc::new(source))
                }
                None => Task::none(),
            };
            self.track_settled();
            return task;
        }
        let task = if let Some(made) = Made::parse(key) {
            self.add_made(made)
        } else {
            self.fail(format!("genie cannot make the track {key}."));
            Task::none()
        };
        self.track_settled();
        task
    }

    /// Shows a track made from the reference with other settings. A name that somebody gave
    /// the track stays.
    fn remake(&mut self, id: u64, made: Made) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let source = Arc::new(track::make(made, Arc::clone(&session.reference)));
        self.swap_source(id, source)
    }

    /// Opens the dialog of the enzymes for the track of sites with this ID, with its enzymes, or
    /// for a new track.
    fn open_enzymes(&mut self, track: Option<u64>) -> Task<Message> {
        let found = track.and_then(|id| {
            let session = self.session.as_ref()?;
            let track = session.tracks.iter().find(|track| track.id == id)?;
            match track.source.made()? {
                Made::Sites(enzymes) => Some((id, enzymes.clone())),
                _ => None,
            }
        });
        let (track, chosen) = found.map_or((None, Vec::new()), |(id, chosen)| (Some(id), chosen));
        self.close_menus();
        self.enzymes = Enzymes {
            track,
            chosen,
            typed: String::new(),
            error: None,
        };
        self.popup = Some(Popup::Enzymes);
        operation::focus(FIELD)
    }

    /// Shows the cuts of the enzymes of the dialog, with the site in the field if there is one.
    fn show_sites(&mut self) -> Task<Message> {
        let state = &mut self.enzymes;
        if !state.add_typed() {
            return Task::none();
        }
        if state.chosen.is_empty() {
            state.error = Some(NONE_PICKED.to_owned());
            return Task::none();
        }
        let made = Made::Sites(state.chosen.clone());
        match state.track {
            Some(id) => self.remake(id, made),
            None => self.add_made(made),
        }
    }

    pub(super) fn enzyme_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Enzymes) {
            return None;
        }
        let state = &self.enzymes;
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let title = if state.track.is_some() {
            "Change the enzymes"
        } else {
            "Restriction sites"
        };
        let heading = row![
            text(title).font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let common = enzyme::common();
        let field = text_input(
            "Type a site with ^ at the cut, for example G^AATTC",
            &state.typed,
        )
        .id(FIELD)
        .font(font::MONO)
        .size(12)
        .padding([3, 8])
        .style(style::locus)
        .on_input(|text| Message::Made(Making::Typed(text)))
        .on_submit(Message::Made(Making::Type));
        let add = button(text("Add").size(12))
            .padding([4, 10])
            .style(style::ghost)
            .on_press(Message::Made(Making::Type));
        let count = match state.chosen.len() {
            0 => "No enzyme".to_owned(),
            1 => "1 enzyme".to_owned(),
            count => format!("{count} enzymes"),
        };
        let words = if state.track.is_some() {
            "Show the sites"
        } else {
            "Add the track"
        };
        let ready = !state.chosen.is_empty() || !state.typed.trim().is_empty();
        let show = button(text(words).size(12))
            .padding([4, 12])
            .style(style::primary)
            .on_press_maybe(ready.then_some(Message::Made(Making::Show)));
        let actions = row![
            text(count).size(11).style(style::muted),
            space::horizontal(),
            show
        ]
        .align_y(Alignment::Center);
        let note = state.error.as_deref().unwrap_or(ABOUT);
        let body = column![
            heading,
            grid(&common, &state.chosen),
            row![field, add].spacing(6).align_y(Alignment::Center),
            chips(&common, &state.chosen),
            actions,
            text(note).size(11).style(style::muted)
        ]
        .spacing(10);
        let card = container(body)
            .id("enzyme-dialog")
            .width(Length::Fill)
            .max_width(WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

/// The common enzymes in rows, each with its site. A picked enzyme has the color of a choice.
fn grid(common: &[Enzyme], chosen: &[Enzyme]) -> Column<'static, Message> {
    let mut grid = Column::new().spacing(1);
    for (line, enzymes) in common.chunks(COLUMNS).enumerate() {
        let mut cells = Row::new().spacing(1);
        for (offset, enzyme) in enzymes.iter().enumerate() {
            let content = row![
                text(enzyme.name.clone())
                    .size(12)
                    .wrapping(text::Wrapping::None),
                space::horizontal(),
                text(enzyme.written())
                    .size(10)
                    .font(font::MONO)
                    .style(style::muted)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            let style = if chosen.contains(enzyme) {
                style::chosen
            } else {
                style::ghost
            };
            cells = cells.push(
                button(content)
                    .width(Length::Fill)
                    .padding([3, 6])
                    .clip(true)
                    .style(style)
                    .on_press(Message::Made(Making::Tick(line * COLUMNS + offset))),
            );
        }
        for _ in enzymes.len()..COLUMNS {
            cells = cells.push(space::horizontal());
        }
        grid = grid.push(cells);
    }
    grid
}

/// The typed sites of the dialog, each with a button that drops it.
fn chips(common: &[Enzyme], chosen: &[Enzyme]) -> Row<'static, Message> {
    let mut chips = Row::new().spacing(4);
    for (at, enzyme) in chosen.iter().enumerate() {
        if common.contains(enzyme) {
            continue;
        }
        let chip = row![
            text(enzyme.name.clone()).size(12).font(font::MONO),
            icon::view(Icon::Close, 8.0)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        chips = chips.push(
            button(chip)
                .padding([2, 6])
                .style(style::chosen)
                .on_press(Message::Made(Making::Drop(at))),
        );
    }
    chips
}

impl Enzymes {
    /// Adds the site of the field to the enzymes, if the field has one. False when the site is
    /// not valid: the error then says why.
    fn add_typed(&mut self) -> bool {
        if self.typed.trim().is_empty() {
            return true;
        }
        match Enzyme::parse(&self.typed) {
            Ok(typed) => {
                if !self.chosen.contains(&typed) {
                    self.chosen.push(typed);
                }
                self.typed.clear();
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::Region;
    use genie_core::enzyme::COMMON;
    use iced::Point;
    use iced_test::simulator;

    use super::{Making, Mode};
    use crate::app::{Genie, Message};
    use crate::saved::Saved;
    use crate::scratch::Scratch;
    use crate::shortcuts::Command;
    use crate::track::{Content, Made};

    /// A genie with a reference of 1,004 bases in a scratch folder: a CpG island at 300-598, a
    /// site `GAATTC` at 700-706 and soft-masked bases at 806-814.
    fn genie(name: &str) -> (Scratch, Genie) {
        let scratch = Scratch::new(name);
        let bases = format!(
            "{}{}{}GAATTC{}acgtacgt{}",
            "A".repeat(300),
            "CGCA".repeat(75),
            "A".repeat(100),
            "A".repeat(100),
            "T".repeat(190)
        );
        let path = scratch.file("ref.fa", format!(">chr1\n{bases}\n"));
        let reference = crate::reference::open(path, None, &mut |_| {}).unwrap();
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(Arc::new(reference))));
        (scratch, genie)
    }

    fn shows(genie: &Genie, words: &str) -> bool {
        simulator(genie.view()).find(words).is_ok()
    }

    fn names(genie: &Genie) -> Vec<String> {
        let session = genie.session.as_ref().unwrap();
        session
            .tracks
            .iter()
            .map(|track| track.name.clone())
            .collect()
    }

    fn made(genie: &Genie, at: usize) -> Option<Made> {
        genie.session.as_ref().unwrap().tracks[at]
            .source
            .made()
            .cloned()
    }

    fn spans(genie: &Genie, at: usize) -> Vec<Region> {
        let track = &genie.session.as_ref().unwrap().tracks[at];
        match &track.data(0).unwrap().content {
            Content::Features(annotation) => annotation
                .features
                .iter()
                .map(genie_core::Feature::span)
                .collect(),
            other => panic!("not features: {other:?}"),
        }
    }

    #[test]
    fn the_sequence_menu_adds_tracks_made_from_the_reference() {
        let (_scratch, mut genie) = genie("made-menu");
        let _ = genie.update(Message::SequenceMenu(Point::new(100.0, 100.0)));
        let mut ui = simulator(genie.view());
        ui.click("CpG islands").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Made(Making::Add(Made::Islands))));
        for made in [Made::Islands, Made::Gc(100), Made::Repeats] {
            let _ = genie.update(Message::Made(Making::Add(made)));
        }
        assert_eq!(names(&genie), ["CpG islands", "GC % (100 bp)", "Repeats"]);
        let modes: Vec<Option<Mode>> = genie
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|track| track.mode)
            .collect();
        assert_eq!(modes, [None, None, Some(Mode::Squished)]);
        genie.load_now();
        assert_eq!(spans(&genie, 0), [Region::between(300, 598)]);
        assert_eq!(spans(&genie, 2), [Region::between(806, 814)]);
        let session = genie.session.as_ref().unwrap();
        let Content::Signal(signal) = &session.tracks[1].data(0).unwrap().content else {
            panic!("GC % is a signal");
        };
        assert_ne!(signal.summaries.len(), 0);
        let _ = genie.update(Message::Made(Making::Add(Made::Islands)));
        assert!(shows(&genie, "The track CpG islands is already open."));
    }

    #[test]
    fn the_genie_menu_and_the_palette_add_the_tracks_too() {
        let (_scratch, mut genie) = genie("made-app-menu");
        let _ = genie.update(Message::AppMenu);
        assert!(shows(&genie, "Reference tracks"));
        assert!(!shows(&genie, "Restriction sites…"));
        let _ = genie.update(Message::MadeMenu(true));
        let mut ui = simulator(genie.view());
        ui.click("Restriction sites…").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Made(Making::Enzymes(None))));
        let _ = genie.update(Message::RecentMenu(false));
        assert!(!shows(&genie, "Restriction sites…"));
        let _ = genie.update(Message::Command(Command::Palette));
        let query = crate::app::Paletting::Query("cpg islands".to_owned());
        let _ = genie.update(Message::Palette(query));
        assert!(shows(&genie, "Add a CpG islands track"));
    }

    #[test]
    fn the_window_of_gc_changes_the_track_and_undo_takes_it_back() {
        let (_scratch, mut genie) = genie("made-window");
        let _ = genie.update(Message::Made(Making::Add(Made::Gc(100))));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::TrackMenu(id, Point::new(100.0, 100.0)));
        assert!(shows(&genie, "Window (bp)"));
        let _ = genie.update(Message::Made(Making::Window(id, 500)));
        assert_eq!(made(&genie, 0), Some(Made::Gc(500)));
        assert_eq!(names(&genie), ["GC % (500 bp)"]);
        let _ = genie.update(Message::Command(Command::Undo));
        assert_eq!(made(&genie, 0), Some(Made::Gc(100)));
        assert_eq!(names(&genie), ["GC % (100 bp)"]);
    }

    #[test]
    fn the_enzyme_dialog_adds_and_changes_the_sites() {
        let (_scratch, mut genie) = genie("made-enzymes");
        let _ = genie.update(Message::Made(Making::Enzymes(None)));
        assert!(shows(&genie, "Restriction sites"));
        assert!(shows(&genie, "A^AGCTT"));
        let eco = COMMON
            .iter()
            .position(|(name, _)| *name == "EcoRI")
            .unwrap();
        let _ = genie.update(Message::Made(Making::Tick(eco)));
        assert!(shows(&genie, "1 enzyme"));
        let _ = genie.update(Message::Made(Making::Typed("G^A^C".to_owned())));
        let _ = genie.update(Message::Made(Making::Type));
        assert!(shows(&genie, "Put one ^ at most in a site, at the cut."));
        let _ = genie.update(Message::Made(Making::Typed("gg^atcc".to_owned())));
        let _ = genie.update(Message::Made(Making::Show));
        assert_eq!(names(&genie), ["EcoRI, GG^ATCC sites"]);
        assert!(!shows(&genie, "Restriction sites"));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::Made(Making::Enzymes(Some(id))));
        assert!(shows(&genie, "Change the enzymes"));
        assert!(shows(&genie, "2 enzymes"));
        let _ = genie.update(Message::Made(Making::Drop(1)));
        let _ = genie.update(Message::Made(Making::Show));
        assert_eq!(names(&genie), ["EcoRI sites"]);
        genie.load_now();
        assert_eq!(spans(&genie, 0), [Region::between(700, 706)]);
        let _ = genie.update(Message::Made(Making::Enzymes(None)));
        let _ = genie.update(Message::Made(Making::Show));
        assert!(shows(&genie, "Pick an enzyme or type a site."));
    }

    #[test]
    fn sessions_make_the_tracks_again() {
        let (scratch, mut genie) = genie("made-session");
        let _ = genie.update(Message::Made(Making::Add(Made::Islands)));
        let eco = COMMON
            .iter()
            .position(|(name, _)| *name == "EcoRI")
            .unwrap();
        let _ = genie.update(Message::Made(Making::Enzymes(None)));
        let _ = genie.update(Message::Made(Making::Tick(eco)));
        let _ = genie.update(Message::Made(Making::Show));
        let reference = Arc::clone(&genie.session.as_ref().unwrap().reference);
        let base = scratch.file("view.genie", "");
        let folder = base.parent().unwrap();
        let text = genie.saved().unwrap().text(folder);
        assert!(text.contains("made = cpg islands"), "{text}");
        let mut again = Genie::default();
        let _ = again.restore(Saved::parse(&text, folder).unwrap());
        let _ = again.update(Message::Opened(Ok(reference)));
        assert_eq!(names(&again), ["CpG islands", "EcoRI sites"]);
        assert!(again.restoring.is_none());
    }
}
