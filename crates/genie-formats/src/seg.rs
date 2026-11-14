//! Copy number segment files: IGV SEG files (as from TCGA, `DNAcopy` or GISTIC), `CNVkit`
//! `.cns` and `.cnr` files, and GATK `ModelSegments` `.seg` files.

use std::collections::HashMap;
use std::io::BufRead;

use genie_core::Region;
use genie_core::segment::Segment;

use crate::table::Table;
use crate::text::lines;
use crate::{Error, Reason};

const CHROMOSOMES: [&str; 5] = ["chrom", "chromosome", "chr", "contig", "chromosome_name"];
const STARTS: [&str; 5] = [
    "loc.start",
    "start",
    "startpos",
    "start_position",
    "chromstart",
];
const ENDS: [&str; 5] = ["loc.end", "end", "endpos", "end_position", "chromend"];
const SAMPLES: [&str; 6] = [
    "id",
    "sample",
    "sample_id",
    "id.sample",
    "sample_name",
    "samplename",
];
const VALUES: [&str; 8] = [
    "seg.mean",
    "segment_mean",
    "log2",
    "mean_log2_copy_ratio",
    "log2_copy_ratio_posterior_50",
    "seg.cn",
    "log2ratio",
    "value",
];
const MARKERS: [&str; 6] = [
    "num.mark",
    "num_probes",
    "num_markers",
    "num.markers",
    "probes",
    "num_points_copy_ratio",
];

/// A segment file in memory.
#[derive(Debug, Default)]
pub struct Segments {
    pub samples: Vec<String>,
    pub table: Table<Segment>,
    /// The name of the value column, as the header line gives it.
    pub value: String,
}

/// Where the fields of a segment are, from the header line.
struct Columns {
    chrom: usize,
    start: usize,
    end: usize,
    sample: Option<usize>,
    value: usize,
    markers: Option<usize>,
    /// `CNVkit` starts count from 0, as in BED. SEG and GATK starts count from 1.
    zero_based: bool,
}

impl Columns {
    fn of(header: &[&str], gatk: bool) -> Option<Self> {
        let names: Vec<String> = header
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let find = |wanted: &[&str]| {
            wanted
                .iter()
                .find_map(|wanted| names.iter().position(|name| name == wanted))
        };
        let chrom = find(&CHROMOSOMES)?;
        let sample = find(&SAMPLES).or_else(|| (chrom > 0 && !gatk).then_some(0));
        Some(Self {
            chrom,
            start: find(&STARTS)?,
            end: find(&ENDS)?,
            sample,
            value: find(&VALUES).unwrap_or(names.len() - 1),
            markers: find(&MARKERS),
            zero_based: !gatk && sample.is_none() && names.iter().any(|name| name == "log2"),
        })
    }
}

fn fields(line: &str) -> Vec<&str> {
    if line.contains('\t') {
        line.split('\t').map(str::trim).collect()
    } else {
        line.split_ascii_whitespace().collect()
    }
}

/// The name of a file without its extensions, as the sample of a file of one sample.
fn stem(name: &str) -> String {
    name.split('.').next().unwrap_or(name).to_owned()
}

/// Reads a segment file. A file without a sample column gets its sample name from the `@RG`
/// header line of GATK, or else from its file name.
pub fn read(reader: impl BufRead, name: &str) -> Result<Segments, Error> {
    let mut columns: Option<Columns> = None;
    let mut gatk = false;
    let mut single: Option<String> = None;
    let mut samples: Vec<String> = Vec::new();
    let mut numbers: HashMap<String, usize> = HashMap::new();
    let mut rows = Vec::new();
    let mut value = String::new();
    lines(reader, |number, line| {
        let fail = |reason| Error::Format {
            line: number,
            reason,
        };
        if line.trim().is_empty() || line.starts_with('#') {
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix('@') {
            gatk = true;
            if rest.starts_with("RG") && single.is_none() {
                single = rest
                    .split('\t')
                    .find_map(|field| field.strip_prefix("SM:"))
                    .map(str::to_owned);
            }
            return Ok(());
        }
        let fields = fields(line);
        let Some(found) = &columns else {
            let found = Columns::of(&fields, gatk).ok_or_else(|| fail(Reason::SegmentHeader))?;
            fields[found.value].clone_into(&mut value);
            columns = Some(found);
            return Ok(());
        };
        let get = |column: usize| {
            fields
                .get(column)
                .copied()
                .ok_or_else(|| fail(Reason::MissingField))
        };
        let position = |column| {
            get(column)?
                .parse::<u64>()
                .map_err(|_| fail(Reason::BadNumber))
        };
        let (start, end) = (position(found.start)?, position(found.end)?);
        let start = if found.zero_based {
            start
        } else {
            start.saturating_sub(1)
        };
        let span = Region::new(start, end).ok_or_else(|| fail(Reason::BadSpan))?;
        let Some(amount) = get(found.value)?
            .parse::<f32>()
            .ok()
            .filter(|amount| amount.is_finite())
        else {
            return Ok(());
        };
        let owner = match found.sample {
            Some(column) => get(column)?.to_owned(),
            None => single.clone().unwrap_or_else(|| stem(name)),
        };
        let sample = *numbers.entry(owner).or_insert_with_key(|owner| {
            samples.push(owner.clone());
            samples.len() - 1
        });
        let markers = found
            .markers
            .and_then(|column| fields.get(column)?.parse::<u32>().ok());
        rows.push((
            get(found.chrom)?.to_owned(),
            span,
            Segment {
                span,
                sample,
                value: amount,
                markers,
            },
        ));
        Ok(())
    })?;
    Ok(Segments {
        samples,
        table: Table::new(rows),
        value,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::Region;

    use super::{Segments, read};
    use crate::{Error, Reason};

    fn opened(text: &str, name: &str) -> Segments {
        read(Cursor::new(text), name).unwrap()
    }

    fn found(segments: &Segments, contig: &str) -> Vec<(String, Region, f32, Option<u32>)> {
        segments
            .table
            .query(contig, Region::between(0, u64::MAX >> 1))
            .into_iter()
            .map(|segment| {
                (
                    segments.samples[segment.sample].clone(),
                    segment.span,
                    segment.value,
                    segment.markers,
                )
            })
            .collect()
    }

    #[test]
    fn seg_files_hold_many_samples_and_count_from_one() {
        let text = "#track name=cohort\n\
            ID\tchrom\tloc.start\tloc.end\tnum.mark\tseg.mean\n\
            tumor1\tchr1\t1\t1000\t12\t0.85\n\
            tumor1\tchr1\t1001\t5000\t40\tNA\n\
            tumor2\tchr1\t101\t2000\t8\t-1.2\n\
            tumor2\t2\t1\t500\t3\t0.1\n";
        let segments = opened(text, "cohort.seg");
        assert_eq!(segments.samples, ["tumor1", "tumor2"]);
        assert_eq!(segments.value, "seg.mean");
        assert_eq!(segments.table.contigs(), ["chr1", "2"]);
        assert_eq!(
            found(&segments, "chr1"),
            [
                (
                    "tumor1".to_owned(),
                    Region::between(0, 1_000),
                    0.85,
                    Some(12)
                ),
                (
                    "tumor2".to_owned(),
                    Region::between(100, 2_000),
                    -1.2,
                    Some(8)
                ),
            ]
        );
        let tcga = "Sample\tChromosome\tStart\tEnd\tNum_Probes\tSegment_Mean\n\
            TCGA-01\t1\t3218610\t95674710\t53225\t0.0055\n";
        let tcga = opened(tcga, "brca.seg");
        assert_eq!(
            (tcga.samples[0].as_str(), tcga.value.as_str()),
            ("TCGA-01", "Segment_Mean")
        );
        let first = &found(&tcga, "1")[0];
        assert_eq!(
            (first.1, first.3),
            (Region::between(3_218_609, 95_674_710), Some(53_225))
        );
    }

    #[test]
    fn cnvkit_files_count_from_zero_and_take_the_file_name() {
        let text = "chromosome\tstart\tend\tgene\tlog2\tdepth\tprobes\tweight\n\
            chr1\t0\t1000\tA,B\t-0.4\t120.5\t7\t3.2\n\
            chr2\t500\t900\t-\t1.1\t300\t2\t1\n";
        let segments = opened(text, "tumor.call.cns");
        assert_eq!(segments.samples, ["tumor"]);
        assert_eq!(segments.value, "log2");
        assert_eq!(
            found(&segments, "chr1"),
            [("tumor".to_owned(), Region::between(0, 1_000), -0.4, Some(7))]
        );
    }

    #[test]
    fn gatk_files_take_the_sample_from_the_read_group() {
        let text = "@HD\tVN:1.6\n@SQ\tSN:chr1\tLN:248956422\n@RG\tID:GATKCopyNumber\tSM:patient7\n\
            CONTIG\tSTART\tEND\tNUM_POINTS_COPY_RATIO\tMEAN_LOG2_COPY_RATIO\tCALL\n\
            chr1\t10001\t20000\t31\t0.32\t+\n";
        let segments = opened(text, "patient7.called.seg");
        assert_eq!(segments.samples, ["patient7"]);
        assert_eq!(segments.value, "MEAN_LOG2_COPY_RATIO");
        assert_eq!(
            found(&segments, "chr1"),
            [(
                "patient7".to_owned(),
                Region::between(10_000, 20_000),
                0.32,
                Some(31)
            )]
        );
        let posterior = "@RG\tID:x\tSM:p\nCONTIG\tSTART\tEND\tNUM_POINTS_COPY_RATIO\t\
            LOG2_COPY_RATIO_POSTERIOR_10\tLOG2_COPY_RATIO_POSTERIOR_50\tLOG2_COPY_RATIO_POSTERIOR_90\n\
            chr1\t1\t100\t5\t0.1\t0.2\t0.3\n";
        let segments = opened(posterior, "p.modelFinal.seg");
        assert_eq!(found(&segments, "chr1")[0].2, 0.2);
    }

    #[test]
    fn broken_files_say_where() {
        let failure = |text: &str| match read(Cursor::new(text), "x.seg") {
            Err(Error::Format { line, reason }) => (line, reason),
            other => panic!("expected a format error, got {other:?}"),
        };
        assert_eq!(failure("chr1\t1\t100\t0.5\n"), (1, Reason::SegmentHeader));
        assert_eq!(
            failure("ID\tchrom\tloc.start\tloc.end\tseg.mean\ns\tchr1\tx\t100\t0.5\n"),
            (2, Reason::BadNumber)
        );
        assert_eq!(
            failure("ID\tchrom\tloc.start\tloc.end\tseg.mean\ns\tchr1\t500\t100\t0.5\n"),
            (2, Reason::BadSpan)
        );
        assert_eq!(
            failure("ID\tchrom\tloc.start\tloc.end\tseg.mean\ns\tchr1\t5\n"),
            (2, Reason::MissingField)
        );
    }
}
