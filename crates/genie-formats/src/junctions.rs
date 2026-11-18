//! Splice junction files: the `SJ.out.tab` files of STAR, and the junction BED files of
//! `TopHat` and regtools, where each junction is a line with a block at each side of its intron.

use std::io::BufRead;

use genie_core::pileup::Junction;
use genie_core::{Region, Strand};

use crate::fields::Field;
use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

/// The splice motifs of STAR, by their number in column 5.
const MOTIFS: [&str; 7] = [
    "non-canonical",
    "GT/AG",
    "CT/AC",
    "GC/AG",
    "CT/GC",
    "AT/AC",
    "GT/AT",
];

/// How the lines of a junction file give a junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The `SJ.out.tab` file of STAR: the first and the last base of the intron, counted from 1.
    Star,
    /// A BED file: a BED12 line with a block at each side of the intron, or a shorter line that
    /// is the intron.
    Bed,
}

impl Layout {
    /// The layout of a junction file by its name: `SJ.out.tab` or `junctions.bed`, also
    /// compressed.
    pub fn of(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let stem = lower
            .strip_suffix(".gz")
            .or_else(|| lower.strip_suffix(".bgz"))
            .unwrap_or(&lower);
        if stem.ends_with("sj.out.tab") {
            Some(Self::Star)
        } else if stem.ends_with("junctions.bed") {
            Some(Self::Bed)
        } else {
            None
        }
    }
}

/// True for the track line of a BED file of junctions, as `TopHat` and IGV write it.
pub fn is_junction_track(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("track")
        && (lower.contains("graphtype=junctions") || lower.contains("name=junctions"))
}

/// A junction of a file, with the other fields of its line for the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub junction: Junction,
    pub fields: Vec<Field>,
}

fn field(label: &str, value: impl Into<String>) -> Field {
    Field {
        label: label.to_owned(),
        value: value.into(),
    }
}

fn number(text: &str) -> Result<u64, Reason> {
    text.parse().map_err(|_| Reason::BadNumber)
}

/// One line of a junction file with its contig, or nothing for a comment, a header or an empty
/// line.
pub fn parse(line: &str, layout: Layout) -> Result<Option<(String, Splice)>, Reason> {
    let fields: Vec<&str> = if line.contains('\t') {
        line.split('\t').map(str::trim).collect()
    } else {
        line.split_ascii_whitespace().collect()
    };
    let first = fields.first().copied().unwrap_or_default();
    if first.is_empty() || first.starts_with('#') || first == "track" || first == "browser" {
        return Ok(None);
    }
    let splice = match layout {
        Layout::Star => star(&fields)?,
        Layout::Bed => bed(&fields)?,
    };
    Ok(Some((first.to_owned(), splice)))
}

fn star(fields: &[&str]) -> Result<Splice, Reason> {
    if fields.len() < 9 {
        return Err(Reason::MissingField);
    }
    let (first, last) = (number(fields[1])?, number(fields[2])?);
    let count = |text: &str| text.parse::<u32>().map_err(|_| Reason::BadNumber);
    let (unique, multi, overhang) = (count(fields[6])?, count(fields[7])?, count(fields[8])?);
    let motif = MOTIFS
        .get(usize::try_from(number(fields[4])?).unwrap_or(usize::MAX))
        .ok_or(Reason::BadNumber)?;
    let annotated = if number(fields[5])? == 0 { "no" } else { "yes" };
    let strand = match fields[3] {
        "0" => Strand::Unknown,
        "1" => Strand::Forward,
        "2" => Strand::Reverse,
        _ => return Err(Reason::BadStrand),
    };
    let span = first
        .checked_sub(1)
        .and_then(|start| Region::new(start, last))
        .filter(|span| !span.is_empty())
        .ok_or(Reason::BadSpan)?;
    Ok(Splice {
        junction: Junction {
            span,
            count: unique,
            strand,
        },
        fields: vec![
            field("Unique reads", unique.to_string()),
            field("Multi-mapping reads", multi.to_string()),
            field("Motif", *motif),
            field("Annotated", annotated),
            field("Largest overhang", format!("{overhang} bp")),
        ],
    })
}

fn bed(fields: &[&str]) -> Result<Splice, Reason> {
    if fields.len() < 3 {
        return Err(Reason::MissingField);
    }
    let (start, end) = (number(fields[1])?, number(fields[2])?);
    let whole = Region::new(start, end).ok_or(Reason::BadSpan)?;
    let text = |index: usize| {
        fields
            .get(index)
            .copied()
            .filter(|text| !text.is_empty() && *text != ".")
    };
    let count = text(4)
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|count| count.is_finite() && *count >= 0.0)
        .map_or(0, |count| count.round().min(f64::from(u32::MAX)) as u32);
    let strand = match text(5) {
        Some("+") => Strand::Forward,
        Some("-") => Strand::Reverse,
        _ => Strand::Unknown,
    };
    let mut found = Vec::new();
    if let Some(name) = text(3) {
        found.push(field("Name", name));
    }
    if text(4).is_some() {
        found.push(field("Reads", count.to_string()));
    }
    let span = if fields.len() >= 12 {
        let list = |text: &str| -> Result<Vec<u64>, Reason> {
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(number)
                .collect()
        };
        let (sizes, starts) = (list(fields[10])?, list(fields[11])?);
        if number(fields[9])? != 2 || sizes.len() != 2 || starts.len() != 2 {
            return Err(Reason::BadBlocks);
        }
        let (left, right) = (start + sizes[0], start + starts[1]);
        if left >= right || right + sizes[1] > end {
            return Err(Reason::BadBlocks);
        }
        found.push(field(
            "Overhangs",
            format!("{} bp, {} bp", sizes[0], sizes[1]),
        ));
        Region::between(left, right)
    } else {
        whole
    };
    Ok(Splice {
        junction: Junction {
            span,
            count,
            strand,
        },
        fields: found,
    })
}

/// A junction file in memory, by contig.
pub fn read_table(reader: impl BufRead, layout: Layout) -> Result<Table<Splice>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let found = parse(line, layout).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, splice)) = found {
            rows.push((contig, splice.junction.span, splice));
        }
        Ok(())
    })?;
    Ok(Table::new(rows))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::pileup::Junction;
    use genie_core::{Region, Strand};

    use super::{Layout, parse, read_table};
    use crate::Reason;

    fn pairs(fields: &[crate::fields::Field]) -> Vec<(&str, &str)> {
        fields
            .iter()
            .map(|field| (field.label.as_str(), field.value.as_str()))
            .collect()
    }

    #[test]
    fn star_lines_give_the_intron_the_unique_reads_and_the_other_counts() {
        let (contig, splice) = parse("chr1\t14830\t14969\t2\t2\t1\t2\t7\t38", Layout::Star)
            .unwrap()
            .unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(
            splice.junction,
            Junction {
                span: Region::between(14_829, 14_969),
                count: 2,
                strand: Strand::Reverse,
            }
        );
        assert_eq!(
            pairs(&splice.fields),
            [
                ("Unique reads", "2"),
                ("Multi-mapping reads", "7"),
                ("Motif", "CT/AC"),
                ("Annotated", "yes"),
                ("Largest overhang", "38 bp"),
            ]
        );
        let (_, other) = parse("chr2\t100\t200\t0\t0\t0\t5\t0\t12", Layout::Star)
            .unwrap()
            .unwrap();
        assert_eq!(other.junction.strand, Strand::Unknown);
        assert_eq!(
            pairs(&other.fields)[2..4],
            [("Motif", "non-canonical"), ("Annotated", "no")]
        );
    }

    #[test]
    fn bad_star_lines_say_why() {
        let star = |line| parse(line, Layout::Star);
        assert_eq!(star("chr1\t100\t200\t1"), Err(Reason::MissingField));
        assert_eq!(
            star("chr1\tx\t200\t1\t1\t0\t5\t0\t12"),
            Err(Reason::BadNumber)
        );
        assert_eq!(
            star("chr1\t100\t200\t3\t1\t0\t5\t0\t12"),
            Err(Reason::BadStrand)
        );
        assert_eq!(
            star("chr1\t300\t200\t1\t1\t0\t5\t0\t12"),
            Err(Reason::BadSpan)
        );
        assert_eq!(star(""), Ok(None));
        assert_eq!(star("# a comment"), Ok(None));
    }

    #[test]
    fn junction_bed_lines_give_the_intron_between_the_two_blocks() {
        let line =
            "chr1\t14782\t15038\tJUNC00000001\t38\t-\t14782\t15038\t255,0,0\t2\t47,69\t0,187";
        let (contig, splice) = parse(line, Layout::Bed).unwrap().unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(
            splice.junction,
            Junction {
                span: Region::between(14_829, 14_969),
                count: 38,
                strand: Strand::Reverse,
            }
        );
        assert_eq!(
            pairs(&splice.fields),
            [
                ("Name", "JUNC00000001"),
                ("Reads", "38"),
                ("Overhangs", "47 bp, 69 bp"),
            ]
        );
        let (_, plain) = parse("chr1\t14829\t14969\tJ2\t5\t+", Layout::Bed)
            .unwrap()
            .unwrap();
        assert_eq!(plain.junction.span, Region::between(14_829, 14_969));
        assert_eq!(plain.junction.count, 5);
        assert_eq!(plain.junction.strand, Strand::Forward);
        let bed = |line| parse(line, Layout::Bed);
        assert_eq!(bed("track name=junctions description=\"TopHat\""), Ok(None));
        assert_eq!(bed("browser position chr1:1-100"), Ok(None));
        assert_eq!(
            bed("chr1\t100\t300\tJ\t5\t+\t100\t300\t0\t3\t10,10,10\t0,50,190"),
            Err(Reason::BadBlocks)
        );
        assert_eq!(
            bed("chr1\t100\t300\tJ\t5\t+\t100\t300\t0\t2\t150,150\t0,150"),
            Err(Reason::BadBlocks)
        );
    }

    #[test]
    fn tables_keep_the_junctions_of_each_contig() {
        let text = "track name=junctions\nchr1\t100\t400\tA\t3\t+\t100\t400\t0\t2\t50,50\t0,250\nchr2\t10\t90\tB\t1\t-\nchr1\t1000\t1300\tC\t9\t+\t1000\t1300\t0\t2\t20,20\t0,280\n";
        let table = read_table(Cursor::new(text), Layout::Bed).unwrap();
        assert_eq!(table.contigs(), ["chr1", "chr2"]);
        let found = table.query("chr1", Region::between(0, 2_000));
        let spans: Vec<Region> = found.iter().map(|splice| splice.junction.span).collect();
        assert_eq!(
            spans,
            [Region::between(150, 350), Region::between(1_020, 1_280)]
        );
        let broken = read_table(Cursor::new("chr1\tx\t9\tA\t1\t+\n"), Layout::Bed);
        assert!(matches!(
            broken,
            Err(crate::Error::Format {
                line: 1,
                reason: Reason::BadNumber
            })
        ));
    }

    #[test]
    fn names_and_track_lines_tell_junction_files() {
        assert_eq!(Layout::of("sample.SJ.out.tab"), Some(Layout::Star));
        assert_eq!(Layout::of("SJ.out.tab.gz"), Some(Layout::Star));
        assert_eq!(Layout::of("junctions.bed"), Some(Layout::Bed));
        assert_eq!(Layout::of("liver_junctions.bed.gz"), Some(Layout::Bed));
        assert_eq!(Layout::of("genes.bed"), None);
        assert_eq!(Layout::of("table.tab"), None);
        assert!(super::is_junction_track("track graphType=junctions name=x"));
        assert!(super::is_junction_track(
            "track name=junctions description=\"TopHat\""
        ));
        assert!(!super::is_junction_track("track name=genes"));
        assert!(!super::is_junction_track("chr1\t1\t2"));
    }
}
