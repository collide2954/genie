use std::slice;

use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strand {
    Forward,
    Reverse,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    name: String,
    span: Region,
    strand: Strand,
    blocks: Vec<Region>,
    coding: Option<Region>,
}

impl Feature {
    pub fn new(name: impl Into<String>, span: Region, strand: Strand) -> Self {
        Self {
            name: name.into(),
            span,
            strand,
            blocks: Vec::new(),
            coding: Some(span),
        }
    }

    #[must_use]
    pub fn with_blocks(mut self, blocks: Vec<Region>) -> Self {
        let span = self.span;
        self.blocks = blocks
            .into_iter()
            .filter_map(|block| block.intersection(span))
            .collect();
        self.blocks.sort_unstable_by_key(|block| block.start());
        self
    }

    #[must_use]
    pub fn with_coding(mut self, coding: Option<Region>) -> Self {
        self.coding = coding.and_then(|coding| coding.intersection(self.span));
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn span(&self) -> Region {
        self.span
    }

    pub fn strand(&self) -> Strand {
        self.strand
    }

    pub fn blocks(&self) -> &[Region] {
        if self.blocks.is_empty() {
            slice::from_ref(&self.span)
        } else {
            &self.blocks
        }
    }

    pub fn coding(&self) -> Option<Region> {
        self.coding
    }
}

#[cfg(test)]
mod tests {
    use super::{Feature, Strand};
    use crate::Region;

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn a_new_feature_is_one_coding_block() {
        let feature = Feature::new("a", region(10, 20), Strand::Forward);
        assert_eq!(feature.name(), "a");
        assert_eq!(feature.blocks(), [region(10, 20)]);
        assert_eq!(feature.coding(), Some(region(10, 20)));
    }

    #[test]
    fn blocks_are_sorted_and_kept_inside_the_span() {
        let feature = Feature::new("a", region(10, 50), Strand::Reverse).with_blocks(vec![
            region(40, 60),
            region(60, 70),
            region(0, 15),
        ]);
        assert_eq!(feature.blocks(), [region(10, 15), region(40, 50)]);
    }

    #[test]
    fn coding_is_kept_inside_the_span() {
        let feature = Feature::new("a", region(10, 50), Strand::Unknown);
        assert_eq!(
            feature.clone().with_coding(Some(region(5, 30))).coding(),
            Some(region(10, 30))
        );
        assert_eq!(feature.with_coding(None).coding(), None);
    }
}
