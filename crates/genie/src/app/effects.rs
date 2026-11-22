//! The changes of a variant on the transcripts of the open feature tracks, for its card, with
//! the protein domain of a change of a codon.

use std::cell::OnceCell;
use std::sync::Arc;

use genie_core::effect::{self, Effect};
use genie_core::{Feature, Region};
use iced::Task;

use super::{Genie, Message, Session};
use crate::card::{Card, Changes, Detail};
use crate::reference::Reference;
use crate::track::{self, Content, Source, domains};

/// The lines of transcripts in a card, before a line that counts the others.
const SHOWN: usize = 3;
/// The longest part of a contig that genie reads at once for the coding parts.
const WHOLE: u64 = 4_000_000;
/// How far a transcript can be from a variant and still have a splice site at it.
const NEAR: u64 = 2;

/// A variant on a contig, as a card shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Site {
    contig: String,
    span: Region,
    reference: String,
    alternates: Vec<String>,
}

impl Site {
    fn of(card: &Card) -> Option<Self> {
        let Detail::Variant { variant, .. } = &card.detail else {
            return None;
        };
        Some(Self {
            contig: card.contig.clone(),
            span: variant.span,
            reference: variant.reference.clone(),
            alternates: variant.alternates.clone(),
        })
    }
}

/// A protein domain at a variant: the IDs of the transcripts that it lies on, or none for a
/// domain of the UniProt track of UCSC, its words in a change line, and its rank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    transcripts: Vec<String>,
    words: String,
    rank: (usize, u64),
}

/// The variant of the last card with effects, the transcripts and the domains at it, and its
/// changes. The changes are empty while genie computes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Effects {
    pub(super) generation: u64,
    site: Option<Site>,
    pub(super) transcripts: Vec<Feature>,
    places: Vec<Place>,
    changes: Changes,
}

impl Genie {
    /// Gives a variant card the changes on the transcripts at the variant: the changes of the
    /// last card of the same variant, or else changes from a background task.
    pub(super) fn find_effects(&mut self, panel: usize, pinned: bool) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let card = if pinned { &self.pinned } else { &self.hover };
        let Some(site) = card.as_ref().and_then(Site::of) else {
            return Task::none();
        };
        let (contig, _) = session.view(panel);
        let (transcripts, ids) = transcripts(session, panel, contig, site.span);
        let places = places(session, panel, contig, site.span);
        let same = self.effects.transcripts == transcripts && self.effects.places == places;
        if self.effects.site.as_ref() == Some(&site) && same {
            let card = if pinned {
                &mut self.pinned
            } else {
                &mut self.hover
            };
            if let Some(card) = card {
                card.effects.clone_from(&self.effects.changes);
            }
            return Task::none();
        }
        let generation = self.effects.generation + 1;
        self.effects = Effects {
            generation,
            site: Some(site.clone()),
            transcripts: transcripts.clone(),
            places: places.clone(),
            changes: Changes::default(),
        };
        if transcripts.is_empty() {
            return Task::none();
        }
        let reference = Arc::clone(&session.reference);
        Task::perform(
            async move { compute(&reference, contig, &site, (&transcripts, &ids), &places) },
            move |changes| Message::Effects(generation, changes),
        )
    }

    pub(super) fn effects_found(&mut self, generation: u64, changes: Changes) {
        if generation != self.effects.generation {
            return;
        }
        for card in [&mut self.hover, &mut self.pinned].into_iter().flatten() {
            if Site::of(card) == self.effects.site {
                card.effects.clone_from(&changes);
            }
        }
        self.effects.changes = changes;
    }
}

/// The transcripts with a coding part at a variant, from the feature tracks of a panel that are
/// not domain tracks, and their IDs.
fn transcripts(
    session: &Session,
    panel: usize,
    contig: usize,
    span: Region,
) -> (Vec<Feature>, Vec<String>) {
    let near = Region::between(span.start().saturating_sub(NEAR), span.end() + NEAR);
    let mut found: Vec<Feature> = Vec::new();
    let mut ids = Vec::new();
    for track in session
        .tracks
        .iter()
        .filter(|track| !track.source.has_domains())
    {
        let Some(data) = track.slot(panel).data(contig) else {
            continue;
        };
        let Content::Features(annotation) = &data.content else {
            continue;
        };
        for feature in &annotation.features {
            if feature.span().overlaps(near)
                && effect::readable(feature)
                && !found.contains(feature)
            {
                let fields = track::feature_fields(&track.source, feature);
                ids.push(domains::transcript_id(feature, &fields));
                found.push(feature.clone());
            }
        }
    }
    (found, ids)
}

/// True for the UniProt domain track of UCSC, whose features are domains on the genome.
fn is_uniprot(source: &Source) -> bool {
    source.is_bigbed() && source.name.to_ascii_lowercase().starts_with("unipdomain")
}

/// The protein domains on the bases of a variant: of the domain tracks of a panel, and of the
/// UniProt domain track of UCSC.
fn places(session: &Session, panel: usize, contig: usize, span: Region) -> Vec<Place> {
    let bases = Region::between(span.start(), span.end().max(span.start() + 1));
    let mut found: Vec<Place> = Vec::new();
    for track in &session.tracks {
        let ours = track.source.has_domains();
        if !ours && !is_uniprot(&track.source) {
            continue;
        }
        let Some(data) = track.slot(panel).data(contig) else {
            continue;
        };
        let Content::Features(annotation) = &data.content else {
            continue;
        };
        let at = |feature: &&Feature| feature.blocks().iter().any(|block| block.overlaps(bases));
        for feature in annotation.features.iter().filter(at) {
            let fields = track::feature_fields(&track.source, feature);
            let (words, rank) = domains::words(feature, &fields);
            let transcripts = fields
                .iter()
                .find(|field| ours && field.label == domains::TRANSCRIPTS)
                .map(|field| field.value.split(", ").map(str::to_owned).collect())
                .unwrap_or_default();
            let place = Place {
                transcripts,
                words,
                rank,
            };
            if !found.contains(&place) {
                found.push(place);
            }
        }
    }
    found
}

/// The words of the best domain at a variant on a transcript: of the domains that lie on the
/// transcript, else of the domains of UCSC, which lie on no transcript of their own.
fn best(places: &[Place], id: &str) -> Option<String> {
    let pick = |keep: &dyn Fn(&Place) -> bool| {
        places
            .iter()
            .filter(|place| keep(place))
            .min_by_key(|place| place.rank)
            .map(|place| place.words.clone())
    };
    pick(&|place| place.transcripts.iter().any(|known| known == id))
        .or_else(|| pick(&|place| place.transcripts.is_empty()))
}

/// Whether an allele is bases, not for example `<DEL>` or `*`.
fn plain(allele: &str) -> bool {
    !allele.is_empty() && allele.bytes().all(|base| b"ACGTNacgtn".contains(&base))
}

/// The changes of a variant on transcripts with their IDs, with the bases of the reference, and
/// the domain of each change of a codon.
fn compute(
    reference: &Reference,
    contig: usize,
    site: &Site,
    (transcripts, ids): (&[Feature], &[String]),
    places: &[Place],
) -> Changes {
    if !plain(&site.reference) {
        return Changes::default();
    }
    let bases = Bases {
        reference,
        contig,
        hull: transcripts
            .iter()
            .filter_map(Feature::coding)
            .reduce(Region::hull),
        limit: WHOLE,
        read: OnceCell::new(),
    };
    let mut found = Vec::new();
    for alternate in site.alternates.iter().filter(|allele| plain(allele)) {
        let variant = (
            site.span.start(),
            site.reference.as_bytes(),
            alternate.as_bytes(),
        );
        for (transcript, id) in transcripts.iter().zip(ids) {
            let mut get = |region| bases.get(region);
            if let Some(effect) = effect::effect(transcript, variant, &mut get) {
                let domain = effect.place.is_none().then(|| best(places, id)).flatten();
                found.push((transcript.name().to_owned(), effect, domain));
            }
        }
    }
    lines(found)
}

/// The reference bases of the coding parts of transcripts. All of them come in one read when
/// they are close together.
struct Bases<'a> {
    reference: &'a Reference,
    contig: usize,
    hull: Option<Region>,
    limit: u64,
    /// The bases of the hull once genie read them, or `None` when the read failed.
    read: OnceCell<Option<Vec<u8>>>,
}

impl Bases<'_> {
    fn get(&self, region: Region) -> Option<Vec<u8>> {
        let inside = |hull: &Region| {
            hull.len() <= self.limit && hull.start() <= region.start() && region.end() <= hull.end()
        };
        let Some(hull) = self.hull.filter(inside) else {
            return self.reference.fetch(self.contig, region).ok();
        };
        let all = self
            .read
            .get_or_init(|| self.reference.fetch(self.contig, hull).ok())
            .as_ref()?;
        let from = usize::try_from(region.start() - hull.start()).ok()?;
        let to = usize::try_from(region.end() - hull.start()).ok()?;
        all.get(from..to).map(<[u8]>::to_vec)
    }
}

/// One line for each different change, protein changes first, and the number of the others. A
/// change can have the words of its domain.
fn lines(found: Vec<(String, Effect, Option<String>)>) -> Changes {
    let mut ranked: Vec<(u8, String)> = Vec::new();
    for (name, effect, domain) in found {
        let entry = line(&name, &effect, domain.as_deref());
        if !ranked.contains(&entry) {
            ranked.push(entry);
        }
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    Changes {
        others: ranked.len().saturating_sub(SHOWN),
        lines: ranked
            .into_iter()
            .take(SHOWN)
            .map(|(_, line)| line)
            .collect(),
    }
}

/// The line of a change, for example "BRCA1 p.Arg1699Trp · c.5095C>T", with the words of its
/// domain after it, and its rank: protein changes, then splice sites and changes of a start or
/// stop codon, then the other places.
fn line(name: &str, effect: &Effect, domain: Option<&str>) -> (u8, String) {
    let (rank, what) = match effect.place {
        None => (0, effect.protein.clone().unwrap_or_default()),
        Some(
            place @ ("splice site"
            | "start or stop codon"
            | "exon deletion"
            | "transcript deletion"),
        ) => (1, place.to_owned()),
        Some(place) => (2, place.to_owned()),
    };
    let mut line = [name, &what]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(coding) = &effect.coding {
        line.push_str(" · ");
        line.push_str(coding);
    }
    if let Some(words) = domain {
        line.push_str(" · ");
        line.push_str(words);
    }
    (rank, line)
}

#[cfg(test)]
mod tests {
    use std::cell::OnceCell;
    use std::path::PathBuf;
    use std::slice;

    use genie_core::effect::{self, Effect};
    use genie_core::{Feature, Region, Strand};

    use super::{Bases, Place, Site, WHOLE, best, compute, lines};
    use crate::card::Changes;
    use crate::reference::{self, Reference};

    fn place(transcripts: &[&str], words: &str, rank: (usize, u64)) -> Place {
        Place {
            transcripts: transcripts.iter().map(|id| (*id).to_owned()).collect(),
            words: words.to_owned(),
            rank,
        }
    }

    /// The change line of a transcript names the best domain at the variant that lies on the
    /// transcript, else the best domain of a track without transcripts, as the UniProt domains
    /// of UCSC.
    #[test]
    fn change_lines_name_the_domain_of_their_transcript() {
        let places = [
            place(&["ENST2", "ENST1"], "in G3DSA:1.10 (aa 1-9)", (4, 27)),
            place(&["ENST1"], "in Kinase (PF00001, aa 1-6)", (0, 18)),
            place(&[], "in Protein kinase", (4, 30)),
        ];
        let kinase = "in Kinase (PF00001, aa 1-6)";
        assert_eq!(best(&places, "ENST1").as_deref(), Some(kinase));
        assert_eq!(
            best(&places, "ENST2").as_deref(),
            Some("in G3DSA:1.10 (aa 1-9)")
        );
        assert_eq!(best(&places, "ENST3").as_deref(), Some("in Protein kinase"));
        assert_eq!(best(&places[..2], "ENST3"), None);
        let found = vec![(
            "T1".to_owned(),
            change(Some("c.8G>A"), Some("p.Arg3His"), None),
            Some(kinase.to_owned()),
        )];
        assert_eq!(
            lines(found).lines,
            ["T1 p.Arg3His · c.8G>A · in Kinase (PF00001, aa 1-6)"]
        );
    }

    fn change(coding: Option<&str>, protein: Option<&str>, place: Option<&'static str>) -> Effect {
        Effect {
            coding: coding.map(str::to_owned),
            protein: protein.map(str::to_owned),
            place,
        }
    }

    #[test]
    fn lines_put_protein_changes_first_and_count_the_others() {
        let named = |name: &str, effect| (name.to_owned(), effect, None);
        let found = vec![
            named("T1", change(None, None, Some("intron"))),
            named(
                "T2",
                change(Some("c.15+1C>T"), Some("p.?"), Some("splice site")),
            ),
            named("T3", change(Some("c.8G>A"), Some("p.Arg3His"), None)),
            named("T3", change(Some("c.8G>A"), Some("p.Arg3His"), None)),
            named("T4", change(None, Some("p.?"), Some("splice site"))),
        ];
        let changes = lines(found.clone());
        assert_eq!(
            changes.lines,
            [
                "T3 p.Arg3His · c.8G>A",
                "T2 splice site · c.15+1C>T",
                "T4 splice site",
            ]
        );
        assert_eq!(changes.others, 1);
        let mut more = found;
        more.push(named("T5", change(None, None, Some("5′ UTR"))));
        more.push(named("", change(Some("c.9T>C"), Some("p.Arg3="), None)));
        let changes = lines(more);
        assert_eq!(
            changes.lines,
            [
                "T3 p.Arg3His · c.8G>A",
                "p.Arg3= · c.9T>C",
                "T2 splice site · c.15+1C>T",
            ]
        );
        assert_eq!(changes.others, 3);
        let deletions = lines(vec![
            named("T6", change(None, None, Some("intron"))),
            named("T7", change(None, Some("p.?"), Some("exon deletion"))),
            named(
                "T8",
                change(None, Some("p.0?"), Some("transcript deletion")),
            ),
        ]);
        assert_eq!(
            deletions.lines,
            ["T7 exon deletion", "T8 transcript deletion", "T6 intron"]
        );
    }

    fn reference() -> Reference {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data/reference.fa.gz");
        reference::open(path, None, &mut |_| {}).unwrap()
    }

    #[test]
    fn bases_come_from_one_read_or_from_a_read_for_each_part() {
        let reference = reference();
        let part = Region::between(95_010, 95_040);
        let bases = |limit| Bases {
            reference: &reference,
            contig: 0,
            hull: Some(Region::between(95_000, 96_000)),
            limit,
            read: OnceCell::new(),
        };
        let (near, far) = (bases(WHOLE), bases(0));
        let wanted = reference.fetch(0, part).ok();
        assert_eq!(near.get(part), wanted);
        assert!(near.read.get().is_some());
        assert_eq!(far.get(part), wanted);
        assert!(far.read.get().is_none());
        assert_eq!(
            near.get(Region::between(99_000, 99_010)),
            reference.fetch(0, Region::between(99_000, 99_010)).ok()
        );
    }

    #[test]
    fn changes_come_from_the_bases_of_the_reference() {
        let reference = reference();
        let span = Region::between(95_000, 95_060);
        let transcript = Feature::new("T1", span, Strand::Forward).with_blocks(vec![span]);
        let base = reference.fetch(0, Region::between(95_004, 95_005)).unwrap()[0];
        let base = char::from(base.to_ascii_uppercase());
        let other = if base == 'A' { "G" } else { "A" };
        let site = |alternates: &[&str]| Site {
            contig: "chr1".to_owned(),
            span: Region::between(95_004, 95_005),
            reference: base.to_string(),
            alternates: alternates
                .iter()
                .map(|allele| (*allele).to_owned())
                .collect(),
        };
        let mut fetch = |region| reference.fetch(0, region).ok();
        let variant = (95_004, base.to_string(), other);
        let wanted = effect::effect(
            &transcript,
            (variant.0, variant.1.as_bytes(), variant.2.as_bytes()),
            &mut fetch,
        )
        .unwrap();
        assert_eq!(
            wanted.coding.as_deref(),
            Some(format!("c.5{base}>{other}").as_str())
        );
        let protein = wanted.protein.unwrap();
        let ids = ["T1".to_owned()];
        let found = (slice::from_ref(&transcript), &ids[..]);
        let changes = compute(&reference, 0, &site(&[other, "<DEL>"]), found, &[]);
        assert_eq!(changes.lines, [format!("T1 {protein} · c.5{base}>{other}")]);
        assert_eq!(changes.others, 0);
        let places = [place(&["T1"], "in Kinase (PF00001, aa 1-20)", (0, 60))];
        let changes = compute(&reference, 0, &site(&[other]), found, &places);
        assert_eq!(
            changes.lines,
            [format!(
                "T1 {protein} · c.5{base}>{other} · in Kinase (PF00001, aa 1-20)"
            )]
        );
        let changes = compute(&reference, 0, &site(&["*"]), found, &[]);
        assert_eq!(changes, Changes::default());
    }
}
