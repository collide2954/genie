//! Contacts between the bins of a chromosome, as Hi-C measures them.

/// The contacts of a region at one bin size.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContactMap {
    /// The size of the bins in bases.
    pub bin: u64,
    /// The two bins of each contact, the first at most the second, and its value. The contacts
    /// are in the order of their bins.
    pub contacts: Vec<(u32, u32, f32)>,
    /// Whether the values are ratios of observed to expected contacts.
    pub ratio: bool,
}

impl ContactMap {
    /// The value of the contact between two bins, if the map has one.
    pub fn value(&self, x: u32, y: u32) -> Option<f32> {
        let (x, y) = (x.min(y), x.max(y));
        self.contacts
            .binary_search_by(|(first, second, _)| (*first, *second).cmp(&(x, y)))
            .ok()
            .map(|at| self.contacts[at].2)
    }
}

#[cfg(test)]
mod tests {
    use super::ContactMap;

    #[test]
    fn values_come_from_either_order_of_the_bins() {
        let map = ContactMap {
            bin: 100,
            contacts: vec![(0, 0, 4.0), (0, 3, 2.5), (2, 5, 1.0)],
            ratio: false,
        };
        assert_eq!(map.value(0, 3), Some(2.5));
        assert_eq!(map.value(5, 2), Some(1.0));
        assert_eq!(map.value(1, 1), None);
    }
}
