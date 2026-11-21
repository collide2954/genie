use std::io::BufReader;

use genie_core::Region;
use genie_core::alignment::{Alignment, DUPLICATE, Filter, QC_FAIL, SECONDARY, SUPPLEMENTARY};
use genie_core::haplotag;
use genie_formats::bgzf::Bgzf;
use genie_formats::cram::{Cram, crai};
use genie_formats::fields;
use genie_formats::{bai, bam};

use super::{Gather, Loaded, Options, Scope};
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
    /// The samples of the read groups of the file (SM of @RG).
    pub samples: Vec<String>,
}

/// The flags that hide reads: the default ones, less the kinds that the options show.
fn flags(options: Options) -> Filter {
    let mut filter = Filter::default();
    for (shown, flag) in [
        (options.duplicates, DUPLICATE),
        (options.kinds.secondary, SECONDARY),
        (options.kinds.failed, QC_FAIL),
    ] {
        if shown {
            filter = filter.showing(flag);
        }
    }
    if options.kinds.supplementary {
        filter
    } else {
        filter.hiding(SUPPLEMENTARY)
    }
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
            samples: header.samples,
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
        let samples = cram.samples.clone();
        Ok(Self {
            index: Reads::Cram(Box::new(cram), index),
            contigs,
            samples,
        })
    }

    /// The reads of a region that pass the options and the rules of the scope. A read without
    /// an HP tag gets the haplotype of the phased SNVs of the scope that it covers.
    pub fn load(
        &self,
        location: &Location,
        contig: &str,
        region: Region,
        (options, scope): (Options, Scope<'_>),
        mut reference: impl FnMut(Region) -> Result<Vec<u8>, String>,
    ) -> Result<Loaded, String> {
        let id = self
            .contigs
            .iter()
            .position(|known| known == contig)
            .ok_or_else(|| format!("The file has no contig {contig}."))?;
        let (filter, rules) = (flags(options), scope.rules);
        let keeps = |alignment: &Alignment| {
            filter.keeps(alignment)
                && alignment.mapq >= options.mapq
                && (rules.is_empty() || fields::passes(rules, &super::read_fields(alignment)))
        };
        let mut gather = Gather::new(region, id, options).with_cells(scope.cells.cloned());
        let mut add = |mut alignment: Alignment| {
            if keeps(&alignment) {
                haplotag::tag(&mut alignment, scope.phased);
                gather.add(alignment);
            }
        };
        match &self.index {
            Reads::Bam(index) => {
                let mut reader = Bgzf::new(location.open()?);
                bam::query(index, &mut reader, id, region, |alignment| {
                    add(alignment);
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            }
            Reads::Cram(cram, index) => {
                let mut reader = BufReader::new(location.open()?);
                let mut fetch = |_: usize, window: Region| reference(window);
                cram.query(index, &mut reader, id, region, &mut fetch, add)
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
