//! Links between two regions, such as chromatin loops from Hi-C or ChIA-PET.

use crate::Region;

/// A link between two regions of the genome.
#[derive(Debug, Clone, PartialEq)]
pub struct Interaction {
    /// The end on this contig. When both ends are on it, the end more to the left.
    pub near: Region,
    /// The other end.
    pub far: Region,
    /// The contig of the other end, when it is not this contig.
    pub elsewhere: Option<String>,
    pub name: String,
    /// The strength of the link, for example a count of contacts.
    pub value: Option<f64>,
    pub color: Option<[u8; 3]>,
    /// The other fields of the record, for the card.
    pub extra: String,
}

impl Interaction {
    /// A link with its ends in order: the near end is the end more to the left.
    pub fn new(first: Region, second: Region, elsewhere: Option<String>) -> Self {
        let (near, far) = if elsewhere.is_none() && second.start() < first.start() {
            (second, first)
        } else {
            (first, second)
        };
        Self {
            near,
            far,
            elsewhere,
            name: String::new(),
            value: None,
            color: None,
            extra: String::new(),
        }
    }

    /// The part of this contig that the link covers: both ends and the gap between them, or only
    /// the near end of a link to another contig.
    pub fn span(&self) -> Region {
        if self.elsewhere.is_some() {
            self.near
        } else {
            self.near.hull(self.far)
        }
    }

    /// The number of bases between the middles of the two ends, for links on one contig.
    pub fn distance(&self) -> Option<u64> {
        if self.elsewhere.is_some() {
            return None;
        }
        let middle = |region: Region| region.start() + region.len() / 2;
        Some(middle(self.far).abs_diff(middle(self.near)))
    }
}

#[cfg(test)]
mod tests {
    use super::Interaction;
    use crate::Region;

    #[test]
    fn links_on_one_contig_put_the_left_end_first() {
        let link = Interaction::new(
            Region::between(50_000, 52_000),
            Region::between(1_000, 2_000),
            None,
        );
        assert_eq!(link.near, Region::between(1_000, 2_000));
        assert_eq!(link.far, Region::between(50_000, 52_000));
        assert_eq!(link.span(), Region::between(1_000, 52_000));
        assert_eq!(link.distance(), Some(49_500));
    }

    #[test]
    fn links_to_other_contigs_cover_only_their_near_end() {
        let near = Region::between(9_000, 9_500);
        let link = Interaction::new(near, Region::between(10, 20), Some("chr2".to_owned()));
        assert_eq!((link.near, link.span()), (near, near));
        assert_eq!(link.elsewhere.as_deref(), Some("chr2"));
        assert_eq!(link.distance(), None);
    }
}
