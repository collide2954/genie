use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;

use crate::Error;
use crate::fai::FaiRecord;

pub struct Fasta<R> {
    reader: R,
    records: Vec<FaiRecord>,
}

impl<R> Fasta<R> {
    pub fn new(reader: R, records: Vec<FaiRecord>) -> Self {
        Self { reader, records }
    }

    pub fn records(&self) -> &[FaiRecord] {
        &self.records
    }
}

impl<R: Read + Seek> Fasta<R> {
    pub fn fetch(&mut self, contig: usize, region: Region) -> Result<Vec<u8>, Error> {
        let record = &self.records[contig];
        let region = region.clamp_to(record.length);
        if region.is_empty() {
            return Ok(Vec::new());
        }
        let start = byte_offset(record, region.start());
        let end = byte_offset(record, region.end() - 1) + 1;
        let mut bytes = vec![0; (end - start) as usize];
        self.reader.seek(SeekFrom::Start(start))?;
        self.reader.read_exact(&mut bytes)?;
        bytes.retain(|byte| !matches!(byte, b'\n' | b'\r'));
        Ok(bytes)
    }
}

fn byte_offset(record: &FaiRecord, position: u64) -> u64 {
    record.offset + position / record.line_bases * record.line_width + position % record.line_bases
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::Fasta;
    use crate::fai;

    fn open(text: &'static str) -> Fasta<Cursor<&'static [u8]>> {
        let records = fai::build(text.as_bytes()).unwrap();
        Fasta::new(Cursor::new(text.as_bytes()), records)
    }

    fn fetch(
        fasta: &mut Fasta<Cursor<&'static [u8]>>,
        contig: usize,
        start: u64,
        end: u64,
    ) -> String {
        let bases = fasta
            .fetch(contig, Region::new(start, end).unwrap())
            .unwrap();
        String::from_utf8(bases).unwrap()
    }

    const TEXT: &str = ">chr1\nACGTA\nCGTAC\nGT\n>chr2\nTTTT\n";

    #[test]
    fn fetches_across_line_breaks() {
        let mut fasta = open(TEXT);
        assert_eq!(fetch(&mut fasta, 0, 0, 12), "ACGTACGTACGT");
        assert_eq!(fetch(&mut fasta, 0, 3, 7), "TACG");
        assert_eq!(fetch(&mut fasta, 1, 1, 3), "TT");
    }

    #[test]
    fn clamps_to_contig_end() {
        let mut fasta = open(TEXT);
        assert_eq!(fetch(&mut fasta, 0, 10, 20), "GT");
        assert_eq!(fetch(&mut fasta, 0, 5, 5), "");
        assert_eq!(fetch(&mut fasta, 0, 30, 40), "");
    }

    #[test]
    fn strips_crlf_and_keeps_case() {
        let mut fasta = open(">c\r\nacGT\r\nACgt\r\n");
        assert_eq!(fetch(&mut fasta, 0, 2, 6), "GTAC");
        assert_eq!(fetch(&mut fasta, 0, 0, 8), "acGTACgt");
    }

    #[test]
    fn exposes_records() {
        let fasta = open(TEXT);
        assert_eq!(fasta.records()[1].name, "chr2");
    }
}
