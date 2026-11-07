use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use genie_core::{Assembly, Contig, Region};
use genie_formats::fai::{self, FaiRecord};
use genie_formats::fasta;
use genie_formats::twobit::{self, TwoBit};

#[derive(Debug)]
enum Source {
    Fasta(Vec<FaiRecord>),
    TwoBit(TwoBit),
}

#[derive(Debug)]
pub struct Reference {
    pub path: PathBuf,
    pub name: String,
    pub assembly: Assembly,
    source: Source,
}

impl PartialEq for Reference {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.assembly == other.assembly
    }
}

pub fn open(path: PathBuf, cache: Option<&Path>) -> Result<Reference, String> {
    let mut file = File::open(&path).map_err(|error| error.to_string())?;
    let mut head = Vec::new();
    (&mut file)
        .take(4)
        .read_to_end(&mut head)
        .map_err(|error| error.to_string())?;
    if twobit::sniff(&head) {
        let reader = TwoBit::read(&mut file).map_err(|error| error.to_string())?;
        let contigs = reader
            .entries()
            .iter()
            .map(|entry| Contig {
                name: entry.name.clone(),
                length: entry.length,
            })
            .collect();
        Reference::new(path, contigs, Source::TwoBit(reader))
    } else {
        let records = fai::load(&path, cache).map_err(|error| error.to_string())?;
        Reference::fasta(path, records)
    }
}

impl Reference {
    pub fn fasta(path: PathBuf, records: Vec<FaiRecord>) -> Result<Self, String> {
        let contigs = records
            .iter()
            .map(|record| Contig {
                name: record.name.clone(),
                length: record.length,
            })
            .collect();
        Self::new(path, contigs, Source::Fasta(records))
    }

    fn new(path: PathBuf, contigs: Vec<Contig>, source: Source) -> Result<Self, String> {
        if contigs.is_empty() {
            return Err("The file has no sequences.".to_owned());
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        Ok(Self {
            path,
            name,
            assembly: Assembly::new(contigs),
            source,
        })
    }

    pub fn fetch(&self, contig: usize, region: Region) -> Result<Vec<u8>, String> {
        let mut file = File::open(&self.path).map_err(|error| error.to_string())?;
        match &self.source {
            Source::Fasta(records) => fasta::fetch(&mut file, &records[contig], region),
            Source::TwoBit(reader) => reader.fetch(&mut file, contig, region),
        }
        .map_err(|error| error.to_string())
    }
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

        fn file(&self, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, content).unwrap();
            path
        }
    }

    const ACGT_2BIT: [u8; 39] = [
        0x43, 0x27, 0x41, 0x1A, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, b'c', 22, 0, 0, 0, 4, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x9C,
    ];

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

    #[test]
    fn fetch_reads_bases() {
        let scratch = Scratch::new("fetch");
        let reference = open(scratch.file("ref.fa", ">a\nACGT\n"), None).unwrap();
        let region = genie_core::Region::new(1, 3).unwrap();
        assert_eq!(reference.fetch(0, region).unwrap(), b"CG");
    }

    #[test]
    fn open_reads_2bit_files() {
        let scratch = Scratch::new("twobit");
        let reference = open(scratch.file("ref.2bit", ACGT_2BIT), None).unwrap();
        assert_eq!(reference.assembly.contigs()[0].name, "c");
        assert_eq!(reference.assembly.contigs()[0].length, 4);
        let region = genie_core::Region::new(0, 4).unwrap();
        assert_eq!(reference.fetch(0, region).unwrap(), b"ACGT");
    }

    #[test]
    fn open_reports_broken_2bit_files() {
        let scratch = Scratch::new("broken");
        assert!(open(scratch.file("ref.2bit", &ACGT_2BIT[..10]), None).is_err());
    }
}
