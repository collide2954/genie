use std::collections::BTreeMap;
use std::io::{self, Read};

use genie_core::Region;

use crate::{Error, Reason};

const UNSET: u64 = u64::MAX;
const MIN_MARKER_DISTANCE: u64 = 0x10000;

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

    pub fn reaching(shift: u32, depth: u32, length: u64) -> Self {
        let mut depth = depth;
        while shift + 3 * (depth + 1) <= 62 && length > 1 << (shift + 3 * depth) {
            depth += 1;
        }
        Self { shift, depth }
    }

    pub fn shift(self) -> u32 {
        self.shift
    }

    pub fn depth(self) -> u32 {
        self.depth
    }

    fn top(self) -> u32 {
        self.shift + 3 * self.depth
    }

    fn first(level: u32) -> u32 {
        ((1 << (3 * level)) - 1) / 7
    }

    fn bins(self) -> u32 {
        Self::first(self.depth + 1)
    }

    fn meta(self) -> u32 {
        self.bins() + 1
    }

    fn bin(self, start: i64, end: i64) -> i64 {
        let last = end - 1;
        for level in (1..=self.depth).rev() {
            let shift = self.shift + 3 * (self.depth - level);
            if start >> shift == last >> shift {
                return i64::from(Self::first(level)) + (start >> shift);
            }
        }
        0
    }

    fn bottom(self, bin: u32) -> usize {
        let (mut level, mut above) = (0, bin);
        while above != 0 {
            level += 1;
            above = (above - 1) >> 3;
        }
        ((bin - Self::first(level)) as usize) << (3 * (self.depth - level))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Bin {
    number: u32,
    offset: u64,
    chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disorder {
    Unsorted,
    Split,
    Backward,
    Long,
}

impl Disorder {
    pub fn reason(self) -> Reason {
        match self {
            Self::Unsorted => Reason::Unsorted,
            Self::Split => Reason::Scattered,
            Self::Backward => Reason::BadSpan,
            Self::Long => Reason::TooLong,
        }
    }
}

pub struct Builder {
    scheme: Scheme,
    csi: bool,
    references: Vec<Option<Building>>,
    last_tid: Option<usize>,
    last_bin: Option<i64>,
    last_off: u64,
    last_coor: i64,
    save_tid: Option<usize>,
    save_bin: Option<i64>,
    save_off: u64,
    off_beg: u64,
    mapped: u64,
    unmapped: u64,
    unplaced: u64,
}

impl Builder {
    pub fn new(scheme: Scheme, csi: bool, start: u64) -> Self {
        Self {
            scheme,
            csi,
            references: Vec::new(),
            last_tid: None,
            last_bin: None,
            last_off: start,
            last_coor: 0xFFFF_FFFF,
            save_tid: None,
            save_bin: None,
            save_off: start,
            off_beg: start,
            mapped: 0,
            unmapped: 0,
            unplaced: 0,
        }
    }

    pub fn push(
        &mut self,
        place: Option<(usize, u64, u64)>,
        offset: u64,
        mapped: bool,
    ) -> Result<(), Disorder> {
        let tid = place.map(|(tid, _, _)| tid);
        let signed = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
        let (mut start, mut end) =
            place.map_or((-1, 0), |(_, start, end)| (signed(start), signed(end)));
        let limit = signed(1 << self.scheme.top());
        if tid.is_some() && (start > limit || end > limit) {
            return Err(Disorder::Long);
        }
        if self.last_tid != tid {
            if let Some(tid) = tid {
                if self.unplaced > 0 {
                    return Err(Disorder::Unsorted);
                }
                if self.references.get(tid).is_some_and(Option::is_some) {
                    return Err(Disorder::Split);
                }
            }
            self.last_tid = tid;
            self.last_bin = None;
        } else if tid.is_some() && self.last_coor > start {
            return Err(Disorder::Unsorted);
        }
        if end < start {
            return Err(Disorder::Backward);
        }
        if let Some(tid) = tid {
            if self.references.len() <= tid {
                self.references.resize_with(tid + 1, || None);
            }
            let reference = self.references[tid].get_or_insert_with(Building::default);
            if mapped {
                start = start.max(0);
                end = end.max(1);
                reference.window(start, end, self.last_off, self.scheme.shift);
            }
        } else {
            self.unplaced += 1;
        }
        let bin = self.scheme.bin(start, end);
        if self.last_bin != Some(bin) {
            if let (Some(tid), Some(saved)) = (self.save_tid, self.save_bin) {
                self.insert(tid, saved, self.save_off, self.last_off);
            }
            if self.last_bin.is_none() && self.save_bin.is_some() {
                if let Some(tid) = self.save_tid {
                    let meta = i64::from(self.scheme.meta());
                    self.insert(tid, meta, self.off_beg, self.last_off);
                    self.insert(tid, meta, self.mapped, self.unmapped);
                }
                (self.mapped, self.unmapped) = (0, 0);
                self.off_beg = self.last_off;
            }
            self.save_off = self.last_off;
            self.save_bin = Some(bin);
            self.last_bin = Some(bin);
            self.save_tid = tid;
        }
        if mapped {
            self.mapped += 1;
        } else {
            self.unmapped += 1;
        }
        self.last_off = offset;
        self.last_coor = start;
        Ok(())
    }

    pub fn finish(mut self, offset: u64) -> (Vec<Bins>, u64) {
        if let (Some(tid), Some(saved)) = (self.save_tid, self.save_bin) {
            self.insert(tid, saved, self.save_off, offset);
            let meta = i64::from(self.scheme.meta());
            self.insert(tid, meta, self.off_beg, offset);
            self.insert(tid, meta, self.mapped, self.unmapped);
        }
        let (scheme, csi) = (self.scheme, self.csi);
        let references = self
            .references
            .into_iter()
            .map(|reference| {
                reference.map_or_else(
                    || Bins::empty(scheme, csi),
                    |reference| reference.finish(scheme, csi),
                )
            })
            .collect();
        (references, self.unplaced)
    }

    fn insert(&mut self, tid: usize, bin: i64, start: u64, end: u64) {
        if let (Some(Some(reference)), Ok(bin)) = (self.references.get_mut(tid), u32::try_from(bin))
        {
            reference
                .bins
                .entry(bin)
                .or_default()
                .push(Chunk { start, end });
        }
    }
}

#[derive(Default)]
struct Building {
    bins: BTreeMap<u32, Vec<Chunk>>,
    linear: Vec<u64>,
}

impl Building {
    fn window(&mut self, start: i64, end: i64, offset: u64, shift: u32) {
        let first = usize::try_from(start >> shift).unwrap_or(0);
        let last = usize::try_from((end - 1) >> shift).unwrap_or(0);
        if self.linear.len() < last + 1 {
            self.linear.resize(last + 1, UNSET);
        }
        for window in first..=last {
            if self.linear[window] == UNSET {
                self.linear[window] = offset;
            }
        }
    }

    fn finish(mut self, scheme: Scheme, csi: bool) -> Bins {
        let count = scheme.bins();
        let mut next = UNSET;
        for value in self.linear.iter_mut().rev() {
            if *value == UNSET {
                *value = next;
            }
            next = *value;
        }
        let offsets: BTreeMap<u32, u64> = self
            .bins
            .keys()
            .map(|&number| {
                let bottom = (number < count).then(|| scheme.bottom(number));
                let offset = bottom.and_then(|at| self.linear.get(at)).copied();
                (number, offset.unwrap_or(0))
            })
            .collect();
        self.compress(scheme);
        let entries = self
            .bins
            .into_iter()
            .map(|(number, chunks)| Bin {
                number,
                offset: if csi { offsets[&number] } else { 0 },
                chunks,
            })
            .collect();
        Bins::new(scheme, entries, (!csi).then_some(self.linear))
    }

    fn compress(&mut self, scheme: Scheme) {
        let count = scheme.bins();
        for level in (1..=scheme.depth).rev() {
            let first = Scheme::first(level);
            let numbers: Vec<u32> = self
                .bins
                .range(first..count)
                .map(|(number, _)| *number)
                .collect();
            for number in numbers {
                let Some(chunks) = self.bins.get_mut(&number) else {
                    continue;
                };
                if level < scheme.depth {
                    chunks.sort_by_key(|chunk| chunk.start);
                }
                let (Some(head), Some(tail)) = (chunks.first(), chunks.last()) else {
                    continue;
                };
                let span = (tail.end >> 16).saturating_sub(head.start >> 16);
                let parent = (number - 1) >> 3;
                if span < MIN_MARKER_DISTANCE && self.bins.contains_key(&parent) {
                    let moved = self.bins.remove(&number).unwrap_or_default();
                    self.bins.entry(parent).or_default().extend(moved);
                }
            }
        }
        if let Some(chunks) = self.bins.get_mut(&0) {
            chunks.sort_by_key(|chunk| chunk.start);
        }
        for (_, chunks) in self.bins.range_mut(..count) {
            let mut merged: Vec<Chunk> = Vec::with_capacity(chunks.len());
            for chunk in chunks.drain(..) {
                match merged.last_mut() {
                    Some(last) if last.end >> 16 >= chunk.start >> 16 => {
                        last.end = last.end.max(chunk.end);
                    }
                    _ => merged.push(chunk),
                }
            }
            *chunks = merged;
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
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

    pub fn empty(scheme: Scheme, csi: bool) -> Self {
        Self::new(scheme, Vec::new(), (!csi).then(Vec::new))
    }

    fn new(scheme: Scheme, mut entries: Vec<Bin>, intervals: Option<Vec<u64>>) -> Self {
        entries.sort_unstable_by_key(|bin| bin.number);
        Self {
            scheme,
            entries,
            intervals,
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        put(out, self.entries.len());
        for bin in &self.entries {
            out.extend(bin.number.to_le_bytes());
            if self.intervals.is_none() {
                out.extend(bin.offset.to_le_bytes());
            }
            put(out, bin.chunks.len());
            for chunk in &bin.chunks {
                out.extend(chunk.start.to_le_bytes());
                out.extend(chunk.end.to_le_bytes());
            }
        }
        if let Some(intervals) = &self.intervals {
            put(out, intervals.len());
            for offset in intervals {
                out.extend(offset.to_le_bytes());
            }
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

pub fn put(out: &mut Vec<u8>, count: usize) {
    out.extend(i32::try_from(count).unwrap_or(i32::MAX).to_le_bytes());
}

pub fn read_count(reader: &mut impl Read) -> Result<usize, Error> {
    let value = read_u32(reader)?.cast_signed();
    usize::try_from(value).map_err(|_| Error::Invalid(Reason::BadIndex))
}

#[cfg(test)]
mod tests {
    use genie_core::Region;

    use super::{Bin, Bins, Builder, Chunk, Disorder, Scheme};

    fn built(number: u32, offset: u64, chunks: &[(u64, u64)]) -> Bin {
        Bin {
            number,
            offset,
            chunks: chunks
                .iter()
                .map(|&(start, end)| Chunk { start, end })
                .collect(),
        }
    }

    #[test]
    fn records_in_one_bin_make_one_chunk_and_a_meta_bin() {
        let mut builder = Builder::new(Scheme::BAI, false, 100);
        builder.push(Some((0, 10, 20)), 200, true).unwrap();
        builder.push(Some((0, 30, 40)), 300, true).unwrap();
        let (references, unplaced) = builder.finish(300);
        assert_eq!(unplaced, 0);
        let expected = Bins::new(
            Scheme::BAI,
            vec![
                built(4681, 0, &[(100, 300)]),
                built(37_450, 0, &[(100, 300), (2, 0)]),
            ],
            Some(vec![100]),
        );
        assert_eq!(references, [expected]);
    }

    #[test]
    fn small_bins_join_their_parent_and_chunks_in_one_block_merge() {
        let mut builder = Builder::new(Scheme::BAI, true, 100);
        builder.push(Some((0, 10, 20)), 200, true).unwrap();
        builder.push(Some((0, 16_000, 17_000)), 300, true).unwrap();
        builder.push(Some((0, 20_000, 20_010)), 400, false).unwrap();
        let (references, _) = builder.finish(400);
        let expected = Bins::new(
            Scheme::BAI,
            vec![
                built(585, 100, &[(100, 400)]),
                built(37_450, 0, &[(100, 400), (2, 1)]),
            ],
            None,
        );
        assert_eq!(references, [expected]);
    }

    #[test]
    fn bins_far_apart_keep_their_own_chunks() {
        let block = 1 << 32;
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(Some((0, 10, 20)), 3 * block, true).unwrap();
        builder
            .push(Some((0, 16_000, 17_000)), 4 * block, true)
            .unwrap();
        builder
            .push(Some((0, 20_000, 20_010)), 7 * block, true)
            .unwrap();
        builder.push(None, 7 * block + 50, false).unwrap();
        let (references, unplaced) = builder.finish(7 * block + 50);
        assert_eq!(unplaced, 1);
        let expected = Bins::new(
            Scheme::BAI,
            vec![
                built(585, 0, &[(3 * block, 4 * block)]),
                built(4681, 0, &[(0, 3 * block)]),
                built(4682, 0, &[(4 * block, 7 * block)]),
                built(37_450, 0, &[(0, 7 * block), (3, 0)]),
            ],
            Some(vec![0, 3 * block]),
        );
        assert_eq!(references, [expected]);
    }

    #[test]
    fn contigs_without_records_have_no_bins() {
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(Some((1, 10, 20)), 50, true).unwrap();
        let (references, _) = builder.finish(50);
        assert_eq!(references.len(), 2);
        assert_eq!(
            references[0],
            Bins::new(Scheme::BAI, Vec::new(), Some(Vec::new()))
        );
    }

    #[test]
    fn records_out_of_order_stop_the_index() {
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(Some((0, 50, 60)), 10, true).unwrap();
        assert_eq!(
            builder.push(Some((0, 10, 20)), 20, true),
            Err(Disorder::Unsorted)
        );
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(Some((0, 50, 60)), 10, true).unwrap();
        builder.push(Some((1, 10, 20)), 20, true).unwrap();
        assert_eq!(
            builder.push(Some((0, 70, 80)), 30, true),
            Err(Disorder::Split)
        );
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(None, 10, false).unwrap();
        assert_eq!(
            builder.push(Some((0, 1, 2)), 20, true),
            Err(Disorder::Unsorted)
        );
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        let far = 1 << 29;
        assert_eq!(
            builder.push(Some((0, far, far + 5)), 10, true),
            Err(Disorder::Long)
        );
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        assert_eq!(
            builder.push(Some((0, 20, 10)), 10, true),
            Err(Disorder::Backward)
        );
    }

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
