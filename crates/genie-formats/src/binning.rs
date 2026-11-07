use std::io::{self, Read};

use genie_core::Region;

use crate::{Error, Reason};

const LEVELS: [(u32, u32); 6] = [(0, 29), (1, 26), (9, 23), (73, 20), (585, 17), (4681, 14)];
const WINDOW: u32 = 14;
const LIMIT: u64 = 1 << 29;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Default)]
pub struct Bins {
    bins: Vec<(u32, Vec<Chunk>)>,
    intervals: Vec<u64>,
}

impl Bins {
    pub fn read(reader: &mut impl Read) -> Result<Self, Error> {
        let mut bins = Vec::new();
        for _ in 0..read_count(reader)? {
            let bin = read_u32(reader)?;
            let chunks = (0..read_count(reader)?)
                .map(|_| {
                    Ok(Chunk {
                        start: read_u64(reader)?,
                        end: read_u64(reader)?,
                    })
                })
                .collect::<io::Result<_>>()?;
            bins.push((bin, chunks));
        }
        bins.sort_unstable_by_key(|(bin, _)| *bin);
        let intervals = (0..read_count(reader)?)
            .map(|_| read_u64(reader))
            .collect::<io::Result<_>>()?;
        Ok(Self { bins, intervals })
    }

    pub fn chunks(&self, region: Region) -> Vec<Chunk> {
        if region.is_empty() || region.start() >= LIMIT {
            return Vec::new();
        }
        let start = region.start();
        let last = region.end().min(LIMIT) - 1;
        let window = (start >> WINDOW) as usize;
        let minimum = self
            .intervals
            .get(window)
            .or(self.intervals.last())
            .copied()
            .unwrap_or(0);
        let mut chunks: Vec<Chunk> = LEVELS
            .into_iter()
            .flat_map(|(offset, shift)| {
                offset + (start >> shift) as u32..=offset + (last >> shift) as u32
            })
            .filter_map(|bin| self.find(bin))
            .flatten()
            .filter(|chunk| chunk.end > minimum)
            .copied()
            .collect();
        chunks.sort_unstable_by_key(|chunk| chunk.start);
        let mut merged: Vec<Chunk> = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            match merged.last_mut() {
                Some(last) if chunk.start <= last.end => last.end = last.end.max(chunk.end),
                _ => merged.push(chunk),
            }
        }
        merged
    }

    fn find(&self, bin: u32) -> Option<&Vec<Chunk>> {
        let found = self.bins.binary_search_by_key(&bin, |(number, _)| *number);
        found.ok().map(|index| &self.bins[index].1)
    }
}

pub fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

pub fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn read_count(reader: &mut impl Read) -> Result<usize, Error> {
    let value = read_u32(reader)?.cast_signed();
    usize::try_from(value).map_err(|_| Error::Invalid(Reason::BadIndex))
}
