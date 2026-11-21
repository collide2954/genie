use std::io::BufReader;

use genie_core::Region;
use genie_core::alignment::{Alignment, DUPLICATE, Filter, QC_FAIL, SECONDARY, SUPPLEMENTARY};
use genie_core::haplotag::{self, Phased, Votes};
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
    /// an HP tag gets the haplotype of the phased SNVs of the scope that it covers, named as the
    /// HP tags of the other reads name it.
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
        let new = || Gather::new(region, id, options).with_cells(scope.cells.cloned());
        let mut stream = |add: &mut dyn FnMut(Alignment)| match &self.index {
            Reads::Bam(index) => {
                let mut reader = Bgzf::new(location.open()?);
                bam::query(index, &mut reader, id, region, |alignment| {
                    add(alignment);
                    Ok(())
                })
                .map_err(|error| error.to_string())
            }
            Reads::Cram(cram, index) => {
                let mut reader = BufReader::new(location.open()?);
                let mut fetch = |_: usize, window: Region| reference(window);
                cram.query(index, &mut reader, id, region, &mut fetch, add)
                    .map_err(|error| error.to_string())
            }
        };
        let gather = gather_phased(new, keeps, scope.phased, &mut stream)?;
        let window = Region::between(
            region.start().saturating_sub(MARGIN),
            region.end().saturating_add(MARGIN),
        );
        let bases = reference(window)?;
        Ok(Loaded::reads(gather, bases, window.start()))
    }
}

/// Reads the reads of a region and gives each one to a function, each time that genie calls it.
type Stream<'a> = dyn FnMut(&mut dyn FnMut(Alignment)) -> Result<(), String> + 'a;

/// The reads of a stream that `keeps` keeps, in a new gather. A read without an HP tag gets the
/// haplotype of the phased SNVs that it covers. When most of the reads with an HP tag and a
/// haplotype from the SNVs disagree in a phase set, the SNVs name the haplotypes there the other
/// way round from the tags: genie reads the stream again and gives the reads of that set the
/// other haplotype.
fn gather_phased(
    new: impl Fn() -> Gather,
    keeps: impl Fn(&Alignment) -> bool,
    phased: &[Phased],
    stream: &mut Stream<'_>,
) -> Result<Gather, String> {
    let (mut votes, mut given) = (Votes::default(), Vec::new());
    let mut gather = new();
    stream(&mut |mut alignment| {
        if keeps(&alignment) {
            if !phased.is_empty() {
                votes.count(&alignment, phased);
            }
            if let Some(set) = haplotag::tag(&mut alignment, phased, &[])
                && !given.contains(&set)
            {
                given.push(set);
            }
            gather.add(alignment);
        }
    })?;
    let swapped: Vec<u64> = votes
        .swapped()
        .into_iter()
        .filter(|set| given.contains(set))
        .collect();
    if swapped.is_empty() {
        return Ok(gather);
    }
    drop(gather);
    let mut gather = new();
    stream(&mut |mut alignment| {
        if keeps(&alignment) {
            haplotag::tag(&mut alignment, phased, &swapped);
            gather.add(alignment);
        }
    })?;
    Ok(gather)
}

#[cfg(test)]
mod tests {
    use genie_core::Region;
    use genie_core::alignment::{Alignment, Op, Tags};
    use genie_core::haplotag::Phased;

    use super::gather_phased;
    use crate::track::{Content, Gather, Loaded, Options};

    /// A read from 8 with the bases of haplotype 1 or 2 of the SNVs at 10, 12 and 14, and an HP
    /// tag.
    fn read(name: &str, haplotype: i64, phase: Option<i64>) -> Alignment {
        let bases = if haplotype == 1 {
            "nnAnCnGnnn"
        } else {
            "nnGnTnAnnn"
        };
        Alignment {
            name: name.to_owned(),
            start: 8,
            flags: 0,
            mapq: 60,
            cigar: vec![(Op::Match, 10)],
            bases: bases.as_bytes().to_vec(),
            qualities: vec![30; bases.len()],
            mate: None,
            tags: Tags {
                phase,
                ..Tags::default()
            },
        }
    }

    /// The haplotype of each read after a gather with the SNVs of phase set 5, and the number
    /// of times that the gather read the stream.
    fn gathered(reads: &[Alignment]) -> (Vec<(String, Option<i64>, bool)>, usize) {
        let sites = [(10, b'A', b'G'), (12, b'C', b'T'), (14, b'G', b'A')].map(
            |(position, first, second)| Phased {
                position,
                bases: [first, second],
                set: 5,
            },
        );
        let region = Region::between(0, 100);
        let options = Options {
            downsample: false,
            ..Options::default()
        };
        let mut passes = 0;
        let gather = gather_phased(
            || Gather::new(region, 0, options),
            |_: &Alignment| true,
            &sites,
            &mut |add: &mut dyn FnMut(Alignment)| {
                passes += 1;
                for read in reads {
                    add(read.clone());
                }
                Ok(())
            },
        )
        .unwrap();
        let loaded = Loaded::reads(gather, vec![b'A'; 200], 0);
        let Content::Reads(pile) = loaded.content else {
            panic!("expected reads");
        };
        let mut found: Vec<(String, Option<i64>, bool)> = pile
            .reads
            .iter()
            .map(|read| {
                let tags = &read.alignment.tags;
                (read.alignment.name.clone(), tags.phase, tags.phase_from_vcf)
            })
            .collect();
        found.sort();
        (found, passes)
    }

    #[test]
    fn the_haplotypes_of_the_snvs_follow_the_hp_tags_of_the_other_reads() {
        let plain = read("plain", 2, None);
        let swapped = [read("a", 2, Some(1)), read("b", 1, Some(2)), plain.clone()];
        let (found, passes) = gathered(&swapped);
        assert_eq!(
            found,
            [
                ("a".to_owned(), Some(1), false),
                ("b".to_owned(), Some(2), false),
                ("plain".to_owned(), Some(1), true),
            ]
        );
        assert_eq!(passes, 2);
        let same = [read("a", 2, Some(2)), read("b", 1, Some(1)), plain.clone()];
        let (found, passes) = gathered(&same);
        assert_eq!(found[2], ("plain".to_owned(), Some(2), true));
        assert_eq!(passes, 1);
        let (found, passes) = gathered(&[plain]);
        assert_eq!(found, [("plain".to_owned(), Some(2), true)]);
        assert_eq!(passes, 1);
    }
}
