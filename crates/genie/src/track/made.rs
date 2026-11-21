//! Tracks that genie makes from the bases of the reference, with no file: the GC content of
//! windows, CpG islands, soft-masked repeats, tandem repeats and the cuts of restriction
//! enzymes.

use std::sync::Arc;

use genie_core::enzyme::{self, Enzyme};
use genie_core::signal::Summary;
use genie_core::{Feature, Region, Strand, composition, cpg, grouped};
use genie_formats::fields::Field;

use crate::reference::Reference;

/// The sizes of the GC windows that the track menu offers.
pub const WINDOWS: [u64; 5] = [5, 20, 100, 500, 1_000];
/// The size of the GC windows of a new track.
pub const WINDOW: u64 = 100;
/// How far past a region genie reads the bases for features, so that the islands, the repeats
/// and the fragments at the edges of the region are whole.
const MARGIN: u64 = 20_000;
const ISLAND_FIELDS: [&str; 5] = ["CpGs", "C and G", "CpG (%)", "GC (%)", "Observed/expected"];
const SITE_FIELDS: [&str; 4] = ["Site", "Cut after", "Fragment before", "Fragment after"];
const TANDEM_FIELDS: [&str; 3] = ["Motif", "Copies", "Length"];
/// The mark of a repeat that goes on past the bases that genie read.
const EDGE: &str = "edge";
const NOTE: &str = "The repeat goes on past the bases that genie read.";
/// The colors of the enzymes or the motifs of a track with more than one of them.
pub(crate) const COLORS: [[u8; 3]; 6] = [
    [0, 114, 178],
    [213, 94, 0],
    [0, 158, 115],
    [230, 159, 0],
    [204, 121, 167],
    [86, 180, 233],
];

/// What a track made from the reference shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Made {
    /// The GC content of windows of this size.
    Gc(u64),
    Islands,
    /// The soft-masked bases, as most genomes mark their repeats.
    Repeats,
    /// The perfect tandem repeats of motifs of 1 to 6 bases.
    Tandem,
    /// The cuts of restriction enzymes.
    Sites(Vec<Enzyme>),
}

impl Made {
    /// The words that name the track in a session, for example `gc 100`, `cpg islands`,
    /// `repeats` or `sites EcoRI G^AATTC`.
    pub fn key(&self) -> String {
        match self {
            Self::Gc(window) => format!("gc {window}"),
            Self::Islands => "cpg islands".to_owned(),
            Self::Repeats => "repeats".to_owned(),
            Self::Tandem => "tandem repeats".to_owned(),
            Self::Sites(enzymes) => format!("sites {}", names(enzymes).join(" ")),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        let key = key.trim();
        if let Some(window) = key.strip_prefix("gc ") {
            let window = window.trim().parse().ok().filter(|&window| window > 0);
            return window.map(Self::Gc);
        }
        match key {
            "cpg islands" => Some(Self::Islands),
            "repeats" => Some(Self::Repeats),
            "tandem repeats" => Some(Self::Tandem),
            _ => {
                let enzymes = key
                    .strip_prefix("sites ")?
                    .split_whitespace()
                    .map(Enzyme::parse)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                (!enzymes.is_empty()).then_some(Self::Sites(enzymes))
            }
        }
    }

    /// The name of a new track.
    pub fn name(&self) -> String {
        match self {
            Self::Gc(window) => format!("GC % ({} bp)", grouped(*window)),
            Self::Islands => "CpG islands".to_owned(),
            Self::Repeats => "Repeats".to_owned(),
            Self::Tandem => "Tandem repeats".to_owned(),
            Self::Sites(enzymes) if enzymes.len() <= 3 => {
                format!("{} sites", names(enzymes).join(", "))
            }
            Self::Sites(enzymes) => format!("Sites of {} enzymes", enzymes.len()),
        }
    }
}

fn names(enzymes: &[Enzyme]) -> Vec<&str> {
    enzymes.iter().map(|enzyme| enzyme.name.as_str()).collect()
}

/// The reference and what a track made from it shows.
#[derive(Debug)]
pub struct MadeFile {
    pub made: Made,
    reference: Arc<Reference>,
    contigs: Vec<String>,
}

impl MadeFile {
    pub fn new(made: Made, reference: Arc<Reference>) -> Self {
        let contigs = reference
            .assembly
            .contigs()
            .iter()
            .map(|contig| contig.name.clone())
            .collect();
        Self {
            made,
            reference,
            contigs,
        }
    }

    pub fn contigs(&self) -> &[String] {
        &self.contigs
    }

    /// True for the cuts of more than one enzyme, which have a color for each enzyme.
    pub fn has_colors(&self) -> bool {
        matches!(&self.made, Made::Sites(enzymes) if enzymes.len() > 1)
    }

    /// The bases of a region of a contig, cut to the contig, with the region and the length of
    /// the contig.
    fn read(&self, contig: &str, region: Region) -> Result<(Vec<u8>, Region, u64), String> {
        let index = self
            .reference
            .assembly
            .find(contig)
            .ok_or_else(|| format!("The reference has no contig {contig}."))?;
        let length = self.reference.assembly.contigs()[index].length;
        let region = region.clamp_to(length);
        Ok((self.reference.fetch(index, region)?, region, length))
    }

    /// The GC content of a region in summaries of `step` bases.
    pub fn summaries(
        &self,
        contig: &str,
        region: Region,
        step: u64,
    ) -> Result<Vec<Summary>, String> {
        let Made::Gc(window) = self.made else {
            return Ok(Vec::new());
        };
        let whole = Region::between(
            region.start() / window * window,
            region.end().div_ceil(window).saturating_mul(window),
        );
        let (bases, read, _) = self.read(contig, whole)?;
        Ok(composition::gc(&bases, read.start(), window, step))
    }

    /// The CpG islands, the repeats or the cuts that touch a region.
    pub fn features(&self, contig: &str, region: Region) -> Result<(Region, Vec<Feature>), String> {
        let wide = Region::between(
            region.start().saturating_sub(MARGIN),
            region.end().saturating_add(MARGIN),
        );
        let (bases, read, length) = self.read(contig, wide)?;
        let features: Vec<Feature> = match &self.made {
            Made::Gc(_) => Vec::new(),
            Made::Islands => cpg::islands(&bases, read.start())
                .iter()
                .map(island)
                .collect(),
            Made::Repeats => composition::masked(&bases, read.start())
                .into_iter()
                .map(|run| repeat(run, (read, length)))
                .collect(),
            Made::Tandem => genie_core::repeat::tandem(&bases, read.start())
                .iter()
                .map(tandem)
                .collect(),
            Made::Sites(enzymes) => sites(enzymes, &bases, (read, length)),
        };
        let touches = |span: Region| span.start() < region.end() && region.start() < span.end();
        let kept = features
            .into_iter()
            .filter(|feature| touches(feature.span()))
            .collect();
        Ok((region, kept))
    }

    /// The fields of a feature for its card and the filters.
    pub fn fields(&self, extra: &str) -> Vec<Field> {
        let labels: &[&str] = match &self.made {
            Made::Islands => &ISLAND_FIELDS,
            Made::Sites(_) => &SITE_FIELDS,
            Made::Tandem => &TANDEM_FIELDS,
            Made::Repeats if extra == EDGE => {
                return vec![Field {
                    label: "Note".to_owned(),
                    value: NOTE.to_owned(),
                }];
            }
            Made::Repeats | Made::Gc(_) => return Vec::new(),
        };
        labels
            .iter()
            .zip(extra.split('\t'))
            .filter(|(_, value)| !value.is_empty())
            .map(|(label, value)| Field {
                label: (*label).to_owned(),
                value: value.to_owned(),
            })
            .collect()
    }
}

fn island(island: &cpg::Island) -> Feature {
    let extra = format!(
        "{}\t{}\t{:.1}\t{:.1}\t{:.2}",
        island.cpgs,
        island.c + island.g,
        island.cpg_percent(),
        island.gc_percent(),
        island.ratio()
    );
    Feature::new(
        format!("CpG: {}", island.cpgs),
        island.span,
        Strand::Unknown,
    )
    .with_extra(extra)
}

/// A run of soft-masked bases. A run at an edge of the bases that genie read, which is not an
/// end of the contig, can go on past it.
fn repeat(run: Region, (read, length): (Region, u64)) -> Feature {
    let cut = (run.start() == read.start() && read.start() > 0)
        || (run.end() == read.end() && read.end() < length);
    Feature::new("Repeat", run, Strand::Unknown).with_extra(if cut { EDGE } else { "" })
}

/// The copies of the motif of a tandem repeat, with one decimal when they are not whole.
pub(crate) fn copies_text(copies: f64) -> String {
    let text = format!("{copies:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// A tandem repeat named by its motif and its copies, as (CAG)21, with the motif, the copies
/// and the length as its fields.
fn tandem(repeat: &genie_core::repeat::Repeat) -> Feature {
    let copies = copies_text(repeat.copies());
    let length = grouped(repeat.span.len());
    let extra = format!("{}\t{copies}\t{length} bp", repeat.motif);
    Feature::new(
        format!("({}){copies}", repeat.motif),
        repeat.span,
        Strand::Unknown,
    )
    .with_extra(extra)
}

/// The cuts of the enzymes, each with its site, its cut and its fragments. A fragment that
/// reaches an end of the contig has its length; a fragment that goes on past the bases that
/// genie read has the least length that it can have.
fn sites(enzymes: &[Enzyme], bases: &[u8], (read, length): (Region, u64)) -> Vec<Feature> {
    let cuts = enzyme::cuts(enzymes, bases, read.start());
    let fragments = enzyme::fragments(&cuts);
    let colored = enzymes.len() > 1;
    cuts.iter()
        .zip(fragments)
        .map(|(cut, (before, after))| {
            let place = cut.place();
            let before = match before {
                Some(size) => size.to_string(),
                None if read.start() == 0 => place.to_string(),
                None => format!("more than {}", place - read.start()),
            };
            let after = match after {
                Some(size) => size.to_string(),
                None if read.end() >= length => (length - place).to_string(),
                None => format!("more than {}", read.end() - place),
            };
            let enzyme = &enzymes[cut.enzyme];
            let at = cut.at.map_or_else(String::new, |at| at.to_string());
            let extra = format!("{}\t{at}\t{before}\t{after}", enzyme.written());
            let color = colored.then(|| COLORS[cut.enzyme % COLORS.len()]);
            Feature::new(enzyme.name.clone(), cut.site, cut.strand)
                .with_color(color)
                .with_extra(extra)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use genie_core::enzyme::Enzyme;
    use genie_core::{Feature, Region, Strand};

    use super::{Made, MadeFile};
    use crate::reference;
    use crate::scratch::Scratch;

    /// A track made from a reference in a scratch folder, which must stay while the track reads.
    fn file(name: &str, made: Made, bases: &str) -> (Scratch, MadeFile) {
        let scratch = Scratch::new(name);
        let path = scratch.file("ref.fa", format!(">chr1\n{bases}\n>chr2\nACGT\n"));
        let reference = reference::open(path, None, &mut |_| {}).unwrap();
        (scratch, MadeFile::new(made, Arc::new(reference)))
    }

    fn sites(names: &[&str]) -> Made {
        Made::Sites(
            names
                .iter()
                .map(|name| Enzyme::parse(name).unwrap())
                .collect(),
        )
    }

    #[test]
    fn keys_name_what_the_track_shows() {
        for (made, key, name) in [
            (Made::Gc(100), "gc 100", "GC % (100 bp)"),
            (Made::Gc(1_000), "gc 1000", "GC % (1,000 bp)"),
            (Made::Islands, "cpg islands", "CpG islands"),
            (Made::Repeats, "repeats", "Repeats"),
            (Made::Tandem, "tandem repeats", "Tandem repeats"),
            (sites(&["EcoRI"]), "sites EcoRI", "EcoRI sites"),
            (
                sites(&["HindIII", "g^aattc"]),
                "sites HindIII G^AATTC",
                "HindIII, G^AATTC sites",
            ),
            (
                sites(&["EcoRI", "BamHI", "NotI", "DpnII"]),
                "sites EcoRI BamHI NotI DpnII",
                "Sites of 4 enzymes",
            ),
        ] {
            assert_eq!(made.key(), key);
            assert_eq!(Made::parse(key), Some(made.clone()), "{key}");
            assert_eq!(made.name(), name);
        }
        for key in ["gc 0", "gc x", "sites", "sites EcoRX", "colors"] {
            assert_eq!(Made::parse(key), None, "{key}");
        }
    }

    #[test]
    fn gc_comes_in_windows_of_the_track() {
        let (_scratch, made) = file("made-gc", Made::Gc(4), "GGGGAAAAGCAT");
        assert_eq!(made.contigs(), ["chr1", "chr2"]);
        let summaries = made.summaries("chr1", Region::between(5, 9), 1).unwrap();
        let values: Vec<(u64, u64, f32)> = summaries
            .iter()
            .map(|summary| (summary.span.start(), summary.span.end(), summary.max))
            .collect();
        assert_eq!(values, [(4, 8, 0.0), (8, 12, 50.0)]);
        assert!(made.summaries("chrX", Region::between(0, 4), 1).is_err());
    }

    #[test]
    fn islands_have_the_fields_of_the_ucsc_track() {
        let bases = format!(
            "{}{}{}",
            "A".repeat(500),
            "CGCA".repeat(75),
            "A".repeat(500)
        );
        let (_scratch, made) = file("made-islands", Made::Islands, &bases);
        let (_, features) = made.features("chr1", Region::between(0, 600)).unwrap();
        assert_eq!(features.len(), 1);
        let island = &features[0];
        assert_eq!(island.name(), "CpG: 75");
        assert_eq!(island.span(), Region::between(500, 798));
        let fields: Vec<(String, String)> = made
            .fields(island.extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pair = |label: &str, value: &str| (label.to_owned(), value.to_owned());
        assert_eq!(
            fields,
            [
                pair("CpGs", "75"),
                pair("C and G", "224"),
                pair("CpG (%)", "50.3"),
                pair("GC (%)", "75.2"),
                pair("Observed/expected", "2.00"),
            ]
        );
        let (_, none) = made.features("chr1", Region::between(900, 1_100)).unwrap();
        assert_eq!(none.len(), 0);
    }

    #[test]
    fn repeats_are_the_lower_case_runs() {
        let bases = format!("ACGT{}ACGT", "acgt".repeat(3));
        let (_scratch, made) = file("made-repeats", Made::Repeats, &bases);
        let (_, features) = made.features("chr1", Region::between(0, 20)).unwrap();
        let spans: Vec<Region> = features.iter().map(Feature::span).collect();
        assert_eq!(spans, [Region::between(4, 16)]);
        assert_eq!(features[0].name(), "Repeat");
        assert_eq!(made.fields(features[0].extra()).len(), 0);
    }

    /// A tandem repeat has its motif and its copies in its name, and its length in its fields.
    #[test]
    fn tandem_repeats_have_their_motif_and_copies() {
        let bases = format!("ACGT{}TTAACCC{}GT", "CAG".repeat(7), "GA".repeat(7));
        let (_scratch, made) = file("made-tandem", Made::Tandem, &bases);
        let (_, features) = made.features("chr1", Region::between(0, 60)).unwrap();
        let shown: Vec<(&str, Region)> = features
            .iter()
            .map(|feature| (feature.name(), feature.span()))
            .collect();
        assert_eq!(
            shown,
            [
                ("(CAG)7", Region::between(4, 25)),
                ("(GA)7.5", Region::between(32, 47))
            ]
        );
        let fields: Vec<(String, String)> = made
            .fields(features[0].extra())
            .into_iter()
            .map(|field| (field.label, field.value))
            .collect();
        let pair = |label: &str, value: &str| (label.to_owned(), value.to_owned());
        assert_eq!(
            fields,
            [
                pair("Motif", "CAG"),
                pair("Copies", "7"),
                pair("Length", "21 bp")
            ]
        );
    }

    #[test]
    fn sites_show_their_cuts_and_fragments() {
        let bases = format!(
            "{}GAATTC{}AAGCTT{}GAATTC{}",
            "A".repeat(10),
            "A".repeat(30),
            "A".repeat(50),
            "A".repeat(10)
        );
        let (_scratch, made) = file("made-sites", sites(&["EcoRI", "HindIII"]), &bases);
        assert!(made.has_colors());
        let (_, features) = made.features("chr1", Region::between(0, 200)).unwrap();
        let shown: Vec<(&str, Region, Strand)> = features
            .iter()
            .map(|feature| (feature.name(), feature.span(), feature.strand()))
            .collect();
        assert_eq!(
            shown,
            [
                ("EcoRI", Region::between(10, 16), Strand::Unknown),
                ("HindIII", Region::between(46, 52), Strand::Unknown),
                ("EcoRI", Region::between(102, 108), Strand::Unknown),
            ]
        );
        assert_ne!(features[0].color(), features[1].color());
        assert_eq!(features[0].color(), features[2].color());
        let fields = |at: usize| -> Vec<(String, String)> {
            made.fields(features[at].extra())
                .into_iter()
                .map(|field| (field.label, field.value))
                .collect()
        };
        let pair = |label: &str, value: &str| (label.to_owned(), value.to_owned());
        assert_eq!(
            fields(1),
            [
                pair("Site", "A^AGCTT"),
                pair("Cut after", "47"),
                pair("Fragment before", "36"),
                pair("Fragment after", "56"),
            ]
        );
        assert_eq!(fields(0)[2], pair("Fragment before", "11"));
        assert_eq!(fields(2)[3], pair("Fragment after", "15"));
        let (_other, one) = file("made-site", sites(&["EcoRI"]), &bases);
        assert!(!one.has_colors());
        let (_, features) = one.features("chr1", Region::between(0, 200)).unwrap();
        assert_eq!(features[0].color(), None);
    }
}
