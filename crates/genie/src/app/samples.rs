//! The sample table of a session: facts about the samples, such as population, sex and
//! phenotype. Tracks with a row for each sample group, sort and color their rows by the fields
//! of the table.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use genie_core::grouped;
use genie_core::samples::Table;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arranging {
    Group(String),
    Sort(String),
    Color(String),
}

/// True for a file that genie opens as a sample table: tab or comma separated files, PLINK
/// files, and text files on this computer that are not track hubs.
pub(super) fn is_table(location: &Location) -> bool {
    let name = location.name().to_ascii_lowercase();
    let stem = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".bgz"))
        .unwrap_or(&name);
    let Some((_, extension)) = stem.rsplit_once('.') else {
        return false;
    };
    if EXTENSIONS.contains(&extension) {
        return true;
    }
    extension == "txt" && location.path().is_some_and(|path| !starts_like_hub(path))
}

/// True when the first line of a file that is not a comment starts with "hub", as track hub
/// files do, or when genie cannot read the file.
fn starts_like_hub(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return true;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .is_none_or(|line| line.trim_start().to_ascii_lowercase().starts_with("hub"))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{is_table, read};
    use crate::location::Location;
    use crate::scratch::Scratch;

    #[test]
    fn tables_open_by_their_type_and_text_files_that_are_not_hubs() {
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
        for name in ["calls.vcf", "genes.bed", "notes", "reads.bam"] {
            assert!(!is_table(&named(name)), "{name}");
        }
        let table = scratch.file("traits.txt", "sample\tpop\nS1\tEUR\n");
        let hub = scratch.file("hub.txt", "# my hub\nhub demo\nshortLabel demo\n");
        assert!(is_table(&Location::File(table)));
        assert!(!is_table(&Location::File(hub)));
        assert!(!is_table(&named("missing.txt")));
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
