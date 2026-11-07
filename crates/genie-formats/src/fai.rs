use std::io::BufRead;

use crate::{Error, Reason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaiRecord {
    pub name: String,
    pub length: u64,
    pub offset: u64,
    pub line_bases: u64,
    pub line_width: u64,
}

pub fn parse(reader: impl BufRead) -> Result<Vec<FaiRecord>, Error> {
    let mut records = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = parse_line(&line).map_err(|reason| Error::Format {
            line: index as u64 + 1,
            reason,
        })?;
        records.push(record);
    }
    Ok(records)
}

fn parse_line(line: &str) -> Result<FaiRecord, Reason> {
    let mut fields = line.split('\t');
    let name = fields
        .next()
        .filter(|name| !name.is_empty())
        .ok_or(Reason::MissingField)?;
    let mut number = || -> Result<u64, Reason> {
        fields
            .next()
            .ok_or(Reason::MissingField)?
            .parse()
            .map_err(|_| Reason::BadNumber)
    };
    let length = number()?;
    let offset = number()?;
    let line_bases = number()?;
    let line_width = number()?;
    if line_bases == 0 && length > 0 {
        return Err(Reason::ZeroLineBases);
    }
    if line_width < line_bases {
        return Err(Reason::ShortLineWidth);
    }
    Ok(FaiRecord {
        name: name.to_owned(),
        length,
        offset,
        line_bases,
        line_width,
    })
}

#[cfg(test)]
mod tests {
    use super::{FaiRecord, parse};
    use crate::{Error, Reason};

    fn record(name: &str, length: u64, offset: u64, line_bases: u64, line_width: u64) -> FaiRecord {
        FaiRecord {
            name: name.to_owned(),
            length,
            offset,
            line_bases,
            line_width,
        }
    }

    fn failure(text: &str) -> (u64, Reason) {
        match parse(text.as_bytes()) {
            Err(Error::Format { line, reason }) => (line, reason),
            other => panic!("expected a format error, got {other:?}"),
        }
    }

    #[test]
    fn parses_records_in_order() {
        let text = "chr1\t248956422\t112\t70\t71\nchr2\t242193529\t252513167\t70\t71\n";
        let records = parse(text.as_bytes()).unwrap();
        assert_eq!(
            records,
            [
                record("chr1", 248_956_422, 112, 70, 71),
                record("chr2", 242_193_529, 252_513_167, 70, 71),
            ]
        );
    }

    #[test]
    fn ignores_quality_column_and_blank_lines() {
        let records = parse("read1\t4\t7\t4\t5\t14\n\n".as_bytes()).unwrap();
        assert_eq!(records, [record("read1", 4, 7, 4, 5)]);
    }

    #[test]
    fn reports_line_and_reason() {
        assert_eq!(
            failure("chr1\t10\t6\t10\t11\nchr2\t10\n"),
            (2, Reason::MissingField)
        );
        assert_eq!(failure("chr1\tten\t6\t10\t11\n"), (1, Reason::BadNumber));
        assert_eq!(failure("chr1\t10\t6\t10\t9\n"), (1, Reason::ShortLineWidth));
        assert_eq!(failure("chr1\t10\t6\t0\t1\n"), (1, Reason::ZeroLineBases));
    }

    #[test]
    fn error_messages_name_the_line() {
        let error = Error::Format {
            line: 7,
            reason: Reason::MissingField,
        };
        assert_eq!(error.to_string(), "Line 7: a field is missing.");
    }
}
