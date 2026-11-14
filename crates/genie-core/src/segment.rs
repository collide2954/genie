//! Copy number segments: parts of the genome of a sample with one value, such as a log2 ratio.

use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub span: Region,
    /// The number of the sample in its file.
    pub sample: usize,
    pub value: f32,
    /// The number of probes, bins or points that give the value, if the file has it.
    pub markers: Option<u32>,
}
