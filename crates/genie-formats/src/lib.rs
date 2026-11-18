pub mod annotations;
pub mod bai;
pub mod bam;
pub mod bbi;
pub mod bcf;
pub mod bed;
pub mod bedgraph;
pub mod bedmethyl;
pub mod bedpe;
pub mod bgzf;
mod binning;
#[cfg(feature = "net-budget")]
pub mod budget;
pub mod chain;
pub mod columns;
pub mod contigs;
pub mod cram;
pub mod crc;
pub mod cytoband;
mod error;
pub mod fai;
pub mod fasta;
pub mod features;
pub mod fields;
pub mod genepred;
pub mod gff;
pub mod gtf;
pub mod gwas;
pub mod gzi;
pub mod gzip;
pub mod hic;
pub mod http;
pub mod hub;
pub mod icon;
pub mod ini;
pub mod md5;
pub mod png;
pub mod remote;
pub mod samples;
pub mod seg;
mod sidecar;
pub mod sort;
pub mod sv;
pub mod tabix;
pub mod table;
mod text;
pub mod twobit;
pub mod vcf;

pub use error::{Error, Reason};

#[cfg(test)]
#[path = "../tests/support/server.rs"]
mod server;
