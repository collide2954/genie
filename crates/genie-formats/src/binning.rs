use std::collections::BTreeMap;
use std::io::{self, Read};

use genie_core::Region;
use genie_core::signal::Tally;

use crate::{Error, Reason};
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::div_mod::{lemma_div_is_ordered, lemma_fundamental_div_mod};
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::power2::{
    lemma_pow2_adds, lemma_pow2_pos, lemma_pow2_strictly_increases, lemma2_to64, pow2,
};
#[cfg(verus_keep_ghost)]
use vstd::bits::{
    lemma_u32_pow2_no_overflow, lemma_u32_shl_is_mul, lemma_u64_pow2_no_overflow,
    lemma_u64_shl_is_mul, lemma_u64_shr_is_div,
};
use vstd::prelude::*;

const UNSET: u64 = u64::MAX;
const MIN_MARKER_DISTANCE: u64 = 0x10000;

verus! {

/// The number of bins of a level: 8 to the power of the level.
pub open spec fn spec_count(level: nat) -> nat
    decreases level,
{
    if level == 0 {
        1
    } else {
        8 * spec_count((level - 1) as nat)
    }
}

/// The number of the first bin of a level: the number of bins of all the levels above it.
pub open spec fn spec_first(level: nat) -> nat
    decreases level,
{
    if level == 0 {
        0
    } else {
        spec_first((level - 1) as nat) + spec_count((level - 1) as nat)
    }
}

/// A level has 2 to the power of 3 times the level bins, and 7 times the bins above a level
/// is one less than its bins.
proof fn lemma_counts(level: nat)
    ensures
        spec_count(level) == pow2(3 * level),
        7 * spec_first(level) + 1 == spec_count(level),
    decreases level,
{
    lemma2_to64();
    if level > 0 {
        let above = (level - 1) as nat;
        lemma_counts(above);
        lemma_pow2_adds(3 * above, 3);
        assert(3 * above + 3 == 3 * level);
        assert(pow2(3 * level) == pow2(3 * above) * 8);
    }
}

/// A number below 2 to the power of `high` divided by 2 to the power of `low` is below 2 to
/// the power of the difference.
proof fn lemma_shifted_below(value: nat, low: nat, high: nat)
    requires
        low <= high,
        value < pow2(high),
    ensures
        value / pow2(low) < pow2((high - low) as nat),
{
    lemma_pow2_pos(low);
    lemma_pow2_adds(low, (high - low) as nat);
    let (width, count) = (pow2(low) as int, pow2((high - low) as nat) as int);
    lemma_fundamental_div_mod(value as int, width);
    vstd::arithmetic::div_mod::lemma_mod_bound(value as int, width);
    let (quotient, rest) = (value as int / width, value as int % width);
    assert(quotient < count) by (nonlinear_arith)
        requires
            value == width * quotient + rest,
            0 <= rest,
            value < width * count,
            width > 0,
    ;
}

/// A binning scheme of an index: on the lowest level, bins of 2 to the power of `shift` bases;
/// above it, `depth` levels with bins of 8 times as many bases, up to the one top bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scheme {
    shift: u32,
    depth: u32,
}

impl Scheme {
    /// A scheme has 9 levels or fewer under its top bin, and its top bin has 2 to the power of
    /// 62 bases or fewer.
    #[verifier::type_invariant]
    spec fn fits(self) -> bool {
        self.depth <= 9 && self.shift + 3 * self.depth <= 62
    }

    pub closed spec fn spec_shift(self) -> nat {
        self.shift as nat
    }

    pub closed spec fn spec_depth(self) -> nat {
        self.depth as nat
    }

    /// The bases of each bin of a level, as a power of 2. Level 0 is the top bin.
    pub open spec fn spec_width(self, level: nat) -> nat {
        (self.spec_shift() + 3 * (self.spec_depth() - level)) as nat
    }

    /// True when a bin of a level holds the records from `start` to `end`: their first and
    /// their last base are in the same bin of the level, and `bin` is the number of that bin.
    pub open spec fn spec_holds(self, bin: int, level: nat, start: nat, end: nat) -> bool {
        let width = pow2(self.spec_width(level));
        level <= self.spec_depth() && start < end && start / width == (end - 1) as nat / width
            && bin == spec_first(level) + start / width
    }

    /// The scheme of BAI and TBI indexes: bins of 16 kb on the lowest level, 5 levels under the
    /// top bin.
    pub const fn bai() -> (scheme: Self)
        ensures
            scheme.spec_shift() == 14 && scheme.spec_depth() == 5,
    {
        Self { shift: 14, depth: 5 }
    }

    /// The scheme of a shift and a depth, if they fit.
    pub fn checked(shift: i32, depth: i32) -> (scheme: Option<Self>)
        ensures
            match scheme {
                Some(scheme) => scheme.spec_shift() == shift && scheme.spec_depth() == depth,
                None => !(0 <= depth <= 9 && 0 <= shift && shift + 3 * depth <= 62),
            },
    {
        if shift < 0 || depth < 0 {
            return None;
        }
        let (shift, depth) = (shift as u32, depth as u32);
        if depth > 9 || shift + 3 * depth > 62 {
            return None;
        }
        Some(Self { shift, depth })
    }

    /// The scheme with a shift and the fewest levels from `depth` up whose top bin reaches
    /// `length`, with 9 levels at most.
    pub fn reaching(shift: u32, depth: u32, length: u64) -> (scheme: Self)
        ensures
            shift <= 62 ==> scheme.spec_shift() == shift,
    {
        let shift = if shift > 62 {
            62
        } else {
            shift
        };
        let mut depth = if depth > 9 {
            9
        } else {
            depth
        };
        if shift + 3 * depth > 62 {
            depth = (62 - shift) / 3;
        }
        while depth < 9 && shift + 3 * (depth + 1) <= 62 && length > 1u64 << (shift + 3 * depth)
            invariant
                shift <= 62,
                depth <= 9,
                shift + 3 * depth <= 62,
            decreases 9 - depth,
        {
            depth += 1;
        }
        Self { shift, depth }
    }

    pub fn shift(self) -> (shift: u32)
        ensures
            shift == self.spec_shift(),
    {
        self.shift
    }

    pub fn depth(self) -> (depth: u32)
        ensures
            depth == self.spec_depth(),
            depth <= 9,
            self.spec_width(0) <= 62,
    {
        proof {
            use_type_invariant(self);
        }
        self.depth
    }

    fn top(self) -> (top: u32)
        ensures
            top == self.spec_width(0),
            top <= 62,
    {
        proof {
            use_type_invariant(self);
        }
        self.shift + 3 * self.depth
    }

    /// The number of the first bin of a level.
    fn first(level: u32) -> (first: u32)
        requires
            level <= 10,
        ensures
            first == spec_first(level as nat),
            7 * first + 1 == pow2(3 * level as nat),
    {
        proof {
            lemma_counts(level as nat);
            lemma_u32_pow2_no_overflow((3 * level) as nat);
            lemma_u32_shl_is_mul(1, (3 * level) as u32);
        }
        ((1u32 << (3 * level)) - 1) / 7
    }

    /// The number of bins of all levels.
    fn bins(self) -> (bins: u32)
        ensures
            bins == spec_first(self.spec_depth() + 1),
            bins < pow2(30),
    {
        proof {
            use_type_invariant(self);
            lemma_counts(self.depth as nat + 1);
            lemma2_to64();
            vstd::arithmetic::power2::lemma_pow2_strictly_increases(
                3 * (self.depth as nat + 1),
                31,
            );
        }
        Self::first(self.depth + 1)
    }

    /// The number of the pseudo-bin of htslib, after the last bin.
    fn meta(self) -> (meta: u32)
        ensures
            meta == spec_first(self.spec_depth() + 1) + 1,
    {
        proof {
            lemma2_to64();
        }
        self.bins() + 1
    }

    /// The bin of the records from `start` to `end`: the bin of the deepest level that holds
    /// all their bases. Records without a place, from -1 to 0, get the bin before the first bin
    /// of the lowest level, and records that end at 0 get the top bin, as in htslib.
    fn bin(self, start: i64, end: i64) -> (bin: i64)
        requires
            (start == -1 && end == 0) || (0 <= start <= pow2(self.spec_width(0)) && 0 <= end
                <= pow2(self.spec_width(0))),
        ensures
            start == -1 ==> bin == spec_first(self.spec_depth()) - 1,
            0 <= start < end ==> exists|level: nat|
                {
                    &&& #[trigger] self.spec_holds(bin as int, level, start as nat, end as nat)
                    &&& forall|deeper: nat|
                        level < deeper <= self.spec_depth() ==> !#[trigger] self.spec_holds(
                            bin as int,
                            deeper,
                            start as nat,
                            end as nat,
                        )
                },
    {
        proof {
            use_type_invariant(self);
        }
        if start < 0 {
            return i64::from(Self::first(self.depth)) - 1;
        }
        if end <= 0 {
            return 0;
        }
        let (low, high) = (start as u64, (end - 1) as u64);
        let mut level = self.depth;
        while level > 0
            invariant
                level <= self.depth,
                self.depth <= 9,
                self.shift + 3 * self.depth <= 62,
                0 <= start,
                0 < end <= pow2(self.spec_width(0)),
                low == start,
                high == end - 1,
                forall|deeper: nat|
                    level < deeper <= self.spec_depth() ==> low as nat / pow2(
                        #[trigger] self.spec_width(deeper),
                    ) != high as nat / pow2(self.spec_width(deeper)),
            decreases level,
        {
            let shift = u64::from(self.shift + 3 * (self.depth - level));
            proof {
                lemma_u64_shr_is_div(low, shift);
                lemma_u64_shr_is_div(high, shift);
            }
            if low >> shift == high >> shift {
                proof {
                    lemma2_to64();
                    lemma_shifted_below(high as nat, shift as nat, self.spec_width(0));
                }
                let bin = i64::from(Self::first(level)) + i64::from((low >> shift) as u32);
                proof {
                    if start < end {
                        assert(self.spec_width(level as nat) == shift);
                        assert(self.spec_holds(
                            bin as int,
                            level as nat,
                            start as nat,
                            end as nat,
                        ));
                        assert forall|deeper: nat| level < deeper <= self.spec_depth() implies !#[trigger]
                            self.spec_holds(bin as int, deeper, start as nat, end as nat) by {
                            assert(low as nat / pow2(self.spec_width(deeper)) != high as nat
                                / pow2(self.spec_width(deeper)));
                        }
                    }
                }
                return bin;
            }
            level -= 1;
        }
        proof {
            if start < end {
                lemma_shifted_below(low as nat, self.spec_width(0), self.spec_width(0));
                lemma_shifted_below(high as nat, self.spec_width(0), self.spec_width(0));
                lemma2_to64();
                assert(self.spec_holds(0, 0, start as nat, end as nat));
                assert forall|deeper: nat| 0 < deeper <= self.spec_depth() implies !#[trigger]
                    self.spec_holds(0, deeper, start as nat, end as nat) by {
                    assert(low as nat / pow2(self.spec_width(deeper)) != high as nat / pow2(
                        self.spec_width(deeper),
                    ));
                }
            }
        }
        0
    }

    /// The first and the last bin of a level that can hold a record with a base from `start`
    /// to `last`.
    fn level_bins(self, level: u32, start: u64, last: u64) -> (bins: (u32, u32))
        requires
            level <= self.spec_depth(),
            start <= last < pow2(self.spec_width(0)),
        ensures
            bins.0 == spec_first(level as nat) + start as nat / pow2(
                self.spec_width(level as nat),
            ),
            bins.1 == spec_first(level as nat) + last as nat / pow2(
                self.spec_width(level as nat),
            ),
    {
        proof {
            use_type_invariant(self);
        }
        let shift = u64::from(self.top() - 3 * level);
        let first = Self::first(level);
        proof {
            lemma_u64_shr_is_div(start, shift);
            lemma_u64_shr_is_div(last, shift);
            lemma_shifted_below(start as nat, shift as nat, self.spec_width(0));
            lemma_shifted_below(last as nat, shift as nat, self.spec_width(0));
            lemma2_to64();
        }
        (first + (start >> shift) as u32, first + (last >> shift) as u32)
    }

    /// The level of a bin number: the level whose bins have the number. Nothing for a number
    /// past the bins of the scheme.
    fn level(self, bin: u32) -> (level: Option<u32>)
        ensures
            match level {
                Some(level) => level <= self.spec_depth() && spec_first(level as nat) <= bin
                    < spec_first(level as nat + 1),
                None => bin >= spec_first(self.spec_depth() + 1),
            },
    {
        proof {
            use_type_invariant(self);
        }
        let mut level: u32 = 0;
        while level < self.depth && bin >= Self::first(level + 1)
            invariant
                level <= self.depth,
                self.depth <= 9,
                spec_first(level as nat) <= bin,
            decreases self.depth - level,
        {
            level += 1;
        }
        if bin >= Self::first(level + 1) {
            return None;
        }
        Some(level)
    }

    /// The bases that a bin covers, up to the length of the contig when the contig ends inside
    /// the bin. Nothing for a number past the bins of the scheme.
    fn span(self, bin: u32, length: u64) -> (span: Option<Region>)
        ensures
            match span {
                Some(span) => exists|level: nat|
                    #![trigger spec_first(level)]
                    level <= self.spec_depth() && spec_first(level) <= bin < spec_first(level + 1)
                        && span.spec_start() == (bin - spec_first(level)) * pow2(
                        self.spec_width(level),
                    ) && span.spec_end() == if length > span.spec_start() && length
                        < span.spec_start() + pow2(self.spec_width(level)) {
                        length as int
                    } else {
                        span.spec_start() + pow2(self.spec_width(level))
                    },
                None => bin >= spec_first(self.spec_depth() + 1),
            },
    {
        proof {
            use_type_invariant(self);
        }
        let level = self.level(bin)?;
        let index = bin - Self::first(level);
        let width = u64::from(self.shift + 3 * (self.depth - level));
        proof {
            let (count, size) = (pow2(3 * level as nat), pow2(width as nat));
            lemma_counts(level as nat);
            lemma_pow2_adds(3 * level as nat, width as nat);
            lemma_pow2_pos(width as nat);
            lemma2_to64();
            vstd::arithmetic::power2::lemma2_to64_rest();
            if self.spec_width(0) < 62 {
                lemma_pow2_strictly_increases(self.spec_width(0), 62);
            }
            assert(3 * level as nat + width as nat == self.spec_width(0));
            assert(index < count);
            assert(index * size + size <= count * size) by (nonlinear_arith)
                requires
                    index < count,
                    size > 0,
            ;
            assert(count * size == pow2(self.spec_width(0)));
            assert(pow2(self.spec_width(0)) <= 0x4000_0000_0000_0000);
            assert((index as u64) * pow2(width as nat) <= 0x4000_0000_0000_0000);
            lemma_u64_pow2_no_overflow(width as nat);
            lemma_u64_shl_is_mul(index as u64, width);
            lemma_u64_shl_is_mul(1, width);
            assert(self.spec_width(level as nat) == width);
        }
        let start = u64::from(index) << width;
        let end = start + (1u64 << width);
        Some(Region::between(start, if length > start { end.min(length) } else { end }))
    }

    /// The first window of the linear index under a bin: the place of its first bin on the
    /// lowest level, from the first bin of that level. Nothing for a number past the bins of
    /// the scheme.
    fn bottom(self, bin: u32) -> (bottom: Option<usize>)
        ensures
            match bottom {
                Some(bottom) => exists|level: nat|
                    #![trigger spec_first(level)]
                    level <= self.spec_depth() && spec_first(level) <= bin < spec_first(level + 1)
                        && bottom == (bin - spec_first(level)) * pow2(
                        (3 * (self.spec_depth() - level)) as nat,
                    ),
                None => bin >= spec_first(self.spec_depth() + 1),
            },
    {
        proof {
            use_type_invariant(self);
        }
        let level = self.level(bin)?;
        let index = bin - Self::first(level);
        let up = 3 * (self.depth - level);
        proof {
            let (count, size) = (pow2(3 * level as nat), pow2(up as nat));
            lemma_counts(level as nat);
            lemma_pow2_adds(3 * level as nat, up as nat);
            lemma_pow2_pos(up as nat);
            lemma2_to64();
            if 3 * self.depth < 27 {
                lemma_pow2_strictly_increases(3 * self.depth as nat, 27);
            }
            assert(3 * level as nat + up as nat == 3 * self.depth as nat);
            assert(index < count);
            assert(index * size + size <= count * size) by (nonlinear_arith)
                requires
                    index < count,
                    size > 0,
            ;
            lemma_u32_shl_is_mul(index, up);
        }
        Some((index << up) as usize)
    }

    /// A record with a base from `start` to `last` has its bin among the bins of its level for
    /// them: thus a query of a region finds the bin of each record that overlaps the region.
    pub proof fn lemma_queries_find_records(
        self,
        bin: int,
        level: nat,
        record: (nat, nat),
        start: nat,
        last: nat,
    )
        requires
            self.spec_holds(bin, level, record.0, record.1),
            record.0 <= last,
            start < record.1,
        ensures
            spec_first(level) + start / pow2(self.spec_width(level)) <= bin <= spec_first(level)
                + last / pow2(self.spec_width(level)),
    {
        let width = pow2(self.spec_width(level));
        lemma_pow2_pos(self.spec_width(level));
        lemma_div_is_ordered(record.0 as int, last as int, width as int);
        lemma_div_is_ordered(start as int, (record.1 - 1) as int, width as int);
    }

    /// On a level, the bin of `start` is not after the bin of `last`, and the bins of the
    /// level have numbers less than 2 to the power of 31.
    proof fn lemma_level_bins(self, level: nat, start: nat, last: nat)
        requires
            self.spec_depth() <= 9,
            level <= self.spec_depth(),
            start <= last < pow2(self.spec_width(0)),
        ensures
            start / pow2(self.spec_width(level)) <= last / pow2(self.spec_width(level)),
            spec_first(level) + last / pow2(self.spec_width(level)) < 0x8000_0000,
    {
        let width = pow2(self.spec_width(level));
        lemma_counts(level);
        lemma_counts(level + 1);
        lemma_shifted_below(last, self.spec_width(level), self.spec_width(0));
        lemma2_to64();
        lemma_pow2_strictly_increases(3 * (level + 1), 31);
        lemma_pow2_pos(self.spec_width(level));
        lemma_div_is_ordered(start as int, last as int, width as int);
        assert(self.spec_width(0) - self.spec_width(level) == 3 * level);
        assert(spec_first(level + 1) == spec_first(level) + spec_count(level));
    }
}

/// The virtual offsets of the start and the end of a part of a BGZF file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub start: u64,
    pub end: u64,
}

/// A bin of an index: its number, the virtual offset of a CSI bin, and its chunks.
#[derive(Debug, PartialEq, Eq)]
struct Bin {
    number: u32,
    offset: u64,
    chunks: Vec<Chunk>,
}

/// The bins of the index of a contig, in the order of their numbers, and the linear index of
/// a BAI or TBI index.
#[derive(Debug, PartialEq, Eq)]
pub struct Bins {
    scheme: Scheme,
    entries: Vec<Bin>,
    intervals: Option<Vec<u64>>,
}

/// A list keeps its chunks when a chunk comes after them, and it has the new chunk.
proof fn lemma_push_keeps(chunks: Seq<Chunk>, chunk: Chunk)
    ensures
        forall|other: Chunk|
            chunks.contains(other) ==> #[trigger] chunks.push(chunk).contains(other),
        chunks.push(chunk).contains(chunk),
{
    assert(chunks.push(chunk)[chunks.len() as int] == chunk);
    assert forall|other: Chunk| chunks.contains(other) implies #[trigger] chunks.push(
        chunk,
    ).contains(other) by {
        let at = choose|at: int| 0 <= at < chunks.len() && chunks[at] == other;
        assert(chunks.push(chunk)[at] == other);
    }
}

/// The chunks of a list, then the chunks of a bin that end after `minimum`.
fn push_after(chunks: &mut Vec<Chunk>, bin: &Bin, minimum: u64)
    ensures
        forall|other: Chunk|
            old(chunks)@.contains(other) ==> #[trigger] final(chunks)@.contains(other),
        forall|part: int|
            0 <= part < bin.chunks.len() && #[trigger] bin.chunks[part].end > minimum
                ==> final(chunks)@.contains(bin.chunks[part]),
{
    let mut part: usize = 0;
    while part < bin.chunks.len()
        invariant
            part <= bin.chunks.len(),
            forall|other: Chunk|
                old(chunks)@.contains(other) ==> #[trigger] chunks@.contains(other),
            forall|piece: int|
                0 <= piece < part && #[trigger] bin.chunks[piece].end > minimum
                    ==> chunks@.contains(bin.chunks[piece]),
        decreases bin.chunks.len() - part,
    {
        let chunk = bin.chunks[part];
        if chunk.end > minimum {
            proof {
                lemma_push_keeps(chunks@, chunk);
            }
            chunks.push(chunk);
        }
        part += 1;
    }
}

/// True when a chunk is inside one of the chunks of a list.
spec fn spec_inside(chunks: Seq<Chunk>, chunk: Chunk) -> bool {
    exists|into: int|
        #![trigger chunks[into]]
        0 <= into < chunks.len() && chunks[into].start <= chunk.start && chunk.end
            <= chunks[into].end
}

/// Chunks in the order of their start, as fewer chunks: a chunk that starts at or before the
/// end of the chunk before joins it. Each chunk is inside a joined chunk, and the joined chunks
/// do not overlap. Nothing when the chunks are not in order.
fn merge(chunks: &[Chunk]) -> (merged: Option<Vec<Chunk>>)
    ensures
        match merged {
            Some(merged) => forall|at: int|
                0 <= at < chunks.len() ==> spec_inside(merged@, #[trigger] chunks[at]),
            None => true,
        },
        match merged {
            Some(merged) => forall|at: int|
                0 <= at < merged.len() - 1 ==> #[trigger] merged[at].end < merged[at + 1].start,
            None => true,
        },
{
    let mut merged: Vec<Chunk> = Vec::with_capacity(chunks.len());
    let ghost mut into: Seq<int> = Seq::empty();
    let mut at: usize = 0;
    while at < chunks.len()
        invariant
            at <= chunks.len(),
            into.len() == at,
            at > 0 <==> merged.len() > 0,
            at > 0 ==> merged[merged.len() - 1].start <= chunks[at - 1].start,
            forall|done: int|
                0 <= done < at ==> 0 <= #[trigger] into[done] < merged.len()
                    && merged[into[done]].start <= chunks[done].start && chunks[done].end
                    <= merged[into[done]].end,
            forall|k: int|
                0 <= k < merged.len() - 1 ==> #[trigger] merged[k].end < merged[k + 1].start,
        decreases chunks.len() - at,
    {
        let chunk = chunks[at];
        if at > 0 && chunk.start < chunks[at - 1].start {
            return None;
        }
        let count = merged.len();
        if count > 0 && chunk.start <= merged[count - 1].end {
            let last = merged[count - 1];
            merged.set(count - 1, Chunk { start: last.start, end: last.end.max(chunk.end) });
            proof {
                into = into.push((count - 1) as int);
            };
        } else {
            merged.push(chunk);
            proof {
                into = into.push(count as int);
            };
        }
        at += 1;
    }
    proof {
        assert forall|done: int| 0 <= done < chunks.len() implies spec_inside(
            merged@,
            #[trigger] chunks[done],
        ) by {
            assert(merged@[into[done]].start <= chunks[done].start);
        }
    }
    Some(merged)
}

impl Bins {
    /// The numbers of the entries go up: no two entries have the same number.
    #[verifier::type_invariant]
    spec fn ordered(self) -> bool {
        forall|before: int, after: int|
            #![trigger self.entries[before], self.entries[after]]
            0 <= before < after < self.entries.len() ==> self.entries[before].number
                < self.entries[after].number
    }

    /// The bins of an index from entries in the order of their numbers. Nothing when an entry
    /// does not come after the entry before it, as two entries with one number give.
    fn ordered_new(scheme: Scheme, entries: Vec<Bin>, intervals: Option<Vec<u64>>) -> (bins:
        Option<Self>) {
        let mut index: usize = 1;
        while index < entries.len()
            invariant
                1 <= index,
                forall|before: int, after: int|
                    #![trigger entries[before], entries[after]]
                    0 <= before < after < index && after < entries.len() ==> entries[before].number
                        < entries[after].number,
            decreases entries.len() - index,
        {
            if entries[index - 1].number >= entries[index].number {
                return None;
            }
            proof {
                assert forall|before: int, after: int|
                    #![trigger entries[before], entries[after]]
                    0 <= before < after < index + 1 && after < entries.len()
                        implies entries[before].number < entries[after].number by {
                    if after == index && before < index - 1 {
                        assert(entries[before].number < entries[index - 1].number);
                    }
                }
            }
            index += 1;
        }
        Some(Bins { scheme, entries, intervals })
    }

    /// The bins of a contig without records: no entries, and an empty linear index for a BAI
    /// or TBI index.
    pub fn empty(scheme: Scheme, csi: bool) -> Self {
        let intervals = if csi {
            None
        } else {
            Some(Vec::new())
        };
        Self { scheme, entries: Vec::new(), intervals }
    }

    /// The entry with a bin number.
    fn find(&self, number: u32) -> (found: Option<usize>)
        ensures
            match found {
                Some(index) => index < self.entries.len() && self.entries[index as int].number
                    == number,
                None => forall|index: int|
                    0 <= index < self.entries.len() ==> #[trigger] self.entries[index].number
                        != number,
            },
    {
        proof {
            use_type_invariant(self);
        }
        let mut low: usize = 0;
        let mut high: usize = self.entries.len();
        while low < high
            invariant
                self.ordered(),
                low <= high <= self.entries.len(),
                forall|index: int|
                    0 <= index < low ==> #[trigger] self.entries[index].number < number,
                forall|index: int|
                    high <= index < self.entries.len() ==> #[trigger] self.entries[index].number
                        > number,
            decreases high - low,
        {
            let middle = low + (high - low) / 2;
            let here = self.entries[middle].number;
            if here == number {
                return Some(middle);
            }
            if here < number {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        None
    }

    /// The offset of a CSI index for the records from `start` on: the offset of the bin of
    /// `start` on the lowest level, or else of the bin that a walk finds first, to the left on
    /// a level and then up to the parent. 0 when the walk finds no bin.
    fn first_offset(&self, start: u64) -> (offset: u64)
        requires
            start < pow2(self.scheme.spec_width(0)),
        ensures
            offset == 0 || exists|index: int|
                0 <= index < self.entries.len() && #[trigger] self.entries[index].offset == offset,
    {
        let depth = self.scheme.depth();
        let shift = u64::from(self.scheme.shift());
        proof {
            lemma_u64_shr_is_div(start, shift);
            lemma_shifted_below(start as nat, shift as nat, self.scheme.spec_width(0));
            lemma_counts(depth as nat);
            lemma_counts(depth as nat + 1);
            lemma2_to64();
            lemma_pow2_strictly_increases(3 * (depth as nat + 1), 31);
        }
        let mut number = Scheme::first(depth) + (start >> shift) as u32;
        loop
            decreases number,
        {
            if let Some(index) = self.find(number) {
                return self.entries[index].offset;
            }
            if number == 0 {
                return 0;
            }
            let before = number - 1;
            let parent = before >> 3;
            assert(parent <= before / 8) by (bit_vector)
                requires
                    parent == before >> 3,
            ;
            let first = parent * 8 + 1;
            number = if number > first {
                number - 1
            } else {
                parent
            };
        }
    }

    /// The linear index value of a BAI or TBI index for the records from `start` on: the value
    /// of the window of `start`, or of the last window when the index has fewer, or 0.
    spec fn spec_linear(self, start: nat) -> u64 {
        match self.intervals {
            Some(intervals) => {
                let window = start / pow2(self.scheme.spec_shift());
                if window < intervals.len() {
                    intervals[window as int]
                } else if intervals.len() > 0 {
                    intervals[intervals.len() - 1]
                } else {
                    0
                }
            },
            None => 0,
        }
    }

    /// The smallest virtual offset of a record from `start` on, by the index: the linear index
    /// of a BAI or TBI index, or the offset of a CSI index.
    fn minimum(&self, start: u64) -> (minimum: u64)
        requires
            start < pow2(self.scheme.spec_width(0)),
        ensures
            self.intervals is Some ==> minimum == self.spec_linear(start as nat),
    {
        match self.intervals.as_ref() {
            Some(intervals) => {
                let shift = u64::from(self.scheme.shift());
                proof {
                    use_type_invariant(self.scheme);
                    lemma_u64_shr_is_div(start, shift);
                    lemma_shifted_below(start as nat, shift as nat, self.scheme.spec_width(0));
                    lemma2_to64();
                    if 3 * self.scheme.spec_depth() < 31 {
                        lemma_pow2_strictly_increases(3 * self.scheme.spec_depth(), 31);
                    }
                }
                let (window, count) = ((start >> shift) as usize, intervals.len());
                if window < count {
                    intervals[window]
                } else if count > 0 {
                    intervals[count - 1]
                } else {
                    0
                }
            },
            None => self.first_offset(start),
        }
    }

    /// A chunk that a query of the bases from `start` to `last` reads: a chunk of a bin of a
    /// level, from the bin of `start` to the bin of `last` on the level, that ends after
    /// `minimum`.
    spec fn wants(
        self,
        start: nat,
        last: nat,
        minimum: u64,
        level: nat,
        index: int,
        part: int,
    ) -> bool {
        &&& level <= self.scheme.spec_depth()
        &&& 0 <= index < self.entries.len()
        &&& 0 <= part < self.entries[index].chunks.len()
        &&& spec_first(level) + start / pow2(self.scheme.spec_width(level))
            <= self.entries[index].number
        &&& self.entries[index].number <= spec_first(level) + last / pow2(
            self.scheme.spec_width(level),
        )
        &&& self.entries[index].chunks[part].end > minimum
    }

    /// The chunks of a list, then the chunks that a query of the bases from `start` to `last`
    /// reads on a level.
    fn level_chunks(&self, chunks: &mut Vec<Chunk>, level: u32, start: u64, last: u64, minimum: u64)
        requires
            level <= self.scheme.spec_depth(),
            start <= last < pow2(self.scheme.spec_width(0)),
        ensures
            forall|other: Chunk|
                old(chunks)@.contains(other) ==> #[trigger] final(chunks)@.contains(other),
            forall|index: int, part: int|
                #[trigger] self.wants(start as nat, last as nat, minimum, level as nat, index, part)
                    ==> final(chunks)@.contains(self.entries[index].chunks[part]),
    {
        proof {
            use_type_invariant(self);
            use_type_invariant(self.scheme);
            self.scheme.lemma_level_bins(level as nat, start as nat, last as nat);
        }
        let (low, high) = self.scheme.level_bins(level, start, last);
        proof {
            let width = pow2(self.scheme.spec_width(level as nat));
            let (a, b) = (start as nat / width, last as nat / width);
            let first = spec_first(level as nat);
            assert(first + a <= first + b) by (nonlinear_arith)
                requires
                    a <= b,
            ;
        }
        let mut number = low;
        while number <= high
            invariant
                self.ordered(),
                low <= number <= high + 1,
                high < 0x8000_0000,
                high == spec_first(level as nat) + last as nat / pow2(
                    self.scheme.spec_width(level as nat),
                ),
                forall|other: Chunk|
                    old(chunks)@.contains(other) ==> #[trigger] chunks@.contains(other),
                forall|index: int, part: int|
                    #[trigger] self.wants(
                        start as nat,
                        last as nat,
                        minimum,
                        level as nat,
                        index,
                        part,
                    ) && self.entries[index].number < number ==> chunks@.contains(
                        self.entries[index].chunks[part],
                    ),
            decreases high + 1 - number,
        {
            let found = self.find(number);
            if let Some(index) = found {
                push_after(chunks, &self.entries[index], minimum);
            }
            proof {
                assert forall|other: int, part: int|
                    #[trigger] self.wants(
                        start as nat,
                        last as nat,
                        minimum,
                        level as nat,
                        other,
                        part,
                    ) && self.entries[other].number < number + 1 implies chunks@.contains(
                    self.entries[other].chunks[part],
                ) by {
                    if let Some(index) = found {
                        if self.entries[other].number == number && other != index as int {
                            if other < index as int {
                                assert(self.entries[other].number
                                    < self.entries[index as int].number);
                            } else {
                                assert(self.entries[index as int].number
                                    < self.entries[other].number);
                            }
                        }
                    } else {
                        assert(self.entries[other].number != number);
                    }
                }
            }
            number += 1;
        }
    }

    /// The chunks that a query of the bases from `start` to `last` reads, on each level.
    fn wanted(&self, start: u64, last: u64, minimum: u64) -> (chunks: Vec<Chunk>)
        requires
            start <= last < pow2(self.scheme.spec_width(0)),
        ensures
            forall|level: nat, index: int, part: int|
                #[trigger] self.wants(start as nat, last as nat, minimum, level, index, part)
                    ==> chunks@.contains(self.entries[index].chunks[part]),
    {
        let mut chunks: Vec<Chunk> = Vec::new();
        let depth = self.scheme.depth();
        let mut level: u32 = 0;
        while level <= depth
            invariant
                depth == self.scheme.spec_depth(),
                depth <= 9,
                start <= last < pow2(self.scheme.spec_width(0)),
                level <= depth + 1,
                forall|done: nat, index: int, part: int|
                    done < level && #[trigger] self.wants(
                        start as nat,
                        last as nat,
                        minimum,
                        done,
                        index,
                        part,
                    ) ==> chunks@.contains(self.entries[index].chunks[part]),
            decreases depth + 1 - level,
        {
            self.level_chunks(&mut chunks, level, start, last, minimum);
            level += 1;
        }
        chunks
    }

    /// The last base of a region that the bins of the scheme reach.
    spec fn spec_last(self, region: Region) -> nat {
        (if region.spec_end() <= pow2(self.scheme.spec_width(0)) {
            region.spec_end() as nat
        } else {
            pow2(self.scheme.spec_width(0))
        } - 1) as nat
    }

    /// The chunks that can hold the records of a region, and the smallest offset of the index
    /// for the region. No chunks for a region with no bases, or that starts after the top bin.
    fn selected(&self, region: Region) -> (found: (Vec<Chunk>, u64))
        ensures
            region.spec_start() < region.spec_end() && region.spec_start() < pow2(
                self.scheme.spec_width(0),
            ) ==> (self.intervals is Some ==> found.1 == self.spec_linear(
                region.spec_start() as nat,
            )) && forall|level: nat, index: int, part: int|
                #[trigger] self.wants(
                    region.spec_start() as nat,
                    self.spec_last(region),
                    found.1,
                    level,
                    index,
                    part,
                ) ==> found.0@.contains(self.entries[index].chunks[part]),
    {
        let top = self.scheme.top();
        proof {
            lemma_u64_pow2_no_overflow(top as nat);
            lemma_u64_shl_is_mul(1, top as u64);
        }
        let limit = 1_u64 << u64::from(top);
        if region.is_empty() || region.start() >= limit {
            return (Vec::new(), 0);
        }
        let start = region.start();
        let last = region.end().min(limit) - 1;
        let minimum = self.minimum(start);
        (self.wanted(start, last, minimum), minimum)
    }
}

} // verus!
impl Default for Scheme {
    fn default() -> Self {
        Self::BAI
    }
}

impl Scheme {
    pub const BAI: Self = Self::bai();

    pub fn new(shift: i32, depth: i32) -> Result<Self, Error> {
        Self::checked(shift, depth).ok_or(Error::Invalid(Reason::BadIndex))
    }
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
                let bottom = scheme.bottom(number);
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
        // The numbers of a map are each different, thus `new` always gives the bins.
        Bins::new(scheme, entries, (!csi).then_some(self.linear))
            .unwrap_or_else(|| Bins::empty(scheme, csi))
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
        Self::new(Scheme::BAI, bins, Some(intervals)).ok_or(Error::Invalid(Reason::BadIndex))
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
        Self::new(scheme, bins, None).ok_or(Error::Invalid(Reason::BadIndex))
    }

    /// The bins of an index from entries in any order. Nothing when two entries have one
    /// number, as a damaged index can give.
    fn new(scheme: Scheme, mut entries: Vec<Bin>, intervals: Option<Vec<u64>>) -> Option<Self> {
        entries.sort_unstable_by_key(|bin| bin.number);
        Self::ordered_new(scheme, entries, intervals)
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

    /// The chunks that can hold the records of a region, in the order of their start, with the
    /// chunks that overlap or touch joined.
    pub fn chunks(&self, region: Region) -> Vec<Chunk> {
        let (mut chunks, _) = self.selected(region);
        chunks.sort_unstable_by_key(|chunk| chunk.start);
        merge(&chunks).unwrap_or(chunks)
    }

    /// What the pseudo-bin of htslib says about the records of the reference: the virtual
    /// offsets of its first record and after its last record, and the number of records.
    fn meta(&self) -> Option<(Chunk, u64)> {
        let index = self.find(self.scheme.meta())?;
        match self.entries[index].chunks.as_slice() {
            [span, counts, ..] => Some((*span, counts.start)),
            _ => None,
        }
    }

    pub fn has_counts(&self) -> bool {
        self.meta().is_some()
    }

    /// An estimate of the records that start in each part of a region, from the index alone:
    /// the linear index of a TBI or BAI index, or the size of each bin of a CSI index. It needs
    /// the pseudo-bin that htslib adds to its indexes. `length` is the length of the contig.
    pub fn density(&self, region: Region, step: u64, length: u64) -> Option<Tally> {
        let (span, records) = self.meta()?;
        let mut tally = Tally::new(region, step);
        if records == 0 {
            return Some(tally);
        }
        let block = self.block_size(span);
        let bytes = |offset: u64| (offset >> 16) as f64 + (offset & 0xFFFF) as f64 * block;
        match &self.intervals {
            Some(intervals) if !intervals.is_empty() => {
                let mut marks: Vec<f64> = intervals.iter().map(|offset| bytes(*offset)).collect();
                marks.push(bytes(span.end));
                for index in 1..marks.len() {
                    marks[index] = marks[index].max(marks[index - 1]);
                }
                let total = marks[marks.len() - 1] - marks[0];
                if total <= 0.0 {
                    return Some(tally);
                }
                let window = 1_u64 << self.scheme.shift;
                for (index, pair) in marks.windows(2).enumerate() {
                    let start = index as u64 * window;
                    let part = Region::between(start, start + window);
                    tally.spread(part, records as f64 * (pair[1] - pair[0]) / total);
                }
            }
            _ => {
                let sizes: Vec<(Region, f64)> = self
                    .entries
                    .iter()
                    .filter_map(|bin| {
                        let span = self.scheme.span(bin.number, length)?;
                        let size: f64 = bin
                            .chunks
                            .iter()
                            .map(|chunk| (bytes(chunk.end) - bytes(chunk.start)).max(0.0))
                            .sum();
                        Some((span, size))
                    })
                    .collect();
                let total: f64 = sizes.iter().map(|(_, size)| size).sum();
                if total <= 0.0 {
                    return Some(tally);
                }
                for (part, size) in sizes {
                    tally.spread(part, records as f64 * size / total);
                }
            }
        }
        Some(tally)
    }

    /// The share of a BGZF block, in compressed bytes, of one uncompressed byte: the usual
    /// distance between the blocks that the index points to, over 64 KiB.
    fn block_size(&self, span: Chunk) -> f64 {
        let mut blocks: Vec<u64> = self
            .entries
            .iter()
            .filter(|bin| bin.number < self.scheme.bins())
            .flat_map(|bin| bin.chunks.iter())
            .flat_map(|chunk| [chunk.start, chunk.end])
            .chain(self.intervals.iter().flatten().copied())
            .chain([span.start, span.end])
            .map(|offset| offset >> 16)
            .collect();
        blocks.sort_unstable();
        blocks.dedup();
        let mut gaps: Vec<u64> = blocks.windows(2).map(|pair| pair[1] - pair[0]).collect();
        gaps.sort_unstable();
        gaps.get(gaps.len() / 2)
            .map_or(1.0, |gap| *gap as f64 / 65_536.0)
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

    use super::{Bin, Bins, Builder, Chunk, Disorder, Scheme, put};
    use crate::{Error, Reason};

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
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
        assert_eq!(references, [expected]);
    }

    #[test]
    fn contigs_without_records_have_no_bins() {
        let mut builder = Builder::new(Scheme::BAI, false, 0);
        builder.push(Some((1, 10, 20)), 50, true).unwrap();
        let (references, _) = builder.finish(50);
        assert_eq!(references.len(), 2);
        assert_eq!(references[0], Bins::empty(Scheme::BAI, false));
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
        Bins::new(scheme, bins, None).unwrap()
    }

    fn starts(bins: &Bins, start: u64, end: u64) -> Vec<u64> {
        bins.chunks(Region::new(start, end).unwrap())
            .iter()
            .map(|chunk| chunk.start)
            .collect()
    }

    /// The bins of a scheme have their places, and a number past them, as a damaged index can
    /// give, has none: not an underflow.
    #[test]
    fn bins_past_the_scheme_have_no_place() {
        let scheme = Scheme::BAI;
        assert_eq!(scheme.span(4681, 1_000), Some(Region::between(0, 1_000)));
        assert_eq!(
            scheme.span(4682, 1_000_000),
            Some(Region::between(16_384, 32_768))
        );
        assert_eq!(scheme.span(0, 1 << 30), Some(Region::between(0, 1 << 29)));
        assert_eq!(scheme.bottom(2), Some(4_096));
        assert_eq!(scheme.bottom(4682), Some(1));
        assert_eq!(scheme.span(37_450, 1_000), None);
        assert_eq!(scheme.bottom(37_450), None);
    }

    /// Two bins with one number, as a damaged index can give, are an error, as in htslib: a
    /// search finds only one of the two, and genie can then miss records.
    #[test]
    fn bins_with_one_number_are_errors() {
        let index = |csi: bool| {
            let mut bytes = Vec::new();
            put(&mut bytes, 2);
            for start in [100_u64, 300] {
                bytes.extend(4681_u32.to_le_bytes());
                if csi {
                    bytes.extend(start.to_le_bytes());
                }
                put(&mut bytes, 1);
                bytes.extend(start.to_le_bytes());
                bytes.extend((start + 100).to_le_bytes());
            }
            if !csi {
                put(&mut bytes, 0);
            }
            bytes
        };
        let bad = |read: Result<Bins, Error>| matches!(read, Err(Error::Invalid(Reason::BadIndex)));
        assert!(bad(Bins::read(&mut index(false).as_slice())));
        assert!(bad(Bins::read_csi(
            &mut index(true).as_slice(),
            Scheme::BAI
        )));
    }

    #[test]
    fn schemes_check_their_shape() {
        assert!(Scheme::new(14, 6).is_ok());
        assert!(Scheme::new(-1, 5).is_err());
        assert!(Scheme::new(14, 20).is_err());
    }

    /// A contig longer than 9 levels of bins can reach, as a damaged header can give, makes a
    /// scheme of 9 levels: its records are then too far for the index, not a panic.
    #[test]
    fn schemes_stop_at_nine_levels() {
        let scheme = Scheme::reaching(14, 0, 1 << 50);
        assert_eq!((scheme.shift(), scheme.depth()), (14, 9));
        assert_eq!(scheme.bins(), 153_391_689);
        assert_eq!(Scheme::reaching(14, 0, 1 << 30).depth(), 6);
        let mut builder = Builder::new(scheme, true, 0);
        let far = 1 << 42;
        assert_eq!(
            builder.push(Some((0, far, far + 5)), 10, true),
            Err(Disorder::Long)
        );
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
