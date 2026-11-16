//! The rows of GWAS summary statistics.

/// The association of a variant with a trait: its position (from 0), its -log10 p, and the text
/// of its row for the card, when genie keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Association {
    pub position: u64,
    pub value: f32,
    pub row: Option<Box<str>>,
}
