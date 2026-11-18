//! BLAT: genie sends a sequence to the BLAT server of UCSC, writes the alignments to a PSL file in
//! its cache folder, opens the file as a track and lists the alignments in a dialog.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use genie_core::alignment::Op;
use genie_core::{Locus, Region, Span, Strand, Target, grouped};
use genie_formats::blat;
use genie_formats::http::Url;
use genie_formats::ini::{self, Section};
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, opaque, operation, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::card::Detail;
use crate::icon::{self, Icon};
use crate::location::Location;
use crate::{export, font, genome, hub, paths, settings, style, update};

use super::{Genie, Message, Popup};

const FIELD: &str = "blat-sequence";
const WIDTH: f32 = 560.0;
const LIST_HEIGHT: f32 = 300.0;
/// UCSC asks programs to send at most one BLAT search in this time.
const GAP: Duration = Duration::from_secs(15);
/// The folder in the cache that keeps the PSL files of the searches.
const FOLDER: &str = "blat";
const NO_GENOME: &str = "BLAT needs a genome of UCSC, such as hg38. Open one in the start view.";
const SEARCHING: &str = "Asking the BLAT server of UCSC…";
const NONE: &str = "BLAT found no alignment.";
const ABOUT: &str = "genie sends the bases to the BLAT server of UCSC.";
const KEY_FIELD: &str = "blat-key";
/// The note of the dialog without a key.
const KEY_NEEDED: &str = "UCSC takes BLAT searches from programs only with your UCSC API key. To make one, log in at genome.ucsc.edu and open the Hub Development page.";
/// The note when UCSC answers with its check for people, not with alignments.
const CHECKED: &str = "UCSC did not take the API key. Check the key, or search on the UCSC page.";
const PAGE: &str = "Search on the UCSC page";
/// The section of the settings file that keeps the API key of UCSC.
const SECTION: &str = "ucsc";
const KEY: &str = "api key";

#[derive(Debug, Clone, PartialEq)]
pub enum Blatting {
    /// Shows the dialog with a query, or with the last one.
    Open(Option<Query>),
    /// Shows the dialog with the read of the pinned card.
    Read,
    Edit(String),
    /// Puts the clipped part of the read in the field.
    Clip,
    /// The API key of UCSC. genie keeps it in its settings file.
    Key(String),
    Submit,
    /// Opens the same search on the BLAT page of UCSC in the web browser.
    Browse,
    /// The PSL file of the alignments and its text.
    Done(Result<(PathBuf, String), String>),
    /// Goes to an alignment of the list.
    Go(usize),
}

/// A sequence to BLAT: its name, its bases, and the longest clipped part of a read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub name: String,
    pub bases: String,
    pub clip: Option<String>,
}

/// An alignment in the list of the dialog.
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    contig: String,
    span: Region,
    strand: Strand,
    identity: String,
    score: i64,
}

#[derive(Debug, Default)]
pub(super) struct Blat {
    name: String,
    text: String,
    clip: Option<String>,
    /// The API key of UCSC, from the settings file.
    key: String,
    searching: bool,
    error: Option<String>,
    hits: Vec<Hit>,
    /// When genie last asked UCSC.
    last: Option<Instant>,
}

impl Blat {
    pub(super) fn keyed(key: String) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }
}

/// The API key of UCSC in the settings file of a folder, or an empty text.
pub(super) fn load_key(folder: &Path) -> String {
    fs::read_to_string(folder.join(settings::FILE))
        .ok()
        .and_then(|text| ini::parse(&text).ok())
        .and_then(|document| Some(document.section(SECTION)?.value(KEY)?.trim().to_owned()))
        .unwrap_or_default()
}

/// Saves the key in the settings file, away from the work of the window.
fn remember_key(key: &str) -> Task<Message> {
    let Some(folder) = paths::config_dir() else {
        return Task::none();
    };
    let key = key.to_owned();
    Task::future(async move {
        let _ = save_key(&folder, &key);
    })
    .discard()
}

fn save_key(folder: &Path, key: &str) -> io::Result<()> {
    settings::store(
        folder,
        Section {
            name: SECTION.to_owned(),
            entries: vec![(KEY.to_owned(), key.trim().to_owned())],
        },
    )
}

/// The longest soft-clipped part of a read, if it has enough bases for BLAT.
fn clipped(read: &genie_core::alignment::Alignment) -> Option<String> {
    let length = |op: Option<&(Op, u32)>| match op {
        Some((Op::SoftClip, length)) => *length as usize,
        _ => 0,
    };
    let (start, end) = (length(read.cigar.first()), length(read.cigar.last()));
    let total = read.bases.len();
    let part = if start >= end {
        read.bases.get(..start)?
    } else {
        read.bases.get(total.checked_sub(end)?..)?
    };
    (part.len() >= blat::FEWEST).then(|| String::from_utf8_lossy(part).into_owned())
}

/// A short code of bases, so that each sequence has a file of its own.
fn code(bases: &str) -> String {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in bases.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// Asks a BLAT server with an API key for the alignments of bases in a genome, and writes them
/// to a PSL file in a folder. Gives the file and its lines.
pub(super) fn search(
    server: &str,
    genome: &str,
    (name, bases): (&str, &str),
    (key, folder): (Option<&str>, Option<&Path>),
) -> Result<(PathBuf, String), String> {
    let folder = folder
        .ok_or_else(|| "genie has no cache folder for the alignments.".to_owned())?
        .join(FOLDER);
    let address = blat::address(server, genome, bases, key);
    let url = Url::parse(&address).map_err(|error| error.to_string())?;
    let answer = hub::read_text(&Location::Url(url))
        .map_err(|error| format!("genie cannot reach BLAT. {error}"))?;
    if blat::is_page(&answer) {
        return Err(CHECKED.to_owned());
    }
    let text = blat::psl(&answer, name)?;
    let path = folder.join(format!("BLAT {} {}.psl", export::safe(name), code(bases)));
    fs::create_dir_all(&folder)
        .and_then(|()| fs::write(&path, format!("{text}\n")))
        .map_err(|error| format!("genie cannot save the alignments. {error}"))?;
    Ok((path, text))
}

/// The alignments of the lines of a PSL file, the best score first.
fn hits(text: &str) -> Vec<Hit> {
    let mut found: Vec<Hit> = text
        .lines()
        .filter_map(|line| {
            let (contig, feature) = genie_formats::psl::parse(line).ok().flatten()?;
            let fields = genie_formats::psl::fields(line);
            let value = |label: &str| {
                fields
                    .iter()
                    .find(|field| field.label == label)
                    .map(|field| field.value.clone())
            };
            Some(Hit {
                contig: contig.to_owned(),
                span: feature.span(),
                strand: feature.strand(),
                identity: value("Identity (%)").unwrap_or_default(),
                score: value("Score")?.parse().ok()?,
            })
        })
        .collect();
    found.sort_by_key(|hit| std::cmp::Reverse(hit.score));
    found
}

impl Genie {
    pub(super) fn blatting(&mut self, blatting: Blatting) -> Task<Message> {
        match blatting {
            Blatting::Open(query) => return self.open_blat(query),
            Blatting::Read => {
                let Some(Detail::Read { read, .. }) = self.pinned.as_ref().map(|card| &card.detail)
                else {
                    return Task::none();
                };
                let query = Query {
                    name: read.name.clone(),
                    bases: String::from_utf8_lossy(&read.bases).into_owned(),
                    clip: clipped(read),
                };
                return self.open_blat(Some(query));
            }
            Blatting::Edit(text) => {
                self.blat.text = text;
                self.blat.error = None;
            }
            Blatting::Clip => {
                if let Some(clip) = self.blat.clip.take() {
                    self.blat.text = clip;
                    self.blat.name = format!("{} clip", self.blat.name);
                }
            }
            Blatting::Key(key) => {
                self.blat.key = key;
                self.blat.error = None;
                return remember_key(&self.blat.key);
            }
            Blatting::Submit => return self.submit_blat(),
            Blatting::Browse => self.browse_blat(),
            Blatting::Done(result) => return self.blat_done(result),
            Blatting::Go(index) => return self.go_to_hit(index),
        }
        Task::none()
    }

    fn open_blat(&mut self, query: Option<Query>) -> Task<Message> {
        self.close_menus();
        self.popup = Some(Popup::Blat);
        if let Some(query) = query {
            self.blat.name = query.name;
            self.blat.text = query.bases;
            self.blat.clip = query.clip;
            self.blat.hits.clear();
            self.blat.error = None;
        }
        operation::focus(FIELD)
    }

    /// The genome of UCSC in view and the bases of the field, or None with the reason in the
    /// dialog.
    fn blat_input(&mut self) -> Option<(String, String)> {
        let genome = self
            .session
            .as_ref()
            .and_then(|session| session.reference.genome.clone())
            .filter(|genome| genome::find(genome).is_some());
        let Some(genome) = genome else {
            self.blat.error = Some(NO_GENOME.to_owned());
            return None;
        };
        match blat::bases(&self.blat.text) {
            Ok(bases) => Some((genome, bases)),
            Err(error) => {
                self.blat.error = Some(error);
                None
            }
        }
    }

    fn browse_blat(&mut self) {
        let Some((genome, bases)) = self.blat_input() else {
            return;
        };
        if let Err(error) = update::browse(&blat::page(blat::SERVER, &genome, &bases)) {
            self.blat.error = Some(format!("genie cannot open the web browser. {error}"));
        }
    }

    fn submit_blat(&mut self) -> Task<Message> {
        let Some((genome, bases)) = self.blat_input() else {
            return Task::none();
        };
        let key = self.blat.key.trim().to_owned();
        if key.is_empty() {
            return Task::none();
        }
        let now = Instant::now();
        if let Some(last) = self.blat.last {
            let waited = now.saturating_duration_since(last);
            if waited < GAP {
                let seconds = GAP.saturating_sub(waited).as_secs_f32().ceil();
                self.blat.error = Some(format!(
                    "UCSC asks for at most one BLAT search each 15 seconds. Try again in {seconds} seconds."
                ));
                return Task::none();
            }
        }
        self.blat.last = Some(now);
        self.blat.searching = true;
        self.blat.error = None;
        self.blat.hits.clear();
        let name = if self.blat.name.trim().is_empty() {
            "Sequence".to_owned()
        } else {
            self.blat.name.clone()
        };
        let cache = self.cache.clone();
        Task::perform(
            async move {
                let place = (Some(key.as_str()), cache.as_deref());
                search(blat::SERVER, &genome, (&name, &bases), place)
            },
            |result| Message::Blat(Blatting::Done(result)),
        )
    }

    fn blat_done(&mut self, result: Result<(PathBuf, String), String>) -> Task<Message> {
        self.blat.searching = false;
        if self.popup.is_none() {
            self.popup = Some(Popup::Blat);
        }
        let (path, text) = match result {
            Ok(found) => found,
            Err(error) => {
                self.blat.error = Some(error);
                return Task::none();
            }
        };
        self.blat.hits = hits(&text);
        if self.blat.hits.is_empty() {
            self.blat.error = Some(NONE.to_owned());
            return Task::none();
        }
        let location = Location::File(path);
        let open = self.session.as_ref().is_some_and(|session| {
            session
                .tracks
                .iter()
                .any(|track| track.source.location == location)
        });
        if open {
            Task::none()
        } else {
            self.open_track(location)
        }
    }

    fn go_to_hit(&mut self, index: usize) -> Task<Message> {
        let Some(hit) = self.blat.hits.get(index) else {
            return Task::none();
        };
        let Some(contig) = self
            .session
            .as_ref()
            .and_then(|session| session.reference.assembly.find(&hit.contig))
        else {
            return Task::none();
        };
        let span = hit.span;
        self.close_menus();
        self.go_to(Target {
            contig,
            span: Span::Region(span),
        })
    }

    fn hit_rows(&self) -> Vec<Element<'_, Message>> {
        self.blat
            .hits
            .iter()
            .enumerate()
            .map(|(index, hit)| {
                let strand = if hit.strand == Strand::Reverse {
                    "−"
                } else {
                    "+"
                };
                let locus = Locus {
                    name: hit.contig.clone(),
                    span: Span::Region(hit.span),
                };
                let place = format!("{locus} ({strand})");
                let about = format!(
                    "{} % · score {} · {} bp",
                    hit.identity,
                    hit.score,
                    grouped(hit.span.len())
                );
                let content = row![
                    text(place).size(12).font(font::MONO),
                    space::horizontal(),
                    text(about).size(11).style(style::muted)
                ]
                .align_y(Alignment::Center);
                button(content)
                    .width(Length::Fill)
                    .padding([3, 8])
                    .style(style::ghost)
                    .on_press(Message::Blat(Blatting::Go(index)))
                    .into()
            })
            .collect()
    }

    pub(super) fn blat_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Blat) {
            return None;
        }
        let state = &self.blat;
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let heading = row![
            text("BLAT a sequence").font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let field = text_input("Bases or FASTA, 20 to 8,000 bases", &state.text)
            .id(FIELD)
            .font(font::MONO)
            .size(12)
            .padding([3, 8])
            .style(style::locus)
            .on_input(|text| Message::Blat(Blatting::Edit(text)))
            .on_submit(Message::Blat(Blatting::Submit));
        let keyless = state.key.trim().is_empty();
        let key = row![
            text("UCSC API key").size(11).style(style::muted),
            text_input("Paste your key", &state.key)
                .id(KEY_FIELD)
                .secure(true)
                .size(12)
                .padding([3, 8])
                .style(style::locus)
                .on_input(|key| Message::Blat(Blatting::Key(key)))
                .on_submit(Message::Blat(Blatting::Submit))
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let ready = !state.searching && !keyless;
        let search = button(text("BLAT").size(12))
            .padding([4, 12])
            .style(style::primary)
            .on_press_maybe(ready.then_some(Message::Blat(Blatting::Submit)));
        let mut actions = row![].spacing(6).align_y(Alignment::Center);
        if let Some(clip) = &state.clip {
            let words = format!(
                "Use the clipped part ({} bases)",
                grouped(clip.len() as u64)
            );
            actions = actions.push(
                button(text(words).size(12))
                    .padding([4, 8])
                    .style(style::ghost)
                    .on_press(Message::Blat(Blatting::Clip)),
            );
        }
        actions = actions.push(space::horizontal());
        if keyless || state.error.as_deref() == Some(CHECKED) {
            actions = actions.push(
                button(text(PAGE).size(12))
                    .padding([4, 8])
                    .style(style::ghost)
                    .on_press(Message::Blat(Blatting::Browse)),
            );
        }
        actions = actions.push(search);
        let note = state
            .error
            .as_deref()
            .or_else(|| state.searching.then_some(SEARCHING))
            .or_else(|| keyless.then_some(KEY_NEEDED))
            .unwrap_or(ABOUT);
        let mut body = column![
            heading,
            field,
            key,
            actions,
            text(note).size(11).style(style::muted)
        ]
        .spacing(10);
        let rows = self.hit_rows();
        if !rows.is_empty() {
            let list = scrollable(Column::with_children(rows).spacing(1))
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0),
                ))
                .style(style::scrollbar)
                .height(Length::Shrink);
            body = body.push(container(list).max_height(LIST_HEIGHT));
        }
        let card = container(body)
            .id("blat-dialog")
            .width(WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::alignment::{Alignment, Op, Tags};
    use iced::time::Instant;
    use iced_test::simulator;

    use super::{Blatting, Query, clipped, search};
    use crate::app::{Genie, Message};
    use crate::card::{Card, Changes, Detail};
    use crate::scratch::Scratch;

    const ANSWER: &str = r#"{"track": "blat", "genome": "hg38",
        "fields": ["matches", "misMatches", "repMatches", "nCount", "qNumInsert", "qBaseInsert",
            "tNumInsert", "tBaseInsert", "strand", "qName", "qSize", "qStart", "qEnd", "tName",
            "tSize", "tStart", "tEnd", "blockCount", "blockSizes", "qStarts", "tStarts"],
        "blat": [
            [24, 0, 0, 0, 0, 0, 0, 0, "+", "YourSeq", 24, 0, 24, "chr2", 1000, 100, 124, 1,
             "24,", "0,", "100,"],
            [20, 4, 0, 0, 0, 0, 0, 0, "-", "YourSeq", 24, 0, 24, "chr1", 1000, 500, 524, 1,
             "24,", "0,", "500,"]
        ]}"#;

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    fn read(cigar: Vec<(Op, u32)>, bases: &str) -> Alignment {
        Alignment {
            name: "read7".to_owned(),
            start: 100,
            flags: 0,
            mapq: 60,
            cigar,
            bases: bases.as_bytes().to_vec(),
            qualities: Vec::new(),
            mate: None,
            tags: Tags::default(),
        }
    }

    #[test]
    fn clipped_parts_need_enough_bases() {
        let bases = format!("{}{}{}", "C".repeat(5), "A".repeat(30), "G".repeat(25));
        let cigar = vec![(Op::SoftClip, 5), (Op::Match, 30), (Op::SoftClip, 25)];
        assert_eq!(clipped(&read(cigar, &bases)), Some("G".repeat(25)));
        let short = vec![(Op::SoftClip, 5), (Op::Match, 55)];
        assert_eq!(clipped(&read(short, &bases)), None);
    }

    #[test]
    fn searches_write_the_alignments_to_a_psl_file() {
        let server = crate::server::serve(vec![(
            "/cgi-bin/hgBlat".to_owned(),
            ANSWER.as_bytes().to_vec(),
        )]);
        let scratch = Scratch::new("blat-search");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        let (path, text) = search(
            &server.url("/cgi-bin/hgBlat"),
            "hg38",
            ("my read", &"ACGT".repeat(6)),
            (Some("key1"), Some(&folder)),
        )
        .unwrap();
        assert!(path.starts_with(folder.join("blat")));
        assert!(path.extension().is_some_and(|extension| extension == "psl"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{text}\n"));
        assert!(text.contains("\tmy_read\t"), "{text}");
        let missing = search(
            &server.url("/none"),
            "hg38",
            ("q", "ACGT"),
            (None, Some(&folder)),
        );
        let error = missing.unwrap_err();
        assert!(error.starts_with("genie cannot reach BLAT."), "{error}");
    }

    /// UCSC answers a search without a valid key with a web page that checks for people.
    #[test]
    fn searches_that_ucsc_checks_ask_for_the_key() {
        let page = b"<html><h4>The Genome Browser is protecting itself from bots.</h4></html>";
        let server = crate::server::serve(vec![("/cgi-bin/hgBlat".to_owned(), page.to_vec())]);
        let scratch = Scratch::new("blat-checked");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        let bases = "ACGT".repeat(6);
        let checked = search(
            &server.url("/cgi-bin/hgBlat"),
            "hg38",
            ("q", &bases),
            (Some("old"), Some(&folder)),
        );
        assert_eq!(checked.unwrap_err(), super::CHECKED);
    }

    #[test]
    fn the_key_stays_in_the_settings_file() {
        let scratch = Scratch::new("blat-key");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        assert_eq!(super::load_key(&folder), "");
        crate::settings::Settings::default().save(&folder).unwrap();
        super::save_key(&folder, " a1b2 ").unwrap();
        assert_eq!(super::load_key(&folder), "a1b2");
        assert_eq!(
            crate::settings::Settings::load(&folder),
            crate::settings::Settings::default()
        );
    }

    #[test]
    fn without_a_key_the_dialog_offers_the_ucsc_page() {
        let mut genie = opened(Some("hg38"));
        let _ = genie.update(Message::Blat(Blatting::Open(None)));
        let _ = genie.update(Message::Blat(Blatting::Edit("ACGT".repeat(6))));
        assert!(shows(&genie, super::KEY_NEEDED));
        let mut ui = simulator(genie.view());
        ui.click("Search on the UCSC page").unwrap();
        let browse = Message::Blat(Blatting::Browse);
        assert!(ui.into_messages().any(|message| message == browse));
        let _ = genie.update(Message::Blat(Blatting::Submit));
        assert!(!genie.blat.searching);
        let _ = genie.update(Message::Blat(Blatting::Key("key1".to_owned())));
        assert!(!shows(&genie, "Search on the UCSC page"));
        let _ = genie.update(Message::Blat(Blatting::Submit));
        assert!(genie.blat.searching);
        let refused = Blatting::Done(Err(super::CHECKED.to_owned()));
        let _ = genie.update(Message::Blat(refused));
        assert!(shows(&genie, super::CHECKED));
        assert!(shows(&genie, "Search on the UCSC page"));
    }

    /// A genie with chr1 and chr2 of 1,000 bases, from a genome of UCSC or from a file.
    fn opened(genome: Option<&str>) -> Genie {
        let mut genie = Genie::default();
        let mut reference = crate::reference::Reference::fasta(
            PathBuf::from("ref.fa"),
            ["chr1", "chr2"]
                .iter()
                .enumerate()
                .map(|(index, name)| genie_formats::fai::FaiRecord {
                    name: (*name).to_owned(),
                    length: 1_000,
                    offset: 6 + index as u64 * 1_100,
                    line_bases: 1_000,
                    line_width: 1_001,
                })
                .collect(),
        )
        .unwrap();
        reference.genome = genome.map(str::to_owned);
        let _ = genie.update(Message::Opened(Ok(Arc::new(reference))));
        genie
    }

    #[test]
    fn the_dialog_needs_a_genome_of_ucsc_and_enough_bases() {
        let mut genie = opened(None);
        let _ = genie.update(Message::Blat(Blatting::Open(None)));
        assert!(shows(&genie, "BLAT a sequence"));
        let _ = genie.update(Message::Blat(Blatting::Edit("ACGT".repeat(6))));
        let _ = genie.update(Message::Blat(Blatting::Submit));
        assert!(shows(
            &genie,
            "BLAT needs a genome of UCSC, such as hg38. Open one in the start view."
        ));
        let mut human = opened(Some("hg38"));
        let _ = human.update(Message::Blat(Blatting::Key("key1".to_owned())));
        let _ = human.update(Message::Blat(Blatting::Open(None)));
        let _ = human.update(Message::Blat(Blatting::Edit("ACGT".to_owned())));
        let _ = human.update(Message::Blat(Blatting::Submit));
        assert!(shows(&human, "BLAT needs at least 20 bases."));
        assert!(!human.blat.searching);
        let _ = human.update(Message::Blat(Blatting::Edit("ACGT".repeat(6))));
        let _ = human.update(Message::Blat(Blatting::Submit));
        assert!(human.blat.searching);
        assert!(shows(&human, "Asking the BLAT server of UCSC…"));
    }

    #[test]
    fn pinned_reads_go_to_the_dialog_with_their_clip() {
        let mut genie = opened(Some("hg38"));
        let bases = format!("{}{}", "A".repeat(30), "G".repeat(25));
        let cigar = vec![(Op::Match, 30), (Op::SoftClip, 25)];
        genie.pinned = Some(Card {
            track: 0,
            contig: "chr1".to_owned(),
            samples: Vec::new(),
            detail: Detail::Read {
                read: read(cigar, &bases),
                mate: None,
                same: false,
                insertion: None,
            },
            at: iced::Point::ORIGIN,
            fields: Vec::new(),
            effects: Changes::default(),
        });
        assert!(shows(&genie, "BLAT"));
        let _ = genie.update(Message::Blat(Blatting::Read));
        assert_eq!(genie.blat.text, bases);
        assert!(shows(&genie, "Use the clipped part (25 bases)"));
        let _ = genie.update(Message::Blat(Blatting::Clip));
        assert_eq!(genie.blat.text, "G".repeat(25));
        let query = Query {
            name: "x".to_owned(),
            bases: "ACGT".to_owned(),
            clip: None,
        };
        let _ = genie.update(Message::Blat(Blatting::Open(Some(query))));
        assert!(!shows(&genie, "Use the clipped part (25 bases)"));
    }

    #[test]
    fn found_alignments_open_as_a_track_and_a_list() {
        let mut genie = opened(Some("hg38"));
        let _ = genie.update(Message::Blat(Blatting::Open(None)));
        let scratch = Scratch::new("blat-done");
        let text = genie_formats::blat::psl(ANSWER, "read7").unwrap();
        let path = scratch.file("BLAT read7 1.psl", format!("{text}\n"));
        let busy = genie.busy;
        let _ = genie.update(Message::Blat(Blatting::Done(Ok((path, text)))));
        assert!(shows(&genie, "chr2:101-124 (+)"));
        assert!(shows(&genie, "chr1:501-524 (−)"));
        assert!(shows(&genie, "100.0 % · score 24 · 24 bp"));
        assert_eq!(genie.busy, busy + 1);
        let _ = genie.update(Message::Blat(Blatting::Go(1)));
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.contig, 0);
        assert!(!shows(&genie, "BLAT a sequence"));
        let _ = genie.update(Message::Blat(Blatting::Done(Ok((
            PathBuf::from("none.psl"),
            String::new(),
        )))));
        assert!(shows(&genie, "BLAT found no alignment."));
    }

    /// One real search of UCSC with your API key in `GENIE_UCSC_API_KEY`: the first 60 bases of
    /// the human mitochondrial genome (rCRS) in hg38.
    #[test]
    #[ignore = "asks the BLAT server of UCSC with the key of GENIE_UCSC_API_KEY"]
    fn real_blat_on_hg38() {
        let Ok(key) = std::env::var("GENIE_UCSC_API_KEY") else {
            println!("Set GENIE_UCSC_API_KEY to your UCSC API key.");
            return;
        };
        let scratch = Scratch::new("blat-real");
        let folder = scratch.file("marker", "").parent().unwrap().to_owned();
        let bases = "GATCACAGGTCTATCACCCTATTAACCACTCACGGGAGCTCTCCATGCATTTGGTATTTT";
        let (_, text) = search(
            genie_formats::blat::SERVER,
            "hg38",
            ("chrM start", bases),
            (Some(&key), Some(&folder)),
        )
        .unwrap();
        println!("{text}");
        let found = super::hits(&text);
        let start = |hit: &super::Hit| hit.contig == "chrM" && hit.span.start() == 0;
        assert!(found.iter().any(start), "{found:?}");
    }

    #[test]
    fn searches_wait_fifteen_seconds_between_them() {
        let mut genie = opened(Some("hg38"));
        let _ = genie.update(Message::Blat(Blatting::Key("key1".to_owned())));
        genie.blat.last = Some(Instant::now());
        let _ = genie.update(Message::Blat(Blatting::Open(None)));
        let _ = genie.update(Message::Blat(Blatting::Edit("ACGT".repeat(6))));
        let _ = genie.update(Message::Blat(Blatting::Submit));
        assert!(shows(
            &genie,
            "UCSC asks for at most one BLAT search each 15 seconds. Try again in 15 seconds."
        ));
    }
}
