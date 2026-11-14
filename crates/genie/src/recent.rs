use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_formats::ini::{self, Section};

use crate::location::Location;
use crate::settings::{self, FILE};

pub const LIMIT: usize = 8;

const SECTION: &str = "recent";
const KEY: &str = "file";
const GENOME: &str = "genome";
const HUBS: &str = "hubs";
const HUB: &str = "hub";
const NAME: &str = "name";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Location(Location),
    Genome(String),
}

impl From<Location> for Entry {
    fn from(location: Location) -> Self {
        Self::Location(location)
    }
}

impl From<PathBuf> for Entry {
    fn from(path: PathBuf) -> Self {
        Self::Location(Location::File(path))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    entries: Vec<Entry>,
}

impl Recent {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn add(&mut self, entry: Entry) {
        self.entries.retain(|known| *known != entry);
        self.entries.insert(0, entry);
        self.entries.truncate(LIMIT);
    }

    pub fn parse(text: &str) -> Self {
        let entries = ini::parse(text)
            .ok()
            .and_then(|document| document.section(SECTION).cloned())
            .map(|section| {
                section
                    .entries
                    .iter()
                    .filter_map(|(key, text)| match key.as_str() {
                        KEY => Some(Entry::Location(
                            Location::parse(text)
                                .unwrap_or_else(|_| Location::File(PathBuf::from(text))),
                        )),
                        GENOME => Some(Entry::Genome(text.clone())),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { entries }
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        ini::write(&ini::Document {
            sections: vec![self.section()],
        })
    }

    fn section(&self) -> Section {
        Section {
            name: SECTION.to_owned(),
            entries: self
                .entries
                .iter()
                .map(|entry| match entry {
                    Entry::Location(location) => (KEY.to_owned(), location.to_string()),
                    Entry::Genome(id) => (GENOME.to_owned(), id.clone()),
                })
                .collect(),
        }
    }

    pub fn load(folder: &Path) -> Self {
        fs::read_to_string(folder.join(FILE))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, folder: &Path) -> io::Result<()> {
        settings::store(folder, self.section())
    }
}

/// The track hubs that genie opened last, newest first, each with its short label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hubs {
    entries: Vec<(Location, String)>,
}

impl Hubs {
    pub fn entries(&self) -> &[(Location, String)] {
        &self.entries
    }

    pub fn add(&mut self, location: Location, name: String) {
        self.entries.retain(|(known, _)| *known != location);
        self.entries.insert(0, (location, name));
        self.entries.truncate(LIMIT);
    }

    pub fn parse(text: &str) -> Self {
        let mut entries: Vec<(Location, String)> = Vec::new();
        let document = ini::parse(text).ok();
        let section = document
            .as_ref()
            .and_then(|document| document.section(HUBS));
        for (key, value) in section.iter().flat_map(|section| &section.entries) {
            match key.as_str() {
                HUB => entries.push((
                    Location::parse(value).unwrap_or_else(|_| Location::File(PathBuf::from(value))),
                    String::new(),
                )),
                NAME => {
                    if let Some((_, name)) = entries.last_mut() {
                        name.clone_from(value);
                    }
                }
                _ => {}
            }
        }
        entries.truncate(LIMIT);
        Self { entries }
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        ini::write(&ini::Document {
            sections: vec![self.section()],
        })
    }

    fn section(&self) -> Section {
        Section {
            name: HUBS.to_owned(),
            entries: self
                .entries
                .iter()
                .flat_map(|(location, name)| {
                    [
                        (HUB.to_owned(), location.to_string()),
                        (NAME.to_owned(), name.clone()),
                    ]
                })
                .collect(),
        }
    }

    pub fn load(folder: &Path) -> Self {
        fs::read_to_string(folder.join(FILE))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, folder: &Path) -> io::Result<()> {
        settings::store(folder, self.section())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{Entry, Hubs, LIMIT, Recent};
    use crate::location::Location;

    #[test]
    fn hubs_keep_their_names_newest_first() {
        let mut hubs = Hubs::default();
        let web = Location::parse("https://example.org/hub.txt").unwrap();
        let local = Location::File(PathBuf::from("/data/my hub/hub.txt"));
        hubs.add(web.clone(), "Web hub".to_owned());
        hubs.add(local.clone(), "Local hub".to_owned());
        hubs.add(web.clone(), "Web hub again".to_owned());
        assert_eq!(
            hubs.entries(),
            [
                (web, "Web hub again".to_owned()),
                (local, "Local hub".to_owned())
            ]
        );
        let text = hubs.text();
        assert!(text.contains("hub = https://example.org/hub.txt\nname = Web hub again"));
        assert_eq!(Hubs::parse(&text), hubs);
        assert_eq!(Hubs::parse("[hubs]\nname = lost\n"), Hubs::default());
        for index in 0..LIMIT + 3 {
            hubs.add(
                Location::File(PathBuf::from(format!("/{index}.txt"))),
                String::new(),
            );
        }
        assert_eq!(hubs.entries().len(), LIMIT);
    }

    fn names(recent: &Recent) -> Vec<String> {
        recent
            .entries()
            .iter()
            .map(|entry| match entry {
                Entry::Location(location) => location.to_string(),
                Entry::Genome(id) => format!("genome {id}"),
            })
            .collect()
    }

    #[test]
    fn genomes_join_the_list() {
        let mut recent = Recent::default();
        recent.add(PathBuf::from("/data/a.fa").into());
        recent.add(Entry::Genome("hg38".to_owned()));
        recent.add(Entry::Genome("mm10".to_owned()));
        recent.add(Entry::Genome("hg38".to_owned()));
        assert_eq!(names(&recent), ["genome hg38", "genome mm10", "/data/a.fa"]);
        let text = recent.text();
        assert!(text.contains("genome = hg38"), "{text}");
        assert_eq!(Recent::parse(&text), recent);
    }

    #[test]
    fn newest_comes_first_without_duplicates() {
        let mut recent = Recent::default();
        for path in ["a.fa", "b.2bit", "a.fa"] {
            recent.add(PathBuf::from(path).into());
        }
        assert_eq!(names(&recent), ["a.fa", "b.2bit"]);
    }

    #[test]
    fn keeps_a_limited_number() {
        let mut recent = Recent::default();
        for index in 0..LIMIT + 2 {
            recent.add(PathBuf::from(format!("{index}.fa")).into());
        }
        assert_eq!(recent.entries().len(), LIMIT);
        assert_eq!(names(&recent)[0], format!("{}.fa", LIMIT + 1));
    }

    #[test]
    fn reads_back_what_it_writes() {
        let mut recent = Recent::default();
        recent.add(PathBuf::from("/data/b.2bit").into());
        recent.add(PathBuf::from("/data/a b.fa").into());
        recent.add(
            Location::parse("https://example.org/hg38.2bit?k=1")
                .unwrap()
                .into(),
        );
        assert!(recent.text().contains("https://example.org/hg38.2bit?k=1"));
        assert_eq!(Recent::parse(&recent.text()), recent);
    }

    #[test]
    fn broken_text_gives_an_empty_list() {
        assert_eq!(Recent::parse("not ini at all"), Recent::default());
    }

    #[test]
    fn saves_to_and_loads_from_a_folder() {
        let folder = std::env::temp_dir().join(format!("genie-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let mut recent = Recent::default();
        recent.add(PathBuf::from("/data/a.fa").into());
        recent.save(&folder).unwrap();
        assert_eq!(Recent::load(&folder), recent);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(Recent::load(&folder), Recent::default());
    }
}
