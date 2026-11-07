use std::io::{BufRead, Read, Seek};

use genie_core::Region;
use genie_core::signal::Summary;

use crate::bgzf::Bgzf;
use crate::tabix::Index;
use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

pub fn parse(line: &str) -> Result<Option<(&str, Summary)>, Reason> {
    let mut fields = line.split_ascii_whitespace();
    let Some(contig) = fields.next() else {
        return Ok(None);
    };
    if contig.starts_with('#') || contig == "track" || contig == "browser" {
        return Ok(None);
    }
    let (Some(start), Some(end), Some(value)) = (fields.next(), fields.next(), fields.next())
    else {
        return Err(Reason::MissingField);
    };
    let number = |text: &str| text.parse::<u64>().map_err(|_| Reason::BadNumber);
    let span = Region::new(number(start)?, number(end)?).ok_or(Reason::BadSpan)?;
    let value: f32 = value.parse().map_err(|_| Reason::BadNumber)?;
    if !value.is_finite() {
        return Err(Reason::BadNumber);
    }
    Ok(Some((contig, Summary::single(span, value))))
}

pub fn read_table(reader: impl BufRead) -> Result<Table<Summary>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, summary)) = parsed {
            rows.push((contig.to_owned(), summary.span, summary));
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
) -> Result<Vec<Summary>, Error> {
    let mut found = Vec::new();
    index.query(reader, contig, region, |line| {
        if let Some((_, summary)) = parse(line).map_err(Error::Invalid)? {
            found.push(summary);
        }
        Ok(())
    })?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::signal::Summary;

    use super::{parse, read_table};
    use crate::{Error, Reason};

    fn value(start: u64, end: u64, value: f32) -> Summary {
        Summary::single(Region::between(start, end), value)
    }

    #[test]
    fn parses_tab_or_space_separated_lines() {
        assert_eq!(
            parse("chr1\t10\t20\t1.5"),
            Ok(Some(("chr1", value(10, 20, 1.5))))
        );
        assert_eq!(
            parse("chr2 0 5  -0.25"),
            Ok(Some(("chr2", value(0, 5, -0.25))))
        );
    }

    #[test]
    fn skips_headers_and_reports_bad_lines() {
        for header in [
            "track type=bedGraph name=x",
            "browser position chr1",
            "#chrom",
        ] {
            assert_eq!(parse(header), Ok(None), "{header}");
        }
        assert_eq!(parse("chr1\t10\t20"), Err(Reason::MissingField));
        assert_eq!(parse("chr1\tx\t20\t1"), Err(Reason::BadNumber));
        assert_eq!(parse("chr1\t10\t20\tnan"), Err(Reason::BadNumber));
        assert_eq!(parse("chr1\t30\t20\t1"), Err(Reason::BadSpan));
    }

    #[test]
    fn tables_hold_every_record() {
        let text = "track type=bedGraph\nchr1\t0\t10\t1\nchr1\t10\t30\t2\nchr2\t5\t6\t3\n";
        let table = read_table(text.as_bytes()).unwrap();
        assert_eq!(
            table.query("chr1", Region::between(5, 12)),
            [value(0, 10, 1.0), value(10, 30, 2.0)]
        );
        assert_eq!(
            table.query("chr2", Region::between(0, 100)),
            [value(5, 6, 3.0)]
        );
        let error = read_table("chr1\t0\t10\t1\nchr1\t0\n".as_bytes()).unwrap_err();
        assert!(matches!(error, Error::Format { line: 2, .. }));
    }
}
