use std::io::{Read, Seek};

use genie_core::{Feature, Region};

use crate::{Error, bed, bgzf::Bgzf, gff, tabix::Index};

const PASSES: usize = 4;

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

    use super::{Kind, query};
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
