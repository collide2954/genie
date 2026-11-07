use std::collections::HashMap;

use genie_core::{Feature, Region, Strand};

use crate::{
    Reason,
    bed::{number, strand},
};

const COLUMNS: usize = 9;
const PARTS: [&str; 7] = [
    "exon",
    "CDS",
    "five_prime_UTR",
    "three_prime_UTR",
    "UTR",
    "start_codon",
    "stop_codon",
];
const EXONS: [&str; 1] = ["exon"];
const CODING: [&str; 1] = ["CDS"];
const PIECES: [&str; 4] = ["CDS", "five_prime_UTR", "three_prime_UTR", "UTR"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub contig: String,
    pub kind: String,
    pub span: Region,
    pub strand: Strand,
    pub parents: Vec<String>,
    pub attributes: Vec<(String, String)>,
}

impl Record {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

pub fn is_part(kind: &str) -> bool {
    PARTS.contains(&kind)
}

pub fn parse(line: &str) -> Result<Option<Record>, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut fields = [""; COLUMNS];
    let mut count = 0;
    for (slot, field) in fields.iter_mut().zip(line.splitn(COLUMNS, '\t')) {
        *slot = field;
        count += 1;
    }
    if count < COLUMNS {
        return Err(Reason::MissingField);
    }
    let start = number(fields[3])?;
    let span = Region::new(start.saturating_sub(1), number(fields[4])?).ok_or(Reason::BadSpan)?;
    let strand = strand(fields[6])?;
    let mut parents = Vec::new();
    let mut attributes = Vec::new();
    for pair in fields[8].split(';') {
        let Some((key, value)) = pair.trim().split_once('=') else {
            continue;
        };
        if key == "Parent" {
            parents.extend(value.split(',').map(decode));
        }
        attributes.push((decode(key), decode(value)));
    }
    Ok(Some(Record {
        contig: fields[0].to_owned(),
        kind: fields[2].to_owned(),
        span,
        strand,
        parents,
        attributes,
    }))
}

pub fn assemble(records: &[Record]) -> Vec<Feature> {
    let ids: HashMap<&str, usize> = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((record.get("ID")?, index)))
        .collect();
    let mut parts = vec![Vec::new(); records.len()];
    let mut parents = vec![None; records.len()];
    let mut grouping = vec![false; records.len()];
    let mut consumed = vec![false; records.len()];
    for (index, record) in records.iter().enumerate() {
        let found = record
            .parents
            .iter()
            .filter_map(|parent| ids.get(parent.as_str()));
        for &parent in found {
            parents[index].get_or_insert(parent);
            if is_part(&record.kind) {
                parts[parent].push(index);
                consumed[index] = true;
            } else {
                grouping[parent] = true;
            }
        }
    }
    records
        .iter()
        .enumerate()
        .filter(|(index, _)| !consumed[*index] && (!parts[*index].is_empty() || !grouping[*index]))
        .map(|(index, record)| {
            let parent = parents[index].map(|parent| &records[parent]);
            let feature = Feature::new(label(record, parent), record.span, record.strand);
            if parts[index].is_empty() {
                return feature;
            }
            let spans = |kinds: &[&str]| -> Vec<Region> {
                parts[index]
                    .iter()
                    .map(|child| &records[*child])
                    .filter(|child| kinds.contains(&child.kind.as_str()))
                    .map(|child| child.span)
                    .collect()
            };
            let mut blocks = spans(&EXONS);
            if blocks.is_empty() {
                blocks = spans(&PIECES);
            }
            let coding = spans(&CODING).into_iter().reduce(Region::hull);
            feature.with_blocks(blocks).with_coding(coding)
        })
        .collect()
}

fn label<'a>(record: &'a Record, parent: Option<&'a Record>) -> &'a str {
    parent
        .and_then(|parent| {
            parent
                .get("Name")
                .or_else(|| parent.get("gene_name"))
                .or_else(|| parent.get("ID"))
        })
        .or_else(|| record.get("gene_name"))
        .or_else(|| record.get("gene"))
        .or_else(|| record.get("Name"))
        .or_else(|| record.get("ID"))
        .unwrap_or_default()
}

fn decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if let [b'%', high, low, ..] = &bytes[index..]
            && let (Some(high), Some(low)) = (hex(*high), hex(*low))
        {
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|value| value as u8)
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{Record, assemble, parse};
    use crate::Reason;

    const GENE: &str = "chr1\tsrc\tgene\t1001\t2000\t.\t+\t.\tID=g1;Name=ABC";
    const MRNA: &str = "chr1\tsrc\tmRNA\t1001\t2000\t.\t+\t.\tID=t1;Parent=g1;Name=ABC-201";
    const EXONS: [&str; 2] = [
        "chr1\tsrc\texon\t1001\t1200\t.\t+\t.\tParent=t1",
        "chr1\tsrc\texon\t1801\t2000\t.\t+\t.\tParent=t1",
    ];
    const CDS: [&str; 2] = [
        "chr1\tsrc\tCDS\t1101\t1200\t.\t+\t0\tID=c1;Parent=t1",
        "chr1\tsrc\tCDS\t1801\t1900\t.\t+\t2\tID=c1;Parent=t1",
    ];

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn record(line: &str) -> Record {
        parse(line).unwrap().unwrap()
    }

    fn features(lines: &[&str]) -> Vec<Feature> {
        let records: Vec<Record> = lines.iter().map(|line| record(line)).collect();
        assemble(&records)
    }

    #[test]
    fn parses_columns_and_attributes() {
        let record = record("chr1\tsrc\tgene\t1001\t2000\t.\t-\t.\tID=g1;Name=A%3Bb; Note=x");
        assert_eq!(record.contig, "chr1");
        assert_eq!(record.kind, "gene");
        assert_eq!(record.span, region(1000, 2000));
        assert_eq!(record.strand, Strand::Reverse);
        assert_eq!(record.get("Name"), Some("A;b"));
        assert_eq!(record.get("Note"), Some("x"));
        assert_eq!(record.get("Dbxref"), None);
    }

    #[test]
    fn splits_parents_before_decoding() {
        let record = record("chr1\t.\texon\t1\t10\t.\t+\t.\tParent=t1,t%2C2");
        assert_eq!(record.parents, ["t1", "t,2"]);
    }

    #[test]
    fn skips_directives_and_comments() {
        for line in ["##gff-version 3", "###", "#note", ""] {
            assert_eq!(parse(line), Ok(None), "{line}");
        }
    }

    #[test]
    fn rejects_bad_lines() {
        for (line, reason) in [
            ("chr1\t.\tgene\t1\t10", Reason::MissingField),
            ("chr1\t.\tgene\tx\t10\t.\t+\t.\t.", Reason::BadNumber),
            ("chr1\t.\tgene\t20\t10\t.\t+\t.\t.", Reason::BadSpan),
            ("chr1\t.\tgene\t1\t10\t.\tx\t.\t.", Reason::BadStrand),
        ] {
            assert_eq!(parse(line), Err(reason), "{line}");
        }
    }

    #[test]
    fn assembles_transcripts_from_exons_and_cds() {
        let expected = Feature::new("ABC", region(1000, 2000), Strand::Forward)
            .with_blocks(vec![region(1000, 1200), region(1800, 2000)])
            .with_coding(Some(region(1100, 1900)));
        let lines = [GENE, MRNA, EXONS[0], EXONS[1], CDS[0], CDS[1]];
        assert_eq!(features(&lines), [expected]);
    }

    #[test]
    fn labels_each_isoform_with_the_gene_name() {
        let other = "chr1\tsrc\tmRNA\t1001\t1500\t.\t+\t.\tID=t2;Parent=g1;Name=ABC-202";
        let features = features(&[GENE, MRNA, other]);
        let names: Vec<&str> = features.iter().map(Feature::name).collect();
        assert_eq!(names, ["ABC", "ABC"]);
    }

    #[test]
    fn prefers_the_gene_name_attribute_without_a_gene_line() {
        let line = "chr1\tsrc\ttranscript\t1\t100\t.\t+\t.\tID=t9;gene_name=XYZ;Name=XYZ-201";
        assert_eq!(features(&[line])[0].name(), "XYZ");
    }

    #[test]
    fn transcripts_without_cds_are_not_coding() {
        let features = features(&[MRNA, EXONS[0], EXONS[1]]);
        assert_eq!(features[0].name(), "ABC-201");
        assert_eq!(features[0].coding(), None);
    }

    #[test]
    fn uses_cds_as_blocks_without_exons() {
        let gene = "chr1\tsrc\tgene\t101\t400\t.\t-\t.\tID=g2;Name=dnaA";
        let cds = "chr1\tsrc\tCDS\t101\t400\t.\t-\t0\tParent=g2";
        let expected = Feature::new("dnaA", region(100, 400), Strand::Reverse)
            .with_blocks(vec![region(100, 400)]);
        assert_eq!(features(&[gene, cds]), [expected]);
    }

    #[test]
    fn keeps_features_without_children_or_parents() {
        let repeat = "chr1\tsrc\trepeat_region\t11\t20\t.\t.\t.\tID=r1";
        assert_eq!(
            features(&[repeat, EXONS[0]]),
            [
                Feature::new("r1", region(10, 20), Strand::Unknown),
                Feature::new("", region(1000, 1200), Strand::Forward),
            ]
        );
    }
}
