use std::ops::Range;

use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tiling {
    size: u64,
}

impl Tiling {
    pub const fn new(size: u64) -> Self {
        Self { size }
    }

    pub const fn covering(self, region: Region) -> Range<u64> {
        if region.is_empty() {
            return 0..0;
        }
        region.start() / self.size..(region.end() - 1) / self.size + 1
    }

    pub const fn span(self, index: u64) -> Region {
        Region::between(index * self.size, (index + 1) * self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::Tiling;
    use crate::Region;

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn covering_lists_overlapping_tiles() {
        let tiling = Tiling::new(4_096);
        assert_eq!(tiling.covering(region(0, 4_096)), 0..1);
        assert_eq!(tiling.covering(region(4_095, 4_097)), 0..2);
        assert_eq!(tiling.covering(region(10_000, 20_000)), 2..5);
    }

    #[test]
    fn empty_regions_cover_no_tiles() {
        assert!(Tiling::new(4_096).covering(region(5, 5)).is_empty());
    }

    #[test]
    fn span_is_the_tile_region() {
        assert_eq!(Tiling::new(4_096).span(2), region(8_192, 12_288));
    }
}
