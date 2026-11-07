use std::fs::{self, File};
use std::path::PathBuf;

use genie_core::Region;
use genie_formats::bigwig::Index;

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
