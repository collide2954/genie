use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;
use vstd::prelude::*;

use crate::bgzf::Bgzf;
use crate::fai::FaiRecord;
use crate::gzi;
use crate::{Error, Reason};

const LINE: usize = 60;

pub fn fetch(
    reader: &mut (impl Read + Seek),
    record: &FaiRecord,
    region: Region,
) -> Result<Vec<u8>, Error> {
    let Some((start, length)) = bytes_of(record, region)? else {
        return Ok(Vec::new());
    };
    reader.seek(SeekFrom::Start(start))?;
    bases(reader, length)
}

pub fn fetch_bgzf(
    reader: &mut Bgzf<impl Read + Seek>,
    index: &gzi::Index,
    record: &FaiRecord,
    region: Region,
) -> Result<Vec<u8>, Error> {
    let Some((start, length)) = bytes_of(record, region)? else {
        return Ok(Vec::new());
    };
    reader.seek(index.virtual_offset(start)?)?;
    bases(reader, length)
}

/// The first byte of the bases of a region of a record, and the number of bytes to read.
fn bytes_of(record: &FaiRecord, region: Region) -> Result<Option<(u64, usize)>, Error> {
    let lines = (record.line_bases, record.line_width);
    let found = span(record.offset, record.length, lines, region).map_err(|damage| {
        Error::Invalid(match damage {
            Damage::NoBases => Reason::ZeroLineBases,
            Damage::ShortLines => Reason::ShortLineWidth,
            Damage::PastTheEnd => Reason::BadOffset,
        })
    })?;
    let Some((start, count)) = found else {
        return Ok(None);
    };
    let count = usize::try_from(count).map_err(|_| Error::Invalid(Reason::BadOffset))?;
    Ok(Some((start, count)))
}

verus! {

/// What is wrong with a record of a FASTA index.
enum Damage {
    /// The record has bases, but no bases on a line.
    NoBases,
    /// A line has fewer bytes than bases.
    ShortLines,
    /// A byte of the record is past the end of 64-bit numbers.
    PastTheEnd,
}

/// The byte of a base of a record in a FASTA file. The first base of the record is at
/// `offset`. Each line before the line of the base has `line_width` bytes: its `line_bases`
/// bases and its line end.
pub open spec fn spec_byte(offset: u64, line_bases: u64, line_width: u64, position: u64) -> int {
    offset as int + (position as int / line_bases as int) * line_width as int + position as int
        % line_bases as int
}

/// The byte of a base, or nothing when the byte is past the end of 64-bit numbers.
fn byte_offset(offset: u64, line_bases: u64, line_width: u64, position: u64) -> (byte: Option<
    u64,
>)
    requires
        line_bases > 0,
    ensures
        match byte {
            Some(byte) => byte == spec_byte(offset, line_bases, line_width, position),
            None => spec_byte(offset, line_bases, line_width, position) > u64::MAX,
        },
{
    let line = position / line_bases;
    let column = position % line_bases;
    proof {
        vstd::arithmetic::div_mod::lemma_mod_bound(position as int, line_bases as int);
        vstd::arithmetic::mul::lemma_mul_nonnegative(line as int, line_width as int);
    }
    let skipped = line.checked_mul(line_width)?;
    let inside = skipped.checked_add(column)?;
    offset.checked_add(inside)
}

/// A later base of a record is at a later byte when each line has at least its bases. Between
/// two bases are the bases from the first to the last and the line ends of the lines between.
proof fn lemma_later_bases_at_later_bytes(
    offset: u64,
    line_bases: u64,
    line_width: u64,
    first: u64,
    last: u64,
)
    requires
        0 < line_bases <= line_width,
        first <= last,
    ensures
        spec_byte(offset, line_bases, line_width, first) <= spec_byte(
            offset,
            line_bases,
            line_width,
            last,
        ),
        spec_byte(offset, line_bases, line_width, last) - spec_byte(
            offset,
            line_bases,
            line_width,
            first,
        ) == (last - first) + (last as int / line_bases as int - first as int / line_bases as int)
            * (line_width - line_bases),
{
    let (bases, width) = (line_bases as int, line_width as int);
    let (low, high) = (first as int, last as int);
    let (low_line, low_column) = (low / bases, low % bases);
    let (high_line, high_column) = (high / bases, high % bases);
    vstd::arithmetic::div_mod::lemma_fundamental_div_mod(low, bases);
    vstd::arithmetic::div_mod::lemma_fundamental_div_mod(high, bases);
    vstd::arithmetic::div_mod::lemma_div_is_ordered(low, high, bases);
    assert((high_line * width + high_column) - (low_line * width + low_column) == (high - low) + (
    high_line - low_line) * (width - bases)) by (nonlinear_arith)
        requires
            low == bases * low_line + low_column,
            high == bases * high_line + high_column,
    ;
    assert((high_line - low_line) * (width - bases) >= 0) by (nonlinear_arith)
        requires
            high_line >= low_line,
            width >= bases,
    ;
}

/// The first byte of the bases of a region of a record, and the number of bytes from it to
/// the byte after its last base: the bases of the region and the line ends between them.
/// Nothing for a region with no bases of the record, which has `length` bases.
fn span(offset: u64, length: u64, lines: (u64, u64), region: Region) -> (span: Result<
    Option<(u64, u64)>,
    Damage,
>)
    ensures
        ({
            let (line_bases, line_width) = lines;
            let first = if region.spec_start() <= length {
                region.spec_start()
            } else {
                length
            };
            let end = if region.spec_end() <= length {
                region.spec_end()
            } else {
                length
            };
            match span {
                Ok(Some((start, count))) => first < end && start == spec_byte(
                    offset,
                    line_bases,
                    line_width,
                    first,
                ) && start + count == spec_byte(offset, line_bases, line_width, (end - 1) as u64)
                    + 1 && count == (end - first) + ((end - 1) / line_bases as int - first as int
                    / line_bases as int) * (line_width - line_bases),
                Ok(None) => first == end,
                Err(Damage::NoBases) => first < end && line_bases == 0,
                Err(Damage::ShortLines) => first < end && 0 < line_bases && line_width
                    < line_bases,
                Err(Damage::PastTheEnd) => first < end && 0 < line_bases <= line_width
                    && spec_byte(offset, line_bases, line_width, (end - 1) as u64) + 1 > u64::MAX,
            }
        }),
{
    let (line_bases, line_width) = lines;
    let region = region.clamp_to(length);
    let (first, end) = (region.start(), region.end());
    if first == end {
        return Ok(None);
    }
    if line_bases == 0 {
        return Err(Damage::NoBases);
    }
    if line_width < line_bases {
        return Err(Damage::ShortLines);
    }
    let last = end - 1;
    proof {
        lemma_later_bases_at_later_bytes(offset, line_bases, line_width, first, last);
    }
    let Some(byte) = byte_offset(offset, line_bases, line_width, last) else {
        return Err(Damage::PastTheEnd);
    };
    let Some(after) = byte.checked_add(1) else {
        return Err(Damage::PastTheEnd);
    };
    let Some(start) = byte_offset(offset, line_bases, line_width, first) else {
        return Err(Damage::PastTheEnd);
    };
    Ok(Some((start, after - start)))
}

} // verus!

pub fn record(header: &str, bases: &[u8]) -> String {
    let mut text = format!(">{header}\n");
    for line in bases.chunks(LINE) {
        text.push_str(&String::from_utf8_lossy(line));
        text.push('\n');
    }
    text
}

fn bases(reader: &mut impl Read, length: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    bytes.retain(|byte| !matches!(byte, b'\n' | b'\r'));
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::{fetch, fetch_bgzf, record};
    use crate::bgzf::{self, Bgzf};
    use crate::{Reason, fai, gzi};

    fn bases(text: &str, contig: usize, start: u64, end: u64) -> String {
        let records = fai::build(text.as_bytes()).unwrap();
        let mut reader = Cursor::new(text.as_bytes());
        let region = Region::new(start, end).unwrap();
        String::from_utf8(fetch(&mut reader, &records[contig], region).unwrap()).unwrap()
    }

    const TEXT: &str = ">chr1\nACGTA\nCGTAC\nGT\n>chr2\nTTTT\n";

    #[test]
    fn fetches_bgzip_fasta_across_blocks() {
        let parts: Vec<&[u8]> = TEXT.as_bytes().chunks(7).collect();
        let bytes = bgzf::tests::file(&parts);
        let records = fai::build(Bgzf::new(Cursor::new(&bytes))).unwrap();
        assert_eq!(records, fai::build(TEXT.as_bytes()).unwrap());
        let index = gzi::Index::build(Cursor::new(&bytes)).unwrap();
        let mut reader = Bgzf::new(Cursor::new(&bytes));
        for (contig, start, end) in [(0, 0, 12), (1, 1, 3), (0, 3, 7), (0, 10, 20), (0, 5, 5)] {
            let region = Region::new(start, end).unwrap();
            let found = fetch_bgzf(&mut reader, &index, &records[contig], region).unwrap();
            assert_eq!(found, bases(TEXT, contig, start, end).as_bytes());
        }
    }

    #[test]
    fn fetches_across_line_breaks() {
        assert_eq!(bases(TEXT, 0, 0, 12), "ACGTACGTACGT");
        assert_eq!(bases(TEXT, 0, 3, 7), "TACG");
        assert_eq!(bases(TEXT, 1, 1, 3), "TT");
    }

    #[test]
    fn clamps_to_contig_end() {
        assert_eq!(bases(TEXT, 0, 10, 20), "GT");
        assert_eq!(bases(TEXT, 0, 5, 5), "");
        assert_eq!(bases(TEXT, 0, 30, 40), "");
    }

    #[test]
    fn records_have_a_header_and_lines_of_sixty() {
        assert_eq!(record("chr1:1-4", b"ACGT"), ">chr1:1-4\nACGT\n");
        let long = [b'A'; 130];
        let lines: Vec<usize> = record("x", &long).lines().map(str::len).collect();
        assert_eq!(lines, [2, 60, 60, 10]);
        assert_eq!(record("x", b""), ">x\n");
    }

    /// A damaged index gives an error, not a panic or bytes from a wrong place: an offset past
    /// the end of 64-bit numbers, no bases per line, or lines shorter than their bases.
    #[test]
    fn damaged_indexes_give_errors() {
        let record = |offset, line_bases, line_width| fai::FaiRecord {
            name: "chr1".to_owned(),
            length: 100,
            offset,
            line_bases,
            line_width,
        };
        let region = Region::new(10, 20).unwrap();
        let fetched = |record: &fai::FaiRecord| {
            let mut reader = Cursor::new(TEXT.as_bytes());
            match fetch(&mut reader, record, region) {
                Err(crate::Error::Invalid(reason)) => Some(reason),
                _ => None,
            }
        };
        assert_eq!(
            fetched(&record(u64::MAX - 5, 60, 61)),
            Some(Reason::BadOffset)
        );
        assert_eq!(fetched(&record(6, 0, 1)), Some(Reason::ZeroLineBases));
        assert_eq!(fetched(&record(6, 5, 4)), Some(Reason::ShortLineWidth));
    }

    #[test]
    fn strips_crlf_and_keeps_case() {
        let text = ">c\r\nacGT\r\nACgt\r\n";
        assert_eq!(bases(text, 0, 2, 6), "GTAC");
        assert_eq!(bases(text, 0, 0, 8), "acGTACgt");
    }
}
