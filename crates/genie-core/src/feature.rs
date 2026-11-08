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
    color: Option<[u8; 3]>,
}

impl Feature {
    pub fn new(name: impl Into<String>, span: Region, strand: Strand) -> Self {
        Self {
            name: name.into(),
            span,
            strand,
            blocks: Vec::new(),
            coding: Some(span),
            color: None,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    #[must_use]
    pub fn with_color(mut self, color: Option<[u8; 3]>) -> Self {
        self.color = color;
        self
    }

    pub fn color(&self) -> Option<[u8; 3]> {
        self.color
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
    fn names_can_change() {
        let feature = Feature::new("NM_1", region(10, 20), Strand::Forward).with_name("ABC1");
        assert_eq!(feature.name(), "ABC1");
        assert_eq!(feature.span(), region(10, 20));
    }

    #[test]
    fn features_keep_a_file_color() {
        let feature = Feature::new("a", region(10, 50), Strand::Unknown);
        assert_eq!(feature.color(), None);
        assert_eq!(
            feature.with_color(Some([200, 10, 0])).color(),
            Some([200, 10, 0])
        );
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
