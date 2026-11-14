//! CRAM reads: containers, slices and records, decoded against a reference.

mod bits;
pub mod crai;
mod encoding;
mod rans;
mod record;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};

use genie_core::Region;
use genie_core::alignment::Alignment;

use self::bits::{Bytes, bad};
use self::encoding::Encoding;
use crate::{Error, Reason, gzip};

const MAGIC: &[u8; 4] = b"CRAM";
const DEFINITION: u64 = 26;
const HEAD_READ: u64 = 1 << 16;
const FILE_HEADER: u8 = 0;
const COMPRESSION_HEADER: u8 = 1;
const SLICE_HEADER: u8 = 2;
pub(crate) const EXTERNAL: u8 = 4;
pub(crate) const CORE: u8 = 5;

/// Reference bases of a contig (by its number in the CRAM header) for a region.
pub type Fetch<'a> = dyn FnMut(usize, Region) -> Result<Vec<u8>, String> + 'a;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cram {
    pub version: (u8, u8),
    pub references: Vec<(String, u64)>,
    pub groups: Vec<String>,
    pub first: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub offset: u64,
    pub header: u64,
    pub length: u64,
    pub reference: i32,
    pub start: i64,
    pub span: i64,
    pub records: i32,
    pub landmarks: Vec<u64>,
}

impl Container {
    pub fn data(&self) -> u64 {
        self.offset + self.header
    }

    pub fn end(&self) -> u64 {
        self.data() + self.length
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub content: u8,
    pub id: i32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct Compression {
    pub names: bool,
    pub delta: bool,
    pub reference: bool,
    pub substitutions: [[u8; 4]; 5],
    pub dictionary: Vec<Vec<[u8; 3]>>,
    pub series: HashMap<[u8; 2], Encoding>,
    pub tags: HashMap<i32, Encoding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Slice {
    pub reference: i32,
    pub start: i64,
    pub span: i64,
    pub records: usize,
    pub blocks: usize,
    pub embedded: i32,
    pub md5: [u8; 16],
}

fn read_at(reader: &mut (impl Read + Seek), offset: u64, size: u64) -> Result<Vec<u8>, Error> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    reader.by_ref().take(size).read_to_end(&mut data)?;
    Ok(data)
}

fn container(
    reader: &mut (impl Read + Seek),
    offset: u64,
    major: u8,
) -> Result<Option<Container>, Error> {
    let head = read_at(reader, offset, HEAD_READ)?;
    if head.is_empty() {
        return Ok(None);
    }
    let mut bytes = Bytes::new(&head);
    let length = u64::from(bytes.u32()?);
    let reference = bytes.itf8()?;
    let start = i64::from(bytes.itf8()?);
    let span = i64::from(bytes.itf8()?);
    let records = bytes.itf8()?;
    let _counter = bytes.ltf8()?;
    let _bases = bytes.ltf8()?;
    let _blocks = bytes.itf8()?;
    let landmarks = (0..bytes.size()?)
        .map(|_| u64::try_from(bytes.itf8()?).map_err(|_| bad()))
        .collect::<Result<Vec<_>, _>>()?;
    if major >= 3 {
        bytes.take(4)?;
    }
    Ok(Some(Container {
        offset,
        header: bytes.at() as u64,
        length,
        reference,
        start,
        span,
        records,
        landmarks,
    }))
}

pub(crate) fn block(bytes: &mut Bytes, major: u8) -> Result<Block, Error> {
    let method = bytes.byte()?;
    let content = bytes.byte()?;
    let id = bytes.itf8()?;
    let packed = bytes.size()?;
    let raw = bytes.size()?;
    let data = bytes.take(packed)?;
    if major >= 3 {
        bytes.take(4)?;
    }
    let data = match method {
        0 => data.to_vec(),
        1 => gzip::inflate(data)?,
        4 => rans::decode(data)?,
        2 | 3 => return Err(Error::Invalid(Reason::CramOldCodec)),
        5..=8 => return Err(Error::Invalid(Reason::CramNewCodec)),
        _ => return Err(bad()),
    };
    if data.len() != raw {
        return Err(bad());
    }
    Ok(Block { content, id, data })
}

fn substitutions(matrix: &[u8]) -> [[u8; 4]; 5] {
    const BASES: [u8; 5] = *b"ACGTN";
    let mut table = [[b'N'; 4]; 5];
    for (row, byte) in matrix.iter().enumerate().take(5) {
        let others = BASES.iter().filter(|base| **base != BASES[row]);
        for (place, base) in others.enumerate() {
            let code = (byte >> (6 - 2 * place)) & 3;
            table[row][usize::from(code)] = *base;
        }
    }
    table
}

fn dictionary(raw: &[u8]) -> Vec<Vec<[u8; 3]>> {
    let mut lines: Vec<&[u8]> = raw.split(|byte| *byte == 0).collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.as_chunks::<3>().0.to_vec())
        .collect()
}

fn compression(data: &[u8]) -> Result<Compression, Error> {
    let mut bytes = Bytes::new(data);
    let mut header = Compression {
        names: true,
        delta: true,
        reference: true,
        substitutions: substitutions(&[0x1B, 0x1B, 0x1B, 0x1B, 0x1B]),
        dictionary: vec![Vec::new()],
        series: HashMap::new(),
        tags: HashMap::new(),
    };
    let _size = bytes.size()?;
    for _ in 0..bytes.size()? {
        let key = bytes.take(2)?;
        match key {
            b"RN" => header.names = bytes.byte()? != 0,
            b"AP" => header.delta = bytes.byte()? != 0,
            b"RR" => header.reference = bytes.byte()? != 0,
            b"SM" => header.substitutions = substitutions(bytes.take(5)?),
            b"TD" => {
                let length = bytes.size()?;
                header.dictionary = dictionary(bytes.take(length)?);
            }
            _ => return Err(bad()),
        }
    }
    let _size = bytes.size()?;
    for _ in 0..bytes.size()? {
        let key = bytes.take(2)?;
        let encoding = Encoding::read(&mut bytes)?;
        header.series.insert([key[0], key[1]], encoding);
    }
    let _size = bytes.size()?;
    for _ in 0..bytes.size()? {
        let key = bytes.itf8()?;
        let encoding = Encoding::read(&mut bytes)?;
        header.tags.insert(key, encoding);
    }
    Ok(header)
}

fn slice_header(data: &[u8]) -> Result<Slice, Error> {
    let mut bytes = Bytes::new(data);
    let reference = bytes.itf8()?;
    let start = i64::from(bytes.itf8()?);
    let span = i64::from(bytes.itf8()?);
    let records = bytes.size()?;
    let _counter = bytes.ltf8()?;
    let blocks = bytes.size()?;
    for _ in 0..bytes.size()? {
        bytes.itf8()?;
    }
    let embedded = bytes.itf8()?;
    let md5 = bytes.take(16)?.try_into().map_err(|_| bad())?;
    Ok(Slice {
        reference,
        start,
        span,
        records,
        blocks,
        embedded,
        md5,
    })
}

fn parse_header(text: &str) -> (Vec<(String, u64)>, Vec<String>) {
    let mut references = Vec::new();
    let mut groups = Vec::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        let kind = fields.next().unwrap_or_default();
        let value = |key: &str| {
            line.split('\t')
                .find_map(|field| field.strip_prefix(key))
                .map(str::to_owned)
        };
        match kind {
            "@SQ" => {
                let length = value("LN:").and_then(|text| text.parse().ok()).unwrap_or(0);
                references.push((value("SN:").unwrap_or_default(), length));
            }
            "@RG" => groups.push(value("ID:").unwrap_or_default()),
            _ => {}
        }
    }
    (references, groups)
}

impl Cram {
    pub fn read(reader: &mut (impl Read + Seek)) -> Result<Self, Error> {
        let definition = read_at(reader, 0, DEFINITION)?;
        if definition.len() != DEFINITION as usize || &definition[..4] != MAGIC {
            return Err(Error::Invalid(Reason::NotCram));
        }
        let version = (definition[4], definition[5]);
        if version.0 != 3 {
            return Err(Error::Invalid(Reason::CramVersion));
        }
        let header = container(reader, DEFINITION, version.0)?.ok_or_else(bad)?;
        let data = read_at(reader, header.data(), header.length)?;
        let first = block(&mut Bytes::new(&data), version.0)?;
        if first.content != FILE_HEADER {
            return Err(bad());
        }
        let mut text = Bytes::new(&first.data);
        let length = text.u32()? as usize;
        let text = text.take(length.min(text.rest().len()))?;
        let (references, groups) = parse_header(&String::from_utf8_lossy(text));
        Ok(Self {
            version,
            references,
            groups,
            first: header.end(),
        })
    }

    fn compression_at(
        &self,
        reader: &mut (impl Read + Seek),
        offset: u64,
    ) -> Result<(Container, Compression), Error> {
        let found = container(reader, offset, self.version.0)?.ok_or_else(bad)?;
        let data = read_at(reader, found.data(), found.length)?;
        let first = block(&mut Bytes::new(&data), self.version.0)?;
        if first.content != COMPRESSION_HEADER {
            return Err(bad());
        }
        let header = compression(&first.data)?;
        Ok((found, header))
    }

    fn slice_at(
        &self,
        reader: &mut (impl Read + Seek),
        container: &Container,
        offset: u64,
    ) -> Result<(Slice, Vec<Block>), Error> {
        let end = container
            .landmarks
            .iter()
            .copied()
            .filter(|landmark| *landmark > offset)
            .min()
            .unwrap_or(container.length);
        let data = read_at(
            reader,
            container.data() + offset,
            end.saturating_sub(offset),
        )?;
        let mut bytes = Bytes::new(&data);
        let head = block(&mut bytes, self.version.0)?;
        if head.content != SLICE_HEADER {
            return Err(bad());
        }
        let slice = slice_header(&head.data)?;
        let blocks = (0..slice.blocks)
            .map(|_| block(&mut bytes, self.version.0))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((slice, blocks))
    }

    /// The reads of a reference in a region, by the slices that the index names.
    pub fn query(
        &self,
        index: &crai::Index,
        reader: &mut (impl Read + Seek),
        reference: usize,
        region: Region,
        fetch: &mut Fetch,
        mut each: impl FnMut(Alignment),
    ) -> Result<(), Error> {
        let wanted = i32::try_from(reference).map_err(|_| bad())?;
        let mut headers: HashMap<u64, (Container, Compression)> = HashMap::new();
        for (offset, slice) in index.slices(wanted, region) {
            let (found, compression) = match headers.entry(offset) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(self.compression_at(reader, offset)?),
            };
            let (head, blocks) = self.slice_at(reader, found, slice)?;
            let reads = record::decode(self, compression, &head, &blocks, Some(wanted), fetch)?;
            for (id, alignment) in reads {
                if id == wanted && alignment.span().overlaps(region) {
                    each(alignment);
                }
            }
        }
        Ok(())
    }

    /// Every read of the file in file order, with the number of its reference (-1 for none).
    pub fn records(
        &self,
        reader: &mut (impl Read + Seek),
        fetch: &mut Fetch,
        mut each: impl FnMut(i32, Alignment),
    ) -> Result<(), Error> {
        let mut offset = self.first;
        while let Some(found) = container(reader, offset, self.version.0)? {
            if found.records > 0 {
                let (_, compression) = self.compression_at(reader, offset)?;
                for landmark in &found.landmarks {
                    let (head, blocks) = self.slice_at(reader, &found, *landmark)?;
                    for (id, alignment) in
                        record::decode(self, &compression, &head, &blocks, None, fetch)?
                    {
                        each(id, alignment);
                    }
                }
            }
            offset = found.end();
        }
        Ok(())
    }

    /// A CRAI index for the file: gzip text with a line for each slice, or for each reference
    /// of a slice that holds several.
    pub fn index(&self, reader: &mut (impl Read + Seek)) -> Result<Vec<u8>, Error> {
        let mut text = String::new();
        let mut offset = self.first;
        while let Some(found) = container(reader, offset, self.version.0)? {
            if found.records > 0 {
                let (_, compression) = self.compression_at(reader, offset)?;
                for (slot, landmark) in found.landmarks.iter().enumerate() {
                    let end = found
                        .landmarks
                        .get(slot + 1)
                        .copied()
                        .unwrap_or(found.length);
                    let size = end.saturating_sub(*landmark);
                    let (head, blocks) = self.slice_at(reader, &found, *landmark)?;
                    let spans = if head.reference == -2 {
                        record::spans(&compression, &head, &blocks)?
                    } else if head.reference < 0 {
                        vec![(-1, 0, 1)]
                    } else {
                        vec![(head.reference, head.start, head.span)]
                    };
                    for (reference, start, span) in spans {
                        let _ = writeln!(
                            text,
                            "{reference}\t{start}\t{span}\t{offset}\t{landmark}\t{size}"
                        );
                    }
                }
            }
            offset = found.end();
        }
        Ok(gzip::compress(text.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::BufReader;
    use std::path::{Path, PathBuf};

    use genie_core::Region;
    use genie_core::alignment::Alignment;

    use super::{Cram, Fetch, crai, substitutions};
    use crate::bgzf::Bgzf;
    use crate::{Error, Reason, bai, bam, gzip};

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name)
    }

    fn open(name: &str) -> BufReader<File> {
        BufReader::new(File::open(data(name)).unwrap())
    }

    /// The bases of reference.fa.gz, upper case, by contig.
    fn reference() -> Vec<Vec<u8>> {
        let packed = std::fs::read(data("reference.fa.gz")).unwrap();
        let text = gzip::inflate(&packed).unwrap();
        let mut contigs: Vec<Vec<u8>> = Vec::new();
        for line in text.split(|byte| *byte == b'\n') {
            if line.first() == Some(&b'>') {
                contigs.push(Vec::new());
            } else if let Some(last) = contigs.last_mut() {
                last.extend(line.iter().map(u8::to_ascii_uppercase));
            }
        }
        contigs
    }

    fn fetcher(contigs: &[Vec<u8>]) -> impl FnMut(usize, Region) -> Result<Vec<u8>, String> + '_ {
        |id, region| {
            let bases = &contigs[id];
            let end = (region.end() as usize).min(bases.len());
            let start = (region.start() as usize).min(end);
            Ok(bases[start..end].to_vec())
        }
    }

    fn bam_reads() -> Vec<(i32, Alignment)> {
        let mut reader = Bgzf::new(File::open(data("cram.bam")).unwrap());
        bam::header(&mut reader).unwrap();
        let mut buffer = Vec::new();
        let mut reads = Vec::new();
        while let Some(read) = bam::record(&mut reader, &mut buffer).unwrap() {
            reads.push(read);
        }
        reads
    }

    /// The first read that differs, with the fields that differ, to keep failures short.
    fn difference(found: &[Alignment], expected: &[Alignment]) -> Option<String> {
        if found.len() != expected.len() {
            return Some(format!(
                "{} reads, expected {}",
                found.len(),
                expected.len()
            ));
        }
        let (index, (read, wanted)) = found
            .iter()
            .zip(expected)
            .enumerate()
            .find(|(_, (read, wanted))| read != wanted)?;
        let text = |bases: &[u8]| String::from_utf8_lossy(bases).into_owned();
        let mut parts = vec![format!("read {index} {}", wanted.name)];
        if read.name != wanted.name {
            parts.push(format!("name {} vs {}", read.name, wanted.name));
        }
        if (read.start, read.flags, read.mapq) != (wanted.start, wanted.flags, wanted.mapq) {
            parts.push(format!(
                "start/flags/mapq {:?} vs {:?}",
                (read.start, read.flags, read.mapq),
                (wanted.start, wanted.flags, wanted.mapq)
            ));
        }
        if read.cigar != wanted.cigar {
            parts.push(format!("cigar {:?} vs {:?}", read.cigar, wanted.cigar));
        }
        if read.bases != wanted.bases {
            parts.push(format!(
                "bases {} vs {}",
                text(&read.bases),
                text(&wanted.bases)
            ));
        }
        if read.qualities != wanted.qualities {
            parts.push(format!(
                "qualities {} vs {}",
                read.qualities.len(),
                wanted.qualities.len()
            ));
        }
        if read.mate != wanted.mate {
            parts.push(format!("mate {:?} vs {:?}", read.mate, wanted.mate));
        }
        if read.tags != wanted.tags {
            parts.push(format!("tags {:?} vs {:?}", read.tags, wanted.tags));
        }
        Some(parts.join("; "))
    }

    fn compare(found: &[(i32, Alignment)], expected: &[(i32, Alignment)]) -> Option<String> {
        let ids = |reads: &[(i32, Alignment)]| reads.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        if found.len() == expected.len() && ids(found) != ids(expected) {
            return Some("reference numbers differ".to_owned());
        }
        let reads = |reads: &[(i32, Alignment)]| {
            reads
                .iter()
                .map(|(_, read)| read.clone())
                .collect::<Vec<_>>()
        };
        difference(&reads(found), &reads(expected))
    }

    fn cram_reads(name: &str, fetch: &mut Fetch) -> Result<Vec<(i32, Alignment)>, Error> {
        let mut reader = open(name);
        let cram = Cram::read(&mut reader)?;
        let mut reads = Vec::new();
        cram.records(&mut reader, fetch, |id, read| reads.push((id, read)))?;
        Ok(reads)
    }

    #[test]
    fn headers_name_the_references_and_read_groups() {
        let cram = Cram::read(&mut open("cram30.cram")).unwrap();
        assert_eq!(cram.version, (3, 0));
        assert_eq!(
            cram.references,
            [
                ("chr1".to_owned(), 120_000),
                ("chr2".to_owned(), 50_003),
                ("chrM".to_owned(), 16_569)
            ]
        );
        assert_eq!(cram.groups, ["groupA", "groupB"]);
        assert_eq!(
            Cram::read(&mut open("cram31.cram")).unwrap().version,
            (3, 1)
        );
        let error = Cram::read(&mut open("cram.bam")).unwrap_err();
        assert!(matches!(error, Error::Invalid(Reason::NotCram)), "{error}");
    }

    #[test]
    fn built_indexes_match_samtools() {
        for name in [
            "cram30.cram",
            "cram-embed.cram",
            "cram-noref.cram",
            "cram-multi.cram",
        ] {
            let mut reader = open(name);
            let cram = Cram::read(&mut reader).unwrap();
            let built = gzip::inflate(&cram.index(&mut reader).unwrap()).unwrap();
            let packed = std::fs::read(data(&format!("{name}.crai"))).unwrap();
            let expected = gzip::inflate(&packed).unwrap();
            assert_eq!(
                String::from_utf8_lossy(&built),
                String::from_utf8_lossy(&expected),
                "{name}"
            );
        }
    }

    #[test]
    fn substitution_matrices_list_the_other_bases_by_code() {
        // 0x1B = 00 01 10 11: the other bases in ACGTN order get codes 0 to 3.
        let table = substitutions(&[0x1B, 0x1B, 0x1B, 0x1B, 0xE4]);
        assert_eq!(table[0], *b"CGTN");
        assert_eq!(table[2], *b"ACTN");
        // 0xE4 = 11 10 01 00: A gets 3, C 2, G 1 and T 0.
        assert_eq!(table[4], *b"TGCA");
    }

    #[test]
    fn cram_3_0_reads_equal_the_bam_reads() {
        let contigs = reference();
        let expected = bam_reads();
        for name in ["cram30.cram", "cram-multi.cram"] {
            let reads = cram_reads(name, &mut fetcher(&contigs)).unwrap();
            assert_eq!(compare(&reads, &expected), None, "{name}");
        }
    }

    #[test]
    fn embedded_and_reference_free_files_need_no_reference() {
        let expected = bam_reads();
        for name in ["cram-embed.cram", "cram-noref.cram"] {
            let mut none = |_: usize, _: Region| Err::<Vec<u8>, String>("no reference".to_owned());
            let reads = cram_reads(name, &mut none).unwrap();
            assert_eq!(compare(&reads, &expected), None, "{name}");
        }
    }

    #[test]
    fn region_queries_equal_the_bam_queries() {
        let contigs = reference();
        let mut reader = open("cram30.cram");
        let cram = Cram::read(&mut reader).unwrap();
        let index = crai::Index::read(File::open(data("cram30.cram.crai")).unwrap()).unwrap();
        let bai = bai::Index::read(File::open(data("cram.bam.bai")).unwrap()).unwrap();
        let mut bgzf = Bgzf::new(File::open(data("cram.bam")).unwrap());
        for (reference, start, end) in [
            (0, 0, 1_000),
            (0, 19_000, 21_000),
            (0, 59_000, 61_000),
            (1, 0, 50_003),
            (2, 5_000, 6_000),
        ] {
            let region = Region::between(start, end);
            let mut found = Vec::new();
            cram.query(
                &index,
                &mut reader,
                reference,
                region,
                &mut fetcher(&contigs),
                |read| {
                    found.push(read);
                },
            )
            .unwrap();
            let mut expected = Vec::new();
            bam::query(&bai, &mut bgzf, reference, region, |read| {
                expected.push(read);
                Ok(())
            })
            .unwrap();
            assert!(!expected.is_empty(), "{reference} {region:?}");
            assert_eq!(
                difference(&found, &expected),
                None,
                "{reference} {region:?}"
            );
        }
    }

    #[test]
    fn a_different_reference_fails_the_slice_check() {
        let mut contigs = reference();
        for base in &mut contigs[0][1_000..1_100] {
            *base = if *base == b'A' { b'C' } else { b'A' };
        }
        let error = cram_reads("cram30.cram", &mut fetcher(&contigs)).unwrap_err();
        assert!(
            matches!(error, Error::Invalid(Reason::CramReference)),
            "{error}"
        );
    }

    #[test]
    fn unread_compressions_say_which() {
        let contigs = reference();
        let error = cram_reads("cram-xz.cram", &mut fetcher(&contigs)).unwrap_err();
        assert!(
            matches!(error, Error::Invalid(Reason::CramOldCodec)),
            "{error}"
        );
        let error = cram_reads("cram31.cram", &mut fetcher(&contigs)).unwrap_err();
        assert!(
            matches!(error, Error::Invalid(Reason::CramNewCodec)),
            "{error}"
        );
    }
}
