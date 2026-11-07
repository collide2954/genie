use std::io::{self, Read};

use genie_core::Region;

use crate::{Error, Reason};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    shift: u32,
    depth: u32,
}

impl Default for Scheme {
    fn default() -> Self {
        Self::BAI
    }
}

impl Scheme {
    pub const BAI: Self = Self {
        shift: 14,
        depth: 5,
    };

    pub fn new(shift: i32, depth: i32) -> Result<Self, Error> {
        let shift = u32::try_from(shift).map_err(|_| Error::Invalid(Reason::BadIndex))?;
        let depth = u32::try_from(depth).map_err(|_| Error::Invalid(Reason::BadIndex))?;
        if depth > 9 || shift + 3 * depth > 62 {
            return Err(Error::Invalid(Reason::BadIndex));
        }
        Ok(Self { shift, depth })
    }

    fn top(self) -> u32 {
        self.shift + 3 * self.depth
    }

    fn first(level: u32) -> u32 {
        ((1 << (3 * level)) - 1) / 7
    }
}

#[derive(Debug)]
struct Bin {
    number: u32,
    offset: u64,
    chunks: Vec<Chunk>,
}

#[derive(Debug, Default)]
pub struct Bins {
    scheme: Scheme,
    entries: Vec<Bin>,
    intervals: Option<Vec<u64>>,
}

impl Bins {
    pub fn read(reader: &mut impl Read) -> Result<Self, Error> {
        let bins = (0..read_count(reader)?)
            .map(|_| {
                let number = read_u32(reader)?;
                let chunks = read_chunks(reader)?;
                Ok(Bin {
                    number,
                    offset: 0,
                    chunks,
                })
            })
            .collect::<Result<_, Error>>()?;
        let intervals = (0..read_count(reader)?)
            .map(|_| read_u64(reader))
            .collect::<io::Result<_>>()?;
        Ok(Self::new(Scheme::BAI, bins, Some(intervals)))
    }

    pub fn read_csi(reader: &mut impl Read, scheme: Scheme) -> Result<Self, Error> {
        let bins = (0..read_count(reader)?)
            .map(|_| {
                let number = read_u32(reader)?;
                let offset = read_u64(reader)?;
                let chunks = read_chunks(reader)?;
                Ok(Bin {
                    number,
                    offset,
                    chunks,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self::new(scheme, bins, None))
    }

    fn new(scheme: Scheme, mut entries: Vec<Bin>, intervals: Option<Vec<u64>>) -> Self {
        entries.sort_unstable_by_key(|bin| bin.number);
        Self {
            scheme,
            entries,
            intervals,
        }
    }

    pub fn chunks(&self, region: Region) -> Vec<Chunk> {
        let top = self.scheme.top();
        let limit = 1_u64 << top;
        if region.is_empty() || region.start() >= limit {
            return Vec::new();
        }
        let start = region.start();
        let last = region.end().min(limit) - 1;
        let minimum = match &self.intervals {
            Some(intervals) => intervals
                .get((start >> self.scheme.shift) as usize)
                .or(intervals.last())
                .copied()
                .unwrap_or(0),
            None => self.first_offset(start),
        };
        let mut chunks: Vec<Chunk> = (0..=self.scheme.depth)
            .flat_map(|level| {
                let offset = Scheme::first(level);
                let shift = top - 3 * level;
                offset + (start >> shift) as u32..=offset + (last >> shift) as u32
            })
            .filter_map(|number| self.find(number))
            .flat_map(|bin| bin.chunks.iter())
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

    fn first_offset(&self, start: u64) -> u64 {
        let mut number = Scheme::first(self.scheme.depth) + (start >> self.scheme.shift) as u32;
        loop {
            if let Some(bin) = self.find(number) {
                return bin.offset;
            }
            if number == 0 {
                return 0;
            }
            let parent = (number - 1) >> 3;
            let first = (parent << 3) + 1;
            number = if number > first { number - 1 } else { parent };
        }
    }

    fn find(&self, number: u32) -> Option<&Bin> {
        let found = self.entries.binary_search_by_key(&number, |bin| bin.number);
        found.ok().map(|index| &self.entries[index])
    }
}

fn read_chunks(reader: &mut impl Read) -> Result<Vec<Chunk>, Error> {
    (0..read_count(reader)?)
        .map(|_| {
            Ok(Chunk {
                start: read_u64(reader)?,
                end: read_u64(reader)?,
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use genie_core::Region;

    use super::{Bin, Bins, Chunk, Scheme};

    fn bins(scheme: Scheme, bins: &[(u32, u64, u64, u64)]) -> Bins {
        let bins = bins
            .iter()
            .map(|&(number, offset, start, end)| Bin {
                number,
                offset,
                chunks: vec![Chunk { start, end }],
            })
            .collect();
        Bins::new(scheme, bins, None)
    }

    fn starts(bins: &Bins, start: u64, end: u64) -> Vec<u64> {
        bins.chunks(Region::new(start, end).unwrap())
            .iter()
            .map(|chunk| chunk.start)
            .collect()
    }

    #[test]
    fn schemes_check_their_shape() {
        assert!(Scheme::new(14, 6).is_ok());
        assert!(Scheme::new(-1, 5).is_err());
        assert!(Scheme::new(14, 20).is_err());
    }

    #[test]
    fn deeper_schemes_reach_long_contigs() {
        let scheme = Scheme::new(14, 6).unwrap();
        let leaf = 37_449 + (600_000_000 >> 14);
        let index = bins(scheme, &[(leaf, 0, 100, 200)]);
        assert_eq!(starts(&index, 600_000_000, 600_000_100), [100]);
        let bai = bins(Scheme::BAI, &[(0, 0, 100, 200)]);
        assert_eq!(starts(&bai, 600_000_000, 600_000_100), [] as [u64; 0]);
    }

    #[test]
    fn csi_drops_chunks_before_the_first_offset() {
        let index = bins(Scheme::BAI, &[(0, 0, 10, 20), (4681, 35, 40, 50)]);
        assert_eq!(starts(&index, 0, 100), [40]);
    }

    #[test]
    fn csi_walks_left_then_up_for_the_first_offset() {
        let index = bins(
            Scheme::BAI,
            &[(0, 0, 10, 20), (4681, 25, 40, 50), (585, 15, 60, 70)],
        );
        assert_eq!(starts(&index, 16_384, 16_400), [60]);
        let index = bins(Scheme::BAI, &[(0, 0, 10, 20), (585, 15, 60, 70)]);
        assert_eq!(starts(&index, 16_384, 16_400), [10, 60]);
    }
}
