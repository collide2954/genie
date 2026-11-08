use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use genie_core::{Region, Tiling};

pub const TILING: Tiling = Tiling::new(4_096);
pub const CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub contig: usize,
    pub index: u64,
}

#[derive(Debug, Default)]
pub struct Tiles {
    loaded: HashMap<Key, Arc<[u8]>>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
}

impl Tiles {
    pub fn request(&mut self, contig: usize, region: Region) -> Vec<Key> {
        TILING
            .covering(region)
            .map(|index| Key { contig, index })
            .filter(|key| !self.loaded.contains_key(key) && self.pending.insert(*key))
            .collect()
    }

    pub fn insert(&mut self, key: Key, bases: Arc<[u8]>) {
        self.pending.remove(&key);
        if self.loaded.insert(key, bases).is_none() {
            self.order.push_back(key);
        }
        while self.loaded.len() > CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.loaded.remove(&oldest);
        }
    }

    pub fn cancel(&mut self, key: Key) {
        self.pending.remove(&key);
    }

    pub fn chunks(&self, contig: usize, region: Region) -> impl Iterator<Item = (u64, &[u8])> {
        TILING.covering(region).filter_map(move |index| {
            self.loaded
                .get(&Key { contig, index })
                .map(|bases| (TILING.span(index).start(), &bases[..]))
        })
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn count(&self) -> usize {
        self.loaded.len()
    }

    pub fn waiting(&self) -> Vec<Key> {
        self.pending.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::Region;

    use super::{CAPACITY, Key, TILING, Tiles};

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn key(index: u64) -> Key {
        Key { contig: 0, index }
    }

    fn bases(byte: u8) -> Arc<[u8]> {
        Arc::from(vec![byte; 4_096])
    }

    #[test]
    fn requests_each_missing_tile_once() {
        let mut tiles = Tiles::default();
        assert_eq!(tiles.request(0, region(0, 5_000)), [key(0), key(1)]);
        assert_eq!(tiles.request(0, region(0, 5_000)), []);
    }

    #[test]
    fn loaded_tiles_become_chunks() {
        let mut tiles = Tiles::default();
        let _ = tiles.request(0, region(0, 5_000));
        tiles.insert(key(1), bases(b'G'));
        let chunks: Vec<(u64, usize)> = tiles
            .chunks(0, region(0, 5_000))
            .map(|(start, bases)| (start, bases.len()))
            .collect();
        assert_eq!(chunks, [(TILING.span(1).start(), 4_096)]);
        assert_eq!(tiles.request(0, region(0, 5_000)), []);
    }

    #[test]
    fn cancelled_tiles_can_be_requested_again() {
        let mut tiles = Tiles::default();
        let _ = tiles.request(0, region(0, 10));
        tiles.cancel(key(0));
        assert_eq!(tiles.request(0, region(0, 10)), [key(0)]);
    }

    #[test]
    fn oldest_tiles_are_evicted_first() {
        let mut tiles = Tiles::default();
        for index in 0..=CAPACITY as u64 {
            tiles.insert(key(index), bases(b'A'));
        }
        assert_eq!(tiles.chunks(0, TILING.span(0)).count(), 0);
        assert_eq!(tiles.chunks(0, TILING.span(1)).count(), 1);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut tiles = Tiles::default();
        let _ = tiles.request(0, region(0, 10));
        tiles.insert(key(0), bases(b'A'));
        tiles.clear();
        assert_eq!(tiles.chunks(0, region(0, 10)).count(), 0);
        assert_eq!(tiles.request(0, region(0, 10)), [key(0)]);
    }
}
