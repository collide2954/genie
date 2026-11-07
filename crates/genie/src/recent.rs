use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_formats::ini::{self, Document, Section};

pub const LIMIT: usize = 8;

const FILE: &str = "settings.ini";
const SECTION: &str = "recent";
const KEY: &str = "file";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    paths: Vec<PathBuf>,
}

impl Recent {
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn add(&mut self, path: PathBuf) {
        self.paths.retain(|known| *known != path);
        self.paths.insert(0, path);
        self.paths.truncate(LIMIT);
    }

    pub fn parse(text: &str) -> Self {
        let paths = ini::parse(text)
            .ok()
            .and_then(|document| document.section(SECTION).cloned())
            .map(|section| section.values(KEY).map(PathBuf::from).collect())
            .unwrap_or_default();
        Self { paths }
    }

    pub fn text(&self) -> String {
        ini::write(&Document {
            sections: vec![Section {
                name: SECTION.to_owned(),
                entries: self
                    .paths
                    .iter()
                    .map(|path| (KEY.to_owned(), path.display().to_string()))
                    .collect(),
            }],
        })
    }

    pub fn load(folder: &Path) -> Self {
        fs::read_to_string(folder.join(FILE))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self, folder: &Path) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        fs::write(folder.join(FILE), self.text())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{LIMIT, Recent};

    fn paths(recent: &Recent) -> Vec<&str> {
        recent
            .paths()
            .iter()
            .map(|path| path.to_str().unwrap())
            .collect()
    }

    #[test]
    fn newest_comes_first_without_duplicates() {
        let mut recent = Recent::default();
        for path in ["a.fa", "b.2bit", "a.fa"] {
            recent.add(PathBuf::from(path));
        }
        assert_eq!(paths(&recent), ["a.fa", "b.2bit"]);
    }

    #[test]
    fn keeps_a_limited_number() {
        let mut recent = Recent::default();
        for index in 0..LIMIT + 2 {
            recent.add(PathBuf::from(format!("{index}.fa")));
        }
        assert_eq!(recent.paths().len(), LIMIT);
        assert_eq!(
            recent.paths()[0],
            PathBuf::from(format!("{}.fa", LIMIT + 1))
        );
    }

    #[test]
    fn reads_back_what_it_writes() {
        let mut recent = Recent::default();
        recent.add(PathBuf::from("/data/b.2bit"));
        recent.add(PathBuf::from("/data/a b.fa"));
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
        recent.add(PathBuf::from("/data/a.fa"));
        recent.save(&folder).unwrap();
        assert_eq!(Recent::load(&folder), recent);
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(Recent::load(&folder), Recent::default());
    }
}
