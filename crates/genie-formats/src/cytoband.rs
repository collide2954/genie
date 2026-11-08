use std::io::{self, BufRead, Write};

use genie_core::{Band, Region, Stain};

use crate::bed::number;
use crate::text::lines;
use crate::{Error, Reason};

pub fn parse(line: &str) -> Result<Option<(&str, Band)>, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut fields = line.split('\t');
    let (Some(contig), Some(start), Some(end)) = (fields.next(), fields.next(), fields.next())
    else {
        return Err(Reason::MissingField);
    };
    let span = Region::new(number(start)?, number(end)?).ok_or(Reason::BadSpan)?;
    let name = fields.next().unwrap_or_default().trim().to_owned();
    let stain = Stain::parse(fields.next().unwrap_or_default().trim());
    Ok(Some((contig, Band { span, name, stain })))
}

pub fn read(reader: impl BufRead) -> Result<Vec<(String, Band)>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, band)) = parsed {
            rows.push((contig.to_owned(), band));
        }
        Ok(())
    })?;
    Ok(rows)
}

pub fn write<'a>(
    mut writer: impl Write,
    rows: impl IntoIterator<Item = (&'a str, &'a Band)>,
) -> io::Result<()> {
    for (contig, band) in rows {
        let stain = match band.stain {
            Stain::Gneg => "gneg".to_owned(),
            Stain::Gpos(depth) => format!("gpos{depth}"),
            Stain::Acen => "acen".to_owned(),
            Stain::Gvar => "gvar".to_owned(),
            Stain::Stalk => "stalk".to_owned(),
        };
        writeln!(
            writer,
            "{contig}\t{}\t{}\t{}\t{stain}",
            band.span.start(),
            band.span.end(),
            band.name.replace(['\t', '\r', '\n'], " ")
        )?;
    }
    writer.flush()
}

#[cfg(test)]
mod tests {
    use genie_core::{Band, Region, Stain};

    use super::{parse, read, write};
    use crate::{Error, Reason};

    fn band(start: u64, end: u64, name: &str, stain: Stain) -> Band {
        Band {
            span: Region::new(start, end).unwrap(),
            name: name.to_owned(),
            stain,
        }
    }

    #[test]
    fn reads_ucsc_band_tables() {
        let text = "#chrom\tchromStart\tchromEnd\tname\tgieStain\n\
                    chr1\t0\t2300000\tp36.33\tgneg\n\
                    chr1\t121700000\t123400000\tp11.1\tacen\n\
                    chrI\t0\t15072434\t\tgneg\n";
        assert_eq!(
            read(text.as_bytes()).unwrap(),
            [
                ("chr1".to_owned(), band(0, 2_300_000, "p36.33", Stain::Gneg)),
                (
                    "chr1".to_owned(),
                    band(121_700_000, 123_400_000, "p11.1", Stain::Acen)
                ),
                ("chrI".to_owned(), band(0, 15_072_434, "", Stain::Gneg)),
            ]
        );
    }

    #[test]
    fn rejects_bad_lines() {
        for (line, reason) in [
            ("chr1\t10", Reason::MissingField),
            ("chr1\tx\t20\tp1\tgneg", Reason::BadNumber),
            ("chr1\t30\t20\tp1\tgneg", Reason::BadSpan),
        ] {
            assert_eq!(parse(line), Err(reason), "{line}");
        }
        let error = read("chr1\t0\t5\tp1\tgneg\nchr1\t5\n".as_bytes()).unwrap_err();
        assert!(matches!(error, Error::Format { line: 2, .. }));
    }

    #[test]
    fn bands_write_and_read_back() {
        let rows = [
            ("chr1", band(0, 10, "p36.33", Stain::Gpos(25))),
            ("chr1", band(10, 20, "p11", Stain::Gvar)),
            ("chr13", band(0, 5, "p12", Stain::Stalk)),
        ];
        let mut written = Vec::new();
        write(
            &mut written,
            rows.iter().map(|(contig, band)| (*contig, band)),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(written.clone()).unwrap(),
            "chr1\t0\t10\tp36.33\tgpos25\nchr1\t10\t20\tp11\tgvar\nchr13\t0\t5\tp12\tstalk\n"
        );
        let back = read(written.as_slice()).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].1, rows[0].1);
        assert_eq!(back[2].0, "chr13");
    }
}
