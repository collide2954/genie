//! Fragment files of single-cell ATAC-seq, as 10x Genomics Cell Ranger ATAC writes them: a line
//! for each fragment with its contig, its start from 0, its end, the barcode of its cell and
//! the number of read pairs of the fragment. The files are compressed with bgzip and have a
//! tabix index.

use std::io::{BufRead, Read, Seek};
use std::path::Path;

use genie_core::Region;

use crate::bgzf::Bgzf;
use crate::tabix::Index;
use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

/// True for a fragment file by its name, as `fragments.tsv.gz` or `atac_fragments.tsv.gz`.
pub fn is_fragments(name: &str) -> bool {
    let name = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name)
        .to_ascii_lowercase();
    ["fragments.tsv", "fragments.tsv.gz", "fragments.tsv.bgz"]
        .iter()
        .any(|end| name.ends_with(end))
}

/// The contig, the place and the barcode of a fragment line, or nothing for a header line.
pub fn parse(line: &str) -> Result<Option<(&str, Region, &str)>, Reason> {
    if line.starts_with('#') || line.trim().is_empty() {
        return Ok(None);
    }
    let mut fields = line.split('\t');
    let (Some(contig), Some(start), Some(end), Some(barcode)) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(Reason::MissingField);
    };
    let number = |text: &str| text.trim().parse::<u64>().map_err(|_| Reason::BadNumber);
    let span = Region::new(number(start)?, number(end)?).ok_or(Reason::BadSpan)?;
    Ok(Some((contig, span, barcode.trim())))
}

/// All the fragments of a file without an index, for a small file: the place and the barcode of
/// each fragment.
pub fn read_table(reader: impl BufRead) -> Result<Table<(Region, String)>, Error> {
    let mut rows = Vec::new();
    lines(reader, |number, line| {
        let parsed = parse(line).map_err(|reason| Error::Format {
            line: number,
            reason,
        })?;
        if let Some((contig, span, barcode)) = parsed {
            rows.push((contig.to_owned(), span, (span, barcode.to_owned())));
        }
        Ok(())
    })?;
    Ok(Table::new(rows))
}

/// Gives the place and the barcode of each fragment of a file with a tabix index that overlaps
/// a region of a contig.
pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    (contig, region): (&str, Region),
    mut each: impl FnMut(Region, &str),
) -> Result<(), Error> {
    index.query(reader, contig, region, |line| {
        if let Some((_, span, barcode)) = parse(line).map_err(Error::Invalid)?
            && span.overlaps(region)
        {
            each(span, barcode);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use genie_core::Region;

    use super::{is_fragments, parse, read_table};
    use crate::Reason;

    #[test]
    fn fragment_lines_give_the_place_and_the_barcode() {
        assert_eq!(
            parse("chr1\t10066\t10279\tAAACGAAAGCGAGCTA-1\t2"),
            Ok(Some((
                "chr1",
                Region::between(10_066, 10_279),
                "AAACGAAAGCGAGCTA-1"
            )))
        );
        assert_eq!(parse("# id=pbmc"), Ok(None));
        assert_eq!(parse("chr1\t10\t20"), Err(Reason::MissingField));
        assert_eq!(parse("chr1\tx\t20\tAAAC-1\t1"), Err(Reason::BadNumber));
        assert!(is_fragments("/data/pbmc_atac_fragments.tsv.gz"));
        assert!(is_fragments("fragments.tsv.gz"));
        assert!(!is_fragments("peaks.bed.gz"));
        let text =
            "# pipeline=cellranger-atac\nchr1\t100\t300\tAAAC-1\t1\nchr1\t250\t400\tGGGT-1\t3\n";
        let table = read_table(text.as_bytes()).unwrap();
        assert_eq!(
            table.query("chr1", Region::between(280, 290)),
            [
                (Region::between(100, 300), "AAAC-1".to_owned()),
                (Region::between(250, 400), "GGGT-1".to_owned())
            ]
        );
    }
}
