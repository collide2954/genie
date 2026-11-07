pub mod bai;
pub mod bam;
pub mod bed;
pub mod bgzf;
mod binning;
mod error;
pub mod fai;
pub mod fasta;
pub mod features;
pub mod gff;
pub mod gzi;
pub mod ini;
pub mod tabix;
pub mod twobit;

pub use error::{Error, Reason};
