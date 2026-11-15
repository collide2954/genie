//! The sample table of a session: facts about the samples, such as population, sex and
//! phenotype. Tracks with a row for each sample group, sort and color their rows by the fields
//! of the table.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

use genie_core::grouped;
use genie_core::samples::{Arrangement, Table};
use genie_core::variant::{Call, Variant};
use genie_formats::gzip;
use iced::Task;

use super::tracks::NO_REFERENCE;
use super::{Genie, Message, Status};
use crate::location::Location;
use crate::track::{self, Track};

/// The largest sample table that genie reads.
const LIMIT: u64 = 64 << 20;
/// The file types that genie opens as sample tables, also when compressed.
const EXTENSIONS: [&str; 5] = ["tsv", "csv", "panel", "fam", "ped"];
/// The most groups that a variant card shows, before a line that counts the others.
const GROUP_LINES: usize = 10;

/// The sample table of a session and the file that it came from.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Samples {
    pub(super) location: Location,
    pub(super) table: Arc<Table>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sampling {
    /// A sample table that genie read, or why it could not read it.
    Opened(Location, Result<Arc<Table>, String>),
    /// Group, sort or color the samples of a track by a field, or stop when the field already
    /// does it.
    Arrange(u64, Arranging),
    /// Forget the sample table.
    Close,
    /// What a text file on the web is, after genie read its first lines.
    Checked(Location, Text),
}

/// What a `.txt` file is, from its first lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    Hub,
    Loops,
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arranging {
    Group(String),
    Sort(String),
    Color(String),
}

/// The type of a file without `.gz` or `.bgz`, in lower case.
fn extension(location: &Location) -> Option<String> {
    let name = location.name().to_ascii_lowercase();
    let stem = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".bgz"))
        .unwrap_or(&name);
    stem.rsplit_once('.')
        .map(|(_, extension)| extension.to_owned())
}

/// True for a file that genie opens as a sample table by its type: tab or comma separated
/// files and PLINK files.
pub(super) fn is_table(location: &Location) -> bool {
    extension(location).is_some_and(|extension| EXTENSIONS.contains(&extension.as_str()))
}

/// True for a `.txt` file: a track hub, a loop list or a sample table.
pub(super) fn is_text(location: &Location) -> bool {
    extension(location).is_some_and(|extension| extension == "txt")
}

/// What a text file is: a loop list when its first line is a loop list header, a track hub
/// when its first line that is not a comment starts with "hub", and else a sample table. A
/// file that genie cannot read is a hub, so that the hub says why it does not open.
pub(super) fn classify(location: &Location) -> Text {
    let head = track::head(location, 50);
    let mut lines = head
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty());
    let Some(first) = lines.clone().next() else {
        return Text::Hub;
    };
    if genie_formats::bedpe::is_loop_list(first) {
        return Text::Loops;
    }
    match lines.find(|line| !line.starts_with('#')) {
        Some(line) if !line.to_ascii_lowercase().starts_with("hub") => Text::Table,
        _ => Text::Hub,
    }
}

/// Reads a sample table from a file on this computer or on the web.
pub(super) fn read(location: &Location) -> Result<Table, String> {
    let name = location.name();
    if location.size().is_ok_and(|size| size > LIMIT) {
        return Err(format!(
            "The sample table {name} is larger than {} MB.",
            LIMIT >> 20
        ));
    }
    let file = location
        .open()
        .map_err(|error| format!("genie cannot open the sample table {name}. {error}"))?;
    let reader: Box<dyn BufRead> = if track::is_compressed(&name) {
        Box::new(BufReader::new(gzip::Reader::new(BufReader::new(file))))
    } else {
        Box::new(BufReader::new(file))
    };
    genie_formats::samples::read(reader, &name)
        .map_err(|error| format!("The sample table {name} is not valid. {error}"))
}

/// A number and a word for what it counts, as "1 sample" or "2,504 samples".
fn count(number: usize, word: &str) -> String {
    if number == 1 {
        format!("1 {word}")
    } else {
        format!("{} {word}s", grouped(number as u64))
    }
}

/// What a sample table holds, and how many samples of the open tracks it knows.
fn opened_words(name: &str, table: &Table, tracks: &[Track]) -> String {
    let mut words = format!(
        "The sample table {name} has {} and {}.",
        count(table.len(), "sample"),
        count(table.columns.len(), "field")
    );
    let names: HashSet<&str> = tracks
        .iter()
        .flat_map(|track| track.source.samples())
        .map(String::as_str)
        .collect();
    if !names.is_empty() {
        let known = names.iter().filter(|name| table.has(name)).count();
        let known = if known == 0 {
            "none".to_owned()
        } else {
            grouped(known as u64)
        };
        let all = count(names.len(), "sample");
        words = format!("{words} It has {known} of the {all} of the open tracks.");
    }
    words
}

/// The menu and palette words for a field that groups, sorts or colors samples.
pub(crate) fn arranging_words(arranging: &Arranging) -> String {
    match arranging {
        Arranging::Group(field) => format!("Group samples by {field}"),
        Arranging::Sort(field) => format!("Sort samples by {field}"),
        Arranging::Color(field) => format!("Color samples by {field}"),
    }
}

/// The fields that group, sort and color the samples of a track now.
pub(crate) fn active(track: &Track) -> Vec<Arranging> {
    let choice = &track.choice;
    [
        choice.group.clone().map(Arranging::Group),
        choice.sort.clone().map(Arranging::Sort),
        choice.color.clone().map(Arranging::Color),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Gives a field to a slot, or empties the slot when it already has the field.
fn toggle(slot: &mut Option<String>, field: String) {
    *slot = if slot.as_deref() == Some(field.as_str()) {
        None
    } else {
        Some(field)
    };
}

impl Genie {
    pub(super) fn open_samples(&mut self, location: Location) -> Task<Message> {
        if self.session.is_none() {
            self.status = Status::Failed(NO_REFERENCE.to_owned());
            return Task::none();
        }
        self.busy += 1;
        Task::perform(
            async move {
                let result = read(&location).map(Arc::new);
                Message::Samples(Sampling::Opened(location, result))
            },
            std::convert::identity,
        )
    }

    /// Opens a text file as what it is. A file on the web is read in the background first.
    pub(super) fn open_any_text(&mut self, location: Location) -> Task<Message> {
        if !location.is_remote() {
            let text = classify(&location);
            return self.open_text(location, text);
        }
        self.busy += 1;
        Task::perform(
            async move {
                let text = classify(&location);
                Message::Samples(Sampling::Checked(location, text))
            },
            std::convert::identity,
        )
    }

    fn open_text(&mut self, location: Location, text: Text) -> Task<Message> {
        match text {
            Text::Hub => self.open_hub(location, true),
            Text::Loops => self.open_track(location),
            Text::Table => self.open_samples(location),
        }
    }

    pub(super) fn sampling(&mut self, sampling: Sampling) -> Task<Message> {
        match sampling {
            Sampling::Opened(location, result) => {
                self.busy = self.busy.saturating_sub(1);
                match result {
                    Ok(table) => return self.samples_opened(location, table),
                    Err(error) => self.fail(error),
                }
            }
            Sampling::Arrange(id, arranging) => self.arrange_samples(id, arranging),
            Sampling::Checked(location, text) => {
                self.busy = self.busy.saturating_sub(1);
                return self.open_text(location, text);
            }
            Sampling::Close => {
                if let Some(session) = &mut self.session {
                    session.samples = None;
                    for track in &mut session.tracks {
                        track.arrange(None);
                    }
                }
            }
        }
        Task::none()
    }

    fn samples_opened(&mut self, location: Location, table: Arc<Table>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        for track in &mut session.tracks {
            track.arrange(Some(&table));
        }
        let words = opened_words(&location.name(), &table, &session.tracks);
        session.samples = Some(Samples { location, table });
        self.inform(words)
    }

    fn arrange_samples(&mut self, id: u64, arranging: Arranging) {
        self.close_menus();
        let Some(session) = &mut self.session else {
            return;
        };
        let table = session
            .samples
            .as_ref()
            .map(|samples| Arc::clone(&samples.table));
        let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) else {
            return;
        };
        let choice = &mut track.choice;
        match arranging {
            Arranging::Group(field) => toggle(&mut choice.group, field),
            Arranging::Sort(field) => toggle(&mut choice.sort, field),
            Arranging::Color(field) => toggle(&mut choice.color, field),
        }
        track.arrange(table.as_deref());
    }
}

/// The lines of a variant card for the groups of the samples: the frequency of the other
/// alleles in each group and the number of called alleles, as "EUR · AF 0.12 · 1,006 alleles".
/// Each call counts as two alleles.
pub(super) fn group_lines(arrangement: &Arrangement, variant: &Variant) -> Vec<String> {
    let groups = &arrangement.groups;
    let mut lines: Vec<String> = groups
        .iter()
        .take(GROUP_LINES)
        .map(|group| {
            let end = (group.first + group.size).min(arrangement.order.len());
            let (mut other, mut alleles) = (0_u64, 0_u64);
            for &sample in &arrangement.order[group.first.min(end)..end] {
                let count = match variant.calls.get(sample) {
                    Some(Call::Reference) => 0,
                    Some(Call::Het) => 1,
                    Some(Call::Alternate) => 2,
                    Some(Call::Missing) | None => continue,
                };
                other += count;
                alleles += 2;
            }
            if alleles == 0 {
                format!("{} · no calls", group.name)
            } else {
                let share = frequency(other, alleles);
                format!("{} · AF {share} · {} alleles", group.name, grouped(alleles))
            }
        })
        .collect();
    match groups.len().saturating_sub(GROUP_LINES) {
        0 => {}
        1 => lines.push("+1 more group".to_owned()),
        more => lines.push(format!("+{more} more groups")),
    }
    lines
}

/// A share of alleles with at most three decimals, or in scientific form when it is small.
fn frequency(other: u64, alleles: u64) -> String {
    let share = other as f64 / alleles.max(1) as f64;
    if share > 0.0 && share < 0.001 {
        return format!("{share:.1e}");
    }
    let text = format!("{share:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use genie_core::Region;
    use genie_core::samples::{Arrangement, Group};
    use genie_core::variant::{Annotations, Call, Variant};

    use super::{Text, classify, group_lines, is_table, is_text, read};
    use crate::location::Location;
    use crate::scratch::Scratch;

    #[test]
    fn group_lines_give_the_allele_frequency_of_each_group() {
        let group = |name: &str, first, size| Group {
            name: name.to_owned(),
            first,
            size,
        };
        let arrangement = Arrangement {
            order: vec![0, 2, 1, 3, 4],
            groups: vec![group("AFR", 0, 2), group("EUR", 2, 2), group("SAS", 4, 1)],
            tints: Vec::new(),
        };
        let variant = Variant {
            id: String::new(),
            span: Region::between(10, 11),
            reference: "A".to_owned(),
            alternates: vec!["G".to_owned()],
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            annotations: Annotations::default(),
            calls: vec![
                Call::Het,
                Call::Reference,
                Call::Alternate,
                Call::Het,
                Call::Missing,
            ],
        };
        assert_eq!(
            group_lines(&arrangement, &variant),
            [
                "AFR · AF 0.75 · 4 alleles",
                "EUR · AF 0.25 · 4 alleles",
                "SAS · no calls"
            ]
        );
        let many = Arrangement {
            order: (0..12).collect(),
            groups: (0..12)
                .map(|index| group(&format!("G{index}"), index, 1))
                .collect(),
            tints: Vec::new(),
        };
        let calls = Variant {
            calls: vec![Call::Reference; 12],
            ..variant
        };
        let lines = group_lines(&many, &calls);
        assert_eq!(lines.len(), 11);
        assert_eq!(lines[0], "G0 · AF 0 · 2 alleles");
        assert_eq!(lines[10], "+2 more groups");
        assert_eq!(
            group_lines(&Arrangement::plain(12), &calls),
            [] as [String; 0]
        );
    }

    #[test]
    fn tables_open_by_their_type_and_text_files_by_their_first_lines() {
        let scratch = Scratch::new("sample-types");
        let named = |name: &str| Location::File(PathBuf::from(format!("/data/{name}")));
        for name in [
            "groups.tsv",
            "cohort.CSV",
            "all.panel",
            "trio.fam",
            "study.ped.gz",
        ] {
            assert!(is_table(&named(name)), "{name}");
        }
        for name in ["calls.vcf", "genes.bed", "notes", "reads.bam", "traits.txt"] {
            assert!(!is_table(&named(name)), "{name}");
        }
        assert!(is_text(&named("hub.txt")) && is_text(&named("loops.TXT.gz")));
        assert!(!is_text(&named("groups.tsv")));
        let file = |name: &str, text: &str| Location::File(scratch.file(name, text));
        let table = file("traits.txt", "sample\tpop\nS1\tEUR\n");
        let hub = file("hub.txt", "# my hub\nhub demo\nshortLabel demo\n");
        let loops = file(
            "loops.txt",
            "chr1\tx1\tx2\tchr2\ty1\ty2\tcolor\to\n1\t10\t20\t1\t50\t60\t0,0,255\t5\n",
        );
        let clinical = file(
            "data_clinical_sample.txt",
            "#Patient Identifier\tSample Identifier\n#A\tB\n#STRING\tSTRING\n#1\t1\n\
             PATIENT_ID\tSAMPLE_ID\nP1\tS1\n",
        );
        assert_eq!(classify(&table), Text::Table);
        assert_eq!(classify(&hub), Text::Hub);
        assert_eq!(classify(&loops), Text::Loops);
        assert_eq!(classify(&clinical), Text::Table);
        assert_eq!(classify(&named("missing.txt")), Text::Hub);
    }

    #[test]
    fn tables_read_from_files_and_say_why_they_fail() {
        let scratch = Scratch::new("sample-read");
        let path = scratch.file("groups.tsv", "sample\tpop\nS1\tEUR\nS2\tAFR\n");
        let table = read(&Location::File(path)).unwrap();
        assert_eq!((table.len(), table.value("S2", 0)), (2, Some("AFR")));
        let broken = scratch.file("broken.tsv", "sample\n");
        let error = read(&Location::File(broken)).unwrap_err();
        assert!(
            error.starts_with("The sample table broken.tsv is not valid."),
            "{error}"
        );
    }
}
