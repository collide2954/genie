use vstd::prelude::*;

verus! {

/// A region of a contig: the positions from its start to its end, without the end.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region {
    start: u64,
    end: u64,
}

impl Region {
    /// The start of a region is never after its end.
    #[verifier::type_invariant]
    spec fn ordered(self) -> bool {
        self.start <= self.end
    }

    /// The first position of a region.
    pub closed spec fn spec_start(self) -> u64 {
        self.start
    }

    /// The position after the last position of a region.
    pub closed spec fn spec_end(self) -> u64 {
        self.end
    }

    /// True when a position is in a region.
    pub open spec fn has(self, position: u64) -> bool {
        self.spec_start() <= position < self.spec_end()
    }

    /// Each region of a slice has its start at or before its end. The function only gives
    /// this fact to proofs: its loop does nothing.
    pub fn ordered_all(regions: &[Region])
        ensures
            forall|at: int|
                0 <= at < regions@.len() ==> #[trigger] regions@[at].spec_start()
                    <= regions@[at].spec_end(),
    {
        let mut at: usize = 0;
        while at < regions.len()
            invariant
                at <= regions.len(),
                forall|done: int|
                    0 <= done < at ==> #[trigger] regions@[done].spec_start()
                        <= regions@[done].spec_end(),
            decreases regions.len() - at,
        {
            #[cfg(verus_keep_ghost)]
            let region = regions[at];
            proof {
                use_type_invariant(region);
            }
            at += 1;
        }
    }

    pub const fn new(start: u64, end: u64) -> (region: Option<Self>)
        ensures
            match region {
                Some(region) => start <= end && region.spec_start() == start
                    && region.spec_end() == end,
                None => start > end,
            },
    {
        if start <= end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub const fn between(a: u64, b: u64) -> (region: Self)
        ensures
            region.spec_start() == if a <= b { a } else { b },
            region.spec_end() == if a <= b { b } else { a },
    {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    #[verifier::when_used_as_spec(spec_start)]
    pub const fn start(self) -> (start: u64)
        ensures
            start == self.spec_start(),
            start <= self.spec_end(),
    {
        proof {
            use_type_invariant(self);
        }
        self.start
    }

    #[verifier::when_used_as_spec(spec_end)]
    pub const fn end(self) -> (end: u64)
        ensures
            end == self.spec_end(),
            self.spec_start() <= end,
    {
        proof {
            use_type_invariant(self);
        }
        self.end
    }

    pub const fn len(self) -> (length: u64)
        ensures
            length == self.spec_end() - self.spec_start(),
    {
        proof {
            use_type_invariant(self);
        }
        self.end - self.start
    }

    pub const fn is_empty(self) -> (empty: bool)
        ensures
            empty == (self.spec_start() == self.spec_end()),
    {
        self.start == self.end
    }

    pub const fn contains(self, position: u64) -> (inside: bool)
        ensures
            inside == self.has(position),
    {
        self.start <= position && position < self.end
    }

    /// True when two regions share a position, or when an empty region is inside the other
    /// region.
    pub const fn overlaps(self, other: Self) -> (overlaps: bool)
        ensures
            overlaps == (self.spec_start() < other.spec_end() && other.spec_start()
                < self.spec_end()),
            self.spec_start() < self.spec_end() && other.spec_start() < other.spec_end() ==> (
            overlaps == exists|position: u64| #[trigger] self.has(position) && other.has(position)),
    {
        proof {
            let position = if self.start >= other.start {
                self.start
            } else {
                other.start
            };
            let full = self.start < self.end && other.start < other.end;
            if full && self.start < other.end && other.start < self.end {
                assert(self.has(position) && other.has(position));
            }
        }
        self.start < other.end && other.start < self.end
    }

    pub fn hull(self, other: Self) -> (hull: Self)
        ensures
            hull.spec_start() == if self.spec_start() <= other.spec_start() {
                self.spec_start()
            } else {
                other.spec_start()
            },
            hull.spec_end() == if self.spec_end() >= other.spec_end() {
                self.spec_end()
            } else {
                other.spec_end()
            },
            forall|position: u64|
                self.has(position) || other.has(position) ==> #[trigger] hull.has(position),
    {
        proof {
            use_type_invariant(self);
            use_type_invariant(other);
        }
        Self { start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn intersection(self, other: Self) -> (shared: Option<Self>)
        ensures
            match shared {
                Some(shared) => shared.spec_start() < shared.spec_end() && forall|position: u64|
                    #[trigger] shared.has(position) == (self.has(position) && other.has(position)),
                None => forall|position: u64|
                    !(#[trigger] self.has(position) && other.has(position)),
            },
            ({
                let start = if self.spec_start() >= other.spec_start() {
                    self.spec_start()
                } else {
                    other.spec_start()
                };
                let end = if self.spec_end() <= other.spec_end() {
                    self.spec_end()
                } else {
                    other.spec_end()
                };
                match shared {
                    Some(shared) => shared.spec_start() == start && shared.spec_end() == end,
                    None => end <= start,
                }
            }),
    {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn expand(self, amount: u64, limit: u64) -> (expanded: Self)
        ensures
            ({
                let start: int = if self.spec_start() >= amount {
                    self.spec_start() - amount
                } else {
                    0
                };
                let end: int = if self.spec_end() + amount <= u64::MAX {
                    self.spec_end() + amount
                } else {
                    u64::MAX as int
                };
                let low: int = if start <= limit { start } else { limit as int };
                let high: int = if end <= limit { end } else { limit as int };
                expanded.spec_start() == low && expanded.spec_end() == high
            }),
    {
        proof {
            use_type_invariant(self);
        }
        Self::between(
            self.start.saturating_sub(amount).min(limit),
            self.end.saturating_add(amount).min(limit),
        )
    }

    pub fn clamp_to(self, length: u64) -> (clamped: Self)
        ensures
            clamped.spec_start() == if self.spec_start() <= length {
                self.spec_start()
            } else {
                length
            },
            clamped.spec_end() == if self.spec_end() <= length {
                self.spec_end()
            } else {
                length
            },
            forall|position: u64|
                #[trigger] clamped.has(position) == (self.has(position) && position < length),
    {
        proof {
            use_type_invariant(self);
        }
        Self { start: self.start.min(length), end: self.end.min(length) }
    }
}

} // verus!

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
    fn hull_covers_both_regions() {
        assert_eq!(region(0, 10).hull(region(20, 30)), region(0, 30));
        assert_eq!(region(5, 8).hull(region(0, 10)), region(0, 10));
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
