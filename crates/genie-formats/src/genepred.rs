use std::io::BufRead;

use genie_core::{Feature, Region};

use crate::bed::{self, number, strand};
use crate::text::lines;
use crate::{Error, Reason};

const FIELDS: usize = 10;
const NAME2: usize = 11;
const BIG_NAME2: usize = 12;

pub fn parse(line: &str) -> Result<Option<(&str, Feature)>, Reason> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let all: Vec<&str> = line.split('\t').collect();
    let binned = all.len() > 3 && !is_strand(all[2]) && is_strand(all[3]);
    let fields = &all[usize::from(binned)..];
    if fields.len() < FIELDS {
        return Err(Reason::MissingField);
    }
    let strand = strand(fields[2])?;
    let span = Region::new(number(fields[3])?, number(fields[4])?).ok_or(Reason::BadSpan)?;
    let coding = Region::new(number(fields[5])?, number(fields[6])?).ok_or(Reason::BadSpan)?;
    let count = number(fields[7])?;
    let (starts, ends) = (list(fields[8])?, list(fields[9])?);
    if starts.len() as u64 != count || ends.len() != starts.len() {
        return Err(Reason::BadBlocks);
    }
    let exons = starts
        .into_iter()
        .zip(ends)
        .map(|(start, end)| Region::new(start, end).ok_or(Reason::BadSpan))
        .collect::<Result<Vec<_>, _>>()?;
    let name = fields
        .get(NAME2)
        .copied()
        .filter(|name| !name.is_empty())
        .unwrap_or(fields[0]);
    let feature = Feature::new(name, span, strand)
        .with_blocks(exons)
        .with_coding((!coding.is_empty()).then_some(coding));
    Ok(Some((fields[1], feature)))
}

pub fn big(line: &str) -> Result<Option<(&str, Feature)>, Reason> {
    let Some((contig, feature)) = bed::parse(line)? else {
        return Ok(None);
    };
    Ok(Some(
        match line
            .split('\t')
            .nth(BIG_NAME2)
            .filter(|name| !name.is_empty())
        {
            Some(name) => (contig, feature.with_name(name)),
            None => (contig, feature),
        },
    ))
}

pub fn read(reader: impl BufRead) -> Result<Vec<(String, Feature)>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, feature)) = parsed {
            rows.push((contig.to_owned(), feature));
        }
        Ok(())
    })?;
    Ok(rows)
}

fn is_strand(field: &str) -> bool {
    matches!(field, "+" | "-")
}

fn list(field: &str) -> Result<Vec<u64>, Reason> {
    field
        .split(',')
        .filter(|value| !value.trim().is_empty())
        .map(number)
        .collect()
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{big, parse, read};
    use crate::{Error, Reason};

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    const SELECT: &str = "585\tNM_001005484.2\tchr1\t+\t65418\t71585\t65564\t70008\t3\t65418,65519,69036,\t65433,65573,71585,\t0\tOR4F5\tcmpl\tcmpl\t-1,0,0,";

    #[test]
    fn reads_ucsc_dumps_with_a_bin_column() {
        let (contig, feature) = parse(SELECT).unwrap().unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(
            feature,
            Feature::new("OR4F5", region(65_418, 71_585), Strand::Forward)
                .with_blocks(vec![
                    region(65_418, 65_433),
                    region(65_519, 65_573),
                    region(69_036, 71_585)
                ])
                .with_coding(Some(region(65_564, 70_008)))
        );
    }

    #[test]
    fn reads_plain_gene_predictions_and_keeps_the_name() {
        let line =
            "NR_046018.2\tchr1\t-\t11873\t14409\t14409\t14409\t2\t11873,12612,\t12227,14409,";
        let (contig, feature) = parse(line).unwrap().unwrap();
        assert_eq!(contig, "chr1");
        assert_eq!(feature.name(), "NR_046018.2");
        assert_eq!(feature.strand(), Strand::Reverse);
        assert_eq!(feature.coding(), None);
        assert_eq!(
            feature.blocks(),
            [region(11_873, 12_227), region(12_612, 14_409)]
        );
    }

    #[test]
    fn rejects_bad_lines() {
        for (line, reason) in [
            ("NM_1\tchr1\t+\t10", Reason::MissingField),
            (
                "NM_1\tchr1\t*\t10\t20\t10\t20\t1\t10,\t20,",
                Reason::BadStrand,
            ),
            (
                "NM_1\tchr1\t+\t10\tx\t10\t20\t1\t10,\t20,",
                Reason::BadNumber,
            ),
            (
                "NM_1\tchr1\t+\t30\t20\t20\t20\t1\t20,\t30,",
                Reason::BadSpan,
            ),
            (
                "NM_1\tchr1\t+\t10\t20\t10\t20\t2\t10,\t20,",
                Reason::BadBlocks,
            ),
        ] {
            assert_eq!(parse(line), Err(reason), "{line}");
        }
        assert_eq!(parse("#bin\tname"), Ok(None));
    }

    #[test]
    fn big_gene_predictions_use_the_gene_name() {
        let line = "CP068277.2\t65418\t71585\tNM_001005484.2\t0\t+\t65564\t70008\t0\t3\t15,54,2549,\t0,101,3618,\tOR4F5\tcmpl\tcmpl\t-1,0,0,\tprotein_coding\tOR4F5\tOR4F5\tnone";
        let (contig, feature) = big(line).unwrap().unwrap();
        assert_eq!(contig, "CP068277.2");
        assert_eq!(feature.name(), "OR4F5");
        assert_eq!(feature.blocks().len(), 3);
        assert_eq!(feature.coding(), Some(region(65_564, 70_008)));
        let bare = "chr1\t10\t20\tNM_1\t0\t+\t10\t20\t0\t1\t10,\t0,";
        assert_eq!(big(bare).unwrap().unwrap().1.name(), "NM_1");
    }

    #[test]
    fn tables_read_with_line_numbers() {
        let text = format!("{SELECT}\n{SELECT}\n");
        let rows = read(text.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].0, "chr1");
        let error = read("NM_1\tchr1\t+\n".as_bytes()).unwrap_err();
        assert!(matches!(error, Error::Format { line: 1, .. }));
    }
}
