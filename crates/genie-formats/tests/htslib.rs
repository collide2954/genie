use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::PathBuf;

use genie_core::alignment::{Alignment, Op};
use genie_core::{Feature, Region};
use genie_formats::bgzf::Bgzf;
use genie_formats::fai::{self, FaiRecord};
use genie_formats::features::{self, Kind};
use genie_formats::tabix::Index;
use genie_formats::{bai, bam, fasta, gzi, vcf};

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

fn bare(features: Vec<Feature>) -> Vec<Feature> {
    features
        .into_iter()
        .map(|feature| feature.with_extra(""))
        .collect()
}

#[test]
fn gtf_gives_the_same_transcripts_as_gff3() {
    let (gff, gtf) = (index("genes.gff3.gz"), index("genes.gtf.gz"));
    for wanted in [
        "chr1:1-30000",
        "chr1:200000-201000",
        "chr2:500000-600000",
        "chr2",
    ] {
        let (name, region) = locus(wanted);
        let mut three = reader("genes.gff3.gz");
        let (_, expected) = features::query(Kind::Gff, &gff, &mut three, name, region).unwrap();
        let mut plain = reader("genes.gtf.gz");
        let (_, found) = features::query(Kind::Gtf, &gtf, &mut plain, name, region).unwrap();
        assert_ne!(expected.len(), 0, "{wanted}");
        assert_eq!(bare(found), bare(expected), "{wanted}");
    }
    let (table, kept) =
        features::read_table(Kind::Gtf, BufReader::new(reader("genes.gtf.gz"))).unwrap();
    let whole = Region::new(0, WHOLE).unwrap();
    let (_, expected) =
        features::query(Kind::Gff, &gff, &mut reader("genes.gff3.gz"), "chr1", whole).unwrap();
    assert_eq!(table.query("chr1", whole).len(), expected.len());
    let names = features::names(Kind::Gtf, BufReader::new(reader("genes.gtf.gz"))).unwrap();
    let found: Vec<&str> = names.iter().map(|entry| entry.name.as_str()).collect();
    assert!(found.contains(&"GENE1003") && found.contains(&"GENE1010-202"));
    assert_eq!(kept, names);
}

fn letter(op: Op) -> char {
    match op {
        Op::Match => 'M',
        Op::Insertion => 'I',
        Op::Deletion => 'D',
        Op::Skip => 'N',
        Op::SoftClip => 'S',
        Op::HardClip => 'H',
        Op::Padding => 'P',
        Op::Equal => '=',
        Op::Diff => 'X',
    }
}

fn sam(alignment: &Alignment, contig: &str) -> String {
    let mut cigar = String::new();
    for (op, length) in &alignment.cigar {
        cigar.push_str(&length.to_string());
        cigar.push(letter(*op));
    }
    let qualities: String = if alignment.qualities.is_empty() {
        "*".to_owned()
    } else {
        alignment
            .qualities
            .iter()
            .map(|quality| char::from(quality + 33))
            .collect()
    };
    [
        alignment.name.clone(),
        alignment.flags.to_string(),
        contig.to_owned(),
        (alignment.start + 1).to_string(),
        alignment.mapq.to_string(),
        cigar,
        String::from_utf8_lossy(&alignment.bases).into_owned(),
        qualities,
    ]
    .join("\t")
}

fn columns(line: &str) -> String {
    let fields: Vec<&str> = line.split('\t').collect();
    [0, 1, 2, 3, 4, 5, 9, 10].map(|at| fields[at]).join("\t")
}

#[test]
fn bam_queries_match_samtools() {
    let index = bai::Index::read(File::open(data("reads.bam.bai")).unwrap()).unwrap();
    let mut reader = reader("reads.bam");
    let header = bam::header(&mut reader).unwrap();
    assert_eq!(header.references.len(), index.len());
    for (wanted, expected) in sections("reads.bam.expected") {
        let (name, region) = locus(&wanted);
        let mut found = Vec::new();
        if let Some(reference) = header
            .references
            .iter()
            .position(|(known, _)| known == name)
        {
            bam::query(&index, &mut reader, reference, region, |alignment| {
                found.push(sam(&alignment, name));
                Ok(())
            })
            .unwrap();
        }
        let expected: Vec<String> = expected.iter().map(|line| columns(line)).collect();
        assert_eq!(found, expected, "{wanted}");
    }
}

#[test]
fn bam_modifications_match_samtools() {
    let index = bai::Index::read(File::open(data("mods.bam.bai")).unwrap()).unwrap();
    let mut reader = reader("mods.bam");
    bam::header(&mut reader).unwrap();
    let mut found = Vec::new();
    let whole = Region::new(0, WHOLE).unwrap();
    bam::query(&index, &mut reader, 0, whole, |alignment| {
        found.extend(alignment.parts(&[], 0).modified);
        Ok(())
    })
    .unwrap();
    found.sort_unstable();
    let mut expected = Vec::new();
    for line in fs::read_to_string(data("mods.bam.expected"))
        .unwrap()
        .lines()
    {
        let (position, column) = line.split_once('\t').unwrap();
        let position: u64 = position.parse().unwrap();
        let mut letters = column.chars();
        while let Some(letter) = letters.next() {
            match letter {
                '^' => {
                    letters.next();
                }
                '[' => {
                    let group: String = letters
                        .by_ref()
                        .take_while(|&letter| letter != ']')
                        .collect();
                    let value: String = group.chars().filter(char::is_ascii_digit).collect();
                    expected.push((position - 1, value.parse::<u8>().unwrap()));
                }
                _ => {}
            }
        }
    }
    expected.sort_unstable();
    assert!(expected.len() > 50, "{}", expected.len());
    assert_eq!(found, expected);
}

fn tag(fields: &[&str], name: &str) -> String {
    fields
        .iter()
        .find_map(|field| {
            let (key, value) = field.split_at_checked(5)?;
            (key.starts_with(name) && key.as_bytes()[2] == b':').then(|| value.to_owned())
        })
        .unwrap_or_default()
}

#[test]
fn bam_mates_and_tags_match_samtools() {
    let index = bai::Index::read(File::open(data("pairs.bam.bai")).unwrap()).unwrap();
    let mut reader = reader("pairs.bam");
    let names: Vec<String> = bam::header(&mut reader)
        .unwrap()
        .references
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for (wanted, expected) in sections("pairs.bam.expected") {
        let (name, region) = locus(&wanted);
        let reference = names.iter().position(|known| known == name).unwrap();
        let mut found = Vec::new();
        bam::query(&index, &mut reader, reference, region, |alignment| {
            let (partner, at, insert) =
                alignment
                    .mate
                    .map_or(("*".to_owned(), "0".to_owned(), "0".to_owned()), |mate| {
                        let partner = if mate.contig == reference {
                            "=".to_owned()
                        } else {
                            names[mate.contig].clone()
                        };
                        (
                            partner,
                            (mate.start + 1).to_string(),
                            mate.insert.to_string(),
                        )
                    });
            let phase = alignment.tags.phase.map(|phase| phase.to_string());
            found.push(
                [
                    alignment.name.clone(),
                    alignment.flags.to_string(),
                    partner,
                    at,
                    insert,
                    phase.unwrap_or_default(),
                    alignment.tags.group.clone().unwrap_or_default(),
                ]
                .join("\t"),
            );
            Ok(())
        })
        .unwrap();
        let expected: Vec<String> = expected
            .iter()
            .map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                [
                    fields[0].to_owned(),
                    fields[1].to_owned(),
                    fields[6].to_owned(),
                    fields[7].to_owned(),
                    fields[8].to_owned(),
                    tag(&fields[11..], "HP"),
                    tag(&fields[11..], "RG"),
                ]
                .join("\t")
            })
            .collect();
        assert_ne!(expected.len(), 0, "{wanted}");
        assert_eq!(found, expected, "{wanted}");
    }
}

fn fai(file: &str) -> Vec<FaiRecord> {
    fai::parse(BufReader::new(
        File::open(data(&format!("{file}.fai"))).unwrap(),
    ))
    .unwrap()
}

fn gzi(file: &str) -> gzi::Index {
    gzi::Index::read(File::open(data(&format!("{file}.gzi"))).unwrap()).unwrap()
}

#[test]
fn bgzip_fasta_matches_samtools_faidx() {
    let records = fai("reference.fa.gz");
    let index = gzi("reference.fa.gz");
    let mut reader = reader("reference.fa.gz");
    for (wanted, expected) in sections("reference.fa.expected") {
        let (name, region) = locus(&wanted);
        let record = records.iter().find(|record| record.name == name).unwrap();
        let found = fasta::fetch_bgzf(&mut reader, &index, record, region).unwrap();
        assert_eq!(
            String::from_utf8(found).unwrap(),
            expected.concat(),
            "{wanted}"
        );
    }
}

#[test]
fn built_fasta_indexes_match_samtools() {
    let records = fai::build(reader("reference.fa.gz")).unwrap();
    assert_eq!(records, fai("reference.fa.gz"));
    let index = gzi::Index::build(File::open(data("reference.fa.gz")).unwrap()).unwrap();
    assert_eq!(index, gzi("reference.fa.gz"));
}

fn variant_index(suffix: &str) -> Index {
    Index::read(File::open(data(&format!("variants.vcf.gz.{suffix}"))).unwrap()).unwrap()
}

#[test]
fn vcf_queries_match_tabix_with_tbi_and_csi() {
    for suffix in ["tbi", "csi"] {
        let index = variant_index(suffix);
        let mut reader = reader("variants.vcf.gz");
        for (wanted, expected) in sections("variants.vcf.expected") {
            let (name, region) = locus(&wanted);
            let mut lines = Vec::new();
            index
                .query(&mut reader, name, region, |line| {
                    lines.push(line.to_owned());
                    Ok(())
                })
                .unwrap();
            assert_eq!(lines, expected, "{suffix} {wanted}");
        }
    }
}

#[test]
fn every_fixture_variant_parses() {
    let (header, table) = vcf::read_table(reader("variants.vcf.gz")).unwrap();
    assert_eq!(header.samples, ["S1", "S2", "S3"]);
    let whole = Region::new(0, WHOLE).unwrap();
    assert_eq!(table.query("chr1", whole).len(), 700);
    let index = variant_index("csi");
    let found = vcf::query(&index, &mut reader("variants.vcf.gz"), "chr2", whole).unwrap();
    assert_eq!(found.len(), 700);
    assert!(found.iter().any(|variant| variant.span.len() > 500));
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
