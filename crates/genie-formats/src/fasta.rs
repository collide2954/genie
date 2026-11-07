use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;

use crate::Error;
use crate::fai::FaiRecord;

pub fn fetch(
    reader: &mut (impl Read + Seek),
    record: &FaiRecord,
    region: Region,
) -> Result<Vec<u8>, Error> {
    let region = region.clamp_to(record.length);
    if region.is_empty() {
        return Ok(Vec::new());
    }
    let start = byte_offset(record, region.start());
    let end = byte_offset(record, region.end() - 1) + 1;
    let mut bytes = vec![0; (end - start) as usize];
    reader.seek(SeekFrom::Start(start))?;
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

    use super::fetch;
    use crate::fai;

    fn bases(text: &str, contig: usize, start: u64, end: u64) -> String {
        let records = fai::build(text.as_bytes()).unwrap();
        let mut reader = Cursor::new(text.as_bytes());
        let region = Region::new(start, end).unwrap();
        String::from_utf8(fetch(&mut reader, &records[contig], region).unwrap()).unwrap()
    }

    const TEXT: &str = ">chr1\nACGTA\nCGTAC\nGT\n>chr2\nTTTT\n";

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
    fn strips_crlf_and_keeps_case() {
        let text = ">c\r\nacGT\r\nACgt\r\n";
        assert_eq!(bases(text, 0, 2, 6), "GTAC");
        assert_eq!(bases(text, 0, 0, 8), "acGTACgt");
    }
}
