use std::path::{Path, PathBuf};

use genie_core::{Assembly, Contig};
use genie_formats::fai::{self, FaiRecord};

#[derive(Debug, PartialEq)]
pub struct Reference {
    pub path: PathBuf,
    pub name: String,
    pub records: Vec<FaiRecord>,
    pub assembly: Assembly,
}

pub fn open(path: PathBuf, cache: Option<&Path>) -> Result<Reference, String> {
    let records = fai::load(&path, cache).map_err(|error| error.to_string())?;
    if records.is_empty() {
        return Err("The file has no sequences.".to_owned());
    }
    let assembly = Assembly::new(
        records
            .iter()
            .map(|record| Contig {
                name: record.name.clone(),
                length: record.length,
            })
            .collect(),
    );
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Ok(Reference {
        path,
        name,
        records,
        assembly,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::open;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("genie-app-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, content: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn open_names_the_file_and_lists_contigs() {
        let scratch = Scratch::new("open");
        let reference = open(scratch.file("ref.fa", ">a\nACGT\n>b\nGG\n"), None).unwrap();
        assert_eq!(reference.name, "ref.fa");
        assert_eq!(reference.assembly.contigs()[1].name, "b");
        assert_eq!(reference.assembly.contigs()[1].length, 2);
    }

    #[test]
    fn open_rejects_files_without_sequences() {
        let scratch = Scratch::new("empty");
        let error = open(scratch.file("ref.fa", ""), None).unwrap_err();
        assert_eq!(error, "The file has no sequences.");
    }

    #[test]
    fn open_reports_format_errors() {
        let scratch = Scratch::new("bad");
        let error = open(scratch.file("ref.fa", "ACGT\n"), None).unwrap_err();
        assert_eq!(error, "Line 1: the sequence has no header line.");
    }
}
