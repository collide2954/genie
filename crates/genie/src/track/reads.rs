use std::io::BufReader;

use genie_core::Region;
use genie_core::alignment::{Alignment, DUPLICATE, Filter};
use genie_formats::bgzf::Bgzf;
use genie_formats::cram::{Cram, crai};
use genie_formats::fields::{self, Rule};
use genie_formats::{bai, bam};

use super::{Gather, Loaded, Options};
use crate::location::Location;

const MARGIN: u64 = 1_000;

/// How a read file finds the reads of a region: a BAM file with its BAI or CSI index, or a
/// CRAM file with its CRAI index.
#[derive(Debug)]
pub enum Reads {
    Bam(bai::Index),
    Cram(Box<Cram>, crai::Index),
}

#[derive(Debug)]
pub struct ReadFile {
    pub index: Reads,
    pub contigs: Vec<String>,
}

fn first_opening(candidates: Vec<Location>) -> Option<crate::location::Reader> {
    candidates
        .into_iter()
        .find_map(|candidate| candidate.open().ok())
}

impl ReadFile {
    pub const SPAN: u64 = 30_000;

    pub fn open(location: &Location, name: &str, index: Option<&Location>) -> Result<Self, String> {
        if super::is_cram(name) {
            Self::open_cram(location, name, index)
        } else {
            Self::open_bam(location, name, index)
        }
    }

    pub(super) fn open_bam(
        location: &Location,
        name: &str,
        index: Option<&Location>,
    ) -> Result<Self, String> {
        let candidates = match index {
            Some(index) => vec![index.clone()],
            None => vec![
                location.with_suffix(".bai"),
                location.with_extension("bai"),
                location.with_suffix(".csi"),
            ],
        };
        let file = first_opening(candidates).ok_or_else(|| {
            format!("There is no index {name}.bai. Index the file with samtools index.")
        })?;
        let index = bai::Index::read(file)
            .map_err(|error| format!("The index {name}.bai is not valid. {error}"))?;
        let header =
            bam::header(&mut Bgzf::new(location.open()?)).map_err(|error| error.to_string())?;
        let contigs = header
            .references
            .into_iter()
            .map(|(contig, _)| contig)
            .collect();
        Ok(Self {
            index: Reads::Bam(index),
            contigs,
        })
    }

    pub(super) fn open_cram(
        location: &Location,
        name: &str,
        index: Option<&Location>,
    ) -> Result<Self, String> {
        let candidates = match index {
            Some(index) => vec![index.clone()],
            None => vec![
                location.with_suffix(".crai"),
                location.with_extension("crai"),
            ],
        };
        let file = first_opening(candidates).ok_or_else(|| {
            format!("There is no index {name}.crai. Index the file with samtools index.")
        })?;
        let index = crai::Index::read(file)
            .map_err(|error| format!("The index {name}.crai is not valid. {error}"))?;
        let cram =
            Cram::read(&mut BufReader::new(location.open()?)).map_err(|error| error.to_string())?;
        let contigs = cram
            .references
            .iter()
            .map(|(contig, _)| contig.clone())
            .collect();
        Ok(Self {
            index: Reads::Cram(Box::new(cram), index),
            contigs,
        })
    }

    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        (options, rules): (Options, &[Rule]),
        mut reference: impl FnMut(Region) -> Result<Vec<u8>, String>,
    ) -> Result<Loaded, String> {
        let id = self
            .contigs
            .iter()
            .position(|known| known == contig)
            .ok_or_else(|| format!("The file has no contig {contig}."))?;
        let filter = if options.duplicates {
            Filter::default().showing(DUPLICATE)
        } else {
            Filter::default()
        };
        let keeps = |alignment: &Alignment| {
            filter.keeps(alignment)
                && (rules.is_empty() || fields::passes(rules, &super::read_fields(alignment)))
        };
        let mut gather = Gather::new(region, id, options);
        match &self.index {
            Reads::Bam(index) => {
                let mut reader = Bgzf::new(location.open()?);
                bam::query(index, &mut reader, id, region, |alignment| {
                    if keeps(&alignment) {
                        gather.add(alignment);
                    }
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            }
            Reads::Cram(cram, index) => {
                let mut reader = BufReader::new(location.open()?);
                let mut fetch = |_: usize, window: Region| reference(window);
                cram.query(index, &mut reader, id, region, &mut fetch, |alignment| {
                    if keeps(&alignment) {
                        gather.add(alignment);
                    }
                })
                .map_err(|error| error.to_string())?;
            }
        }
        let window = Region::between(
            region.start().saturating_sub(MARGIN),
            region.end().saturating_add(MARGIN),
        );
        let bases = reference(window)?;
        Ok(Loaded::reads(gather, bases, window.start()))
    }
}
