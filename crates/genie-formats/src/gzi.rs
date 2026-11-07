use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::binning::read_u64;
use crate::sidecar::{self, Sidecar};
use crate::{Error, Reason, bgzf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    blocks: Vec<(u64, u64)>,
}

impl Index {
    pub fn read(mut reader: impl Read) -> Result<Self, Error> {
        let mut blocks = Vec::new();
        for _ in 0..read_u64(&mut reader)? {
            blocks.push((read_u64(&mut reader)?, read_u64(&mut reader)?));
        }
        let ordered = blocks
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 <= pair[1].1);
        if !ordered {
            return Err(Error::Invalid(Reason::BadIndex));
        }
        Ok(Self { blocks })
    }

    pub fn build(mut reader: impl Read) -> Result<Self, Error> {
        let mut blocks = Vec::new();
        let mut compressed = Vec::new();
        let mut next = (0, 0);
        while let Some((size, length)) = bgzf::read_block(&mut reader, &mut compressed)? {
            if next.0 > 0 && length > 0 {
                blocks.push(next);
            }
            next.0 += size as u64;
            next.1 += length as u64;
        }
        Ok(Self { blocks })
    }

    pub fn write(&self, mut writer: impl Write) -> io::Result<()> {
        writer.write_all(&(self.blocks.len() as u64).to_le_bytes())?;
        for (compressed, uncompressed) in &self.blocks {
            writer.write_all(&compressed.to_le_bytes())?;
            writer.write_all(&uncompressed.to_le_bytes())?;
        }
        writer.flush()
    }

    pub fn virtual_offset(&self, position: u64) -> Result<u64, Error> {
        let after = self.blocks.partition_point(|block| block.1 <= position);
        let (offset, start) = after.checked_sub(1).map_or((0, 0), |at| self.blocks[at]);
        let within = position - start;
        if within > 0xFFFF {
            return Err(Error::Invalid(Reason::BadOffset));
        }
        Ok(offset << 16 | within)
    }
}

pub fn load(
    path: &Path,
    cache: Option<&Path>,
    progress: &mut dyn FnMut(f32),
) -> Result<Index, Error> {
    sidecar::load(path, cache, progress, Index::build)
}

impl Sidecar for Index {
    const EXTENSION: &'static str = "gzi";

    fn parse(file: File) -> Result<Self, Error> {
        Self::read(BufReader::new(file))
    }

    fn save(&self, file: File) -> io::Result<()> {
        self.write(BufWriter::new(file))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Index, load};
    use crate::bgzf::tests::{block, file};
    use crate::fai::tests::{Scratch, cached};
    use crate::{Error, Reason};

    #[test]
    fn load_builds_and_caches_a_missing_index() {
        let scratch = Scratch::new("gzi-build");
        let path = scratch.file("ref.fa.gz", file(&[b"ab", b"cd"]));
        let cache = scratch.0.join("cache");
        let built = Index::build(Cursor::new(file(&[b"ab", b"cd"]))).unwrap();
        assert_eq!(load(&path, Some(&cache), &mut |_| {}).unwrap(), built);
        assert_eq!(cached(&cache).len(), 1);
        assert_eq!(load(&path, Some(&cache), &mut |_| {}).unwrap(), built);
        assert!(!scratch.0.join("ref.fa.gz.gzi").exists());
    }

    #[test]
    fn load_prefers_the_index_next_to_the_file() {
        let scratch = Scratch::new("gzi-sidecar");
        let path = scratch.file("ref.fa.gz", file(&[b"ab", b"cd"]));
        scratch.file("ref.fa.gz.gzi", bytes(&[(7, 9)]));
        assert_eq!(load(&path, None, &mut |_| {}).unwrap(), index(&[(7, 9)]));
    }

    fn bytes(blocks: &[(u64, u64)]) -> Vec<u8> {
        let mut bytes = (blocks.len() as u64).to_le_bytes().to_vec();
        for (compressed, uncompressed) in blocks {
            bytes.extend(compressed.to_le_bytes());
            bytes.extend(uncompressed.to_le_bytes());
        }
        bytes
    }

    fn index(blocks: &[(u64, u64)]) -> Index {
        Index::read(bytes(blocks).as_slice()).unwrap()
    }

    #[test]
    fn maps_positions_to_virtual_offsets() {
        let index = index(&[(100, 65280), (200, 130_560)]);
        assert_eq!(index.virtual_offset(0).unwrap(), 0);
        assert_eq!(index.virtual_offset(65279).unwrap(), 65279);
        assert_eq!(index.virtual_offset(65280).unwrap(), 100 << 16);
        assert_eq!(index.virtual_offset(130_570).unwrap(), (200 << 16) + 10);
    }

    #[test]
    fn skips_empty_blocks() {
        let index = index(&[(50, 0), (90, 10), (120, 10)]);
        assert_eq!(index.virtual_offset(0).unwrap(), 50 << 16);
        assert_eq!(index.virtual_offset(12).unwrap(), 120 << 16 | 2);
    }

    #[test]
    fn rejects_bad_indexes() {
        let backwards = Index::read(bytes(&[(200, 10), (100, 20)]).as_slice());
        assert!(matches!(backwards, Err(Error::Invalid(Reason::BadIndex))));
        let short = Index::read(&bytes(&[(100, 10)])[..12]);
        assert!(matches!(short, Err(Error::Io(_))));
        let far = index(&[(100, 10)]).virtual_offset(10 + 65536);
        assert!(matches!(far, Err(Error::Invalid(Reason::BadOffset))));
    }

    #[test]
    fn builds_from_block_headers() {
        let first = block(b"hello ").len() as u64;
        let second = block(b"").len() as u64;
        let built = Index::build(Cursor::new(file(&[b"hello ", b"", b"world\n"]))).unwrap();
        assert_eq!(built, index(&[(first + second, 6)]));
    }

    #[test]
    fn written_index_reads_back() {
        let built = Index::build(Cursor::new(file(&[b"a", b"bc", b"d"]))).unwrap();
        let mut written = Vec::new();
        built.write(&mut written).unwrap();
        assert_eq!(Index::read(written.as_slice()).unwrap(), built);
    }
}
