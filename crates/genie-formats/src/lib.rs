pub mod bai;
pub mod bam;
pub mod bbi;
pub mod bed;
pub mod bedgraph;
pub mod bgzf;
mod binning;
pub mod contigs;
pub mod cytoband;
mod error;
pub mod fai;
pub mod fasta;
pub mod features;
pub mod fields;
pub mod genepred;
pub mod gff;
pub mod gtf;
pub mod gzi;
pub mod gzip;
pub mod http;
pub mod icon;
pub mod ini;
pub mod png;
pub mod remote;
mod sidecar;
pub mod sort;
pub mod tabix;
pub mod table;
mod text;
pub mod twobit;
pub mod vcf;

pub use error::{Error, Reason};

#[cfg(test)]
#[path = "../tests/support/server.rs"]
mod server;
