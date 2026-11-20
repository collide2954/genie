//! Tracks of motif sites: genie reads a motif file, lists its motifs in a dialog, and shows the
//! sites of the picked motifs in the reference.

use std::io::{BufReader, Read};
use std::sync::Arc;

use genie_formats::gzip;
use genie_formats::motifs::{self, MotifSet};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, opaque, operation, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::icon::{self, Icon};
use crate::location::Location;
use crate::track::{self, motifs::P};
use crate::{font, style};

use super::tracks::NO_REFERENCE;
use super::{Genie, Message, Popup, Status};

const FIELD: &str = "motif-search";
const WIDTH: f32 = 640.0;
const LIST_HEIGHT: f32 = 320.0;
/// The most rows that the list shows. A search finds the others.
const MOST_ROWS: usize = 300;
/// The longest consensus that a row shows.
const CONSENSUS: usize = 24;
/// The largest motif file that genie reads.
const LIMIT: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Motifing {
    /// The motifs of a file that genie read.
    Read(Location, Result<Arc<MotifSet>, String>),
    /// The words of the search of the dialog.
    Query(String),
    /// Picks the motif at a place in the file, or drops it.
    Tick(usize),
    /// Shows the sites of the picked motifs.
    Show,
    /// Opens the dialog for the motif track with this ID.
    Pick(u64),
    /// Shows the sites of a motif track with p under this threshold.
    Threshold(u64, f64),
}

/// The dialog of the motifs of a file: the picks, the search, the track that it changes and
/// the threshold of the track.
#[derive(Debug)]
pub(super) struct MotifDialog {
    location: Location,
    set: Arc<MotifSet>,
    chosen: Vec<usize>,
    query: String,
    track: Option<u64>,
    p: f64,
}

/// The motifs of a file, plain or compressed with gzip.
pub(super) fn read(location: &Location) -> Result<MotifSet, String> {
    let name = location.name();
    if location.size()? > LIMIT {
        return Err(format!("The file {name} is larger than 64 MB."));
    }
    let file = BufReader::new(location.open()?);
    let mut text = String::new();
    let read = if track::is_compressed(&name) {
        gzip::Reader::new(file).read_to_string(&mut text)
    } else {
        BufReader::new(file).read_to_string(&mut text)
    };
    read.map_err(|error| format!("genie cannot read {name}. {error}"))?;
    motifs::read(&text).map_err(|error| format!("The file {name} is not valid. {error}"))
}

impl Genie {
    /// Reads a motif file in the background.
    pub(super) fn open_motifs(&mut self, location: Location) -> Task<Message> {
        if self.session.is_none() {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        }
        self.busy += 1;
        Task::perform(
            async move {
                let set = read(&location).map(Arc::new);
                (location, set)
            },
            |(location, set)| Message::Motifs(Motifing::Read(location, set)),
        )
    }

    pub(super) fn motifing(&mut self, motifing: Motifing) -> Task<Message> {
        match motifing {
            Motifing::Read(location, result) => return self.motifs_read(location, result),
            Motifing::Query(query) => {
                if let Some(dialog) = &mut self.motif_dialog {
                    dialog.query = query;
                }
            }
            Motifing::Tick(index) => {
                if let Some(dialog) = &mut self.motif_dialog {
                    match dialog.chosen.iter().position(|&known| known == index) {
                        Some(at) => {
                            dialog.chosen.remove(at);
                        }
                        None if index < dialog.set.motifs.len() => dialog.chosen.push(index),
                        None => {}
                    }
                }
            }
            Motifing::Show => return self.show_motifs(),
            Motifing::Pick(id) => {
                if let Some(dialog) = self.dialog_of(id) {
                    return self.open_motif_dialog(dialog);
                }
            }
            Motifing::Threshold(id, p) => {
                let Some(dialog) = self.dialog_of(id) else {
                    return Task::none();
                };
                let picks = (dialog.chosen, p);
                return self.swap_motifs(id, dialog.location, dialog.set, picks);
            }
        }
        Task::none()
    }

    /// Shows the motifs of a file that genie read: the track of a session with its picks, or
    /// the dialog to pick them.
    fn motifs_read(
        &mut self,
        location: Location,
        result: Result<Arc<MotifSet>, String>,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let set = match result {
            Ok(set) => set,
            Err(error) => {
                self.fail(error);
                self.track_settled();
                return Task::none();
            }
        };
        let saved = self
            .restoring
            .as_ref()
            .and_then(|saved| saved.tracks.iter().find(|track| track.location == location))
            .and_then(|track| track.motifs.clone());
        if let Some((ids, p)) = saved {
            let picks = ids
                .iter()
                .filter_map(|id| set.motifs.iter().position(|motif| motif.id == *id))
                .collect();
            let task = self.add_motifs(location, set, (picks, p));
            self.track_settled();
            return task;
        }
        self.track_settled();
        let open = self.session.as_ref().and_then(|session| {
            let found = session
                .tracks
                .iter()
                .find(|track| track.source.location == location);
            found.map(|track| track.id)
        });
        let dialog = match open.and_then(|id| self.dialog_of(id)) {
            Some(dialog) => dialog,
            None => MotifDialog {
                location,
                set,
                chosen: Vec::new(),
                query: String::new(),
                track: None,
                p: P,
            },
        };
        self.open_motif_dialog(dialog)
    }

    /// The dialog for the motif track with this ID, with its picks and its threshold.
    fn dialog_of(&self, id: u64) -> Option<MotifDialog> {
        let track = self
            .session
            .as_ref()?
            .tracks
            .iter()
            .find(|track| track.id == id)?;
        let file = track.source.motifs()?;
        Some(MotifDialog {
            location: track.source.location.clone(),
            set: Arc::clone(&file.set),
            chosen: file.picks.clone(),
            query: String::new(),
            track: Some(id),
            p: file.p,
        })
    }

    fn open_motif_dialog(&mut self, dialog: MotifDialog) -> Task<Message> {
        self.close_menus();
        self.motif_dialog = Some(dialog);
        self.popup = Some(Popup::Motifs);
        operation::focus(FIELD)
    }

    fn add_motifs(
        &mut self,
        location: Location,
        set: Arc<MotifSet>,
        picks: (Vec<usize>, f64),
    ) -> Task<Message> {
        self.close_menus();
        let Some(session) = &self.session else {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        };
        let reference = Arc::clone(&session.reference);
        let source = Arc::new(track::motif_sites(location, set, picks, reference));
        self.add_track(&source)
    }

    fn swap_motifs(
        &mut self,
        id: u64,
        location: Location,
        set: Arc<MotifSet>,
        picks: (Vec<usize>, f64),
    ) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let reference = Arc::clone(&session.reference);
        let source = Arc::new(track::motif_sites(location, set, picks, reference));
        self.swap_source(id, source)
    }

    /// Shows the sites of the picked motifs of the dialog: a new track, or the track that the
    /// dialog changes.
    fn show_motifs(&mut self) -> Task<Message> {
        let Some(dialog) = self.motif_dialog.take() else {
            return Task::none();
        };
        let picks = (dialog.chosen, dialog.p);
        match dialog.track {
            Some(id) => self.swap_motifs(id, dialog.location, dialog.set, picks),
            None => self.add_motifs(dialog.location, dialog.set, picks),
        }
    }

    pub(super) fn motif_dialog_view(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Motifs) {
            return None;
        }
        let dialog = self.motif_dialog.as_ref()?;
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let title = if dialog.track.is_some() {
            "Change the motifs".to_owned()
        } else {
            format!("Motifs of {}", dialog.location.name())
        };
        let heading = row![
            text(title).font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let field = text_input(
            "Find a motif by name or ID, for example CTCF",
            &dialog.query,
        )
        .id(FIELD)
        .size(12)
        .padding([3, 8])
        .style(style::locus)
        .on_input(|query| Message::Motifs(Motifing::Query(query)));
        let (rows, more) = motif_rows(dialog);
        let list = scrollable(rows)
            .direction(Direction::Vertical(
                Scrollbar::new().width(6).scroller_width(6).margin(0),
            ))
            .style(style::scrollbar)
            .height(Length::Shrink);
        let count = dialog.set.motifs.len();
        let picked = match dialog.chosen.len() {
            0 => format!("No pick: all {count} motifs show in views of 5 kb or less."),
            1 => "1 motif picked".to_owned(),
            picks => format!("{picks} motifs picked"),
        };
        let words = if dialog.track.is_some() {
            "Show the sites"
        } else {
            "Add the track"
        };
        let show = button(text(words).size(12))
            .padding([4, 12])
            .style(style::primary)
            .on_press(Message::Motifs(Motifing::Show));
        let actions = row![
            text(picked).size(11).style(style::muted),
            space::horizontal(),
            show
        ]
        .align_y(Alignment::Center);
        let mut body = column![heading, field, container(list).max_height(LIST_HEIGHT)].spacing(10);
        if more {
            body = body.push(
                text(format!("Type to find more of the {count} motifs."))
                    .size(11)
                    .style(style::muted),
            );
        }
        let card = container(body.push(actions))
            .id("motif-dialog")
            .width(Length::Fill)
            .max_width(WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

/// The rows of the motifs that the search finds, picked ones in the color of a choice, and
/// whether the search finds more motifs than the list shows.
fn motif_rows(dialog: &MotifDialog) -> (Column<'static, Message>, bool) {
    let query = dialog.query.trim().to_lowercase();
    let found: Vec<usize> = (0..dialog.set.motifs.len())
        .filter(|&index| {
            let motif = &dialog.set.motifs[index];
            query.is_empty()
                || motif.id.to_lowercase().contains(&query)
                || motif.name.to_lowercase().contains(&query)
        })
        .collect();
    let mut rows = Column::new().spacing(1);
    for &index in found.iter().take(MOST_ROWS) {
        let motif = &dialog.set.motifs[index];
        let mut consensus = motif.consensus();
        if consensus.len() > CONSENSUS {
            consensus.truncate(CONSENSUS);
            consensus.push('…');
        }
        let content = row![
            text(motif.name.clone())
                .size(12)
                .wrapping(text::Wrapping::None),
            text(motif.id.clone())
                .size(11)
                .style(style::muted)
                .wrapping(text::Wrapping::None),
            space::horizontal(),
            text(format!("{} bp", motif.probabilities.len()))
                .size(11)
                .style(style::muted),
            text(consensus)
                .size(11)
                .font(font::MONO)
                .style(style::muted)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let style = if dialog.chosen.contains(&index) {
            style::chosen
        } else {
            style::ghost
        };
        rows = rows.push(
            button(content)
                .width(Length::Fill)
                .padding([2, 8])
                .clip(true)
                .style(style)
                .on_press(Message::Motifs(Motifing::Tick(index))),
        );
    }
    (rows, found.len() > MOST_ROWS)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::{Feature, Region};
    use iced_test::simulator;

    use super::{Motifing, read};
    use crate::app::{Genie, Message};
    use crate::location::Location;
    use crate::saved::Saved;
    use crate::scratch::Scratch;
    use crate::shortcuts::Command;
    use crate::track::Content;

    const MEME: &str = "\
MEME version 4

ALPHABET= ACGT

MOTIF M1 GATA
letter-probability matrix: alength= 4 w= 7 nsites= 100
 0.01 0.01 0.97 0.01
 0.97 0.01 0.01 0.01
 0.01 0.01 0.01 0.97
 0.01 0.01 0.01 0.97
 0.97 0.01 0.01 0.01
 0.01 0.97 0.01 0.01
 0.97 0.01 0.01 0.01

MOTIF M2 CEBP
letter-probability matrix: alength= 4 w= 8 nsites= 100
 0.01 0.97 0.01 0.01
 0.01 0.97 0.01 0.01
 0.01 0.97 0.01 0.01
 0.01 0.01 0.97 0.01
 0.01 0.01 0.97 0.01
 0.01 0.01 0.97 0.01
 0.01 0.01 0.01 0.97
 0.01 0.01 0.01 0.97
";

    /// A genie with a reference of 84 bases: GATTACA at 20-27 and its reverse at 57-64, and
    /// a motif file.
    fn genie(name: &str) -> (Scratch, Genie, Location) {
        let scratch = Scratch::new(name);
        let bases = format!(
            "{}GATTACA{}TGTAATC{}",
            "T".repeat(20),
            "T".repeat(30),
            "T".repeat(20)
        );
        let fasta = scratch.file("ref.fa", format!(">chr1\n{bases}\n"));
        let reference = crate::reference::open(fasta, None, &mut |_| {}).unwrap();
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(Arc::new(reference))));
        let file = Location::File(scratch.file("jaspar.meme", MEME));
        (scratch, genie, file)
    }

    fn opened(genie: &mut Genie, file: &Location) {
        let set = read(file).map(Arc::new);
        let _ = genie.update(Message::Motifs(Motifing::Read(file.clone(), set)));
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

    fn p(genie: &Genie) -> f64 {
        let track = &genie.session.as_ref().unwrap().tracks[0];
        track.source.motifs().unwrap().p
    }

    #[test]
    fn motif_files_open_a_dialog_and_the_picks_make_a_track() {
        let (_scratch, mut genie, file) = genie("motif-dialog");
        opened(&mut genie, &file);
        assert!(shows(&genie, "Motifs of jaspar.meme"));
        assert!(shows(&genie, "GATA"));
        assert!(shows(&genie, "CEBP"));
        assert!(shows(
            &genie,
            "No pick: all 2 motifs show in views of 5 kb or less."
        ));
        let _ = genie.update(Message::Motifs(Motifing::Query("ceb".to_owned())));
        assert!(!shows(&genie, "GATA"));
        let _ = genie.update(Message::Motifs(Motifing::Query(String::new())));
        let _ = genie.update(Message::Motifs(Motifing::Tick(0)));
        assert!(shows(&genie, "1 motif picked"));
        let _ = genie.update(Message::Motifs(Motifing::Show));
        assert_eq!(names(&genie), ["GATA motifs"]);
        assert!(!shows(&genie, "Motifs of jaspar.meme"));
        genie.load_now();
        let track = &genie.session.as_ref().unwrap().tracks[0];
        let Content::Features(annotation) = &track.data(0).unwrap().content else {
            panic!("features");
        };
        let spans: Vec<Region> = annotation.features.iter().map(Feature::span).collect();
        assert_eq!(spans, [Region::between(20, 27), Region::between(57, 64)]);
        let id = track.id;
        let _ = genie.update(Message::Motifs(Motifing::Pick(id)));
        assert!(shows(&genie, "Change the motifs"));
        let _ = genie.update(Message::Motifs(Motifing::Tick(1)));
        let _ = genie.update(Message::Motifs(Motifing::Show));
        assert_eq!(names(&genie), ["GATA, CEBP motifs"]);
        let _ = genie.update(Message::Motifs(Motifing::Threshold(id, 0.001)));
        assert!((p(&genie) - 0.001).abs() < 1e-12);
        let _ = genie.update(Message::Command(Command::Undo));
        assert!((p(&genie) - 0.0001).abs() < 1e-12);
        opened(&mut genie, &file);
        assert!(shows(&genie, "Change the motifs"));
    }

    #[test]
    fn the_track_menu_changes_the_motifs_and_the_threshold() {
        let (_scratch, mut genie, file) = genie("motif-menu");
        opened(&mut genie, &file);
        let _ = genie.update(Message::Motifs(Motifing::Tick(1)));
        let _ = genie.update(Message::Motifs(Motifing::Show));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::TrackMenu(id, iced::Point::new(100.0, 100.0)));
        let mut ui = simulator(genie.view());
        ui.click("Motifs…").unwrap();
        ui.click("1e-3").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Motifs(Motifing::Pick(id))));
        assert!(messages.contains(&Message::Motifs(Motifing::Threshold(id, 0.001))));
    }

    #[test]
    fn sessions_keep_the_picks_of_motif_tracks() {
        let (scratch, mut genie, file) = genie("motif-session");
        opened(&mut genie, &file);
        let _ = genie.update(Message::Motifs(Motifing::Tick(1)));
        let _ = genie.update(Message::Motifs(Motifing::Show));
        let id = genie.session.as_ref().unwrap().tracks[0].id;
        let _ = genie.update(Message::Motifs(Motifing::Threshold(id, 0.001)));
        let reference = Arc::clone(&genie.session.as_ref().unwrap().reference);
        let base = scratch.file("view.genie", "");
        let folder = base.parent().unwrap();
        let text = genie.saved().unwrap().text(folder);
        assert!(text.contains("motifs = M2"), "{text}");
        let mut again = Genie::default();
        let _ = again.restore(Saved::parse(&text, folder).unwrap());
        let _ = again.update(Message::Opened(Ok(reference)));
        opened(&mut again, &file);
        assert_eq!(names(&again), ["CEBP motifs"]);
        assert!((p(&again) - 0.001).abs() < 1e-12);
        assert!(!shows(&again, "Motifs of jaspar.meme"));
        assert!(again.restoring.is_none());
    }

    #[test]
    fn jaspar_text_files_are_motif_files() {
        let scratch = Scratch::new("motif-names");
        let jaspar = scratch.file(
            "JASPAR2024_pfms_jaspar.txt",
            ">MA0004.1\tArnt\nA  [ 4 19 ]\nC  [16 0 ]\nG  [ 0 1 ]\nT  [ 0 0 ]\n",
        );
        assert!(crate::track::is_motifs(&Location::File(jaspar)));
        let table = scratch.file("samples.txt", "sample\tgroup\nA\tx\n");
        assert!(!crate::track::is_motifs(&Location::File(table)));
        assert!(crate::track::is_motifs(&Location::File("a/b.meme".into())));
    }
}
