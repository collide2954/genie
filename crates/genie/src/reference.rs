use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use genie_core::{Assembly, Contig, Region};
use genie_formats::bgzf::{self, Bgzf};
use genie_formats::fai::{self, FaiRecord};
use genie_formats::twobit::{self, TwoBit};
use genie_formats::{fasta, gzi};

use crate::paths;

const FAI_SHARE: f32 = 0.95;

#[derive(Debug)]
enum Source {
    Fasta(Vec<FaiRecord>),
    Bgzf(Vec<FaiRecord>, gzi::Index),
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

pub fn open(
    path: PathBuf,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(f32),
) -> Result<Reference, String> {
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
        return Reference::new(path, contigs, Source::TwoBit(reader));
    }
    if !bgzf::sniff(&head) {
        let records = fai::load(&path, cache, progress).map_err(|error| error.to_string())?;
        return Reference::fasta(path, records);
    }
    let records = fai::load(&path, cache, &mut |share| progress(share * FAI_SHARE))
        .map_err(|error| error.to_string())?;
    let index = gzi::load(&path, cache, &mut |share| {
        progress(FAI_SHARE + share * (1.0 - FAI_SHARE));
    })
    .map_err(|error| error.to_string())?;
    Reference::new(path, contigs(&records), Source::Bgzf(records, index))
}

fn contigs(records: &[FaiRecord]) -> Vec<Contig> {
    records
        .iter()
        .map(|record| Contig {
            name: record.name.clone(),
            length: record.length,
        })
        .collect()
}

impl Reference {
    pub fn fasta(path: PathBuf, records: Vec<FaiRecord>) -> Result<Self, String> {
        Self::new(path, contigs(&records), Source::Fasta(records))
    }

    fn new(path: PathBuf, contigs: Vec<Contig>, source: Source) -> Result<Self, String> {
        if contigs.is_empty() {
            return Err("The file has no sequences.".to_owned());
        }
        Ok(Self {
            name: paths::file_name(&path),
            path,
            assembly: Assembly::new(contigs),
            source,
        })
    }

    pub fn fetch(&self, contig: usize, region: Region) -> Result<Vec<u8>, String> {
        let mut file = File::open(&self.path).map_err(|error| error.to_string())?;
        match &self.source {
            Source::Fasta(records) => fasta::fetch(&mut file, &records[contig], region),
            Source::Bgzf(records, index) => {
                fasta::fetch_bgzf(&mut Bgzf::new(file), index, &records[contig], region)
            }
            Source::TwoBit(reader) => reader.fetch(&mut file, contig, region),
        }
        .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::open;
    use crate::scratch::Scratch;

    const ACGT_2BIT: [u8; 39] = [
        0x43, 0x27, 0x41, 0x1A, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, b'c', 22, 0, 0, 0, 4, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x9C,
    ];

    #[test]
    fn open_names_the_file_and_lists_contigs() {
        let scratch = Scratch::new("open");
        let reference = open(
            scratch.file("ref.fa", ">a\nACGT\n>b\nGG\n"),
            None,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(reference.name, "ref.fa");
        assert_eq!(reference.assembly.contigs()[1].name, "b");
        assert_eq!(reference.assembly.contigs()[1].length, 2);
    }

    #[test]
    fn open_rejects_files_without_sequences() {
        let scratch = Scratch::new("empty");
        let error = open(scratch.file("ref.fa", ""), None, &mut |_| {}).unwrap_err();
        assert_eq!(error, "The file has no sequences.");
    }

    #[test]
    fn open_reports_format_errors() {
        let scratch = Scratch::new("bad");
        let error = open(scratch.file("ref.fa", "ACGT\n"), None, &mut |_| {}).unwrap_err();
        assert_eq!(error, "Line 1: the sequence has no header line.");
    }

    #[test]
    fn fetch_reads_bases() {
        let scratch = Scratch::new("fetch");
        let reference = open(scratch.file("ref.fa", ">a\nACGT\n"), None, &mut |_| {}).unwrap();
        let region = genie_core::Region::new(1, 3).unwrap();
        assert_eq!(reference.fetch(0, region).unwrap(), b"CG");
    }

    fn bgzip(parts: &[&[u8]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for part in parts.iter().copied().chain([&b""[..]]) {
            let length = u16::try_from(part.len()).unwrap();
            bytes.extend([31, 139, 8, 4, 0, 0, 0, 0, 0, 255, 6, 0, b'B', b'C', 2, 0]);
            bytes.extend((length + 30).to_le_bytes());
            bytes.push(1);
            bytes.extend(length.to_le_bytes());
            bytes.extend((!length).to_le_bytes());
            bytes.extend(part);
            bytes.extend([0; 4]);
            bytes.extend(u32::from(length).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn open_reads_bgzip_fasta() {
        let scratch = Scratch::new("bgzip");
        let file = scratch.file("ref.fa.gz", bgzip(&[b">a\nAC", b"GT\n>b\nGG\n"]));
        let reference = open(file, None, &mut |_| {}).unwrap();
        assert_eq!(reference.assembly.contigs()[0].length, 4);
        assert_eq!(reference.assembly.contigs()[1].name, "b");
        let region = genie_core::Region::new(1, 4).unwrap();
        assert_eq!(reference.fetch(0, region).unwrap(), b"CGT");
        let region = genie_core::Region::new(0, 2).unwrap();
        assert_eq!(reference.fetch(1, region).unwrap(), b"GG");
    }

    #[test]
    fn opening_without_indexes_reports_progress() {
        let scratch = Scratch::new("progress");
        let file = scratch.file("ref.fa.gz", bgzip(&[b">a\nAC", b"GT\n>b\nGG\n"]));
        let mut shares = Vec::new();
        open(file, None, &mut |share| shares.push(share)).unwrap();
        assert!(
            shares.windows(2).all(|pair| pair[0] <= pair[1]),
            "{shares:?}"
        );
        assert_eq!(shares.last(), Some(&1.0));
    }

    #[test]
    fn open_rejects_plain_gzip() {
        let scratch = Scratch::new("gzip");
        let mut plain = bgzip(&[b">a\nACGT\n"]);
        plain[3] = 0;
        let error = open(scratch.file("ref.fa.gz", plain), None, &mut |_| {}).unwrap_err();
        assert_eq!(error, "The file is not compressed with bgzip.");
    }

    #[test]
    fn open_reads_2bit_files() {
        let scratch = Scratch::new("twobit");
        let reference = open(scratch.file("ref.2bit", ACGT_2BIT), None, &mut |_| {}).unwrap();
        assert_eq!(reference.assembly.contigs()[0].name, "c");
        assert_eq!(reference.assembly.contigs()[0].length, 4);
        let region = genie_core::Region::new(0, 4).unwrap();
        assert_eq!(reference.fetch(0, region).unwrap(), b"ACGT");
    }

    #[test]
    fn open_reports_broken_2bit_files() {
        let scratch = Scratch::new("broken");
        assert!(
            open(
                scratch.file("ref.2bit", &ACGT_2BIT[..10]),
                None,
                &mut |_| {}
            )
            .is_err()
        );
    }
}
