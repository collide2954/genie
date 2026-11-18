//! PSL files: the alignments of BLAT, as UCSC writes them. Each line aligns a query with a part of
//! the target in blocks. genie shows each alignment on its target, which is the reference in view.

use genie_core::{Feature, Region, Strand};

use crate::Reason;
use crate::bed::number;
use crate::fields::Field;

/// The columns of a PSL line.
const COLUMNS: usize = 21;

/// True for the name of a PSL file, also when it is compressed.
pub fn is_psl(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let stem = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".bgz"))
        .unwrap_or(&name);
    stem.rsplit_once('.')
        .is_some_and(|(_, extension)| extension == "psl")
}

/// The numbers of a list such as `50,50,`.
fn numbers(field: &str) -> Result<Vec<u64>, Reason> {
    field
        .split(',')
        .filter(|item| !item.trim().is_empty())
        .map(number)
        .collect()
}

/// An alignment as a feature on its target: the name of the query, the span and the strand on the
/// target, and its blocks. Header lines give nothing. The line stays with the feature for its card.
pub fn parse(line: &str) -> Result<Option<(&str, Feature)>, Reason> {
    if !line.starts_with(|letter: char| letter.is_ascii_digit()) {
        return Ok(None);
    }
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < COLUMNS {
        return Err(Reason::MissingField);
    }
    for column in [0, 1, 2, 4, 6, 10, 11, 12, 14] {
        number(columns[column])?;
    }
    let span = Region::new(number(columns[15])?, number(columns[16])?).ok_or(Reason::BadSpan)?;
    let strands = columns[8].as_bytes();
    let target_reverse = strands.get(1) == Some(&b'-');
    let strand = match strands.first() {
        Some(b'-') => Strand::Reverse,
        Some(b'+') if target_reverse => Strand::Reverse,
        Some(b'+') => Strand::Forward,
        _ => return Err(Reason::BadStrand),
    };
    let count = usize::try_from(number(columns[17])?).map_err(|_| Reason::BadBlocks)?;
    let (sizes, starts) = (numbers(columns[18])?, numbers(columns[20])?);
    if sizes.len() != count || starts.len() != count {
        return Err(Reason::BadBlocks);
    }
    let size = number(columns[14])?;
    let blocks = sizes
        .iter()
        .zip(&starts)
        .map(|(&length, &start)| {
            let end = start.checked_add(length).ok_or(Reason::BadBlocks)?;
            let start = if target_reverse {
                size.checked_sub(end).ok_or(Reason::BadBlocks)?
            } else {
                start
            };
            Ok(Region::between(start, start.saturating_add(length)))
        })
        .collect::<Result<Vec<Region>, Reason>>()?;
    let feature = Feature::new(columns[9], span, strand)
        .with_blocks(blocks)
        .with_extra(line);
    Ok(Some((columns[13], feature)))
}

/// The fields of an alignment for its card and its filters: the place on the query, the identity,
/// the score, the matches, the mismatches and the gaps. The identity counts the matches of all
/// aligned bases, and the score is that of the BLAT page of UCSC: the matches, half the repeat
/// matches, less the mismatches and the gaps.
pub fn fields(line: &str) -> Vec<Field> {
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < COLUMNS {
        return Vec::new();
    }
    let value = |column: usize| number(columns[column]).unwrap_or(0);
    let (matches, wrong, repeats) = (value(0), value(1), value(2));
    let field = |label: &str, value: String| Field {
        label: label.to_owned(),
        value,
    };
    let mut found = vec![
        field(
            "Query",
            format!(
                "{}:{}-{}",
                columns[9],
                value(11).saturating_add(1),
                value(12)
            ),
        ),
        field("Query length", columns[10].to_owned()),
    ];
    let aligned = matches + repeats + wrong;
    if aligned > 0 {
        let identity = (matches + repeats) as f64 * 100.0 / aligned as f64;
        found.push(field("Identity (%)", format!("{identity:.1}")));
    }
    let score = i128::from(matches) + i128::from(repeats / 2)
        - i128::from(wrong)
        - i128::from(value(4))
        - i128::from(value(6));
    found.extend([
        field("Score", score.to_string()),
        field("Matches", matches.to_string()),
        field("Mismatches", wrong.to_string()),
        field("Query gaps", value(4).to_string()),
        field("Target gaps", value(6).to_string()),
    ]);
    found
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Strand};

    use super::{fields, is_psl, parse};
    use crate::Reason;

    const LINE: &str = "95\t3\t2\t0\t1\t10\t1\t200\t-\tread7\t150\t20\t130\tchr17\t83257441\t1000\t1310\t2\t50,50,\t20,80,\t1000,1260,";

    #[test]
    fn names_end_with_psl() {
        assert!(is_psl("hits.psl") && is_psl("HITS.PSL") && is_psl("hits.psl.gz"));
        assert!(!is_psl("hits.paf"));
    }

    #[test]
    fn alignments_lie_on_their_target_in_blocks() {
        let (contig, feature) = parse(LINE).unwrap().unwrap();
        assert_eq!(contig, "chr17");
        assert_eq!(feature.name(), "read7");
        assert_eq!(feature.span(), Region::between(1_000, 1_310));
        assert_eq!(feature.strand(), Strand::Reverse);
        assert_eq!(
            feature.blocks(),
            [Region::between(1_000, 1_050), Region::between(1_260, 1_310)]
        );
    }

    #[test]
    fn translated_alignments_on_the_reverse_target_count_from_its_end() {
        let line =
            "30\t0\t0\t0\t0\t0\t0\t0\t+-\tprot\t10\t0\t10\tchr1\t1000\t100\t130\t1\t30,\t0,\t870,";
        let (_, feature) = parse(line).unwrap().unwrap();
        assert_eq!(feature.blocks(), [Region::between(100, 130)]);
        assert_eq!(feature.strand(), Strand::Reverse);
    }

    #[test]
    fn cards_show_the_place_on_the_query_the_identity_and_the_score() {
        let (_, feature) = parse(LINE).unwrap().unwrap();
        let found: Vec<(String, String)> = fields(feature.extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pairs = [
            ("Query", "read7:21-130"),
            ("Query length", "150"),
            ("Identity (%)", "97.0"),
            ("Score", "91"),
            ("Matches", "95"),
            ("Mismatches", "3"),
            ("Query gaps", "1"),
            ("Target gaps", "1"),
        ];
        let wanted: Vec<(String, String)> = pairs
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(found, wanted);
    }

    #[test]
    fn headers_give_nothing_and_wrong_lines_say_why() {
        for header in [
            "psLayout version 3",
            "",
            "match\tmis- \trep. \tN's",
            "     \tmatch\tmatch\t   ",
            "---------------------------------------------------------------",
        ] {
            assert_eq!(parse(header), Ok(None), "{header}");
        }
        let short = "95\t3\t2\t0\t1\t10\t1\t200\t-\tread7";
        assert_eq!(parse(short).map(|_| ()), Err(Reason::MissingField));
        let backwards = LINE.replace("\t1000\t1310\t", "\t1310\t1000\t");
        assert_eq!(parse(&backwards).map(|_| ()), Err(Reason::BadSpan));
        let blocks = LINE.replace("\t2\t50,50,", "\t3\t50,50,");
        assert_eq!(parse(&blocks).map(|_| ()), Err(Reason::BadBlocks));
    }
}
