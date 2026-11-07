use std::fs::{self, File};
use std::path::PathBuf;

use genie_core::signal::Summary;
use genie_core::{Feature, Region};
use genie_formats::bbi::Index;
use genie_formats::bgzf::Bgzf;
use genie_formats::{bed, bedgraph, tabix};

const FILES: [&str; 3] = ["signal.bw", "steps.bw", "plain.bw"];

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

struct Query {
    kind: String,
    contig: String,
    region: Region,
    bins: usize,
    lines: Vec<String>,
}

fn queries(file: &str) -> Vec<Query> {
    let text = fs::read_to_string(data(&format!("{file}.expected"))).unwrap();
    let mut queries: Vec<Query> = Vec::new();
    for line in text.lines() {
        let Some(header) = line.strip_prefix('>') else {
            queries.last_mut().unwrap().lines.push(line.to_owned());
            continue;
        };
        let fields: Vec<&str> = header.split(' ').collect();
        queries.push(Query {
            kind: fields[0].to_owned(),
            contig: fields[1].to_owned(),
            region: Region::new(fields[2].parse().unwrap(), fields[3].parse().unwrap()).unwrap(),
            bins: fields.get(4).map_or(0, |bins| bins.parse().unwrap()),
            lines: Vec::new(),
        });
    }
    queries
}

fn close(found: f64, expected: f64) -> bool {
    (found - expected).abs() <= 1e-5 * expected.abs().max(1.0)
}

#[test]
fn records_match_bigwig_to_bedgraph() {
    for file in FILES {
        let mut reader = File::open(data(file)).unwrap();
        let index = Index::read(&mut reader).unwrap();
        for query in queries(file).iter().filter(|query| query.kind == "records") {
            let found = index
                .records(&mut reader, &query.contig, query.region)
                .unwrap();
            assert_eq!(found.len(), query.lines.len(), "{file} {}", query.contig);
            for (record, line) in found.iter().zip(&query.lines) {
                let fields: Vec<&str> = line.split('\t').collect();
                let span = (record.span.start(), record.span.end());
                assert_eq!(fields[0], query.contig);
                assert_eq!(
                    span,
                    (fields[1].parse().unwrap(), fields[2].parse().unwrap())
                );
                assert!(
                    close(f64::from(record.max), fields[3].parse().unwrap()),
                    "{line}"
                );
                assert_eq!(record.min, record.max);
            }
        }
    }
}

#[test]
fn summaries_match_bigwig_summary() {
    for file in FILES {
        let mut reader = File::open(data(file)).unwrap();
        let index = Index::read(&mut reader).unwrap();
        for query in queries(file).iter().filter(|query| query.kind != "records") {
            let label = format!("{file} {} {} {:?}", query.kind, query.contig, query.region);
            let bins = index
                .bins(&mut reader, &query.contig, query.region, query.bins)
                .unwrap();
            if query.lines == ["none"] {
                assert!(bins.iter().all(Option::is_none), "{label}");
                continue;
            }
            let expected: Vec<&str> = query.lines[0].split('\t').collect();
            assert_eq!(bins.len(), expected.len(), "{label}");
            for (bin, text) in bins.iter().zip(expected) {
                match (bin, text) {
                    (None, "n/a") => {}
                    (Some(bin), text) if text != "n/a" => {
                        let found = match query.kind.as_str() {
                            "mean" => bin.mean(),
                            "min" => f64::from(bin.min),
                            _ => f64::from(bin.max),
                        };
                        assert!(
                            close(found, text.parse().unwrap()),
                            "{label}: {found} {text}"
                        );
                    }
                    _ => panic!("{label}: {bin:?} {text}"),
                }
            }
        }
    }
}

#[test]
fn zoom_levels_give_fewer_records() {
    let mut reader = File::open(data("signal.bw")).unwrap();
    let index = Index::read(&mut reader).unwrap();
    let whole = Region::new(0, 3_000_000).unwrap();
    let raw = index.records(&mut reader, "chr1", whole).unwrap();
    let zoomed = index.summaries(&mut reader, "chr1", whole, 15_000).unwrap();
    assert!(
        zoomed.len() * 10 < raw.len(),
        "{} {}",
        zoomed.len(),
        raw.len()
    );
    let bases: u64 = raw.iter().map(|record| u64::from(record.count)).sum();
    let counted: u64 = zoomed.iter().map(|summary| u64::from(summary.count)).sum();
    assert_eq!(counted, bases);
}

#[test]
fn bedgraph_matches_the_bigwig_of_the_same_data() {
    let mut reader = File::open(data("plain.bw")).unwrap();
    let index = Index::read(&mut reader).unwrap();
    let tabix = tabix::Index::read(File::open(data("plain.bedGraph.gz.tbi")).unwrap()).unwrap();
    let mut bgzf = Bgzf::new(File::open(data("plain.bedGraph.gz")).unwrap());
    let table =
        bedgraph::read_table(Bgzf::new(File::open(data("plain.bedGraph.gz")).unwrap())).unwrap();
    for (start, end) in [
        (0, 1_000_000),
        (5_000, 9_000),
        (123_456, 400_000),
        (999_000, 999_999),
    ] {
        let region = Region::new(start, end).unwrap();
        let clip = |summaries: Vec<Summary>| -> Vec<Summary> {
            summaries
                .into_iter()
                .filter_map(|summary| {
                    let span = summary.span.intersection(region)?;
                    Some(Summary::single(span, summary.max))
                })
                .collect()
        };
        let expected = index.records(&mut reader, "chr2", region).unwrap();
        assert!(!expected.is_empty() || start == 999_000);
        let queried = bedgraph::query(&tabix, &mut bgzf, "chr2", region).unwrap();
        assert_eq!(clip(queried), expected, "{region:?}");
        assert_eq!(clip(table.query("chr2", region)), expected, "{region:?}");
    }
}

fn sections(file: &str) -> Vec<(String, Vec<String>)> {
    let text = fs::read_to_string(data(&format!("{file}.expected"))).unwrap();
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        match line.strip_prefix('>') {
            Some(header) => sections.push((header.to_owned(), Vec::new())),
            None => sections.last_mut().unwrap().1.push(line.to_owned()),
        }
    }
    sections
}

fn bed_lines(lines: &[String]) -> Vec<(String, Feature)> {
    lines
        .iter()
        .map(|line| {
            let (contig, feature) = bed::parse(line).unwrap().unwrap();
            (contig.to_owned(), feature)
        })
        .collect()
}

#[test]
fn bigbed_matches_ucsc_tools() {
    let mut reader = File::open(data("genes.bb")).unwrap();
    let index = Index::read(&mut reader).unwrap();
    assert!(index.is_bed());
    let mut checked = 0;
    for (header, lines) in sections("genes.bb") {
        let fields: Vec<&str> = header.split(' ').collect();
        match fields[0] {
            "records" => {
                let (contig, region) = match fields[1].split_once(':') {
                    Some((contig, range)) => {
                        let (start, end) = range.split_once('-').unwrap();
                        let region = Region::new(start.parse().unwrap(), end.parse().unwrap());
                        (contig, region.unwrap())
                    }
                    None => (fields[1], Region::new(0, 1 << 32).unwrap()),
                };
                let found = index.features(&mut reader, contig, region).unwrap();
                let found: Vec<(String, Feature)> = found
                    .into_iter()
                    .map(|feature| (contig.to_owned(), feature))
                    .collect();
                assert_eq!(found, bed_lines(&lines), "{header}");
            }
            "name" => {
                let found = index.find(&mut reader, fields[1]).unwrap();
                assert_eq!(found, bed_lines(&lines), "{header}");
                let upper = fields[1].to_ascii_uppercase();
                let found = index.find(&mut reader, &upper).unwrap();
                assert_eq!(found, bed_lines(&lines), "{upper}");
            }
            kind => {
                let start: u64 = fields[2].parse().unwrap();
                let end: u64 = fields[3].parse().unwrap();
                let count: usize = fields[4].parse().unwrap();
                let region = Region::new(start, end).unwrap();
                let bins = index.bins(&mut reader, fields[1], region, count).unwrap();
                let expected: Vec<&str> = lines[0].split('\t').collect();
                for (place, (bin, text)) in bins.iter().zip(expected).enumerate() {
                    let low = start + (end - start) * place as u64 / count as u64;
                    let high = start + (end - start) * (place as u64 + 1) / count as u64;
                    let found = bin.map(|bin| match kind {
                        "mean" => bin.mean(),
                        _ => bin.count / (high - low) as f64,
                    });
                    match (found, text) {
                        (None, "n/a") => {}
                        (Some(found), text) => {
                            assert!(
                                close(found, text.parse().unwrap()),
                                "{header}: {found} {text}"
                            );
                        }
                        (None, text) => panic!("{header}: no bin for {text}"),
                    }
                }
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 15);
}

#[test]
fn contigs_come_from_the_chromosome_tree() {
    let index = Index::read(&mut File::open(data("signal.bw")).unwrap()).unwrap();
    let mut names = index.names().to_vec();
    names.sort();
    assert_eq!(
        names,
        [
            "chr1",
            "chr2",
            "chr3",
            "chr4",
            "chr5",
            "chr6",
            "chr7",
            "chr8",
            "chr9",
            "chrM",
            "chrUn_KI270742v1",
            "chrX"
        ]
    );
}
