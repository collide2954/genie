use crate::{Region, Strand};

pub const UNMAPPED: u16 = 0x4;
pub const REVERSE: u16 = 0x10;
pub const SECONDARY: u16 = 0x100;
pub const QC_FAIL: u16 = 0x200;
pub const DUPLICATE: u16 = 0x400;
pub const SUPPLEMENTARY: u16 = 0x800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Match,
    Insertion,
    Deletion,
    Skip,
    SoftClip,
    HardClip,
    Padding,
    Equal,
    Diff,
}

impl Op {
    pub const fn consumes_reference(self) -> bool {
        matches!(
            self,
            Self::Match | Self::Deletion | Self::Skip | Self::Equal | Self::Diff
        )
    }

    pub const fn consumes_read(self) -> bool {
        matches!(
            self,
            Self::Match | Self::Insertion | Self::SoftClip | Self::Equal | Self::Diff
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alignment {
    pub name: String,
    pub start: u64,
    pub flags: u16,
    pub mapq: u8,
    pub cigar: Vec<(Op, u32)>,
    pub bases: Vec<u8>,
    pub qualities: Vec<u8>,
}

impl Alignment {
    pub fn end(&self) -> u64 {
        let length: u64 = self
            .cigar
            .iter()
            .filter(|(op, _)| op.consumes_reference())
            .map(|(_, length)| u64::from(*length))
            .sum();
        self.start + length
    }

    pub fn span(&self) -> Region {
        Region::between(self.start, self.end())
    }

    pub fn strand(&self) -> Strand {
        if self.has(REVERSE) {
            Strand::Reverse
        } else {
            Strand::Forward
        }
    }

    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    hidden: u16,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            hidden: UNMAPPED | SECONDARY | QC_FAIL | DUPLICATE,
        }
    }
}

impl Filter {
    pub fn keeps(self, alignment: &Alignment) -> bool {
        !alignment.has(self.hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Alignment, DUPLICATE, Filter, Op, QC_FAIL, REVERSE, SECONDARY, SUPPLEMENTARY, UNMAPPED,
    };
    use crate::{Region, Strand};

    fn alignment(flags: u16, cigar: Vec<(Op, u32)>) -> Alignment {
        Alignment {
            name: "read".to_owned(),
            start: 100,
            flags,
            mapq: 60,
            cigar,
            bases: Vec::new(),
            qualities: Vec::new(),
        }
    }

    #[test]
    fn spans_the_reference_operations() {
        let cigar = vec![
            (Op::SoftClip, 6),
            (Op::Match, 10),
            (Op::Insertion, 2),
            (Op::Match, 5),
            (Op::Deletion, 3),
            (Op::Skip, 4),
            (Op::Equal, 1),
            (Op::Diff, 1),
            (Op::HardClip, 9),
        ];
        let read = alignment(0, cigar);
        assert_eq!(read.end(), 124);
        assert_eq!(read.span(), Region::new(100, 124).unwrap());
    }

    #[test]
    fn strand_comes_from_the_reverse_flag() {
        assert_eq!(alignment(0, Vec::new()).strand(), Strand::Forward);
        assert_eq!(alignment(REVERSE, Vec::new()).strand(), Strand::Reverse);
    }

    #[test]
    fn default_filter_hides_duplicates_failures_and_secondary_reads() {
        let filter = Filter::default();
        for flags in [DUPLICATE, QC_FAIL, SECONDARY, UNMAPPED] {
            assert!(!filter.keeps(&alignment(flags, Vec::new())), "{flags:#x}");
        }
        for flags in [0, REVERSE, SUPPLEMENTARY] {
            assert!(filter.keeps(&alignment(flags, Vec::new())), "{flags:#x}");
        }
    }
}
