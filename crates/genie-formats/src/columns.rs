//! Tables of tab-separated columns with a tabix index, as the CADD scores: the index gives the
//! columns of the contig, the start and the end, and the last header line names the columns.

use std::io::{BufRead, Read, Seek};

use genie_core::Region;

use crate::Error;
use crate::bgzf::Bgzf;
use crate::tabix::Index;

/// The rows that tell which columns hold numbers.
const SAMPLE: usize = 100;

/// The columns of a table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Columns {
    /// The name of each column: from the last header line, or else "Column 1", "Column 2", ….
    pub names: Vec<String>,
    /// The columns (from 0) whose values are numbers in the first rows, except the columns of
    /// the place of the rows.
    pub numbers: Vec<usize>,
    /// The first column with text, except the columns of the place, to name the rows.
    pub label: Option<usize>,
    /// The columns (from 0) of the contig, the start and the end of the rows.
    pub place: Vec<usize>,
}

impl Columns {
    /// Reads the header lines and the first rows of a table.
    pub fn read(mut reader: impl BufRead, index: &Index) -> Result<Self, Error> {
        let meta = char::from(index.meta());
        let place: Vec<usize> = index
            .columns()
            .into_iter()
            .filter(|column| *column > 0)
            .map(|column| column - 1)
            .collect();
        let mut header: Option<Vec<String>> = None;
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut bytes = Vec::new();
        let mut skipped = 0;
        while rows.len() < SAMPLE {
            bytes.clear();
            if reader.read_until(b'\n', &mut bytes)? == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&bytes);
            let line = text.trim_end_matches(['\n', '\r']);
            let skip = skipped < index.skip();
            skipped += 1;
            if line.starts_with(meta) || skip {
                let names = line.trim_start_matches(meta).split('\t');
                header = Some(names.map(|name| name.trim().to_owned()).collect());
            } else if !line.trim().is_empty() {
                rows.push(line.split('\t').map(str::to_owned).collect());
            }
        }
        let width = rows
            .iter()
            .map(Vec::len)
            .chain(header.as_ref().map(Vec::len))
            .max()
            .unwrap_or(0);
        let names: Vec<String> = (0..width)
            .map(|column| {
                header
                    .as_ref()
                    .and_then(|names| names.get(column))
                    .filter(|name| !name.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("Column {}", column + 1))
            })
            .collect();
        let numeric = |column: usize| {
            let values: Vec<&str> = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|value| value.trim())
                .filter(|value| !matches!(*value, "" | "." | "NA" | "nan" | "NaN"))
                .collect();
            !values.is_empty() && values.iter().all(|value| value.parse::<f64>().is_ok())
        };
        let free: Vec<usize> = (0..width)
            .filter(|column| !place.contains(column))
            .collect();
        let numbers = free
            .iter()
            .copied()
            .filter(|column| numeric(*column))
            .collect();
        let label = free.iter().copied().find(|column| !numeric(*column));
        Ok(Self {
            names,
            numbers,
            label,
            place,
        })
    }
}

/// Calls `each` with the span and the fields of each row of a region.
pub fn query<R: Read + Seek>(
    index: &Index,
    reader: &mut Bgzf<R>,
    contig: &str,
    region: Region,
    mut each: impl FnMut(Region, &[&str]),
) -> Result<(), Error> {
    index.query(reader, contig, region, |line| {
        if let Some((_, span)) = index.place(line) {
            let fields: Vec<&str> = line.split('\t').collect();
            each(span, &fields);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use genie_core::Region;

    use super::{Columns, query};
    use crate::bgzf::{self, Bgzf};
    use crate::tabix::{self, Index, Layout};

    const CADD: &str = "## CADD GRCh38-v1.7 (c) University of Washington\n\
        #Chrom\tPos\tRef\tAlt\tRawScore\tPHRED\n\
        1\t10001\tT\tA\t0.702\t8.49\n\
        1\t10001\tT\tC\t0.751\t8.88\n\
        1\t10002\tA\tG\t0.630\t7.91\n\
        2\t500\tG\tC\t1.500\t15.20\n";

    /// A table compressed with bgzip and its tabix index for the contig, start and end columns,
    /// counted from 1.
    fn indexed(text: &str, columns: [usize; 3]) -> (Vec<u8>, Index) {
        let mut writer = bgzf::Writer::new(Vec::new());
        writer.write_all(text.as_bytes()).unwrap();
        let data = writer.finish().unwrap();
        let layout = Layout::generic(columns);
        let index = tabix::build(Cursor::new(&data), layout, false).unwrap();
        (data, Index::read(Cursor::new(index)).unwrap())
    }

    #[test]
    fn headers_name_the_columns_and_rows_tell_the_numbers() {
        let (data, index) = indexed(CADD, [1, 2, 2]);
        let columns = Columns::read(Bgzf::new(Cursor::new(data)), &index).unwrap();
        assert_eq!(
            columns.names,
            ["Chrom", "Pos", "Ref", "Alt", "RawScore", "PHRED"]
        );
        assert_eq!(columns.numbers, [4, 5]);
        assert_eq!(columns.label, Some(2));
        assert_eq!(columns.place, [0, 1, 1]);
        let plain = "chr1\t5\t9\tx\t1\nchr1\t20\t30\ty\t2\n";
        let (data, index) = indexed(plain, [1, 2, 3]);
        let unnamed = Columns::read(Bgzf::new(Cursor::new(data)), &index).unwrap();
        assert_eq!(
            unnamed.names,
            ["Column 1", "Column 2", "Column 3", "Column 4", "Column 5"]
        );
        assert_eq!((unnamed.numbers, unnamed.label), (vec![4], Some(3)));
    }

    #[test]
    fn regions_give_their_rows_with_their_spans() {
        let (data, index) = indexed(CADD, [1, 2, 2]);
        let mut reader = Bgzf::new(Cursor::new(data));
        let mut found = Vec::new();
        query(
            &index,
            &mut reader,
            "1",
            Region::between(10_000, 10_001),
            |span, fields| {
                found.push((span, fields[5].to_owned()));
            },
        )
        .unwrap();
        assert_eq!(
            found,
            [
                (Region::between(10_000, 10_001), "8.49".to_owned()),
                (Region::between(10_000, 10_001), "8.88".to_owned()),
            ]
        );
    }
}
