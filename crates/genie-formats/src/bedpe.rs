//! BEDPE files: links between two regions, as Hi-C and ChIA-PET loop callers write them.

use std::io::BufRead;

use genie_core::Region;
use genie_core::interaction::Interaction;

use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

/// The first column after the ten standard columns. Loop callers often put a color here.
const EXTRA: usize = 10;
/// The names of the columns of the two ends in the header of a loop list.
const ENDS: [&str; 6] = ["chr1", "x1", "x2", "chr2", "y1", "y2"];
/// Names of a column with the value of a link, as loop callers write them.
const VALUES: [&str; 6] = ["score", "value", "o", "observed", "count", "counts"];

/// Where the name, the value and the color of a link are, and where the fields that its card
/// shows start. A column can be left out of the fields, as the color that the arc shows.
#[derive(Debug, Clone, Copy)]
struct Layout {
    name: Option<usize>,
    score: Option<usize>,
    color: Option<usize>,
    fields: usize,
    skip: Option<usize>,
}

/// The ten standard columns of BEDPE, and a color after them.
const STANDARD: Layout = Layout {
    name: Some(6),
    score: Some(7),
    color: Some(EXTRA),
    fields: EXTRA,
    skip: None,
};

impl Layout {
    /// The layout of a header that names its own columns after the ends, as in the loop
    /// lists of `HiCCUPS`: `chr1 x1 x2 chr2 y1 y2 color o e_bl …`.
    fn named(names: &[String]) -> Option<Self> {
        let after: Vec<String> = names
            .iter()
            .skip(6)
            .map(|name| name.to_ascii_lowercase())
            .collect();
        if after.first().is_none_or(|first| first == "name") {
            return None;
        }
        let at = |wanted: &[&str]| {
            after
                .iter()
                .position(|name| wanted.contains(&name.as_str()))
                .map(|place| place + 6)
        };
        let color = at(&["color", "colour"]);
        Some(Self {
            name: at(&["name"]),
            score: at(&VALUES),
            color,
            fields: 6,
            skip: color,
        })
    }

    fn columns(self, names: &[String]) -> Vec<String> {
        names
            .iter()
            .enumerate()
            .skip(self.fields)
            .filter(|(place, _)| Some(*place) != self.skip)
            .map(|(_, name)| name.clone())
            .collect()
    }
}

/// True for the header line of a loop list, as Juicer and `HiCCUPS` write it.
pub fn is_loop_list(line: &str) -> bool {
    let fields = fields(line.trim_start_matches('#'));
    fields.len() >= ENDS.len()
        && fields
            .iter()
            .zip(ENDS)
            .all(|(field, name)| field.eq_ignore_ascii_case(name))
}

/// One line of a BEDPE file.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub first: (String, Region),
    pub second: (String, Region),
    pub name: String,
    pub score: Option<f64>,
    pub color: Option<[u8; 3]>,
    /// The columns after column 10, separated by tabs.
    pub extra: String,
}

/// A BEDPE file in memory: each link is on the contig of each of its ends.
#[derive(Debug, Default)]
pub struct Bedpe {
    pub table: Table<Interaction>,
    /// The names of the columns after column 10, from the header line of the file.
    pub columns: Vec<String>,
}

fn fields(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        line.split('\t').map(str::trim).collect()
    } else {
        line.split_ascii_whitespace().collect()
    }
}

/// One line, or nothing for a comment, a header or a link with an end that the file does not
/// give.
pub fn parse(line: &str) -> Result<Option<Pair>, Reason> {
    parse_with(line, STANDARD)
}

fn parse_with(line: &str, layout: Layout) -> Result<Option<Pair>, Reason> {
    let fields = fields(line);
    let first = fields.first().copied().unwrap_or_default();
    if first.is_empty() || first.starts_with('#') || first == "track" || first == "browser" {
        return Ok(None);
    }
    if fields.len() < 6 {
        return Err(Reason::MissingField);
    }
    if fields[1] == "-1" || fields[4] == "-1" {
        return Ok(None);
    }
    let number = |text: &str| text.parse::<u64>().map_err(|_| Reason::BadNumber);
    let region = |start, end| Region::new(number(start)?, number(end)?).ok_or(Reason::BadSpan);
    let text = |index: usize| {
        fields
            .get(index)
            .copied()
            .filter(|text| !text.is_empty() && *text != ".")
    };
    let extra: Vec<&str> = fields
        .iter()
        .enumerate()
        .skip(layout.fields)
        .filter(|(place, _)| Some(*place) != layout.skip)
        .map(|(_, field)| *field)
        .collect();
    Ok(Some(Pair {
        first: (fields[0].to_owned(), region(fields[1], fields[2])?),
        second: (fields[3].to_owned(), region(fields[4], fields[5])?),
        name: layout.name.and_then(text).unwrap_or_default().to_owned(),
        score: layout
            .score
            .and_then(text)
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|value| value.is_finite()),
        color: layout.color.and_then(text).and_then(rgb),
        extra: extra.join("\t"),
    }))
}

/// A color written as `r,g,b`.
pub fn rgb(text: &str) -> Option<[u8; 3]> {
    let mut parts = text.split(',').map(|part| part.trim().parse::<u8>().ok());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Some(red)), Some(Some(green)), Some(Some(blue)), None) => Some([red, green, blue]),
        _ => None,
    }
}

/// The names of the columns, if the line is a header such as
/// `#chr1 x1 x2 chr2 y1 y2 name score strand1 strand2 color observed`.
fn header(line: &str) -> Option<Vec<String>> {
    let fields = fields(line.trim_start_matches('#'));
    let numbered = fields.get(1)?.parse::<u64>().is_ok();
    (fields.len() >= 6 && !numbered).then(|| fields.iter().map(|name| (*name).to_owned()).collect())
}

fn links(pair: Pair) -> Vec<(String, Region, Interaction)> {
    let Pair {
        first: (one, a),
        second: (two, b),
        name,
        score,
        color,
        extra,
    } = pair;
    let link = |near, far, elsewhere| Interaction {
        name: name.clone(),
        value: score,
        color,
        extra: extra.clone(),
        ..Interaction::new(near, far, elsewhere)
    };
    if one == two {
        let found = link(a, b, None);
        return vec![(one, found.span(), found)];
    }
    let there = link(b, a, Some(one.clone()));
    let here = link(a, b, Some(two.clone()));
    vec![(one, a, here), (two, b, there)]
}

pub fn read_table(reader: impl BufRead) -> Result<Bedpe, Error> {
    let mut rows = Vec::new();
    let mut columns = Vec::new();
    let mut layout = STANDARD;
    let mut started = false;
    lines(reader, |number, line| {
        if !started && let Some(names) = header(line) {
            layout = Layout::named(&names).unwrap_or(STANDARD);
            columns = layout.columns(&names);
            return Ok(());
        }
        let pair = parse_with(line, layout).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some(pair) = pair {
            started = true;
            rows.extend(links(pair));
        }
        Ok(())
    })?;
    Ok(Bedpe {
        table: Table::new(rows),
        columns,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::{Pair, is_loop_list, parse, read_table, rgb};
    use crate::{Error, Reason};

    #[test]
    fn lines_give_both_ends_and_the_columns_of_loop_callers() {
        let line = "chr1\t1000\t2000\tchr1\t50000\t52000\t.\t.\t.\t.\t255,0,0\t45\t12.5";
        assert_eq!(
            parse(line),
            Ok(Some(Pair {
                first: ("chr1".to_owned(), Region::between(1_000, 2_000)),
                second: ("chr1".to_owned(), Region::between(50_000, 52_000)),
                name: String::new(),
                score: None,
                color: Some([255, 0, 0]),
                extra: "255,0,0\t45\t12.5".to_owned(),
            }))
        );
        let plain = parse("1 10 20 X 30 40 loop7 8.5 + -").unwrap().unwrap();
        assert_eq!((plain.name.as_str(), plain.score), ("loop7", Some(8.5)));
        assert_eq!(plain.second.0, "X");
        assert_eq!((plain.color, plain.extra.as_str()), (None, ""));
        for skipped in [
            "",
            "# juicer_tools",
            "track name=loops",
            "chr1\t-1\t-1\tchr2\t5\t9",
        ] {
            assert_eq!(parse(skipped), Ok(None), "{skipped}");
        }
        assert_eq!(parse("chr1\t1\t2\tchr1\t3"), Err(Reason::MissingField));
        assert_eq!(parse("chr1\t9\t2\tchr1\t3\t4"), Err(Reason::BadSpan));
        assert_eq!(parse("chr1\tx\t2\tchr1\t3\t4"), Err(Reason::BadNumber));
        assert_eq!(rgb("0, 128 ,255"), Some([0, 128, 255]));
        assert_eq!(rgb("256,0,0"), None);
    }

    #[test]
    fn tables_find_links_that_cross_the_view_and_both_ends_of_far_links() {
        let text = "#chr1\tx1\tx2\tchr2\ty1\ty2\tname\tscore\tstrand1\tstrand2\tcolor\tobserved\n\
            # juicer_tools version 1.22.01\n\
            chr1\t1000\t2000\tchr1\t900000\t902000\ta\t5\t.\t.\t0,0,255\t30\n\
            chr1\t50000\t51000\tchr1\t60000\t61000\tb\t9\t.\t.\t0,0,255\t12\n\
            chr1\t70000\t71000\tchr2\t5000\t6000\tc\t2\t.\t.\t0,0,255\t4\n";
        let bedpe = read_table(Cursor::new(text)).unwrap();
        assert_eq!(bedpe.columns, ["color", "observed"]);
        assert_eq!(bedpe.table.contigs(), ["chr1", "chr2"]);
        let names = |contig: &str, region: Region| -> Vec<String> {
            let mut found: Vec<String> = bedpe
                .table
                .query(contig, region)
                .into_iter()
                .map(|link| link.name)
                .collect();
            found.sort();
            found
        };
        assert_eq!(names("chr1", Region::between(400_000, 410_000)), ["a"]);
        assert_eq!(
            names("chr1", Region::between(55_000, 75_000)),
            ["a", "b", "c"]
        );
        let far = bedpe.table.query("chr2", Region::between(0, 10_000));
        assert_eq!(far.len(), 1);
        assert_eq!(far[0].near, Region::between(5_000, 6_000));
        assert_eq!(far[0].far, Region::between(70_000, 71_000));
        assert_eq!(far[0].elsewhere.as_deref(), Some("chr1"));
        assert_eq!((far[0].value, far[0].color), (Some(2.0), Some([0, 0, 255])));
        let plain = read_table(Cursor::new("chr1 x1 x2 chr2 y1 y2\nchr1 1 2 chr1 5 6\n")).unwrap();
        assert_eq!(plain.columns, Vec::<String>::new());
        assert_eq!(plain.table.query("chr1", Region::between(0, 10)).len(), 1);
        let broken = read_table(Cursor::new(
            "chr1\t1\t2\tchr1\t5\t6\nchr1\tx\t2\tchr1\t5\t6\n",
        ));
        assert!(matches!(
            broken,
            Err(Error::Format {
                line: 2,
                reason: Reason::BadNumber
            })
        ));
    }

    /// The loop lists of `HiCCUPS` name all their columns, with the color right after the ends.
    #[test]
    fn loop_lists_take_the_names_of_their_columns_and_their_color() {
        let text = "chr1\tx1\tx2\tchr2\ty1\ty2\tcolor\to\te_bl\tfdr_bl\n\
            1\t1050000\t1060000\t1\t1180000\t1190000\t0,255,255\t241\t143.514\t1.1e-16\n";
        assert!(is_loop_list(text.lines().next().unwrap()));
        assert!(is_loop_list("#chr1\tx1\tx2\tchr2\ty1\ty2\tname\tscore"));
        assert!(!is_loop_list("chr1\t100\t200\tchr1\t300\t400"));
        assert!(!is_loop_list("hub mine"));
        let bedpe = read_table(Cursor::new(text)).unwrap();
        assert_eq!(bedpe.columns, ["o", "e_bl", "fdr_bl"]);
        let links = bedpe.table.query("1", Region::between(0, 2_000_000));
        assert_eq!(links.len(), 1);
        let link = &links[0];
        assert_eq!(link.name, "");
        assert_eq!((link.value, link.color), (Some(241.0), Some([0, 255, 255])));
        assert_eq!(link.extra, "241\t143.514\t1.1e-16");
    }
}
