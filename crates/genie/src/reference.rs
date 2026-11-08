use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use genie_core::{Assembly, Band, Contig, Region};
use genie_formats::bgzf::{self, Bgzf};
use genie_formats::fai::{self, FaiRecord};
use genie_formats::twobit::{self, TwoBit};
use genie_formats::{fasta, gzi};

use crate::location::Location;

const FAI_SHARE: f32 = 0.95;

#[derive(Debug)]
enum Source {
    Fasta(Vec<FaiRecord>),
    Bgzf(Vec<FaiRecord>, gzi::Index),
    TwoBit(TwoBit),
}

#[derive(Debug)]
pub struct Reference {
    pub location: Location,
    pub name: String,
    pub assembly: Assembly,
    source: Source,
    bands: Vec<Vec<Band>>,
}

impl PartialEq for Reference {
    fn eq(&self, other: &Self) -> bool {
        self.location == other.location && self.assembly == other.assembly
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    pub sizes: HashMap<String, u64>,
    pub aliases: Vec<Vec<String>>,
    pub bands: Vec<(String, Band)>,
}

pub fn open_with(
    location: impl Into<Location>,
    extras: &Extras,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(f32),
) -> Result<Reference, String> {
    let reference = sized(location.into(), &extras.sizes, cache, progress)?;
    Ok(reference
        .with_aliases(&extras.aliases)
        .with_bands(&extras.bands))
}

pub fn open(
    location: impl Into<Location>,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(f32),
) -> Result<Reference, String> {
    open_with(location, &Extras::default(), cache, progress)
}

fn sized(
    location: Location,
    sizes: &HashMap<String, u64>,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(f32),
) -> Result<Reference, String> {
    let mut file = location.open()?;
    let mut head = Vec::new();
    (&mut file)
        .take(4)
        .read_to_end(&mut head)
        .map_err(|error| error.to_string())?;
    if twobit::sniff(&head) {
        let reader = TwoBit::read_with(&mut file, sizes).map_err(|error| error.to_string())?;
        let contigs = reader
            .entries()
            .iter()
            .map(|entry| Contig {
                name: entry.name.clone(),
                length: entry.length,
            })
            .collect();
        return Reference::new(location, contigs, Source::TwoBit(reader));
    }
    let compressed = bgzf::sniff(&head);
    let Location::File(path) = &location else {
        return remote_fasta(location, compressed);
    };
    if !compressed {
        let records = fai::load(path, cache, progress).map_err(|error| error.to_string())?;
        return Reference::fasta(path.clone(), records);
    }
    let records = fai::load(path, cache, &mut |share| progress(share * FAI_SHARE))
        .map_err(|error| error.to_string())?;
    let index = gzi::load(path, cache, &mut |share| {
        progress(FAI_SHARE + share * (1.0 - FAI_SHARE));
    })
    .map_err(|error| error.to_string())?;
    Reference::new(
        location.clone(),
        contigs(&records),
        Source::Bgzf(records, index),
    )
}

fn remote_fasta(location: Location, compressed: bool) -> Result<Reference, String> {
    let name = location.name();
    let sidecar = |suffix: &str| {
        location.with_suffix(suffix).open().map_err(|error| {
            format!("genie needs the index {name}{suffix} next to the file. {error}")
        })
    };
    let records = fai::parse(BufReader::new(sidecar(".fai")?))
        .map_err(|error| format!("The index {name}.fai is not valid. {error}"))?;
    if !compressed {
        return Reference::new(location, contigs(&records), Source::Fasta(records));
    }
    let index = gzi::Index::read(sidecar(".gzi")?)
        .map_err(|error| format!("The index {name}.gzi is not valid. {error}"))?;
    Reference::new(location, contigs(&records), Source::Bgzf(records, index))
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
        Self::new(
            Location::File(path),
            contigs(&records),
            Source::Fasta(records),
        )
    }

    fn new(location: Location, contigs: Vec<Contig>, source: Source) -> Result<Self, String> {
        if contigs.is_empty() {
            return Err("The file has no sequences.".to_owned());
        }
        Ok(Self {
            name: location.name(),
            location,
            assembly: Assembly::new(contigs),
            source,
            bands: Vec::new(),
        })
    }

    #[must_use]
    pub fn with_aliases(mut self, rows: &[Vec<String>]) -> Self {
        self.assembly = self.assembly.with_aliases(rows);
        self
    }

    #[must_use]
    pub fn with_bands(mut self, rows: &[(String, Band)]) -> Self {
        let mut bands = vec![Vec::new(); self.assembly.contigs().len()];
        for (contig, band) in rows {
            if let Some(index) = self.assembly.find(contig) {
                bands[index].push(band.clone());
            }
        }
        for list in &mut bands {
            list.sort_by_key(|band| band.span.start());
        }
        self.bands = bands;
        self
    }

    pub fn bands(&self, contig: usize) -> &[Band] {
        self.bands.get(contig).map_or(&[], Vec::as_slice)
    }

    pub fn fetch(&self, contig: usize, region: Region) -> Result<Vec<u8>, String> {
        let mut file = self.location.open()?;
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
    use std::collections::HashMap;

    use genie_core::{Band, Region, Stain};

    use super::{Extras, open, open_with};
    use crate::location::Location;
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
    fn bands_and_aliases_join_the_reference() {
        let scratch = Scratch::new("bands");
        let file = scratch.file("ref.fa", ">chr1\nACGTACGTAC\n>chr2\nGG\n");
        let band = |start, end, name: &str| Band {
            span: Region::new(start, end).unwrap(),
            name: name.to_owned(),
            stain: Stain::Gneg,
        };
        let rows = vec![
            ("chr1".to_owned(), band(5, 10, "p1")),
            ("NC_1".to_owned(), band(0, 5, "p2")),
            ("chr9".to_owned(), band(0, 5, "p9")),
        ];
        let aliases = vec![vec!["chr1".to_owned(), "NC_1".to_owned()]];
        let reference = open(file, None, &mut |_| {})
            .unwrap()
            .with_aliases(&aliases)
            .with_bands(&rows);
        assert_eq!(reference.assembly.find("NC_1"), Some(0));
        assert_eq!(reference.bands(0), [band(0, 5, "p2"), band(5, 10, "p1")]);
        assert_eq!(reference.bands(1), []);
        assert_eq!(reference.bands(7), []);
    }

    #[test]
    fn known_sizes_aliases_and_bands_open_with_a_2bit() {
        let scratch = Scratch::new("extras");
        let file = scratch.file("ref.2bit", ACGT_2BIT);
        let extras = Extras {
            sizes: HashMap::from([("c".to_owned(), 3)]),
            aliases: vec![vec!["c".to_owned(), "NC_9".to_owned()]],
            bands: vec![(
                "NC_9".to_owned(),
                Band {
                    span: Region::new(0, 3).unwrap(),
                    name: "p1".to_owned(),
                    stain: Stain::Gpos(50),
                },
            )],
        };
        let reference = open_with(file, &extras, None, &mut |_| {}).unwrap();
        assert_eq!(reference.assembly.contigs()[0].length, 3);
        assert_eq!(reference.assembly.find("NC_9"), Some(0));
        assert_eq!(reference.bands(0).len(), 1);
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
    fn remote_references_read_their_indexes_from_the_server() {
        let fixture = |name: &str| {
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../genie-formats/tests/data")
                .join(name);
            (format!("/{name}"), std::fs::read(path).unwrap())
        };
        let server = crate::server::serve(vec![
            ("/ref.fa".to_owned(), b">a\nACGT\n>b\nGG\n".to_vec()),
            (
                "/ref.fa.fai".to_owned(),
                b"a\t4\t3\t4\t5\nb\t2\t11\t2\t3\n".to_vec(),
            ),
            ("/ref.2bit".to_owned(), ACGT_2BIT.to_vec()),
            ("/bare.fa".to_owned(), b">a\nACGT\n".to_vec()),
            fixture("reference.fa.gz"),
            fixture("reference.fa.gz.fai"),
            fixture("reference.fa.gz.gzi"),
        ]);
        let at = |path: &str| Location::parse(&server.url(path)).unwrap();
        let fasta = open(at("/ref.fa"), None, &mut |_| {}).unwrap();
        assert_eq!(fasta.name, "ref.fa");
        let region = genie_core::Region::new(0, 2).unwrap();
        assert_eq!(fasta.fetch(1, region).unwrap(), b"GG");
        let twobit = open(at("/ref.2bit"), None, &mut |_| {}).unwrap();
        let region = genie_core::Region::new(0, 4).unwrap();
        assert_eq!(twobit.fetch(0, region).unwrap(), b"ACGT");
        let remote = open(at("/reference.fa.gz"), None, &mut |_| {}).unwrap();
        let local_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data/reference.fa.gz");
        let local = open(local_path, None, &mut |_| {}).unwrap();
        assert_eq!(remote.assembly, local.assembly);
        let region = genie_core::Region::new(1_000, 1_100).unwrap();
        assert_eq!(remote.fetch(0, region), local.fetch(0, region));
        let error = open(at("/bare.fa"), None, &mut |_| {}).unwrap_err();
        let expected =
            "genie needs the index bare.fa.fai next to the file. The server answered 404";
        assert!(error.starts_with(expected), "{error}");
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
