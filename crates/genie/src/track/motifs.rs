//! Tracks of the sites of DNA motifs in the reference: the motifs of a file that somebody
//! picks, scanned on the two strands at a threshold of p.

use std::fmt;
use std::sync::Arc;

use genie_core::motif::Scorer;
use genie_core::{Feature, Region, Strand};
use genie_formats::fields::Field;
use genie_formats::motifs::MotifSet;

use super::made::COLORS;
use crate::reference::Reference;

/// The thresholds of p that the track menu offers.
pub const THRESHOLDS: [f64; 3] = [0.001, 0.0001, 0.000_01];
/// The threshold of a new track, as FIMO.
pub const P: f64 = 0.0001;
/// The widest view that shows the sites of picked motifs.
const PICKED_SPAN: u64 = 1_000_000;
/// The widest view that shows the sites of all motifs of a file.
const ALL_SPAN: u64 = 5_000;
const FIELDS: [&str; 4] = ["Motif ID", "Score", "p-value", "Bases"];

/// The motifs of a file, the picked motifs (none for all of them), the threshold of p, and the
/// reference that the track scans.
pub struct MotifFile {
    pub set: Arc<MotifSet>,
    pub picks: Vec<usize>,
    pub p: f64,
    scorers: Vec<(usize, Scorer)>,
    reference: Arc<Reference>,
    contigs: Vec<String>,
}

impl fmt::Debug for MotifFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MotifFile of {} motifs, {} picked, p < {}",
            self.set.motifs.len(),
            self.picks.len(),
            self.p
        )
    }
}

impl MotifFile {
    pub fn new(set: Arc<MotifSet>, picks: Vec<usize>, p: f64, reference: Arc<Reference>) -> Self {
        let shown: Vec<usize> = if picks.is_empty() {
            (0..set.motifs.len()).collect()
        } else {
            picks.clone()
        };
        let scorers = shown
            .into_iter()
            .filter_map(|index| Some((index, Scorer::new(set.motifs.get(index)?, set.background))))
            .collect();
        let contigs = reference
            .assembly
            .contigs()
            .iter()
            .map(|contig| contig.name.clone())
            .collect();
        Self {
            set,
            picks,
            p,
            scorers,
            reference,
            contigs,
        }
    }

    pub fn contigs(&self) -> &[String] {
        &self.contigs
    }

    /// The widest view that shows sites: all motifs take much more work than a few.
    pub fn span(&self) -> u64 {
        if self.picks.is_empty() {
            ALL_SPAN
        } else {
            PICKED_SPAN
        }
    }

    /// True when the track shows more than one motif, each in its own color.
    pub fn has_colors(&self) -> bool {
        self.scorers.len() > 1
    }

    /// The name of a new track: its motifs, or the file for all of its motifs.
    pub fn name(&self, file: &str) -> String {
        let names: Vec<&str> = self
            .picks
            .iter()
            .filter_map(|&index| Some(self.set.motifs.get(index)?.name.as_str()))
            .collect();
        match names.len() {
            0 => file.to_owned(),
            1..=3 => format!("{} motifs", names.join(", ")),
            count => format!("{count} motifs"),
        }
    }

    /// The sites of the motifs that touch a region.
    pub fn features(&self, contig: &str, region: Region) -> Result<(Region, Vec<Feature>), String> {
        let index = self
            .reference
            .assembly
            .find(contig)
            .ok_or_else(|| format!("The reference has no contig {contig}."))?;
        let length = self.reference.assembly.contigs()[index].length;
        let longest = self
            .scorers
            .iter()
            .map(|(_, scorer)| scorer.len())
            .max()
            .unwrap_or(0) as u64;
        let wide = Region::between(
            region.start().saturating_sub(longest),
            region.end().saturating_add(longest),
        )
        .clamp_to(length);
        let bases = self.reference.fetch(index, wide)?;
        let colored = self.has_colors();
        let mut features = Vec::new();
        for (place, (index, scorer)) in self.scorers.iter().enumerate() {
            let motif = &self.set.motifs[*index];
            let color = colored.then(|| COLORS[place % COLORS.len()]);
            for site in scorer.scan(&bases, wide.start(), self.p) {
                if site.span.start() >= region.end() || site.span.end() <= region.start() {
                    continue;
                }
                let from = (site.span.start() - wide.start()) as usize;
                let to = (site.span.end() - wide.start()) as usize;
                let mut matched: Vec<u8> = bases[from..to].to_ascii_uppercase();
                if site.strand == Strand::Reverse {
                    matched = matched.iter().rev().map(|base| complement(*base)).collect();
                }
                let extra = format!(
                    "{}\t{:.2}\t{:.2e}\t{}",
                    motif.id,
                    site.score,
                    site.p,
                    String::from_utf8_lossy(&matched)
                );
                features.push(
                    Feature::new(motif.name.clone(), site.span, site.strand)
                        .with_color(color)
                        .with_extra(extra),
                );
            }
        }
        features.sort_by_key(|feature| feature.span().start());
        Ok((region, features))
    }

    /// The fields of a site for its card and the filters.
    pub fn fields(extra: &str) -> Vec<Field> {
        FIELDS
            .iter()
            .zip(extra.split('\t'))
            .map(|(label, value)| Field {
                label: (*label).to_owned(),
                value: value.to_owned(),
            })
            .collect()
    }
}

fn complement(base: u8) -> u8 {
    match base {
        b'A' => b'T',
        b'C' => b'G',
        b'G' => b'C',
        b'T' => b'A',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::motif::Motif;
    use genie_core::{Region, Strand};
    use genie_formats::motifs::MotifSet;

    use super::{MotifFile, P};
    use crate::reference;
    use crate::scratch::Scratch;

    fn motif(id: &str, name: &str, bases: &str) -> Motif {
        Motif {
            id: id.to_owned(),
            name: name.to_owned(),
            probabilities: bases
                .bytes()
                .map(|base| {
                    let at = b"ACGT".iter().position(|known| *known == base).unwrap();
                    let mut row = [0.01; 4];
                    row[at] = 0.97;
                    row
                })
                .collect(),
            sites: Some(100.0),
        }
    }

    fn set() -> Arc<MotifSet> {
        Arc::new(MotifSet {
            motifs: vec![
                motif("M1", "AAA", "GATTACA"),
                motif("M2", "BBB", "CCCGGGTT"),
                motif("M3", "CCC", "TGCATGCA"),
            ],
            background: [0.25; 4],
        })
    }

    fn file(name: &str, picks: Vec<usize>, bases: &str) -> (Scratch, MotifFile) {
        let scratch = Scratch::new(name);
        let path = scratch.file("ref.fa", format!(">chr1\n{bases}\n"));
        let reference = reference::open(path, None, &mut |_| {}).unwrap();
        (
            scratch,
            MotifFile::new(set(), picks, P, Arc::new(reference)),
        )
    }

    #[test]
    fn picked_motifs_show_their_sites_on_both_strands() {
        let bases = format!(
            "{}GATTACA{}TGTAATC{}",
            "T".repeat(20),
            "T".repeat(30),
            "T".repeat(20)
        );
        let (_scratch, file) = file("motif-sites", vec![0], &bases);
        assert_eq!(file.contigs(), ["chr1"]);
        assert_eq!(file.span(), 1_000_000);
        assert!(!file.has_colors());
        assert_eq!(file.name("jaspar.meme"), "AAA motifs");
        let (_, features) = file.features("chr1", Region::between(0, 84)).unwrap();
        let found: Vec<(&str, Region, Strand)> = features
            .iter()
            .map(|feature| (feature.name(), feature.span(), feature.strand()))
            .collect();
        assert_eq!(
            found,
            [
                ("AAA", Region::between(20, 27), Strand::Forward),
                ("AAA", Region::between(57, 64), Strand::Reverse),
            ]
        );
        let fields: Vec<(String, String)> = MotifFile::fields(features[1].extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        assert_eq!(fields[0], ("Motif ID".to_owned(), "M1".to_owned()));
        assert_eq!(fields[3], ("Bases".to_owned(), "GATTACA".to_owned()));
        let p: f64 = fields[2].1.parse().unwrap();
        assert!(p < P, "{p}");
        let score: f64 = fields[1].1.parse().unwrap();
        assert!(score > 10.0, "{score}");
        let (_, edge) = file.features("chr1", Region::between(25, 30)).unwrap();
        assert_eq!(edge.len(), 1);
    }

    #[test]
    fn all_motifs_show_in_small_views_with_a_color_each() {
        let bases = format!(
            "{}CCCGGGTT{}TGCATGCA{}",
            "T".repeat(10),
            "T".repeat(10),
            "T".repeat(10)
        );
        let (_scratch, file) = file("motif-all", Vec::new(), &bases);
        assert_eq!(file.span(), 5_000);
        assert!(file.has_colors());
        assert_eq!(file.name("jaspar.meme"), "jaspar.meme");
        let (_, features) = file.features("chr1", Region::between(0, 46)).unwrap();
        let names: Vec<&str> = features.iter().map(genie_core::Feature::name).collect();
        assert_eq!(names, ["BBB", "CCC", "CCC"]);
        assert_ne!(features[0].color(), features[1].color());
        let (_scratch, two) = file_two();
        assert_eq!(two.name("x.meme"), "AAA, CCC motifs");
    }

    fn file_two() -> (Scratch, MotifFile) {
        file("motif-two", vec![0, 2], "ACGT")
    }
}
