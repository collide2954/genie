use std::sync::Arc;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, opaque, operation, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::genome;
use crate::hub::{self, Entry, Hub, Wanted};
use crate::icon::{self, Icon};
use crate::location::Location;
use crate::track::{self, Change, Source};
use crate::{font, style};

use super::{Genie, Message, Popup, Status};

pub(super) const SEARCH: &str = "hub-search";
const WIDTH: f32 = 560.0;
/// The most rows that the dialog shows at a time.
const ROWS: usize = 500;
const GENOMES: usize = 100;
const INDENT: f32 = 14.0;
const HEADING: f32 = 24.0;
const LINE: f32 = 25.0;
const DOUBLE: f32 = 40.0;
const HINT: &str = "Click a track to add it or to remove it.";

#[derive(Debug, Clone, PartialEq)]
pub enum Hubbing {
    Ask,
    Loaded(Location, bool, Result<Arc<Hub>, String>),
    Show(usize),
    Query(String),
    Toggle(usize),
    Genome(usize),
    Added(Location, Result<Arc<Source>, String>),
}

/// The hub that the dialog shows, and the words to find.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct View {
    hub: usize,
    query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Off,
    Opening,
    Added,
}

#[derive(Default)]
struct List<'a> {
    rows: Vec<Element<'a, Message>>,
    height: f32,
}

impl<'a> List<'a> {
    fn push(&mut self, row: impl Into<Element<'a, Message>>, height: f32) {
        self.rows.push(row.into());
        self.height += height;
    }
}

impl Genie {
    pub(super) fn hubbing(&mut self, hubbing: Hubbing) -> Task<Message> {
        match hubbing {
            Hubbing::Ask => self.ask_address(true),
            Hubbing::Loaded(location, show, result) => self.hub_loaded(&location, show, result),
            Hubbing::Show(index) => self.show_hub(index),
            Hubbing::Query(query) => {
                if let Some(view) = &mut self.hub_view {
                    view.query = query;
                }
                Task::none()
            }
            Hubbing::Toggle(index) => self.toggle_hub_track(index),
            Hubbing::Genome(index) => self.open_hub_genome(index),
            Hubbing::Added(data, result) => self.hub_track_added(&data, result),
        }
    }

    pub(super) fn open_hub(&mut self, location: Location, show: bool) -> Task<Message> {
        let reference = self.session.as_ref().map(|session| {
            let reference = &session.reference;
            (reference.genome.clone(), reference.location.clone())
        });
        if self.session.is_none() {
            self.status = Status::Opening(location.name());
        }
        self.busy += 1;
        let at = location.clone();
        Task::perform(
            async move {
                let wanted =
                    reference
                        .as_ref()
                        .map_or_else(Wanted::default, |(genome, location)| Wanted {
                            genome: genome.as_deref(),
                            reference: Some(location),
                        });
                hub::load(&at, wanted).map(Arc::new)
            },
            move |result| Message::Hub(Hubbing::Loaded(location.clone(), show, result)),
        )
    }

    fn hub_loaded(
        &mut self,
        location: &Location,
        show: bool,
        result: Result<Arc<Hub>, String>,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        let hub = match result {
            Ok(hub) => hub,
            Err(error) => {
                self.fail(error);
                return Task::none();
            }
        };
        if self.session.is_none() {
            self.status = Status::Idle;
        }
        let known = self
            .hubs
            .iter()
            .position(|known| known.location == *location);
        let index = if let Some(index) = known {
            self.hubs[index] = hub;
            index
        } else {
            self.hubs.push(hub);
            self.hubs.len() - 1
        };
        if show {
            self.show_hub(index)
        } else {
            Task::none()
        }
    }

    fn show_hub(&mut self, index: usize) -> Task<Message> {
        if index >= self.hubs.len() {
            return Task::none();
        }
        self.close_menus();
        self.popup = Some(Popup::Hub);
        self.hub_view = Some(View {
            hub: index,
            query: String::new(),
        });
        operation::focus(SEARCH)
    }

    fn shown_hub(&self) -> Option<&Arc<Hub>> {
        self.hubs.get(self.hub_view.as_ref()?.hub)
    }

    fn toggle_hub_track(&mut self, index: usize) -> Task<Message> {
        let Some(entry) = self.shown_hub().and_then(|hub| hub.tracks.get(index)) else {
            return Task::none();
        };
        let (Some(data), Some(typed)) = (entry.data.clone(), entry.typed()) else {
            return Task::none();
        };
        let found = entry.index.clone();
        let Some(session) = &self.session else {
            return Task::none();
        };
        let open = session
            .tracks
            .iter()
            .find(|track| track.source.location == data)
            .map(|track| track.id);
        if let Some(id) = open {
            return self.change_track(id, Change::Remove);
        }
        if self.hub_adding.contains(&data) {
            return Task::none();
        }
        self.hub_adding.push(data.clone());
        self.busy += 1;
        let at = data.clone();
        Task::perform(
            async move { track::open_typed(at, typed, found.as_ref()).map(Arc::new) },
            move |result| Message::Hub(Hubbing::Added(data.clone(), result)),
        )
    }

    fn hub_track_added(
        &mut self,
        data: &Location,
        result: Result<Arc<Source>, String>,
    ) -> Task<Message> {
        self.busy = self.busy.saturating_sub(1);
        self.hub_adding.retain(|known| known != data);
        let source = match result {
            Ok(source) => source,
            Err(error) => {
                self.fail(error);
                return Task::none();
            }
        };
        let label = self
            .hubs
            .iter()
            .flat_map(|hub| &hub.tracks)
            .find(|entry| entry.data.as_ref() == Some(data))
            .map(|entry| entry.short.clone());
        let task = self.add_track(Arc::clone(&source));
        if let (Some(label), Some(session)) = (label, &mut self.session)
            && let Some(track) = session
                .tracks
                .iter_mut()
                .find(|track| track.source == source)
        {
            track.name = label;
        }
        task
    }

    fn open_hub_genome(&mut self, index: usize) -> Task<Message> {
        let Some(hub) = self.shown_hub() else {
            return Task::none();
        };
        let Some(chosen) = hub.genomes.get(index) else {
            return Task::none();
        };
        let (location, sequence) = (hub.location.clone(), chosen.sequence.clone());
        let known = genome::find(&chosen.name);
        let task = match (sequence, known) {
            (Some(sequence), _) => {
                self.close_menus();
                self.open(sequence)
            }
            (None, Some(known)) => self.open_genome(&known.id),
            (None, None) => return Task::none(),
        };
        self.hub_next = Some(location);
        task
    }

    /// Forgets the hubs of the last reference, and reads again a hub that waits for this one.
    pub(super) fn reopen_hubs(&mut self) -> Task<Message> {
        self.hubs.clear();
        self.hub_view = None;
        self.hub_adding.clear();
        match self.hub_next.take() {
            Some(location) => self.open_hub(location, true),
            None => Task::none(),
        }
    }

    fn hub_state(&self, data: &Location) -> State {
        let added = self.session.as_ref().is_some_and(|session| {
            session
                .tracks
                .iter()
                .any(|track| track.source.location == *data)
        });
        if added {
            State::Added
        } else if self.hub_adding.contains(data) {
            State::Opening
        } else {
            State::Off
        }
    }

    pub(super) fn hub_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Hub) {
            return None;
        }
        let view = self.hub_view.as_ref()?;
        let hub = self.hubs.get(view.hub)?;
        let words: Vec<String> = view
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let title = row![
            text(hub.short.clone()).font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let mut head = Column::new().spacing(2).push(title);
        if hub.long != hub.short {
            head = head.push(text(hub.long.clone()).size(11).style(style::muted));
        }
        let (about, placeholder, list, note) = if let Some(chosen) = hub.genome() {
            let mut about = format!("Tracks for {}", chosen.name);
            if !chosen.organism.is_empty() {
                about = format!("{about} ({})", chosen.organism);
            }
            let state = |data: &Location| self.hub_state(data);
            let (list, note) = track_list(hub, &words, &state);
            (about, "Find a track", list, note)
        } else {
            let about = match &self.session {
                Some(session) => format!(
                    "This hub has no tracks for {}. Open one of its genomes.",
                    session.reference.label()
                ),
                None => "Open a genome of this hub to see its tracks.".to_owned(),
            };
            let (list, note) = genome_list(hub, &words);
            (about, "Find a genome", list, note)
        };
        head = head.push(text(about).size(11).style(style::muted));
        let input = text_input(placeholder, &view.query)
            .id(SEARCH)
            .size(12)
            .padding([4, 8])
            .style(style::locus)
            .on_input(|query| Message::Hub(Hubbing::Query(query)));
        let most = (self.window.height - 260.0).clamp(160.0, 560.0);
        let height = list.height.clamp(LINE, most);
        let body = scrollable(Column::with_children(list.rows).spacing(1))
            .direction(Direction::Vertical(
                Scrollbar::new().width(6).scroller_width(6).margin(0),
            ))
            .style(style::scrollbar)
            .height(height);
        let card = container(
            column![head, input, body, text(note).size(11).style(style::muted)].spacing(8),
        )
        .id("hub-dialog")
        .width(WIDTH)
        .padding(16)
        .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

fn track_list<'a>(
    hub: &Hub,
    words: &[String],
    state: &dyn Fn(&Location) -> State,
) -> (List<'a>, String) {
    let searching = !words.is_empty();
    let found: Vec<(usize, &Entry)> = hub
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, entry)| !searching || (!entry.container && entry.matches(words)))
        .collect();
    let total = found.iter().filter(|(_, entry)| !entry.container).count();
    let mut list = List::default();
    for (index, entry) in found.into_iter().take(ROWS) {
        if entry.container {
            list.push(heading(entry), HEADING);
        } else {
            let state = entry.data.as_ref().map_or(State::Off, state);
            let (row, height) = track_row(index, entry, searching, state);
            list.push(row, height);
        }
    }
    let note = if total > ROWS {
        format!("genie shows {ROWS} of {total} tracks. Type words to find a track.")
    } else if total == 0 && searching {
        "No track has these words.".to_owned()
    } else if total == 0 {
        "This hub has no tracks for this genome.".to_owned()
    } else {
        HINT.to_owned()
    };
    (list, note)
}

fn heading<'a>(entry: &Entry) -> Element<'a, Message> {
    container(
        text(entry.short.clone())
            .size(11)
            .font(font::UI_STRONG)
            .style(style::muted)
            .wrapping(text::Wrapping::None),
    )
    .padding(Padding {
        top: 6.0,
        bottom: 2.0,
        left: 8.0 + entry.depth as f32 * INDENT,
        right: 8.0,
    })
    .clip(true)
    .into()
}

fn track_row<'a>(
    index: usize,
    entry: &Entry,
    searching: bool,
    state: State,
) -> (Element<'a, Message>, f32) {
    let depth = if searching { 0 } else { entry.depth };
    let (status, enabled) = match (&entry.data, entry.typed(), state) {
        (None, _, _) => ("This track has no file.".to_owned(), false),
        (_, None, _) if entry.kind.is_empty() => ("This track has no type.".to_owned(), false),
        (_, None, _) => (format!("genie cannot show {} tracks.", entry.kind), false),
        (_, _, State::Added) => ("Added".to_owned(), true),
        (_, _, State::Opening) => ("Opening…".to_owned(), false),
        (_, _, State::Off) => (entry.kind.clone(), true),
    };
    let title = row![
        text(entry.short.clone())
            .size(12)
            .wrapping(text::Wrapping::None),
        space::horizontal(),
        text(status)
            .size(11)
            .style(style::muted)
            .wrapping(text::Wrapping::None)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let about = if searching {
        Some(match &entry.group {
            Some(group) => format!("{group} · {}", entry.long),
            None => entry.long.clone(),
        })
    } else {
        (entry.long != entry.short).then(|| entry.long.clone())
    };
    let height = if about.is_some() { DOUBLE } else { LINE };
    let lines = column![title].spacing(1).push(about.map(|about| {
        text(about)
            .size(11)
            .style(style::muted)
            .wrapping(text::Wrapping::None)
    }));
    let row = button(lines)
        .width(Length::Fill)
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: 8.0 + depth as f32 * INDENT,
            right: 8.0,
        })
        .clip(true)
        .style(if state == State::Added {
            style::chosen
        } else {
            style::ghost
        })
        .on_press_maybe(enabled.then_some(Message::Hub(Hubbing::Toggle(index))));
    (row.into(), height)
}

fn genome_list<'a>(hub: &Hub, words: &[String]) -> (List<'a>, String) {
    let known = genome::all();
    let mut list = List::default();
    let found: Vec<(usize, &hub::Genome)> = hub
        .genomes
        .iter()
        .enumerate()
        .filter(|(_, genome)| {
            let words_of = format!("{} {} {}", genome.name, genome.organism, genome.description)
                .to_lowercase();
            words.iter().all(|word| words_of.contains(word.as_str()))
        })
        .collect();
    let total = found.len();
    for (index, genome) in found.into_iter().take(GENOMES) {
        let named = known.iter().find(|known| known.id == genome.name);
        let label = named.map_or_else(|| genome.name.clone(), |known| known.label.clone());
        let openable = genome.sequence.is_some() || named.is_some();
        let status = if openable {
            "Open"
        } else {
            "genie cannot open this genome."
        };
        let about: Vec<&str> = [genome.organism.as_str(), genome.description.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
        let title = row![
            text(label).size(12).wrapping(text::Wrapping::None),
            space::horizontal(),
            text(status)
                .size(11)
                .style(style::muted)
                .wrapping(text::Wrapping::None)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let height = if about.is_empty() { LINE } else { DOUBLE };
        let lines = column![title].spacing(1).push((!about.is_empty()).then(|| {
            text(about.join(" · "))
                .size(11)
                .style(style::muted)
                .wrapping(text::Wrapping::None)
        }));
        let row = button(lines)
            .width(Length::Fill)
            .padding([3, 8])
            .clip(true)
            .style(style::ghost)
            .on_press_maybe(openable.then_some(Message::Hub(Hubbing::Genome(index))));
        list.push(row, height);
    }
    let note = if total > GENOMES {
        format!("genie shows {GENOMES} of {total} genomes. Type words to find a genome.")
    } else if total == 0 {
        "No genome has these words.".to_owned()
    } else {
        "Click a genome to open it with the tracks of this hub.".to_owned()
    };
    (list, note)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_formats::fai::FaiRecord;
    use iced_test::simulator;

    use super::Hubbing;
    use crate::app::{Address, Genie, Message, Paletting};
    use crate::hub::{self, Wanted};
    use crate::location::Location;
    use crate::reference::Reference;
    use crate::scratch::Scratch;
    use crate::shortcuts::Command;
    use crate::track::{self, Typed};

    const HUB: &str = "hub mine\nshortLabel My hub\nlongLabel My long hub\n\
        genomesFile genomes.txt\nemail a@b.c\n";
    const TRACK_DB: &str = "track genes\ntype bigGenePred\nbigDataUrl data/genes\n\
        shortLabel Genes\nlongLabel Genes of the test\n\n\
        track wave\ntype bigWig\nbigDataUrl wave.bw\nshortLabel Wave\n\n\
        track contacts\ntype hic\nbigDataUrl contacts.hic\nshortLabel Contacts\n";

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data")
            .join(name)
    }

    fn local_hub(scratch: &Scratch) -> Location {
        let file = scratch.file("hub.txt", HUB);
        let folder = file.parent().unwrap().to_owned();
        fs::create_dir_all(folder.join("ref/data")).unwrap();
        fs::write(
            folder.join("genomes.txt"),
            "genome ref\ntrackDb ref/trackDb.txt\n",
        )
        .unwrap();
        fs::write(folder.join("ref/trackDb.txt"), TRACK_DB).unwrap();
        fs::copy(fixture("genes.bb"), folder.join("ref/data/genes")).unwrap();
        fs::copy(fixture("signal.bw"), folder.join("ref/wave.bw")).unwrap();
        Location::File(file)
    }

    fn reference(path: PathBuf) -> Arc<Reference> {
        let records = vec![FaiRecord {
            name: "chr1".to_owned(),
            length: 3_000_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        Arc::new(Reference::fasta(path, records).unwrap())
    }

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    fn load(genie: &mut Genie, location: &Location, wanted: Wanted<'_>) {
        let loaded = hub::load(location, wanted).map(Arc::new);
        let message = Hubbing::Loaded(location.clone(), true, loaded);
        let _ = genie.update(Message::Hub(message));
    }

    #[test]
    fn hub_tracks_come_and_go_with_a_click() {
        let scratch = Scratch::new("app-hub-tracks");
        let location = local_hub(&scratch);
        let mut genie = Genie::default();
        let fasta = PathBuf::from("ref.fa");
        let _ = genie.update(Message::Opened(Ok(reference(fasta.clone()))));
        let _ = genie.update(Message::Reopen(location.clone()));
        assert_eq!(genie.busy, 1);
        let open = Location::File(fasta);
        let wanted = Wanted {
            genome: None,
            reference: Some(&open),
        };
        load(&mut genie, &location, wanted);
        assert_eq!(genie.busy, 0);
        for text in [
            "My hub",
            "My long hub",
            "Tracks for ref",
            "Genes of the test",
            "bigWig",
            "genie cannot show hic tracks.",
        ] {
            assert!(shows(&genie, text), "{text}");
        }
        let mut ui = simulator(genie.view());
        ui.click("Genes").unwrap();
        let toggle = Message::Hub(Hubbing::Toggle(0));
        assert!(ui.into_messages().eq([toggle.clone()]));
        let _ = genie.update(toggle.clone());
        assert!(shows(&genie, "Opening…"));
        let data = genie.hubs[0].tracks[0].data.clone().unwrap();
        let source = track::open_typed(data.clone(), Typed::Big, None).map(Arc::new);
        let _ = genie.update(Message::Hub(Hubbing::Added(data, source)));
        let tracks = &genie.session.as_ref().unwrap().tracks;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Genes");
        assert!(shows(&genie, "Added"));
        let _ = genie.update(toggle);
        assert!(genie.session.as_ref().unwrap().tracks.is_empty());
        let _ = genie.update(Message::Hub(Hubbing::Query("WAVE".to_owned())));
        assert!(shows(&genie, "Wave"));
        assert!(!shows(&genie, "Genes"));
        let _ = genie.update(Message::Hub(Hubbing::Query("nothing".to_owned())));
        assert!(shows(&genie, "No track has these words."));
    }

    #[test]
    fn the_tracks_menu_asks_for_hubs_and_lists_them() {
        let scratch = Scratch::new("app-hub-menu");
        let location = local_hub(&scratch);
        let mut genie = Genie::default();
        let fasta = PathBuf::from("ref.fa");
        let _ = genie.update(Message::Opened(Ok(reference(fasta.clone()))));
        let _ = genie.update(Message::CatalogMenu);
        assert!(shows(&genie, "Track hubs"));
        let mut ui = simulator(genie.view());
        ui.click("Open a track hub…").unwrap();
        let ask = Message::Hub(Hubbing::Ask);
        assert!(ui.into_messages().any(|message| message == ask));
        let _ = genie.update(ask);
        assert!(shows(&genie, "Open a track hub from the web"));
        let address = "https://example.org/x/hub.txt".to_owned();
        let _ = genie.update(Message::Address(Address::Edit(address)));
        let _ = genie.update(Message::Address(Address::Submit));
        assert_eq!(genie.busy, 1);
        assert_eq!(genie.address, None);
        let open = Location::File(fasta);
        let wanted = Wanted {
            genome: None,
            reference: Some(&open),
        };
        load(&mut genie, &location, wanted);
        let _ = genie.update(Message::MenuDismissed);
        let _ = genie.update(Message::CatalogMenu);
        let mut ui = simulator(genie.view());
        ui.click("My hub").unwrap();
        let show = Message::Hub(Hubbing::Show(0));
        assert!(ui.into_messages().any(|message| message == show));
        let _ = genie.update(show);
        assert!(shows(&genie, "Tracks for ref"));
        let _ = genie.update(Message::Command(Command::Palette));
        let _ = genie.update(Message::Palette(Paletting::Query("hub".to_owned())));
        assert!(shows(&genie, "Open a track hub…"));
        assert!(shows(&genie, "Show the hub My hub"));
    }

    #[test]
    fn assembly_hubs_open_their_genome_then_the_hub_again() {
        let text = "hub asm\nshortLabel Assembly\nuseOneFile on\nemail a@b.c\n\n\
            genome myAsm\ntwoBitPath myAsm.2bit\norganism Fly\n\n\
            track genes\ntype bigBed\nbigDataUrl genes.bb\nshortLabel Genes\n";
        let scratch = Scratch::new("app-hub-assembly");
        let location = Location::File(scratch.file("hub.txt", text));
        let mut genie = Genie::default();
        let _ = genie.update(Message::Reopen(location.clone()));
        assert!(shows(&genie, "Opening hub.txt…"));
        load(&mut genie, &location, Wanted::default());
        assert!(shows(
            &genie,
            "Open a genome of this hub to see its tracks."
        ));
        assert!(shows(&genie, "myAsm"));
        assert!(shows(&genie, "Fly"));
        let _ = genie.update(Message::Hub(Hubbing::Genome(0)));
        assert_eq!(genie.hub_next.as_ref(), Some(&location));
        assert!(shows(&genie, "Opening myAsm.2bit…"));
        let Some(Location::File(sequence)) = genie.hubs[0].genomes[0].sequence.clone() else {
            panic!("the genome has a sequence on this computer");
        };
        let _ = genie.update(Message::Opened(Ok(reference(sequence))));
        assert_eq!(genie.hub_next, None);
        assert_eq!(genie.hubs.len(), 0);
        assert_eq!(genie.busy, 1);
    }

    #[test]
    fn hubs_without_the_open_genome_offer_their_genomes() {
        let scratch = Scratch::new("app-hub-other");
        let location = local_hub(&scratch);
        let mut genie = Genie::default();
        let fasta = PathBuf::from("other.fa");
        let _ = genie.update(Message::Opened(Ok(reference(fasta.clone()))));
        let open = Location::File(fasta);
        let wanted = Wanted {
            genome: None,
            reference: Some(&open),
        };
        load(&mut genie, &location, wanted);
        assert!(shows(
            &genie,
            "This hub has no tracks for other.fa. Open one of its genomes."
        ));
        assert!(shows(&genie, "genie cannot open this genome."));
        let mut ui = simulator(genie.view());
        ui.click("ref").unwrap();
        let mut messages = ui.into_messages();
        assert!(!messages.any(|message| matches!(message, Message::Hub(_))));
    }
}
