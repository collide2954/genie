use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_formats::ini::{self, Section};

use crate::location::Location;
use crate::settings::{self, FILE};

pub const LIMIT: usize = 8;

const SECTION: &str = "recent";
const KEY: &str = "file";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    entries: Vec<Location>,
}

impl Recent {
    pub fn entries(&self) -> &[Location] {
        &self.entries
    }

    pub fn add(&mut self, location: Location) {
        self.entries.retain(|known| *known != location);
        self.entries.insert(0, location);
        self.entries.truncate(LIMIT);
    }

    pub fn parse(text: &str) -> Self {
        let entries = ini::parse(text)
            .ok()
            .and_then(|document| document.section(SECTION).cloned())
            .map(|section| {
                section
                    .values(KEY)
                    .map(|text| {
                        Location::parse(text)
                            .unwrap_or_else(|_| Location::File(PathBuf::from(text)))
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
                .map(|location| (KEY.to_owned(), location.to_string()))
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

    use super::{LIMIT, Recent};
    use crate::location::Location;

    fn names(recent: &Recent) -> Vec<String> {
        recent.entries().iter().map(Location::to_string).collect()
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
        recent.add(Location::parse("https://example.org/hg38.2bit?k=1").unwrap());
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
