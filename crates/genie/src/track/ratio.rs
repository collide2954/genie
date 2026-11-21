//! Depth ratio tracks: the log2 ratio of the depths of two read files in bins, as tumor over
//! normal, with segments of the same ratio. Wide views take the depth of each file from its index
//! alone, as goleft indexcov does; views of 1 Mb or less count the reads in bins.

use std::collections::HashMap;
use std::io::BufReader;
use std::sync::{Arc, Mutex, PoisonError};

use genie_core::Region;
use genie_core::alignment::{Alignment, SECONDARY, SUPPLEMENTARY, UNMAPPED};
use genie_core::depth::{self, Bin, Segment};
use genie_formats::bam;
use genie_formats::bgzf::Bgzf;

use super::reads::Reads;
use super::{Format, Source, Track};

/// The words that start the key of a depth ratio track in a session.
pub const KEY: &str = "depth ratio";
/// The windows of the linear index of a BAI file, and the bins of wide views.
pub const WINDOW: u64 = 16_384;
/// The widest view whose bins come from the reads.
pub const READS_SPAN: u64 = 1_000_000;
/// The smallest bin of a view whose bins come from the reads.
const LEAST_BIN: u64 = 500;
/// The pixels of a bin of a view whose bins come from the reads.
const BIN_PIXELS: u64 = 4;
/// The most bytes of reads that genie reads for the bins of a view: on the computer and on the
/// web. Past them, the bins come from the indexes.
const BUDGET: (u64, u64) = (32 << 20, 8 << 20);
/// What a ratio track says when its two read tracks are not open.
pub const NOT_LINKED: &str = "The two read tracks of this ratio are not open.";
/// What a ratio track says when an index cannot estimate the depth.
pub const NO_DENSITY: &str = "The index of a read file cannot give its depth: it has no counts of reads (samtools index adds them).";

/// The bins and the segments of a region, and the step of the view that loaded them.
#[derive(Debug, PartialEq)]
pub struct Ratios {
    pub bins: Vec<Bin>,
    pub segments: Vec<Segment>,
    /// Whether the bins come from the reads, else from the indexes.
    pub reads: bool,
    pub step: u64,
}

/// A read file of a ratio on a contig: its source, the name of the contig in the file and the
/// length of the contig.
#[derive(Debug, Clone)]
pub struct Input {
    pub source: Arc<Source>,
    pub contig: String,
    pub length: u64,
}

/// The two read files of a ratio track on the contig in view, and whether the view is narrow
/// enough to count their reads.
#[derive(Debug, Clone)]
pub struct Depths {
    pub inputs: (Input, Input),
    pub narrow: bool,
}

impl Depths {
    /// The read tracks of a ratio track on a contig of a length, from its options, for a view.
    /// Read tracks with a lift do not count.
    pub fn of(
        tracks: &[Track],
        track: &Track,
        (contig, length): (usize, u64),
        visible: Region,
    ) -> Option<Self> {
        let (first, second) = track.options.depths?;
        let input = |id: u64| {
            let found = tracks
                .iter()
                .find(|other| other.id == id && other.source.has_reads() && other.lift.is_none())?;
            Some(Input {
                source: Arc::clone(&found.source),
                contig: found.contigs.get(contig)?.clone()?,
                length,
            })
        };
        Some(Self {
            inputs: (input(first)?, input(second)?),
            narrow: visible.len() <= READS_SPAN,
        })
    }
}

/// The bins of the indexes over a whole contig, the usual depth of each file in records per
/// Mb, and the segments of the bins.
#[derive(Debug)]
struct Whole {
    bins: Vec<Bin>,
    usual: (f64, f64),
    segments: Vec<Segment>,
}

/// A track of the ratio of the depths of two read tracks. It reads nothing itself: the read
/// tracks give their files when it loads.
#[derive(Debug)]
pub struct RatioFile {
    contigs: Vec<String>,
    wholes: Mutex<HashMap<(usize, usize, String), Arc<Whole>>>,
}

impl RatioFile {
    /// A ratio on the contigs of an assembly.
    pub fn new(contigs: Vec<String>) -> Self {
        Self {
            contigs,
            wholes: Mutex::new(HashMap::new()),
        }
    }

    pub fn contigs(&self) -> &[String] {
        &self.contigs
    }

    /// The bins and the segments of a region. Views of `READS_SPAN` or less count the reads in
    /// bins of `BIN_PIXELS` pixels, if their data is within the budget.
    pub fn load(
        &self,
        depths: Option<&Depths>,
        region: Region,
        step: u64,
        bases: &mut dyn FnMut(Region) -> Result<Vec<u8>, String>,
    ) -> Result<Ratios, String> {
        let depths = depths.ok_or(NOT_LINKED)?;
        let (detail, (first, second)) = (depths.narrow, &depths.inputs);
        let whole = self.whole(first, second)?;
        let segments = whole
            .segments
            .iter()
            .filter(|segment| segment.span.intersection(region).is_some())
            .copied()
            .collect();
        let size = (step * BIN_PIXELS).max(LEAST_BIN);
        let counted = if detail {
            counted(first, region, size, bases)?.zip(counted(second, region, size, bases)?)
        } else {
            None
        };
        let (bins, reads) = if let Some(((spans, first), (_, second))) = counted {
            let usual = (
                whole.usual.0 * size as f64 / 1e6,
                whole.usual.1 * size as f64 / 1e6,
            );
            let lengths: Vec<f64> = spans.iter().map(|span| span.len() as f64).collect();
            let scale = |counts: &[f64]| -> Vec<f64> {
                counts
                    .iter()
                    .zip(&lengths)
                    .map(|(count, length)| count * size as f64 / length)
                    .collect()
            };
            let bins = depth::ratios(&spans, (&scale(&first), &scale(&second)), usual);
            (bins, true)
        } else {
            let bins = whole
                .bins
                .iter()
                .filter(|bin| bin.span.intersection(region).is_some())
                .copied()
                .collect();
            (bins, false)
        };
        Ok(Ratios {
            bins,
            segments,
            reads,
            step,
        })
    }

    /// The bins, the usual depths and the segments of the whole contig, from the indexes.
    fn whole(&self, first: &Input, second: &Input) -> Result<Arc<Whole>, String> {
        let key = (
            Arc::as_ptr(&first.source) as usize,
            Arc::as_ptr(&second.source) as usize,
            first.contig.clone(),
        );
        let mut wholes = self.wholes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = wholes.get(&key) {
            return Ok(Arc::clone(found));
        }
        let length = first.length.min(second.length);
        let whole_region = Region::between(0, length);
        let (spans, a) = indexed(first, whole_region).ok_or(NO_DENSITY)?;
        let (_, b) = indexed(second, whole_region).ok_or(NO_DENSITY)?;
        let usual = (depth::median(&a), depth::median(&b));
        let (Some(usual_a), Some(usual_b)) = usual else {
            return Err(NO_DENSITY.to_owned());
        };
        let bins = depth::ratios(&spans, (&a, &b), (usual_a, usual_b));
        let segments = depth::segments(&bins);
        let whole = Arc::new(Whole {
            bins,
            usual: (usual_a, usual_b),
            segments,
        });
        wholes.insert(key, Arc::clone(&whole));
        Ok(whole)
    }
}

/// The bins of a region and a value for each one.
type Values = (Vec<Region>, Vec<f64>);

/// The windows of a region and the depth of a read file in each one, in records per Mb, from its
/// index alone.
fn indexed(input: &Input, region: Region) -> Option<Values> {
    let Format::Reads(file) = &input.source.format else {
        return None;
    };
    let reference = file.contigs.iter().position(|name| *name == input.contig)?;
    let tally = match &file.index {
        Reads::Bam(index) => index.density(reference, region, WINDOW, input.length)?,
        Reads::Cram(_, index) => index.density(i32::try_from(reference).ok()?, region, WINDOW)?,
    };
    let summaries = tally.summaries();
    let spans = summaries.iter().map(|summary| summary.span).collect();
    let values = summaries
        .iter()
        .map(|summary| f64::from(summary.max))
        .collect();
    Some((spans, values))
}

/// The bins of a region and the reads that start in each one, if the reads of the region are
/// within the budget. Only mapped primary reads count.
fn counted(
    input: &Input,
    region: Region,
    size: u64,
    bases: &mut dyn FnMut(Region) -> Result<Vec<u8>, String>,
) -> Result<Option<Values>, String> {
    let Format::Reads(file) = &input.source.format else {
        return Ok(None);
    };
    let Some(reference) = file.contigs.iter().position(|name| *name == input.contig) else {
        return Ok(None);
    };
    let location = input.source.data();
    let budget = if location.is_remote() {
        BUDGET.1
    } else {
        BUDGET.0
    };
    let bytes: u64 = match &file.index {
        Reads::Bam(index) => index
            .chunks(reference, region)
            .iter()
            .map(|chunk| (chunk.end >> 16).saturating_sub(chunk.start >> 16) + (1 << 16))
            .sum(),
        Reads::Cram(_, index) => index
            .entries
            .iter()
            .filter(|entry| i32::try_from(reference).is_ok_and(|id| id == entry.reference))
            .filter(|entry| {
                let start = u64::try_from(entry.start - 1).unwrap_or(0);
                let end = start + u64::try_from(entry.span).unwrap_or(0);
                start < region.end() && region.start() < end
            })
            .map(|entry| entry.size)
            .sum(),
    };
    if bytes > budget {
        return Ok(None);
    }
    let count = usize::try_from(region.len().div_ceil(size)).unwrap_or(0);
    let spans: Vec<Region> = (0..count as u64)
        .map(|index| {
            let start = region.start() + index * size;
            Region::between(start, (start + size).min(region.end()))
        })
        .collect();
    let mut counts = vec![0.0; count];
    let mut add = |alignment: Alignment| {
        if alignment.has(UNMAPPED) || alignment.has(SECONDARY) || alignment.has(SUPPLEMENTARY) {
            return;
        }
        if alignment.start < region.start() {
            return;
        }
        let at = usize::try_from((alignment.start - region.start()) / size).unwrap_or(usize::MAX);
        if let Some(found) = counts.get_mut(at) {
            *found += 1.0;
        }
    };
    match &file.index {
        Reads::Bam(index) => {
            let mut reader = Bgzf::new(location.open()?);
            bam::query(index, &mut reader, reference, region, |alignment| {
                add(alignment);
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        }
        Reads::Cram(cram, index) => {
            let mut reader = BufReader::new(location.open()?);
            let mut fetch = |_: usize, window: Region| bases(window);
            cram.query(index, &mut reader, reference, region, &mut fetch, add)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(Some((spans, counts)))
}
