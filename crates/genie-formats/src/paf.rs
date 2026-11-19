//! PAF files: the pairwise alignments of minimap2, wfmash and other aligners. A line aligns a
//! part of a query sequence with a part of a target sequence. genie shows each alignment on its
//! target, which is the reference in view.

use std::io::BufRead;

use genie_core::dotplot::{Builder, Hit, Plot};
use genie_core::{Feature, Region, Strand};

use crate::bed::{number, strand};
use crate::fields::{Field, Rule};
use crate::{Error, Reason};

/// The shortest deletion from the target in the CIGAR of an alignment that splits the alignment
/// into blocks. Shorter deletions stay in a block.
pub const GAP: u64 = 50;
/// The columns before the tags.
const COLUMNS: usize = 12;
/// The MAPQ of an alignment without one.
const NO_MAPQ: &str = "255";
/// Tags with long values, such as the CIGAR, that cards do not show.
const LONG: [&str; 3] = ["cg", "cs", "ds"];

/// True for the name of a PAF file, also when it is compressed.
pub fn is_paf(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let stem = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".bgz"))
        .unwrap_or(&name);
    stem.rsplit_once('.')
        .is_some_and(|(_, extension)| extension == "paf")
}

/// An alignment as a feature on its target: the name of the query, the span and the strand on
/// the target, and the blocks between the deletions of `GAP` bases or more in its CIGAR (the
/// `cg` tag). The line stays with the feature for its card.
pub fn parse(line: &str) -> Result<Option<(&str, Feature)>, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < COLUMNS {
        return Err(Reason::MissingField);
    }
    for column in [1, 2, 3, 9, 10, 11] {
        number(columns[column])?;
    }
    let start = number(columns[7])?;
    let span = Region::new(start, number(columns[8])?).ok_or(Reason::BadSpan)?;
    let mut feature = Feature::new(columns[0], span, strand(columns[4])?).with_extra(line);
    if let Some(cigar) = tag(&columns[COLUMNS..], "cg") {
        feature = feature.with_blocks(blocks(start, cigar)?);
    }
    Ok(Some((columns[5], feature)))
}

/// Adds the alignment of a line to a dot plot. False for an empty line or a comment.
pub fn add(builder: &mut Builder, line: &str) -> Result<bool, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(false);
    }
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < COLUMNS {
        return Err(Reason::MissingField);
    }
    let span =
        |start: &str, end: &str| Region::new(number(start)?, number(end)?).ok_or(Reason::BadSpan);
    let on_query = span(columns[2], columns[3])?;
    let on_target = span(columns[7], columns[8])?;
    let reverse = strand(columns[4])? == Strand::Reverse;
    let (query_length, target_length) = (number(columns[1])?, number(columns[6])?);
    let (matches, block) = (number(columns[9])?, number(columns[10])?);
    let mapq = (columns[11] != NO_MAPQ)
        .then(|| columns[11].parse().ok())
        .flatten();
    let kind = tag(&columns[COLUMNS..], "tp").and_then(|value| value.chars().next());
    let query = builder.query(columns[0], query_length);
    let target = builder.target(columns[5], target_length);
    builder.push(Hit {
        query,
        target,
        on_query,
        on_target,
        reverse,
        matches,
        block,
        mapq,
        kind,
    });
    Ok(true)
}

/// Adds the alignment of a line to a dot plot when the fields of the line pass the rules of a
/// filter. False for a line that the rules hide, an empty line or a comment.
pub fn keep(builder: &mut Builder, line: &str, rules: &[Rule]) -> Result<bool, Reason> {
    let blank = line.is_empty() || line.starts_with('#');
    if !rules.is_empty() && !blank && !crate::fields::passes(rules, &fields(line)) {
        return Ok(false);
    }
    add(builder, line)
}

/// The alignments of all lines of a PAF file that pass the rules of a filter, for a dot plot.
pub fn alignments(mut reader: impl BufRead, rules: &[Rule]) -> Result<Builder, Error> {
    let mut builder = Builder::default();
    let mut bytes = Vec::new();
    let mut number = 0;
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(builder);
        }
        number += 1;
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        keep(&mut builder, line, rules).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
    }
}

/// The value of a tag, as `NM:i:3` holds 3 for NM.
fn tag<'a>(tags: &[&'a str], name: &str) -> Option<&'a str> {
    tags.iter().find_map(|tag| {
        let (key, rest) = tag.split_once(':')?;
        (key == name).then(|| rest.split_once(':').map_or(rest, |(_, value)| value))
    })
}

/// The parts of the target that a CIGAR aligns, split at the deletions of `GAP` bases or more.
fn blocks(start: u64, cigar: &str) -> Result<Vec<Region>, Reason> {
    let mut found = Vec::new();
    let (mut at, mut from, mut length) = (start, start, 0_u64);
    for byte in cigar.bytes() {
        if byte.is_ascii_digit() {
            length = length
                .saturating_mul(10)
                .saturating_add(u64::from(byte - b'0'));
            continue;
        }
        match byte {
            b'D' | b'N' if length >= GAP => {
                if at > from {
                    found.push(Region::between(from, at));
                }
                at = at.saturating_add(length);
                from = at;
            }
            b'M' | b'=' | b'X' | b'D' | b'N' => at = at.saturating_add(length),
            b'I' | b'S' | b'H' | b'P' => {}
            _ => return Err(Reason::BadBlocks),
        }
        length = 0;
    }
    if at > from {
        found.push(Region::between(from, at));
    }
    Ok(found)
}

/// The fields of an alignment for its card and its filters: the place on the query, the
/// identity, the matches, the length of the block, MAPQ and the short tags. The numbers have no
/// commas, so that filters can read them.
pub fn fields(line: &str) -> Vec<Field> {
    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < COLUMNS {
        return Vec::new();
    }
    let field = |label: &str, value: String| Field {
        label: label.to_owned(),
        value,
    };
    let (matches, block) = (number(columns[9]), number(columns[10]));
    let mut found = Vec::new();
    if let Ok(start) = number(columns[2]) {
        let place = format!("{}:{}-{}", columns[0], start + 1, columns[3]);
        found.push(field("Query", place));
    }
    found.push(field("Query length", columns[1].to_owned()));
    if let (Ok(matches), Ok(block)) = (matches, block)
        && block > 0
    {
        let identity = matches as f64 * 100.0 / block as f64;
        found.push(field("Identity (%)", format!("{identity:.1}")));
    }
    found.push(field("Matches", columns[9].to_owned()));
    found.push(field("Block length", columns[10].to_owned()));
    if columns[11] != NO_MAPQ {
        found.push(field("MAPQ", columns[11].to_owned()));
    }
    for tag in &columns[COLUMNS..] {
        let mut parts = tag.splitn(3, ':');
        let (Some(name), Some(_), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if LONG.contains(&name) {
            continue;
        }
        let (label, value) = match name {
            "tp" => ("Type", kind(value).to_owned()),
            "NM" => ("Edits", value.to_owned()),
            "dv" => ("Divergence", value.to_owned()),
            other => (other, value.to_owned()),
        };
        found.push(field(label, value));
    }
    found
}

/// The fields of an alignment of a dot plot, as [`fields`] gives them for its line: the place on
/// the query, the identity, the matches, the block length, MAPQ and the type.
pub fn plot_fields(plot: &Plot, hit: &Hit) -> Vec<Field> {
    let field = |label: &str, value: String| Field {
        label: label.to_owned(),
        value,
    };
    let query = &plot.queries[hit.query];
    let place = format!(
        "{}:{}-{}",
        query.name,
        hit.on_query.start() + 1,
        hit.on_query.end()
    );
    let mut found = vec![field("Query", place)];
    if hit.block > 0 {
        let identity = format!("{:.1}", hit.identity() * 100.0);
        found.push(field("Identity (%)", identity));
    }
    found.push(field("Matches", hit.matches.to_string()));
    found.push(field("Block length", hit.block.to_string()));
    found.extend(hit.mapq.map(|mapq| field("MAPQ", mapq.to_string())));
    found.extend(
        hit.kind
            .map(|kind| field("Type", self::kind(&kind.to_string()).to_owned())),
    );
    found
}

/// The type of an alignment that minimap2 gives in the `tp` tag.
fn kind(value: &str) -> &str {
    match value {
        "P" => "primary",
        "S" => "secondary",
        "I" | "i" => "inversion",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use genie_core::dotplot::Builder;
    use genie_core::{Region, Strand};

    use super::{add, alignments, fields, is_paf, parse, plot_fields};

    #[test]
    fn plot_alignments_have_the_fields_of_their_lines() {
        let mut builder = Builder::default();
        add(&mut builder, LINE).unwrap();
        let plot = builder.finish(|_| None);
        let (_, feature) = parse(LINE).unwrap().unwrap();
        let mut wanted = fields(feature.extra());
        let others = ["Query length", "Edits", "Divergence"];
        wanted.retain(|field| !others.contains(&field.label.as_str()));
        assert_eq!(plot_fields(&plot, &plot.hits[0]), wanted);
    }
    use crate::Reason;
    use crate::fields::{Rule, Test};

    #[test]
    fn files_give_the_alignments_that_the_filter_keeps() {
        let text = format!("{LINE}\r\nq\t100\t0\t100\t+\tchr2\t1000\t10\t110\t100\t100\t255\n\n");
        let all = alignments(text.as_bytes(), &[]).unwrap().finish(|_| None);
        assert_eq!(all.hits.len(), 2);
        let rule = Rule {
            label: "Identity (%)".to_owned(),
            test: Test::AtLeast(95.0),
        };
        let kept = alignments(text.as_bytes(), &[rule])
            .unwrap()
            .finish(|_| None);
        assert_eq!(kept.hits.len(), 1);
        assert_eq!(kept.queries[0].name, "q");
        let broken = alignments("# header\nx\t1\n".as_bytes(), &[]).unwrap_err();
        assert_eq!(broken.to_string(), "Line 2: a field is missing.");
    }

    #[test]
    fn lines_add_their_alignment_to_a_dot_plot() {
        let mut builder = Builder::default();
        assert_eq!(add(&mut builder, ""), Ok(false));
        assert_eq!(add(&mut builder, "# comment"), Ok(false));
        assert_eq!(add(&mut builder, LINE), Ok(true));
        let plain = "q\t100\t0\t100\t+\tchr2\t1000\t10\t110\t100\t100\t255";
        assert_eq!(add(&mut builder, plain), Ok(true));
        let wrong = "bad\t100\t50\t10\t+\tchr3\t1000\t10\t110\t100\t100\t60";
        assert_eq!(add(&mut builder, wrong), Err(Reason::BadSpan));
        let plot = builder.finish(|name| (name == "chr1").then_some(0));
        let names: Vec<&str> = plot
            .targets
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        assert_eq!(names, ["chr1", "chr2"]);
        assert_eq!(plot.targets[0].length, 248_956_422);
        assert_eq!(plot.queries[0].name, "contig_7");
        assert_eq!(plot.queries[0].length, 50_000);
        let hit = plot.hits[0];
        assert_eq!(hit.on_query, Region::between(1_000, 4_100));
        assert_eq!(hit.on_target, Region::between(20_000, 23_200));
        assert!(hit.reverse);
        assert_eq!((hit.matches, hit.block), (2_900, 3_200));
        assert_eq!((hit.mapq, hit.kind), (Some(60), Some('P')));
        let other = plot.hits[1];
        assert!(!other.reverse);
        assert_eq!((other.mapq, other.kind), (None, None));
    }

    const LINE: &str = "contig_7\t50000\t1000\t4100\t-\tchr1\t248956422\t20000\t23200\t2900\t3200\t60\ttp:A:P\tNM:i:300\tdv:f:0.0123\tcg:Z:1000M100D2000M10I100M";

    #[test]
    fn names_end_with_paf() {
        for name in ["asm.paf", "ASM.PAF", "asm.paf.gz", "asm.paf.bgz"] {
            assert!(is_paf(name), "{name}");
        }
        assert!(!is_paf("asm.bed") && !is_paf("paf.txt"));
    }

    #[test]
    fn alignments_lie_on_their_target_with_blocks_between_long_deletions() {
        let (contig, feature) = parse(LINE).unwrap().unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(feature.name(), "contig_7");
        assert_eq!(feature.span(), Region::between(20_000, 23_200));
        assert_eq!(feature.strand(), Strand::Reverse);
        assert_eq!(
            feature.blocks(),
            [
                Region::between(20_000, 21_000),
                Region::between(21_100, 23_200)
            ]
        );
        let plain = LINE.replace("\tcg:Z:1000M100D2000M10I100M", "\tcg:Z:1000M40D2160M");
        let (_, short) = parse(&plain).unwrap().unwrap();
        assert_eq!(short.blocks(), [Region::between(20_000, 23_200)]);
    }

    #[test]
    fn cards_show_the_place_on_the_query_and_the_identity() {
        let (_, feature) = parse(LINE).unwrap().unwrap();
        let found: Vec<(String, String)> = fields(feature.extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pairs = [
            ("Query", "contig_7:1001-4100"),
            ("Query length", "50000"),
            ("Identity (%)", "90.6"),
            ("Matches", "2900"),
            ("Block length", "3200"),
            ("MAPQ", "60"),
            ("Type", "primary"),
            ("Edits", "300"),
            ("Divergence", "0.0123"),
        ];
        let wanted: Vec<(String, String)> = pairs
            .iter()
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(found, wanted);
    }

    #[test]
    fn missing_mapq_and_long_tags_do_not_show() {
        let line =
            "q\t100\t0\t100\t+\tchr2\t1000\t10\t110\t100\t100\t255\tcs:Z::100\tds:Z:x\ts1:i:90";
        let (_, feature) = parse(line).unwrap().unwrap();
        let labels: Vec<String> = fields(feature.extra())
            .into_iter()
            .map(|field| field.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Query",
                "Query length",
                "Identity (%)",
                "Matches",
                "Block length",
                "s1"
            ]
        );
    }

    #[test]
    fn short_or_wrong_lines_say_why() {
        assert_eq!(parse(""), Ok(None));
        assert!(parse("# comment").unwrap().is_none());
        for (line, reason) in [
            ("q\t100\t0\t100\t+\tchr2", Reason::MissingField),
            (
                "q\t100\t0\t100\t+\tchr2\t1000\tx\t110\t100\t100\t60",
                Reason::BadNumber,
            ),
            (
                "q\t100\t0\t100\t+\tchr2\t1000\t200\t110\t100\t100\t60",
                Reason::BadSpan,
            ),
            (
                "q\t100\t0\t100\t*\tchr2\t1000\t10\t110\t100\t100\t60",
                Reason::BadStrand,
            ),
        ] {
            assert_eq!(parse(line).map(|_| ()), Err(reason), "{line}");
        }
    }
}
