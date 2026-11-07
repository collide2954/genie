#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    start: u64,
    end: u64,
}

impl Region {
    pub const fn new(start: u64, end: u64) -> Option<Self> {
        if start <= end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub const fn between(a: u64, b: u64) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    pub const fn start(self) -> u64 {
        self.start
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    pub const fn len(self) -> u64 {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn contains(self, position: u64) -> bool {
        self.start <= position && position < self.end
    }

    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(Self { start, end })
    }

    pub fn expand(self, amount: u64, limit: u64) -> Self {
        Self::between(
            self.start.saturating_sub(amount).min(limit),
            self.end.saturating_add(amount).min(limit),
        )
    }

    pub fn clamp_to(self, length: u64) -> Self {
        Self {
            start: self.start.min(length),
            end: self.end.min(length),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Region;

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn rejects_start_after_end() {
        assert_eq!(Region::new(5, 4), None);
    }

    #[test]
    fn between_accepts_either_order() {
        assert_eq!(Region::between(20, 10), region(10, 20));
        assert_eq!(Region::between(10, 20), region(10, 20));
    }

    #[test]
    fn length_is_end_minus_start() {
        assert_eq!(region(10, 25).len(), 15);
        assert!(!region(10, 25).is_empty());
        assert!(region(7, 7).is_empty());
    }

    #[test]
    fn contains_is_half_open() {
        let region = region(10, 20);
        assert!(!region.contains(9));
        assert!(region.contains(10));
        assert!(region.contains(19));
        assert!(!region.contains(20));
    }

    #[test]
    fn clamp_to_limits_both_ends() {
        assert_eq!(region(10, 90).clamp_to(50), region(10, 50));
        assert_eq!(region(60, 90).clamp_to(50), region(50, 50));
        assert_eq!(region(10, 20).clamp_to(50), region(10, 20));
    }

    #[test]
    fn expand_grows_both_sides_up_to_the_limit() {
        assert_eq!(region(100, 200).expand(50, 1_000), region(50, 250));
        assert_eq!(region(10, 200).expand(50, 220), region(0, 220));
    }

    #[test]
    fn intersection_is_the_shared_part() {
        assert_eq!(
            region(0, 20).intersection(region(10, 30)),
            Some(region(10, 20))
        );
        assert_eq!(region(0, 10).intersection(region(10, 20)), None);
    }

    #[test]
    fn touching_regions_do_not_overlap() {
        assert!(!region(0, 10).overlaps(region(10, 20)));
        assert!(region(0, 10).overlaps(region(9, 11)));
        assert!(region(5, 5).overlaps(region(0, 10)));
    }
}
