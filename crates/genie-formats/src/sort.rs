use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use crate::bam::{self as records};
use crate::bgzf::{Bgzf, Writer};
use crate::tabix::{Indexer, Layout};
use crate::{Error, Reason, bai};

const REPORT_EVERY: u64 = 1 << 14;
const ENTRY_COST: usize = 40;
const TBI_LIMIT: u64 = 1 << 29;
const FASTA: &str = "##FASTA";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Reading,
    Writing { done: u64, total: u64 },
}

pub struct Budget<'a> {
    pub folder: &'a Path,
    pub memory: usize,
}

type Key = (u32, u64, u64);
type Report<'a> = &'a mut dyn FnMut(Stage) -> bool;

#[derive(Default)]
struct Run {
    keys: Vec<(Key, usize, usize)>,
    arena: Vec<u8>,
}

impl Run {
    fn size(&self) -> usize {
        self.arena.len() + self.keys.len() * ENTRY_COST
    }

    fn push(&mut self, key: Key, bytes: &[u8]) {
        self.keys.push((key, self.arena.len(), bytes.len()));
        self.arena.extend_from_slice(bytes);
    }

    fn sort(&mut self) {
        self.keys.sort_unstable_by_key(|(key, _, _)| *key);
    }

    fn entry(&self, at: usize) -> (Key, &[u8]) {
        let (key, offset, length) = self.keys[at];
        (key, &self.arena[offset..offset + length])
    }
}

struct Sorter<'a> {
    budget: &'a Budget<'a>,
    run: Run,
    spilled: Vec<PathBuf>,
    count: u64,
}

impl<'a> Sorter<'a> {
    fn new(budget: &'a Budget<'a>) -> Self {
        Self {
            budget,
            run: Run::default(),
            spilled: Vec::new(),
            count: 0,
        }
    }

    fn add(&mut self, key: Key, bytes: &[u8]) -> io::Result<()> {
        self.run.push(key, bytes);
        self.count += 1;
        if self.run.size() >= self.budget.memory {
            self.spill()?;
        }
        Ok(())
    }

    fn spill(&mut self) -> io::Result<()> {
        self.run.sort();
        let path = self.budget.folder.join(format!(
            "run-{}-{}.tmp",
            std::process::id(),
            self.spilled.len()
        ));
        self.spilled.push(path.clone());
        let mut file = BufWriter::new(File::create(&path)?);
        for at in 0..self.run.keys.len() {
            let ((rank, start, sequence), bytes) = self.run.entry(at);
            file.write_all(&rank.to_le_bytes())?;
            file.write_all(&start.to_le_bytes())?;
            file.write_all(&sequence.to_le_bytes())?;
            file.write_all(&(bytes.len() as u64).to_le_bytes())?;
            file.write_all(bytes)?;
        }
        file.flush()?;
        self.run = Run::default();
        Ok(())
    }

    fn drain(mut self, mut each: impl FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        if self.spilled.is_empty() {
            self.run.sort();
            for at in 0..self.run.keys.len() {
                each(self.run.entry(at).1)?;
            }
            return Ok(());
        }
        if !self.run.keys.is_empty() {
            self.spill()?;
        }
        let mut readers = self
            .spilled
            .iter()
            .map(|path| File::open(path).map(BufReader::new))
            .collect::<io::Result<Vec<_>>>()?;
        let mut buffers = vec![Vec::new(); readers.len()];
        let mut heap = BinaryHeap::new();
        for (at, reader) in readers.iter_mut().enumerate() {
            if let Some(key) = next_entry(reader, &mut buffers[at])? {
                heap.push(Reverse((key, at)));
            }
        }
        while let Some(Reverse((_, at))) = heap.pop() {
            each(&buffers[at])?;
            if let Some(key) = next_entry(&mut readers[at], &mut buffers[at])? {
                heap.push(Reverse((key, at)));
            }
        }
        Ok(())
    }
}

impl Drop for Sorter<'_> {
    fn drop(&mut self) {
        for path in &self.spilled {
            let _ = fs::remove_file(path);
        }
    }
}

fn next_entry(reader: &mut impl Read, bytes: &mut Vec<u8>) -> io::Result<Option<Key>> {
    let mut head = [0; 28];
    match reader.read_exact(&mut head) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let long = |at: usize| {
        let mut value = [0; 8];
        value.copy_from_slice(&head[at..at + 8]);
        u64::from_le_bytes(value)
    };
    let rank = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let length = usize::try_from(long(20)).map_err(|_| io::Error::from(ErrorKind::InvalidData))?;
    bytes.resize(length, 0);
    reader.read_exact(bytes)?;
    Ok(Some((rank, long(4), long(12))))
}

fn stopped() -> Error {
    Error::Io(io::Error::from(ErrorKind::Interrupted))
}

fn line_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn text(
    mut input: impl BufRead,
    layout: Layout,
    output: impl Write,
    budget: &Budget,
    report: Report,
) -> Result<Vec<u8>, Error> {
    let mut header = Vec::new();
    let mut ranks: HashMap<String, u32> = HashMap::new();
    let mut sorter = Sorter::new(budget);
    let (mut line, mut number, mut longest) = (Vec::new(), 0, 0);
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        number += 1;
        if number % REPORT_EVERY == 0 && !report(Stage::Reading) {
            return Err(stopped());
        }
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        let text = line_of(&line);
        let trimmed = text.trim_end_matches(['\n', '\r']);
        if layout == Layout::GFF && trimmed.starts_with(FASTA) {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }
        let place = if trimmed.starts_with('#') {
            None
        } else {
            layout.place(trimmed)
        };
        match place {
            Some((name, start, end)) => {
                let rank = if let Some(rank) = ranks.get(name) {
                    *rank
                } else {
                    let rank = u32::try_from(ranks.len()).unwrap_or(u32::MAX);
                    ranks.insert(name.to_owned(), rank);
                    rank
                };
                longest = longest.max(end);
                sorter.add((rank, start, number), &line)?;
            }
            None if sorter.count == 0 => header.extend_from_slice(&line),
            None if trimmed.starts_with('#') => {}
            None => {
                return Err(Error::Format {
                    line: number,
                    reason: Reason::BadLine,
                });
            }
        }
    }
    let total = sorter.count;
    let mut writer = Writer::new(output);
    let mut indexer = Indexer::new(layout, longest > TBI_LIMIT);
    for line in header.split_inclusive(|byte| *byte == b'\n') {
        writer.write_all(line)?;
        indexer.add(&line_of(line), writer.virtual_offset())?;
    }
    let mut done = 0;
    sorter.drain(|line| {
        writer.write_all(line)?;
        indexer.add(&line_of(line), writer.virtual_offset())?;
        done += 1;
        if done % REPORT_EVERY == 0 && !report(Stage::Writing { done, total }) {
            return Err(stopped());
        }
        Ok(())
    })?;
    let end = writer.virtual_offset();
    writer.finish()?;
    indexer.finish(end)
}

pub fn copy_text(
    mut input: impl BufRead,
    layout: Layout,
    csi: bool,
    output: impl Write,
    report: Report,
) -> Result<Vec<u8>, Error> {
    let mut writer = Writer::new(output);
    let mut indexer = Indexer::new(layout, csi);
    let (mut line, mut number) = (Vec::new(), 0);
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        number += 1;
        if number % REPORT_EVERY == 0 && !report(Stage::Reading) {
            return Err(stopped());
        }
        if !line.ends_with(b"\n") {
            line.push(b'\n');
        }
        let text = line_of(&line);
        if layout == Layout::GFF && text.starts_with(FASTA) {
            break;
        }
        writer.write_all(&line)?;
        indexer.add(&text, writer.virtual_offset())?;
    }
    let end = writer.virtual_offset();
    writer.finish()?;
    indexer.finish(end)
}

struct Tee<'a, R> {
    inner: &'a mut R,
    copy: Vec<u8>,
}

impl<R: Read> Read for Tee<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.copy.extend_from_slice(&buffer[..count]);
        Ok(count)
    }
}

pub fn bam(
    input: impl Read,
    output: impl Write,
    budget: &Budget,
    report: Report,
) -> Result<Vec<u8>, Error> {
    let mut reader = Bgzf::new(input);
    let mut tee = Tee {
        inner: &mut reader,
        copy: Vec::new(),
    };
    let header = records::header(&mut tee)?;
    let head = tee.copy;
    let mut sorter = Sorter::new(budget);
    let (mut buffer, mut number) = (Vec::new(), 0);
    while records::raw(&mut reader, &mut buffer)? {
        number += 1;
        if number % REPORT_EVERY == 0 && !report(Stage::Reading) {
            return Err(stopped());
        }
        let place = records::place(&buffer)?;
        let rank = place.reference.map_or(u32::MAX, |reference| {
            u32::try_from(reference).unwrap_or(u32::MAX)
        });
        sorter.add((rank, place.start, number), &buffer)?;
    }
    let total = sorter.count;
    let mut writer = Writer::new(output);
    writer.write_all(&head)?;
    writer.end_block()?;
    let mut indexer = bai::Indexer::new(&header, writer.virtual_offset());
    let mut done = 0;
    sorter.drain(|record| {
        let size = u32::try_from(record.len()).map_err(|_| Error::Invalid(Reason::BadRecord))?;
        writer.write_all(&size.to_le_bytes())?;
        writer.write_all(record)?;
        indexer.add(record, writer.virtual_offset())?;
        done += 1;
        if done % REPORT_EVERY == 0 && !report(Stage::Writing { done, total }) {
            return Err(stopped());
        }
        Ok(())
    })?;
    let end = writer.virtual_offset();
    writer.finish()?;
    indexer.finish(end)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, ErrorKind, Read};
    use std::path::PathBuf;

    use genie_core::Region;

    use super::{Budget, Stage, bam, copy_text, text};
    use crate::bam::tests::{head, read};
    use crate::bgzf::{Bgzf, tests::file};
    use crate::tabix::{Index, Layout};
    use crate::{Error, Reason, bai};

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("genie-sort-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn unpacked(bytes: &[u8]) -> String {
        let mut text = String::new();
        Bgzf::new(Cursor::new(bytes))
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    #[test]
    fn sorting_text_orders_records_on_disk() {
        let input = "track name=x\n#chrom\tstart\tend\nchr2\t50\t60\tb\nchr1\t30\t40\tc\n\
                     chr2\t10\t20\ta\n#note\nchr1\t5\t9\td\nchr1\t30\t35\te";
        let folder = folder("text");
        let budget = Budget {
            folder: &folder,
            memory: 64,
        };
        let mut output = Vec::new();
        let index = text(
            input.as_bytes(),
            Layout::BED,
            &mut output,
            &budget,
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(
            unpacked(&output),
            "track name=x\n#chrom\tstart\tend\nchr2\t10\t20\ta\nchr2\t50\t60\tb\n\
             chr1\t5\t9\td\nchr1\t30\t40\tc\nchr1\t30\t35\te\n"
        );
        let index = Index::read(Cursor::new(index)).unwrap();
        assert_eq!(index.names(), ["chr2", "chr1"]);
        let mut found = Vec::new();
        let mut reader = Bgzf::new(Cursor::new(output));
        let region = Region::new(25, 32).unwrap();
        index
            .query(&mut reader, "chr1", region, |line| {
                found.push(line.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(found, ["chr1\t30\t40\tc", "chr1\t30\t35\te"]);
        assert!(fs::read_dir(&folder).unwrap().next().is_none());
    }

    #[test]
    fn sorting_gff_drops_markers_and_sequences() {
        let input = "##gff-version 3\nchr1\t.\tgene\t50\t60\t.\t+\t.\tID=b\n###\n\
                     chr1\t.\tgene\t10\t20\t.\t+\t.\tID=a\n##FASTA\n>chr1\nACGT\n";
        let folder = folder("gff");
        let budget = Budget {
            folder: &folder,
            memory: 1 << 20,
        };
        let mut output = Vec::new();
        text(
            input.as_bytes(),
            Layout::GFF,
            &mut output,
            &budget,
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(
            unpacked(&output),
            "##gff-version 3\nchr1\t.\tgene\t10\t20\t.\t+\t.\tID=a\n\
             chr1\t.\tgene\t50\t60\t.\t+\t.\tID=b\n"
        );
    }

    #[test]
    fn copying_text_needs_sorted_records() {
        let sorted = "#chrom\nchr1\t10\t20\nchr1\t30\t40\n";
        let mut output = Vec::new();
        let index = copy_text(
            sorted.as_bytes(),
            Layout::BED,
            false,
            &mut output,
            &mut |_| true,
        );
        assert!(index.is_ok());
        assert_eq!(unpacked(&output), sorted);
        let unsorted = "chr1\t30\t40\nchr1\t10\t20\n";
        let error = copy_text(
            unsorted.as_bytes(),
            Layout::BED,
            false,
            Vec::new(),
            &mut |_| true,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::Format {
                line: 2,
                reason: Reason::Unsorted
            }
        ));
    }

    #[test]
    fn a_stop_ends_the_sort() {
        let input = "chr1\t5\t9\n".repeat(20_000);
        let folder = folder("stop");
        let budget = Budget {
            folder: &folder,
            memory: 1 << 20,
        };
        let mut stages = Vec::new();
        let error = text(
            input.as_bytes(),
            Layout::BED,
            Vec::new(),
            &budget,
            &mut |stage| {
                stages.push(stage);
                false
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::Io(ref error) if error.kind() == ErrorKind::Interrupted));
        assert_eq!(stages, [Stage::Reading]);
    }

    #[test]
    fn sorting_bam_orders_records_and_indexes_them() {
        let on = |reference: i32, mut record: Vec<u8>| {
            record[4..8].copy_from_slice(&reference.to_le_bytes());
            record
        };
        let first = read(100, "first", 0, &[(4, 0)], "ACGT");
        let second = read(500, "second", 0, &[(4, 0)], "ACGT");
        let third = on(1, read(50, "third", 0, &[(4, 0)], "ACGT"));
        let unplaced = on(-1, read(-1, "none", 4, &[], "AC"));
        let mut input = head(&[("chr1", 100_000), ("chr2", 50_000)]);
        for record in [&third, &second, &unplaced, &first] {
            input.extend(record);
        }
        let folder = folder("bam");
        let budget = Budget {
            folder: &folder,
            memory: 40,
        };
        let mut output = Vec::new();
        let index = bam(
            Cursor::new(file(&[&input])),
            &mut output,
            &budget,
            &mut |_| true,
        )
        .unwrap();
        let mut reader = Bgzf::new(Cursor::new(output));
        let header = crate::bam::header(&mut reader).unwrap();
        assert_eq!(header.references.len(), 2);
        let mut names = Vec::new();
        while let Some((_, alignment)) = crate::bam::record(&mut reader, &mut Vec::new()).unwrap() {
            names.push(alignment.name);
        }
        assert_eq!(names, ["first", "second", "third", "none"]);
        let index = bai::Index::read(Cursor::new(index)).unwrap();
        assert_eq!(index.len(), 2);
        assert_eq!(index.chunks(1, Region::new(0, 100).unwrap()).len(), 1);
    }
}
