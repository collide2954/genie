//! The changes of a variant on the transcripts of the open feature tracks, for its card.

use std::cell::OnceCell;
use std::sync::Arc;

use genie_core::effect::{self, Effect};
use genie_core::{Feature, Region};
use iced::Task;

use super::{Genie, Message, Session};
use crate::card::{Card, Detail};
use crate::reference::Reference;
use crate::track::Content;

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

/// The variant of the last card with effects, the transcripts at it, and its lines. The lines
/// are empty while genie computes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Effects {
    pub(super) generation: u64,
    site: Option<Site>,
    pub(super) transcripts: Vec<Feature>,
    lines: Vec<String>,
}

impl Genie {
    /// Gives a variant card the changes on the transcripts at the variant: the lines of the
    /// last card of the same variant, or else lines from a background task.
    pub(super) fn find_effects(&mut self, panel: usize, pinned: bool) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let card = if pinned { &self.pinned } else { &self.hover };
        let Some(site) = card.as_ref().and_then(Site::of) else {
            return Task::none();
        };
        let (contig, _) = session.view(panel);
        let transcripts = transcripts(session, panel, contig, site.span);
        if self.effects.site.as_ref() == Some(&site) && self.effects.transcripts == transcripts {
            let card = if pinned {
                &mut self.pinned
            } else {
                &mut self.hover
            };
            if let Some(card) = card {
                card.effects.clone_from(&self.effects.lines);
            }
            return Task::none();
        }
        let generation = self.effects.generation + 1;
        self.effects = Effects {
            generation,
            site: Some(site.clone()),
            transcripts: transcripts.clone(),
            lines: Vec::new(),
        };
        if transcripts.is_empty() {
            return Task::none();
        }
        let reference = Arc::clone(&session.reference);
        Task::perform(
            async move { compute(&reference, contig, &site, &transcripts) },
            move |lines| Message::Effects(generation, lines),
        )
    }

    pub(super) fn effects_found(&mut self, generation: u64, lines: Vec<String>) {
        if generation != self.effects.generation {
            return;
        }
        for card in [&mut self.hover, &mut self.pinned].into_iter().flatten() {
            if Site::of(card) == self.effects.site {
                card.effects.clone_from(&lines);
            }
        }
        self.effects.lines = lines;
    }
}

/// The transcripts with a coding part at a variant, from all feature tracks of a panel.
fn transcripts(session: &Session, panel: usize, contig: usize, span: Region) -> Vec<Feature> {
    let near = Region::between(span.start().saturating_sub(NEAR), span.end() + NEAR);
    let mut found: Vec<Feature> = Vec::new();
    for track in &session.tracks {
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
                found.push(feature.clone());
            }
        }
    }
    found
}

/// Whether an allele is bases, not for example `<DEL>` or `*`.
fn plain(allele: &str) -> bool {
    !allele.is_empty() && allele.bytes().all(|base| b"ACGTNacgtn".contains(&base))
}

/// The lines of the changes of a variant on transcripts, with the bases of the reference.
fn compute(
    reference: &Reference,
    contig: usize,
    site: &Site,
    transcripts: &[Feature],
) -> Vec<String> {
    if !plain(&site.reference) {
        return Vec::new();
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
        for transcript in transcripts {
            let mut get = |region| bases.get(region);
            if let Some(effect) = effect::effect(transcript, variant, &mut get) {
                found.push((transcript.name().to_owned(), effect));
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

/// One line for each different change, protein changes first, then a line that counts the
/// others.
fn lines(found: Vec<(String, Effect)>) -> Vec<String> {
    let mut ranked: Vec<(u8, String)> = Vec::new();
    for (name, effect) in found {
        let entry = line(&name, &effect);
        if !ranked.contains(&entry) {
            ranked.push(entry);
        }
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    let more = ranked.len().saturating_sub(SHOWN);
    let mut lines: Vec<String> = ranked
        .into_iter()
        .take(SHOWN)
        .map(|(_, line)| line)
        .collect();
    match more {
        0 => {}
        1 => lines.push("+1 more transcript".to_owned()),
        _ => lines.push(format!("+{more} more transcripts")),
    }
    lines
}

/// The line of a change, for example "BRCA1 p.Arg1699Trp · c.5095C>T", and its rank: protein
/// changes, then splice sites and changes of a start or stop codon, then the other places.
fn line(name: &str, effect: &Effect) -> (u8, String) {
    let (rank, what) = match effect.place {
        None => (0, effect.protein.clone().unwrap_or_default()),
        Some(place @ ("splice site" | "start or stop codon")) => (1, place.to_owned()),
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
    (rank, line)
}

#[cfg(test)]
mod tests {
    use std::cell::OnceCell;
    use std::path::PathBuf;
    use std::slice;

    use genie_core::effect::{self, Effect};
    use genie_core::{Feature, Region, Strand};

    use super::{Bases, Site, WHOLE, compute, lines};
    use crate::reference::{self, Reference};

    fn change(coding: Option<&str>, protein: Option<&str>, place: Option<&'static str>) -> Effect {
        Effect {
            coding: coding.map(str::to_owned),
            protein: protein.map(str::to_owned),
            place,
        }
    }

    #[test]
    fn lines_put_protein_changes_first_and_count_the_others() {
        let named = |name: &str, effect| (name.to_owned(), effect);
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
        assert_eq!(
            lines(found.clone()),
            [
                "T3 p.Arg3His · c.8G>A",
                "T2 splice site · c.15+1C>T",
                "T4 splice site",
                "+1 more transcript"
            ]
        );
        let mut more = found;
        more.push(named("T5", change(None, None, Some("5′ UTR"))));
        more.push(named("", change(Some("c.9T>C"), Some("p.Arg3="), None)));
        assert_eq!(
            lines(more),
            [
                "T3 p.Arg3His · c.8G>A",
                "p.Arg3= · c.9T>C",
                "T2 splice site · c.15+1C>T",
                "+3 more transcripts"
            ]
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
        assert_eq!(
            compute(
                &reference,
                0,
                &site(&[other, "<DEL>"]),
                slice::from_ref(&transcript)
            ),
            [format!("T1 {protein} · c.5{base}>{other}")]
        );
        assert_eq!(
            compute(&reference, 0, &site(&["*"]), &[transcript]),
            [] as [String; 0]
        );
    }
}
