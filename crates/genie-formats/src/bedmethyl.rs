use std::collections::BTreeMap;
use std::io::{BufRead, Read, Seek};

use genie_core::Region;
use genie_core::methylation::Site;
use genie_core::modification;
use genie_core::signal::Summary;

use crate::bgzf::Bgzf;
use crate::tabix::Index;
use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

const COUNTS: usize = 9;
const FIRST_COUNT: usize = 9;
const ON_CYTOSINE: [&str; 5] = ["m", "h", "f", "c", "C"];
const FIRST: &str = "5mC";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub span: Region,
    pub code: String,
    pub strand: u8,
    pub valid: u32,
    pub modified: u32,
}

pub fn parse(line: &str) -> Result<Option<(&str, Record)>, Reason> {
    let line = line.trim_end_matches(['\n', '\r']);
    let first = line.split_ascii_whitespace().next().unwrap_or_default();
    if first.is_empty() || first.starts_with('#') || first == "track" || first == "browser" {
        return Ok(None);
    }
    let fields: Vec<&str> = line.split('\t').collect();
    let contig = fields[0];
    let counts: Vec<&str> = match fields.len() {
        10 => fields[FIRST_COUNT].split_ascii_whitespace().collect(),
        length if length >= FIRST_COUNT + COUNTS => {
            fields[FIRST_COUNT..FIRST_COUNT + COUNTS].to_vec()
        }
        _ => return Err(Reason::MissingField),
    };
    if counts.len() < COUNTS {
        return Err(Reason::MissingField);
    }
    let whole = |text: &str| text.trim().parse::<u64>().map_err(|_| Reason::BadNumber);
    let count = |text: &str| text.trim().parse::<u32>().map_err(|_| Reason::BadNumber);
    let span = Region::new(whole(fields[1])?, whole(fields[2])?).ok_or(Reason::BadSpan)?;
    let code = fields[3].trim();
    let strand = match fields[5].trim().as_bytes() {
        [strand @ (b'+' | b'-' | b'.')] => *strand,
        _ => return Err(Reason::BadStrand),
    };
    if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(Reason::MissingField);
    }
    Ok(Some((
        contig,
        Record {
            span,
            code: code.to_owned(),
            strand,
            valid: count(counts[0])?,
            modified: count(counts[2])?,
        },
    )))
}

pub fn looks_like(line: &str) -> bool {
    matches!(parse(line), Ok(Some(_)))
}

pub fn read_table(reader: impl BufRead) -> Result<Table<Record>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, record)) = parsed {
            rows.push((contig.to_owned(), record.span, record));
        }
        Ok(())
    })?;
    Ok(Table::new(rows))
}

pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    contig: &str,
    region: Region,
) -> Result<Vec<Record>, Error> {
    let mut found = Vec::new();
    index.query(reader, contig, region, |line| {
        if let Some((_, record)) = parse(line).map_err(Error::Invalid)? {
            found.push(record);
        }
        Ok(())
    })?;
    Ok(found)
}

pub fn codes(records: &[Record]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for record in records {
        let name = modification::name(&record.code);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if let Some(at) = names.iter().position(|name| name == FIRST) {
        let first = names.remove(at);
        names.insert(0, first);
    }
    names
}

pub fn sites(records: &[Record], name: &str, join: bool) -> Vec<Site> {
    let mut sites: BTreeMap<u64, (u32, u32)> = BTreeMap::new();
    for record in records {
        if modification::name(&record.code) != name {
            continue;
        }
        let shifts = join
            && record.strand == b'-'
            && ON_CYTOSINE.contains(&record.code.as_str())
            && record.span.start() > 0;
        let position = record.span.start() - u64::from(shifts);
        let entry = sites.entry(position).or_default();
        entry.0 += record.modified;
        entry.1 += record.valid;
    }
    sites
        .into_iter()
        .filter(|(_, (_, valid))| *valid > 0)
        .map(|(position, (modified, total))| Site {
            position,
            modified,
            total,
        })
        .collect()
}

pub fn percents(sites: &[Site]) -> Vec<Summary> {
    sites
        .iter()
        .map(|site| {
            let percent = site.modified as f32 * 100.0 / site.total as f32;
            Summary::single(Region::between(site.position, site.position + 1), percent)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::methylation::Site;
    use genie_core::signal::Summary;

    use super::{Record, codes, looks_like, parse, percents, read_table, sites};
    use crate::{Error, Reason};

    const TABS: &str =
        "chr1\t100\t101\tm\t20\t+\t100\t101\t255,0,0\t20\t75.00\t15\t5\t0\t0\t1\t0\t0";
    const SPACES: &str = "chr1\t101\t102\tm\t10\t-\t101\t102\t255,0,0\t10 50.00 5 5 0 0 0 0 0";

    fn record(start: u64, code: &str, strand: u8, valid: u32, modified: u32) -> Record {
        Record {
            span: Region::between(start, start + 1),
            code: code.to_owned(),
            strand,
            valid,
            modified,
        }
    }

    #[test]
    fn parses_both_layouts_of_modkit_lines() {
        assert_eq!(
            parse(TABS),
            Ok(Some(("chr1", record(100, "m", b'+', 20, 15))))
        );
        assert_eq!(
            parse(SPACES),
            Ok(Some(("chr1", record(101, "m", b'-', 10, 5))))
        );
        for header in ["#chrom", "track name=x", ""] {
            assert_eq!(parse(header), Ok(None), "{header}");
        }
        assert_eq!(parse("chr1\t100\t101\tm"), Err(Reason::MissingField));
        let bad = TABS.replace("\t20\t75.00", "\tx\t75.00");
        assert_eq!(parse(&bad), Err(Reason::BadNumber));
    }

    #[test]
    fn knows_modkit_lines_from_other_beds() {
        assert!(looks_like(TABS));
        assert!(looks_like(SPACES));
        assert!(!looks_like("chr1\t100\t200\tgeneA\t0\t+"));
        assert!(!looks_like(
            "chr1\t100\t200\tgeneA\t0\t+\t100\t200\t0\t2\t10,20,\t0,80,"
        ));
        assert!(!looks_like("chr1\t100\t200\t1.5"));
        assert!(!looks_like("#chrom"));
    }

    #[test]
    fn sites_give_the_percent_modified_and_can_join_cpg_strands() {
        let records = [
            record(100, "m", b'+', 20, 15),
            record(101, "m", b'-', 10, 5),
            record(100, "h", b'+', 20, 2),
            record(200, "a", b'-', 4, 1),
        ];
        let single =
            |start: u64, value: f32| Summary::single(Region::between(start, start + 1), value);
        let summaries = |name: &str, join: bool| percents(&sites(&records, name, join));
        let joined = sites(&records, "5mC", true);
        assert_eq!(
            joined,
            [Site {
                position: 100,
                modified: 20,
                total: 30
            }]
        );
        let percent = percents(&joined);
        assert_eq!(percent[0].span, Region::between(100, 101));
        assert!(
            (percent[0].sum - 66.666_67).abs() < 0.001,
            "{}",
            percent[0].sum
        );
        assert_eq!(
            summaries("5mC", false),
            [single(100, 75.0), single(101, 50.0)]
        );
        assert_eq!(summaries("5hmC", true), [single(100, 10.0)]);
        assert_eq!(summaries("6mA", true), [single(200, 25.0)]);
        assert_eq!(codes(&records), ["5mC", "5hmC", "6mA"]);
        assert_eq!(
            codes(&[record(5, "h", b'+', 1, 1), record(5, "m", b'+', 1, 1)]),
            ["5mC", "5hmC"]
        );
    }

    #[test]
    fn tables_hold_the_records_of_each_contig() {
        let text = format!("{TABS}\n{SPACES}\n");
        let table = read_table(text.as_bytes()).unwrap();
        assert_eq!(table.query("chr1", Region::between(0, 1_000)).len(), 2);
        let error = read_table("chr1\t1\t2\tm\n".as_bytes()).unwrap_err();
        assert!(matches!(error, Error::Format { line: 1, .. }));
    }
}
