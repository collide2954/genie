use std::io::{self, BufRead, Write};

use genie_core::{Feature, Region, Strand};

use crate::features::Entry;
use crate::text::lines;
use crate::{Error, Reason};

const COLUMNS: usize = 12;

pub fn regions(reader: impl BufRead) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, feature)) = parsed {
            entries.push(Entry {
                name: feature.name().to_owned(),
                contig: contig.to_owned(),
                span: feature.span(),
            });
        }
        Ok(())
    })?;
    Ok(entries)
}

pub fn write(mut writer: impl Write, entries: &[Entry]) -> io::Result<()> {
    for entry in entries {
        let (start, end) = (entry.span.start(), entry.span.end());
        write!(writer, "{}\t{start}\t{end}", entry.contig)?;
        if !entry.name.is_empty() {
            write!(writer, "\t{}", entry.name.replace(['\t', '\r', '\n'], " "))?;
        }
        writeln!(writer)?;
    }
    writer.flush()
}

pub fn write_features<'a>(
    mut writer: impl Write,
    rows: impl IntoIterator<Item = (&'a str, &'a Feature)>,
) -> io::Result<()> {
    for (contig, feature) in rows {
        let span = feature.span();
        let thick = feature
            .coding()
            .unwrap_or_else(|| Region::between(span.start(), span.start()));
        let strand = match feature.strand() {
            Strand::Forward => '+',
            Strand::Reverse => '-',
            Strand::Unknown => '.',
        };
        let color = feature
            .color()
            .map_or_else(|| "0".to_owned(), |[r, g, b]| format!("{r},{g},{b}"));
        let blocks = feature.blocks();
        write!(
            writer,
            "{contig}\t{}\t{}\t{}\t0\t{strand}\t{}\t{}\t{color}\t{}\t",
            span.start(),
            span.end(),
            feature.name().replace(['\t', '\r', '\n'], " "),
            thick.start(),
            thick.end(),
            blocks.len()
        )?;
        for block in blocks {
            write!(writer, "{},", block.len())?;
        }
        writer.write_all(b"\t")?;
        for block in blocks {
            write!(writer, "{},", block.start() - span.start())?;
        }
        writeln!(writer)?;
    }
    writer.flush()
}

pub fn parse(line: &str) -> Result<Option<(&str, Feature)>, Reason> {
    if line.is_empty()
        || line.starts_with('#')
        || matches!(
            line.split_ascii_whitespace().next(),
            Some("track" | "browser")
        )
    {
        return Ok(None);
    }
    let mut fields = [""; COLUMNS];
    let mut count = 0;
    for (slot, field) in fields.iter_mut().zip(line.split('\t')) {
        *slot = field;
        count += 1;
    }
    if count < 3 {
        return Err(Reason::MissingField);
    }
    let start = number(fields[1])?;
    let span = Region::new(start, number(fields[2])?).ok_or(Reason::BadSpan)?;
    let strand = if count > 5 {
        strand(fields[5])?
    } else {
        Strand::Unknown
    };
    let mut feature = Feature::new(fields[3], span, strand);
    if count > 7 {
        let thick = Region::new(number(fields[6])?, number(fields[7])?).ok_or(Reason::BadSpan)?;
        feature = feature.with_coding((!thick.is_empty()).then_some(thick));
    }
    if count > 8 {
        feature = feature.with_color(rgb(fields[8]));
    }
    if count == COLUMNS {
        feature = feature.with_blocks(blocks(start, fields[9], fields[10], fields[11])?);
    }
    Ok(Some((fields[0], feature)))
}

fn rgb(field: &str) -> Option<[u8; 3]> {
    let mut parts = field
        .trim()
        .split(',')
        .map(|part| part.trim().parse::<u8>().ok());
    let color = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(color)
}

pub(crate) fn number(field: &str) -> Result<u64, Reason> {
    field.trim().parse().map_err(|_| Reason::BadNumber)
}

pub(crate) fn strand(field: &str) -> Result<Strand, Reason> {
    match field {
        "+" => Ok(Strand::Forward),
        "-" => Ok(Strand::Reverse),
        "." | "?" | "" => Ok(Strand::Unknown),
        _ => Err(Reason::BadStrand),
    }
}

fn blocks(start: u64, count: &str, sizes: &str, starts: &str) -> Result<Vec<Region>, Reason> {
    let count = number(count)?;
    let list = |field: &str| {
        field
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .map(number)
            .collect::<Result<Vec<_>, _>>()
    };
    let (sizes, starts) = (list(sizes)?, list(starts)?);
    if sizes.len() as u64 != count || starts.len() as u64 != count {
        return Err(Reason::BadBlocks);
    }
    Ok(starts
        .into_iter()
        .zip(sizes)
        .map(|(offset, size)| {
            let first = start.saturating_add(offset);
            Region::between(first, first.saturating_add(size))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{parse, regions, write, write_features};
    use crate::features::Entry;
    use crate::{Error, Reason};

    fn entry(name: &str, contig: &str, span: Region) -> Entry {
        Entry {
            name: name.to_owned(),
            contig: contig.to_owned(),
            span,
        }
    }

    #[test]
    fn regions_read_and_write_as_bed() {
        let text = "track name=regions\nchr1\t10\t20\tpromoter A\t0\t+\nchr2\t5\t9\n";
        let entries = regions(text.as_bytes()).unwrap();
        assert_eq!(
            entries,
            [
                entry("promoter A", "chr1", region(10, 20)),
                entry("", "chr2", region(5, 9))
            ]
        );
        let mut written = Vec::new();
        write(&mut written, &entries).unwrap();
        let written = String::from_utf8(written).unwrap();
        assert_eq!(written, "chr1\t10\t20\tpromoter A\nchr2\t5\t9\n");
        assert_eq!(regions(written.as_bytes()).unwrap(), entries);
        let mut tabbed = Vec::new();
        write(&mut tabbed, &[entry("a\tb", "chr1", region(0, 1))]).unwrap();
        assert_eq!(tabbed, b"chr1\t0\t1\ta b\n");
        assert!(matches!(
            regions("chr1\t0\t5\nchr1\tx\t2\n".as_bytes()),
            Err(Error::Format { line: 2, .. })
        ));
    }

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn features_write_as_bed12_and_read_back() {
        let gene = Feature::new("ABC1", region(100, 200), Strand::Reverse)
            .with_blocks(vec![region(100, 120), region(170, 200)])
            .with_coding(Some(region(110, 190)))
            .with_color(Some([10, 20, 30]));
        let noncoding = Feature::new("lnc\tRNA", region(5, 9), Strand::Unknown)
            .with_blocks(vec![region(5, 9)])
            .with_coding(None);
        let mut written = Vec::new();
        write_features(&mut written, [("chr1", &gene), ("chr2", &noncoding)]).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert_eq!(
            text,
            "chr1\t100\t200\tABC1\t0\t-\t110\t190\t10,20,30\t2\t20,30,\t0,70,\n\
             chr2\t5\t9\tlnc RNA\t0\t.\t5\t5\t0\t1\t4,\t0,\n"
        );
        let mut lines = text.lines();
        assert_eq!(parse(lines.next().unwrap()), Ok(Some(("chr1", gene))));
        assert_eq!(
            parse(lines.next().unwrap()),
            Ok(Some(("chr2", noncoding.with_name("lnc RNA"))))
        );
    }

    #[test]
    fn parses_three_columns() {
        let feature = Feature::new("", region(10, 20), Strand::Unknown);
        assert_eq!(parse("chr1\t10\t20"), Ok(Some(("chr1", feature))));
    }

    #[test]
    fn parses_names_and_strands() {
        let (_, feature) = parse("chr1\t10\t20\tgeneA\t0\t-").unwrap().unwrap();
        assert_eq!(
            feature,
            Feature::new("geneA", region(10, 20), Strand::Reverse)
        );
    }

    #[test]
    fn parses_thick_parts_as_coding() {
        let (_, feature) = parse("chr1\t10\t100\ta\t0\t+\t20\t80").unwrap().unwrap();
        assert_eq!(feature.coding(), Some(region(20, 80)));
        let (_, feature) = parse("chr1\t10\t100\ta\t0\t+\t10\t10").unwrap().unwrap();
        assert_eq!(feature.coding(), None);
    }

    #[test]
    fn parses_blocks() {
        let line = "chr1\t100\t200\ta\t0\t+\t110\t190\t0\t2\t20,30,\t0,70,";
        let (_, feature) = parse(line).unwrap().unwrap();
        assert_eq!(feature.blocks(), [region(100, 120), region(170, 200)]);
    }

    #[test]
    fn parses_item_colors() {
        let colored = "chr1\t10\t20\ta\t0\t+\t10\t20\t255,0,128";
        assert_eq!(
            parse(colored).unwrap().unwrap().1.color(),
            Some([255, 0, 128])
        );
        for plain in [
            "chr1\t10\t20\ta\t0\t+\t10\t20\t0",
            "chr1\t10\t20\ta\t0\t+\t10\t20\tred",
        ] {
            assert_eq!(parse(plain).unwrap().unwrap().1.color(), None, "{plain}");
        }
    }

    #[test]
    fn skips_header_lines() {
        for line in ["track name=genes", "browser position chr1", "#chrom", ""] {
            assert_eq!(parse(line), Ok(None), "{line}");
        }
    }

    #[test]
    fn rejects_bad_lines() {
        for (line, reason) in [
            ("chr1\t10", Reason::MissingField),
            ("chr1\tx\t20", Reason::BadNumber),
            ("chr1\t30\t20", Reason::BadSpan),
            ("chr1\t10\t20\ta\t0\tx", Reason::BadStrand),
            (
                "chr1\t10\t20\ta\t0\t+\t10\t20\t0\t2\t5,\t0,",
                Reason::BadBlocks,
            ),
        ] {
            assert_eq!(parse(line), Err(reason), "{line}");
        }
    }
}
