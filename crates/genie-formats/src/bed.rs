use genie_core::{Feature, Region, Strand};

use crate::Reason;

const COLUMNS: usize = 12;

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
    if count == COLUMNS {
        feature = feature.with_blocks(blocks(start, fields[9], fields[10], fields[11])?);
    }
    Ok(Some((fields[0], feature)))
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

    use super::parse;
    use crate::Reason;

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
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
