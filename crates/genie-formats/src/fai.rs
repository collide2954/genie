use std::io::{self, BufRead, Write};

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

pub fn write(records: &[FaiRecord], mut writer: impl Write) -> io::Result<()> {
    for record in records {
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}",
            record.name, record.length, record.offset, record.line_bases, record.line_width
        )?;
    }
    writer.flush()
}

pub fn build(mut reader: impl BufRead) -> Result<Vec<FaiRecord>, Error> {
    let mut builder = Builder::default();
    let mut line = Vec::new();
    let mut number = 0;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(builder.records);
        }
        number += 1;
        builder.line(&line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
    }
}

#[derive(Default)]
struct Builder {
    records: Vec<FaiRecord>,
    position: u64,
    short: bool,
    blank: bool,
}

impl Builder {
    fn line(&mut self, line: &[u8]) -> Result<(), Reason> {
        self.position += line.len() as u64;
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        if let Some(header) = content.strip_prefix(b">") {
            return self.header(header);
        }
        let Some(record) = self.records.last_mut() else {
            return if content.is_empty() {
                Ok(())
            } else {
                Err(Reason::NoHeader)
            };
        };
        if content.is_empty() {
            self.blank = true;
            return Ok(());
        }
        if self.blank {
            return Err(Reason::BlankLine);
        }
        let bases = content.len() as u64;
        let width = line.len() as u64;
        if record.line_bases == 0 {
            record.line_bases = bases;
            record.line_width = width;
        } else if self.short
            || bases > record.line_bases
            || (bases == record.line_bases && width != record.line_width && line.ends_with(b"\n"))
        {
            return Err(Reason::UnevenLines);
        } else if bases < record.line_bases {
            self.short = true;
        }
        record.length += bases;
        Ok(())
    }

    fn header(&mut self, header: &[u8]) -> Result<(), Reason> {
        let name = header
            .split(u8::is_ascii_whitespace)
            .next()
            .unwrap_or_default();
        if name.is_empty() {
            return Err(Reason::EmptyName);
        }
        self.records.push(FaiRecord {
            name: String::from_utf8_lossy(name).into_owned(),
            length: 0,
            offset: self.position,
            line_bases: 0,
            line_width: 0,
        });
        self.short = false;
        self.blank = false;
        Ok(())
    }
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
    use super::{FaiRecord, build, parse, write};
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

    fn build_failure(text: &str) -> (u64, Reason) {
        match build(text.as_bytes()) {
            Err(Error::Format { line, reason }) => (line, reason),
            other => panic!("expected a format error, got {other:?}"),
        }
    }

    #[test]
    fn build_records_offsets_and_line_shape() {
        let records = build(">chr1 desc\nACGT\nAC\n>chr2\nGG\n".as_bytes()).unwrap();
        assert_eq!(
            records,
            [record("chr1", 6, 11, 4, 5), record("chr2", 2, 25, 2, 3)]
        );
    }

    #[test]
    fn build_handles_crlf_and_missing_final_newline() {
        assert_eq!(
            build(">c\r\nACGT\r\nAC\r\n".as_bytes()).unwrap(),
            [record("c", 6, 4, 4, 6)]
        );
        assert_eq!(
            build(">c\nACGT\nAC".as_bytes()).unwrap(),
            [record("c", 6, 3, 4, 5)]
        );
    }

    #[test]
    fn build_allows_blank_lines_between_records() {
        let records = build(">c\nACGT\n\n\n>d\nAC\n".as_bytes()).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn build_rejects_malformed_fasta() {
        assert_eq!(build_failure("ACGT\n"), (1, Reason::NoHeader));
        assert_eq!(build_failure(">\nACGT\n"), (1, Reason::EmptyName));
        assert_eq!(build_failure(">c\nACG\nACGT\n"), (3, Reason::UnevenLines));
        assert_eq!(
            build_failure(">c\nACGT\nAC\nACGT\n"),
            (4, Reason::UnevenLines)
        );
        assert_eq!(build_failure(">c\nACGT\n\nACGT\n"), (4, Reason::BlankLine));
    }

    #[test]
    fn write_produces_samtools_text() {
        let mut text = Vec::new();
        write(&[record("chr1", 6, 11, 4, 5)], &mut text).unwrap();
        assert_eq!(text, b"chr1\t6\t11\t4\t5\n");
    }

    #[test]
    fn written_index_parses_back() {
        let records = build(">a\nACGT\nAC\n>b x\nGG\n".as_bytes()).unwrap();
        let mut text = Vec::new();
        write(&records, &mut text).unwrap();
        assert_eq!(parse(text.as_slice()).unwrap(), records);
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
