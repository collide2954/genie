use std::io::{BufRead, Read, Seek};

use genie_core::{Feature, Region};

use crate::{Error, bed, bgzf::Bgzf, gff, tabix::Index};

const PASSES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub contig: String,
    pub span: Region,
}

pub fn names(kind: Kind, mut reader: impl BufRead) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if line.starts_with("##FASTA") {
            break;
        }
        entries.extend(match kind {
            Kind::Bed => bed_name(line),
            Kind::Gff => gff_name(line),
        });
    }
    Ok(entries)
}

fn bed_name(line: &str) -> Option<Entry> {
    let (contig, feature) = bed::parse(line).ok()??;
    (!feature.name().is_empty()).then(|| Entry {
        name: feature.name().to_owned(),
        contig: contig.to_owned(),
        span: feature.span(),
    })
}

fn gff_name(line: &str) -> Option<Entry> {
    let record = gff::parse(line).ok()??;
    if gff::is_part(&record.kind) {
        return None;
    }
    let name = record.get("Name").or_else(|| record.get("gene_name"))?;
    Some(Entry {
        name: name.to_owned(),
        span: record.span,
        contig: record.contig,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Bed,
    Gff,
}

impl Kind {
    pub fn of(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        let stem = name
            .strip_suffix(".gz")
            .or_else(|| name.strip_suffix(".bgz"))
            .unwrap_or(&name);
        match stem.rsplit_once('.')?.1 {
            "bed" => Some(Self::Bed),
            "gff" | "gff3" => Some(Self::Gff),
            _ => None,
        }
    }
}

pub fn query<R: Read + Seek>(
    kind: Kind,
    index: &Index,
    reader: &mut Bgzf<R>,
    contig: &str,
    region: Region,
) -> Result<(Region, Vec<Feature>), Error> {
    match kind {
        Kind::Bed => {
            let mut features = Vec::new();
            index.query(reader, contig, region, |line| {
                if let Some((_, feature)) = bed::parse(line).map_err(Error::Invalid)? {
                    features.push(feature);
                }
                Ok(())
            })?;
            Ok((region, features))
        }
        Kind::Gff => {
            let mut wanted = region;
            for pass in 1.. {
                let mut records = Vec::new();
                index.query(reader, contig, wanted, |line| {
                    if let Some(record) = gff::parse(line).map_err(Error::Invalid)? {
                        records.push(record);
                    }
                    Ok(())
                })?;
                let hull = records
                    .iter()
                    .fold(wanted, |hull, record| hull.hull(record.span));
                if hull == wanted || pass == PASSES {
                    return Ok((wanted, gff::assemble(&records)));
                }
                wanted = hull;
            }
            unreachable!()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use genie_core::{Feature, Region, Strand};

    use super::{Entry, Kind, names, query};
    use crate::{
        bgzf::{
            Bgzf,
            tests::{block, file},
        },
        tabix::tests::{BED, Bins, GFF, index},
    };

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn run(kind: Kind, lines: &[&str], wanted: Region) -> (Region, Vec<Feature>) {
        let text: String = lines.iter().flat_map(|line| [*line, "\n"]).collect();
        let end = (block(text.as_bytes()).len() as u64) << 16;
        let bins: Bins = &[(0, &[(0, end)])];
        let preset = match kind {
            Kind::Bed => BED,
            Kind::Gff => GFF,
        };
        let index = index(preset, &["chr1"], &[(bins, &[0])]);
        let mut reader = Bgzf::new(Cursor::new(file(&[text.as_bytes()])));
        query(kind, &index, &mut reader, "chr1", wanted).unwrap()
    }

    fn entry(name: &str, contig: &str, start: u64, end: u64) -> Entry {
        Entry {
            name: name.to_owned(),
            contig: contig.to_owned(),
            span: region(start, end),
        }
    }

    #[test]
    fn reads_names_from_bed_lines() {
        let text =
            "#chrom\nchr1\t10\t20\tgeneA\nchr1\t30\t40\nchr1\tx\t9\tbad\nchr2\t5\t9\tgeneB\r\n";
        assert_eq!(
            names(Kind::Bed, text.as_bytes()).unwrap(),
            [entry("geneA", "chr1", 10, 20), entry("geneB", "chr2", 5, 9)]
        );
    }

    #[test]
    fn reads_gene_and_transcript_names_from_gff_lines() {
        let text = [
            "##gff-version 3",
            "chr1\t.\tgene\t1001\t2000\t.\t+\t.\tID=g1;Name=ABC",
            "chr1\t.\tmRNA\t1001\t2000\t.\t+\t.\tID=t1;Parent=g1;Name=ABC-201",
            "chr1\t.\texon\t1001\t1200\t.\t+\t.\tParent=t1;Name=exon1",
            "chr1\t.\tlnc_RNA\t3001\t4000\t.\t-\t.\tID=t2;gene_name=XYZ",
            "##FASTA",
            ">chr1",
            "ACGT",
        ]
        .join("\n");
        assert_eq!(
            names(Kind::Gff, text.as_bytes()).unwrap(),
            [
                entry("ABC", "chr1", 1_000, 2_000),
                entry("ABC-201", "chr1", 1_000, 2_000),
                entry("XYZ", "chr1", 3_000, 4_000),
            ]
        );
    }

    #[test]
    fn knows_kinds_by_name() {
        assert_eq!(Kind::of("genes.bed.gz"), Some(Kind::Bed));
        assert_eq!(Kind::of("Genes.BED"), Some(Kind::Bed));
        assert_eq!(Kind::of("gencode.gff3.gz"), Some(Kind::Gff));
        assert_eq!(Kind::of("refseq.gff.bgz"), Some(Kind::Gff));
        assert_eq!(Kind::of("calls.vcf.gz"), None);
    }

    #[test]
    fn queries_bed_features_in_a_region() {
        let lines = ["chr1\t10\t20\ta", "chr1\t30\t40\tb", "chr1\t50\t60\tc"];
        let (loaded, features) = run(Kind::Bed, &lines, region(25, 45));
        assert_eq!(loaded, region(25, 45));
        assert_eq!(
            features,
            [Feature::new("b", region(30, 40), Strand::Unknown)]
        );
    }

    #[test]
    fn widens_gff_queries_to_whole_transcripts() {
        let lines = [
            "chr1\t.\tgene\t1001\t9000\t.\t+\t.\tID=g1;Name=ABC",
            "chr1\t.\tmRNA\t1001\t9000\t.\t+\t.\tID=t1;Parent=g1",
            "chr1\t.\texon\t1001\t1200\t.\t+\t.\tParent=t1",
            "chr1\t.\texon\t8801\t9000\t.\t+\t.\tParent=t1",
        ];
        let (loaded, features) = run(Kind::Gff, &lines, region(4_000, 5_000));
        assert_eq!(loaded, region(1_000, 9_000));
        let expected = Feature::new("ABC", region(1_000, 9_000), Strand::Forward)
            .with_blocks(vec![region(1_000, 1_200), region(8_800, 9_000)])
            .with_coding(None);
        assert_eq!(features, [expected]);
    }
}
