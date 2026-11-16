use std::collections::HashMap;
use std::sync::Arc;

use genie_core::association::Association;
use genie_core::interaction::Interaction;
use genie_core::segment::Segment;
use genie_core::signal::Summary;
use genie_core::variant::Variant;
use genie_core::{Assembly, Contig, Feature, Region, Strand};
use genie_formats::chain::Chains;

use super::{Content, Loaded, Options, Scope, Source, load_kept};
use crate::location::Location;

/// A chain file that moves the data of a track from its assembly to the assembly in view.
#[derive(Debug)]
pub struct Lift {
    pub location: Location,
    pub chains: Arc<Chains>,
    /// The contig of the data file of each target contig of the chains.
    files: HashMap<String, String>,
    /// The target contig of the chains of each contig of the data file.
    targets: HashMap<String, String>,
}

impl Lift {
    /// The lift of the data of a file with these contigs. The names of the contigs of the file
    /// and of the chains can differ, as `chr1` and `1`.
    pub fn new(location: Location, chains: Arc<Chains>, contigs: &[String]) -> Self {
        let names: Vec<String> = chains.targets().into_iter().map(str::to_owned).collect();
        let assembly = Assembly::new(
            names
                .iter()
                .map(|name| Contig {
                    name: name.clone(),
                    length: 0,
                })
                .collect(),
        );
        let (mut files, mut targets) = (HashMap::new(), HashMap::new());
        for contig in contigs {
            if let Some(at) = assembly.find(contig) {
                files
                    .entry(names[at].clone())
                    .or_insert_with(|| contig.clone());
                targets.insert(contig.clone(), names[at].clone());
            }
        }
        Self {
            location,
            chains,
            files,
            targets,
        }
    }

    /// The contig in view, the span and the strand change of a span of a contig of the file.
    pub fn place(&self, contig: &str, span: Region) -> Option<(&str, Region, bool)> {
        self.chains.lift(self.targets.get(contig)?, span)
    }

    /// The contigs in view that the chains reach.
    pub fn contigs(&self) -> Vec<String> {
        self.chains
            .queries()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

/// Loads a region of the assembly in view from the regions of the file that align with it, and
/// moves the records to the assembly in view.
pub fn load_lifted(
    source: &Source,
    lift: &Lift,
    (contig, region): (&str, Region),
    options: Options,
    scope: Scope<'_>,
) -> Result<Loaded, String> {
    if source.has_reads() || source.has_contacts() {
        return Err(
            "genie cannot lift read tracks and Hi-C tracks to another assembly.".to_owned(),
        );
    }
    let mut lifted = Lifted::default();
    for (target, part) in lift.chains.back(contig, region) {
        let Some(file) = lift.files.get(target) else {
            continue;
        };
        let loaded = load_kept(source, file, part, options, scope, |_| Err(String::new()))?;
        lifted.add(loaded.content, (target, contig), &lift.chains);
    }
    let loaded = lifted.into_loaded(region, scope.step);
    Ok(Loaded { options, ..loaded })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Features,
    Variants,
    Signal,
    Links,
    Segments,
    Associations,
}

/// The lifted records of the parts of a region.
#[derive(Default)]
struct Lifted {
    kind: Option<Kind>,
    features: Vec<Feature>,
    variants: Vec<Variant>,
    summaries: Vec<Summary>,
    links: Vec<Interaction>,
    segments: Vec<Segment>,
    associations: Vec<Association>,
    samples: usize,
}

impl Lifted {
    fn add(&mut self, content: Content, (target, contig): (&str, &str), chains: &Chains) {
        let place = |span: Region| {
            let (name, lifted, reverse) = chains.lift(target, span)?;
            (name == contig).then_some((lifted, reverse))
        };
        match content {
            Content::Features(annotation) => {
                self.kind = Some(Kind::Features);
                self.features
                    .extend(annotation.features.iter().filter_map(|feature| {
                        let (span, reverse) = place(feature.span())?;
                        Some(lifted_feature(feature, (span, reverse), &place))
                    }));
            }
            Content::Variants(calls) => {
                self.kind = Some(Kind::Variants);
                self.samples = calls.samples;
                self.variants
                    .extend(calls.variants.into_iter().filter_map(|variant| {
                        let (span, reverse) = place(variant.span)?;
                        Some(lifted_variant(variant, span, reverse))
                    }));
            }
            Content::Signal(signal) => {
                self.kind = Some(Kind::Signal);
                self.summaries
                    .extend(signal.summaries.into_iter().filter_map(|summary| {
                        let (span, _) = place(summary.span)?;
                        Some(Summary { span, ..summary })
                    }));
            }
            Content::Links(found) => {
                self.kind = Some(Kind::Links);
                self.links.extend(
                    found
                        .into_iter()
                        .filter_map(|link| lifted_link(link, (target, contig), chains)),
                );
            }
            Content::Segments(copies) => {
                self.kind = Some(Kind::Segments);
                self.samples = copies.samples;
                self.segments
                    .extend(copies.segments.into_iter().filter_map(|segment| {
                        let (span, _) = place(segment.span)?;
                        Some(Segment { span, ..segment })
                    }));
            }
            Content::Associations(found) => {
                self.kind = Some(Kind::Associations);
                self.associations
                    .extend(found.into_iter().filter_map(|association| {
                        let at = association.position;
                        let (span, _) = place(Region::between(at, at + 1))?;
                        Some(Association {
                            position: span.start(),
                            ..association
                        })
                    }));
            }
            Content::Reads(_) | Content::Contacts(_) => {}
        }
    }

    fn into_loaded(mut self, region: Region, step: u64) -> Loaded {
        match self.kind {
            Some(Kind::Variants) => {
                self.variants.sort_by_key(|variant| variant.span.start());
                Loaded::variants(region, self.variants, self.samples)
            }
            Some(Kind::Signal) => {
                self.summaries.sort_by_key(|summary| summary.span.start());
                Loaded::signal(region, self.summaries, step)
            }
            Some(Kind::Links) => Loaded::links(region, self.links),
            Some(Kind::Segments) => {
                self.segments
                    .sort_by_key(|segment| (segment.sample, segment.span.start()));
                Loaded::segments(region, self.segments, self.samples)
            }
            Some(Kind::Associations) => {
                self.associations
                    .sort_by_key(|association| association.position);
                Loaded::associations(region, self.associations)
            }
            Some(Kind::Features) | None => {
                self.features.sort_by_key(|feature| feature.span().start());
                Loaded::features(region, self.features)
            }
        }
    }
}

/// A feature with its span, its exons and its coding part on the assembly in view. On the
/// reverse strand of the chain, the feature changes strand.
fn lifted_feature(
    feature: &Feature,
    (span, reverse): (Region, bool),
    place: &impl Fn(Region) -> Option<(Region, bool)>,
) -> Feature {
    let strand = match (feature.strand(), reverse) {
        (Strand::Forward, true) => Strand::Reverse,
        (Strand::Reverse, true) => Strand::Forward,
        (strand, _) => strand,
    };
    let mut blocks: Vec<Region> = feature
        .blocks()
        .iter()
        .filter_map(|block| place(*block).map(|(block, _)| block))
        .collect();
    blocks.sort_by_key(|block| block.start());
    let coding = feature
        .coding()
        .and_then(|coding| place(coding).map(|(coding, _)| coding));
    Feature::new(feature.name(), span, strand)
        .with_blocks(blocks)
        .with_coding(coding)
        .with_color(feature.color())
        .with_extra(feature.extra())
}

/// A variant at its span on the assembly in view. On the reverse strand of the chain, its
/// alleles of bases change to their reverse complements.
fn lifted_variant(variant: Variant, span: Region, reverse: bool) -> Variant {
    if !reverse {
        return Variant { span, ..variant };
    }
    Variant {
        span,
        reference: complement(&variant.reference),
        alternates: variant
            .alternates
            .iter()
            .map(|allele| complement(allele))
            .collect(),
        ..variant
    }
}

/// The reverse complement of an allele of bases. Other alleles, as `<DEL>` or `*`, stay.
fn complement(allele: &str) -> String {
    if !allele.bytes().all(|base| b"ACGTNacgtn".contains(&base)) {
        return allele.to_owned();
    }
    allele
        .bytes()
        .rev()
        .map(|base| match base {
            b'A' => 'T',
            b'C' => 'G',
            b'G' => 'C',
            b'T' => 'A',
            b'a' => 't',
            b'c' => 'g',
            b'g' => 'c',
            b't' => 'a',
            other => char::from(other),
        })
        .collect()
}

/// A link with its two ends on the assembly in view.
fn lifted_link(
    link: Interaction,
    (target, contig): (&str, &str),
    chains: &Chains,
) -> Option<Interaction> {
    let (near_contig, near, _) = chains.lift(target, link.near)?;
    if near_contig != contig {
        return None;
    }
    let far_target = link.elsewhere.as_deref().unwrap_or(target);
    let (far_contig, far, _) = chains.lift(far_target, link.far)?;
    let elsewhere = (far_contig != contig).then(|| far_contig.to_owned());
    Some(Interaction {
        name: link.name,
        value: link.value,
        color: link.color,
        extra: link.extra,
        ..Interaction::new(near, far, elsewhere)
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use genie_core::variant::{Annotations, Variant};
    use genie_core::{Assembly, Contig, Feature, Region, Strand};
    use genie_formats::chain::Chains;

    use super::{Lift, complement, lifted_feature, lifted_variant};
    use crate::location::Location;
    use crate::track::{Content, Loaded, Options, Scope, load_track, names, open};

    const CHAINS: &str = "chain 1000 chr1 5000 + 0 3000 chr1 6000 + 0 3100 1\n\
        1000 0 100\n\
        2000\n\
        \n\
        chain 500 chr2 1000 + 100 600 chrB 2000 - 200 700 2\n\
        500\n";

    fn chains() -> Arc<Chains> {
        Arc::new(Chains::read(Cursor::new(CHAINS)).unwrap())
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data")
            .join(name)
    }

    /// GENE1003 of the fixture is at chr1:50,367-57,400. The first chains move chr1 10,000 bases
    /// to the right on chrA; the second put chr1 on the reverse strand of chrA.
    #[test]
    fn lifted_tracks_load_and_name_their_features_in_view() {
        let source = open(fixture("genes.gff3.gz")).unwrap();
        let view = Assembly::new(vec![Contig {
            name: "chrA".to_owned(),
            length: 300_000,
        }]);
        let scope = Scope {
            step: 1,
            detail: true,
            rules: &[],
            length: 300_000,
        };
        let gene = |loaded: Loaded| {
            let Content::Features(annotation) = loaded.content else {
                panic!("expected features");
            };
            let found = annotation
                .features
                .iter()
                .find(|feature| feature.name() == "GENE1003");
            found.cloned().unwrap()
        };
        let no_reference = |_: Region| Err(String::new());
        let at = ("chr1", Region::between(50_000, 58_000));
        let plain = load_track(&source, None, at, Options::default(), scope, no_reference);
        let plain = gene(plain.unwrap());
        for (strand, start, place) in [
            ("+", 10_000, (60_366, 67_400)),
            ("-", 0, (242_600, 249_634)),
        ] {
            let text = format!(
                "chain 1 chr1 248956422 + 0 100000 chrA 300000 {strand} {start} {} 1\n100000\n",
                start + 100_000
            );
            let chains = Arc::new(Chains::read(Cursor::new(text)).unwrap());
            let location = Location::from(std::path::PathBuf::from("a.chain"));
            let lift = Lift::new(location, chains, source.contigs());
            let wanted = Region::between(place.0 - 100, place.1 + 100);
            let loaded = load_track(
                &source,
                Some(&lift),
                ("chrA", wanted),
                Options::default(),
                scope,
                no_reference,
            );
            let moved = gene(loaded.unwrap());
            assert_eq!(moved.span(), Region::between(place.0, place.1), "{strand}");
            assert_eq!(moved.strand() == plain.strand(), strand == "+", "{strand}");
            let found = names(&source, &view, Some(&lift)).unwrap();
            let named = found.find("gene1003").unwrap();
            assert_eq!((named.contig, named.span), (0, moved.span()), "{strand}");
        }
    }

    #[test]
    fn lifts_name_the_contigs_of_both_assemblies() {
        let files = ["1".to_owned(), "2".to_owned(), "MT".to_owned()];
        let lift = Lift::new(
            Location::from(std::path::PathBuf::from("a.chain")),
            chains(),
            &files,
        );
        assert_eq!(lift.contigs(), ["chr1", "chrB"]);
        assert_eq!(lift.files.get("chr1").map(String::as_str), Some("1"));
        assert_eq!(lift.files.get("chr2").map(String::as_str), Some("2"));
        assert_eq!(
            lift.place("1", Region::between(1_500, 1_600)),
            Some(("chr1", Region::between(1_600, 1_700), false))
        );
        assert_eq!(lift.place("MT", Region::between(0, 10)), None);
    }

    #[test]
    fn features_and_variants_move_with_their_strand() {
        let chains = chains();
        let place = |span: Region| {
            let (_, lifted, reverse) = chains.lift("chr1", span)?;
            Some((lifted, reverse))
        };
        let gene =
            Feature::new("geneA", Region::between(1_500, 1_600), Strand::Forward).with_blocks(
                vec![Region::between(1_500, 1_520), Region::between(1_580, 1_600)],
            );
        let moved = lifted_feature(&gene, (Region::between(1_600, 1_700), false), &place);
        assert_eq!(moved.span(), Region::between(1_600, 1_700));
        assert_eq!(
            moved.blocks(),
            [Region::between(1_600, 1_620), Region::between(1_680, 1_700)]
        );
        let flipped = lifted_feature(&gene, (Region::between(10, 20), true), &place);
        assert_eq!(flipped.strand(), Strand::Reverse);
        let variant = Variant {
            id: "rs1".to_owned(),
            span: Region::between(100, 102),
            reference: "AC".to_owned(),
            alternates: vec!["A".to_owned(), "<DEL>".to_owned()],
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            annotations: Annotations::default(),
            structure: None,
            calls: Vec::new(),
        };
        let reversed = lifted_variant(variant, Region::between(5, 7), true);
        assert_eq!(reversed.reference, "GT");
        assert_eq!(reversed.alternates, ["T", "<DEL>"]);
        assert_eq!(complement("acgN"), "Ncgt");
    }
}
