use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;

use crate::Error;
use crate::bgzf::Bgzf;
use crate::fai::FaiRecord;
use crate::gzi;

const LINE: usize = 60;

pub fn fetch(
    reader: &mut (impl Read + Seek),
    record: &FaiRecord,
    region: Region,
) -> Result<Vec<u8>, Error> {
    let Some((start, length)) = span(record, region) else {
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
    let Some((start, length)) = span(record, region) else {
        return Ok(Vec::new());
    };
    reader.seek(index.virtual_offset(start)?)?;
    bases(reader, length)
}

pub fn record(header: &str, bases: &[u8]) -> String {
    let mut text = format!(">{header}\n");
    for line in bases.chunks(LINE) {
        text.push_str(&String::from_utf8_lossy(line));
        text.push('\n');
    }
    text
}

fn span(record: &FaiRecord, region: Region) -> Option<(u64, usize)> {
    let region = region.clamp_to(record.length);
    if region.is_empty() {
        return None;
    }
    let start = byte_offset(record, region.start());
    let end = byte_offset(record, region.end() - 1) + 1;
    Some((start, (end - start) as usize))
}

fn bases(reader: &mut impl Read, length: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    bytes.retain(|byte| !matches!(byte, b'\n' | b'\r'));
    Ok(bytes)
}

fn byte_offset(record: &FaiRecord, position: u64) -> u64 {
    record.offset + position / record.line_bases * record.line_width + position % record.line_bases
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::{fetch, fetch_bgzf, record};
    use crate::bgzf::{self, Bgzf};
    use crate::{fai, gzi};

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

    #[test]
    fn strips_crlf_and_keeps_case() {
        let text = ">c\r\nacGT\r\nACgt\r\n";
        assert_eq!(bases(text, 0, 2, 6), "GTAC");
        assert_eq!(bases(text, 0, 0, 8), "acGTACgt");
    }
}
