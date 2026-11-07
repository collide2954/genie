use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;

use genie_core::Region;
use genie_formats::bgzf::Bgzf;
use genie_formats::features::{self, Kind};
use genie_formats::tabix::Index;

const WHOLE: u64 = 1 << 29;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn index(file: &str) -> Index {
    Index::read(File::open(data(&format!("{file}.tbi"))).unwrap()).unwrap()
}

fn reader(file: &str) -> Bgzf<File> {
    Bgzf::new(File::open(data(file)).unwrap())
}

fn sections(name: &str) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in fs::read_to_string(data(name)).unwrap().lines() {
        match line.strip_prefix('>') {
            Some(region) => sections.push((region.to_owned(), Vec::new())),
            None => sections.last_mut().unwrap().1.push(line.to_owned()),
        }
    }
    sections
}

fn locus(text: &str) -> (&str, Region) {
    let Some((name, range)) = text.split_once(':') else {
        return (text, Region::new(0, WHOLE).unwrap());
    };
    let (start, end) = range.split_once('-').unwrap();
    let start: u64 = start.parse().unwrap();
    (name, Region::new(start - 1, end.parse().unwrap()).unwrap())
}

fn query(file: &str, wanted: &str) -> Vec<String> {
    let (name, region) = locus(wanted);
    let mut lines = Vec::new();
    index(file)
        .query(&mut reader(file), name, region, |line| {
            lines.push(line.to_owned());
            Ok(())
        })
        .unwrap();
    lines
}

#[test]
fn bed_queries_match_tabix() {
    for (wanted, expected) in sections("genes.bed.expected") {
        assert_eq!(query("genes.bed.gz", &wanted), expected, "{wanted}");
    }
}

#[test]
fn gff_queries_match_tabix() {
    for (wanted, expected) in sections("genes.gff3.expected") {
        assert_eq!(query("genes.gff3.gz", &wanted), expected, "{wanted}");
    }
}

#[test]
fn bgzf_reads_every_block() {
    let mut text = String::new();
    reader("genes.bed.gz").read_to_string(&mut text).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "#chrom\tstart\tend\tname");
    assert_eq!(lines.len(), 1 + 3 * 900);
    let whole = sections("genes.bed.expected")
        .into_iter()
        .find(|(region, _)| region == "chrX")
        .unwrap()
        .1;
    assert_eq!(lines[lines.len() - 900..], whole);
}

#[test]
fn gff_transcripts_keep_exons_outside_the_query() {
    let index = index("genes.gff3.gz");
    let mut reader = reader("genes.gff3.gz");
    let whole = Region::new(0, WHOLE).unwrap();
    let (_, all) = features::query(Kind::Gff, &index, &mut reader, "chr1", whole).unwrap();
    let transcript = all
        .iter()
        .find(|feature| feature.blocks().len() > 2)
        .unwrap();
    let intron = transcript.blocks()[0].end();
    let probe = Region::new(intron + 10, intron + 11).unwrap();
    let (_, found) = features::query(Kind::Gff, &index, &mut reader, "chr1", probe).unwrap();
    assert!(found.contains(transcript));
}

#[test]
fn names_cover_every_gene() {
    let entries = features::names(Kind::Gff, reader("genes.gff3.gz")).unwrap();
    let genes = entries
        .iter()
        .filter(|entry| !entry.name.contains('-'))
        .count();
    assert_eq!(genes, 2 * 120);
    let names = features::names(Kind::Bed, reader("genes.bed.gz")).unwrap();
    assert_eq!(names.len(), 3 * 900);
}
