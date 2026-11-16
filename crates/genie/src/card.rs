use genie_core::alignment::{
    Alignment, FIRST, MATE_REVERSE, MATE_UNMAPPED, PAIRED, Part, REVERSE, SECOND,
};
use genie_core::association::Association;
use genie_core::interaction::Interaction;
use genie_core::pileup::Junction;
use genie_core::segment::Segment;
use genie_core::signal::Bin;
use genie_core::variant::{Annotation, Call, Structure, Sv, Variant};
use genie_core::{Feature, Locus, Region, Span, Strand, grouped};
use genie_formats::fields::{self, Field};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, column, container, pin, row, scrollable, text};
use iced::{Alignment as Align, Element, Length, Point, Size};

use crate::app::{Choice, Message};
use crate::context::Context;
use crate::icon::{self, Icon};
use crate::{font, style};

pub const WIDTH: f32 = 240.0;
const WIDE: f32 = 320.0;
const PADDING: f32 = 8.0;
const BASE_HEIGHT: f32 = 52.0;
const LINE_HEIGHT: f32 = 16.0;
const OFFSET_X: f32 = 12.0;
const OFFSET_Y: f32 = 16.0;
const EDGE: f32 = 4.0;
const TIP_WIDTH: f32 = 120.0;
const ALLELE_LIMIT: usize = 12;
const ALLELE_KEEP: usize = 8;
const LABEL_WIDTH: f32 = 110.0;
const FIELD_GAP: f32 = 6.0;
const HOVER_FIELDS: usize = 6;
const VALUE_LIMIT: usize = 160;
const CHARACTER: f32 = 6.0;
const TITLE_CHARACTER: f32 = 7.0;
const WRAP: text::Wrapping = text::Wrapping::WordOrGlyph;
const PART: &str = "Other part";
const CIGAR_LIMIT: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Feature(Feature),
    Read {
        read: Alignment,
        mate: Option<String>,
        same: bool,
    },
    Junction(Junction),
    Methylation {
        name: String,
        position: u64,
        group: Option<String>,
        counts: Vec<(String, u32, u32)>,
    },
    Variant {
        variant: Variant,
        sample: Option<(String, Call)>,
        /// The allele frequency of each group of samples, while a field groups them.
        groups: Vec<String>,
    },
    Signal {
        span: Region,
        bin: Bin,
        /// The records that a density counts per Mb, for example "variants".
        unit: Option<&'static str>,
    },
    Match {
        pattern: String,
        span: Region,
        strand: Strand,
        bases: String,
        wrong: usize,
    },
    /// A PCR product of two primers. Its strand is forward when the forward primer is at its left.
    Product {
        forward: String,
        reverse: String,
        span: Region,
        strand: Strand,
        wrong: usize,
    },
    Link(Interaction),
    Segment {
        sample: String,
        segment: Segment,
        /// The name of the value column of the file, for example seg.mean or log2.
        column: String,
    },
    Contact(Pair),
    /// A row of GWAS summary statistics.
    Association {
        association: Association,
        /// The name of the variant in its row, for example its rsID.
        name: String,
    },
}

/// A pair of bins of a Hi-C map.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub bins: [Region; 2],
    /// The value of the contact, if the map has one.
    pub value: Option<f32>,
    /// Whether the value is a ratio of observed to expected contacts.
    pub ratio: bool,
    /// The normalization of the value, NONE for counts as they are.
    pub normalization: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub track: u64,
    pub contig: String,
    pub detail: Detail,
    pub at: Point,
    /// The fields of the sample of the card in the sample table.
    pub samples: Vec<Field>,
    pub fields: Vec<Field>,
    pub effects: Changes,
}

/// The changes of a variant on the transcripts at it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// One line for each of the first transcripts.
    pub lines: Vec<String>,
    /// The number of the other transcripts.
    pub others: usize,
}

impl Changes {
    /// The line that counts the other transcripts.
    fn others(&self) -> Option<String> {
        match self.others {
            0 => None,
            1 => Some("+1 more transcript".to_owned()),
            count => Some(format!("+{count} more transcripts")),
        }
    }

    fn all(&self) -> impl Iterator<Item = String> + '_ {
        self.lines.iter().cloned().chain(self.others())
    }
}

/// One line for each of the first annotations of a variant in its file, and the line that
/// counts the other annotations.
fn annotations(detail: &Detail) -> (Vec<String>, Option<String>) {
    let Detail::Variant { variant, .. } = detail else {
        return (Vec::new(), None);
    };
    let alleles = variant.alternates.len() > 1;
    let lines = variant
        .annotations
        .first
        .iter()
        .map(|annotation| annotation_line(annotation, alleles))
        .collect();
    let others = match variant.annotations.others {
        0 => None,
        1 => Some("+1 more annotation".to_owned()),
        count => Some(format!("+{count} more annotations")),
    };
    (lines, others)
}

/// The gene, the consequences, the impact and the changes of an annotation, for example
/// `BRCA1 · missense_variant · MODERATE · ENST00000357654.9:c.5095C>T · p.Arg1699Trp`.
fn annotation_line(annotation: &Annotation, allele: bool) -> String {
    let mut parts = Vec::new();
    if allele && !annotation.allele.is_empty() {
        parts.push(format!("Allele {}", annotation.allele));
    }
    if !annotation.gene.is_empty() {
        parts.push(annotation.gene.clone());
    }
    parts.push(annotation.consequences.join(", "));
    parts.push(annotation.impact.word().to_owned());
    let change: Vec<&str> = [annotation.transcript.as_str(), annotation.coding.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    if !change.is_empty() {
        parts.push(change.join(":"));
    }
    if !annotation.protein.is_empty() {
        parts.push(annotation.protein.clone());
    }
    parts.join(" · ")
}

fn strand(strand: Strand) -> Option<&'static str> {
    match strand {
        Strand::Forward => Some("forward strand"),
        Strand::Reverse => Some("reverse strand"),
        Strand::Unknown => None,
    }
}

pub fn facts(feature: &Feature) -> String {
    let mut facts = vec![format!("{} bp", grouped(feature.span().len()))];
    facts.extend(strand(feature.strand()).map(str::to_owned));
    let blocks = feature.blocks().len();
    if blocks > 1 {
        facts.push(format!("{blocks} exons"));
    }
    facts.join(" · ")
}

pub fn coding(feature: &Feature) -> Option<String> {
    let coding = feature
        .coding()
        .filter(|coding| *coding != feature.span())?;
    Some(format!(
        "Coding {}-{}",
        grouped(coding.start() + 1),
        grouped(coding.end())
    ))
}

pub fn read_facts(read: &Alignment) -> String {
    let length = if read.bases.is_empty() {
        read.span().len()
    } else {
        read.bases.len() as u64
    };
    let mut facts = vec![format!("{} bp", grouped(length))];
    facts.extend(strand(read.strand()).map(str::to_owned));
    facts.push(format!("MAPQ {}", read.mapq));
    if read.mapq == 0 {
        facts.push("placed in more than one spot".to_owned());
    }
    facts.join(" · ")
}

pub fn cigar(read: &Alignment) -> String {
    let mut ops = String::new();
    for (op, length) in &read.cigar {
        let part = format!("{length}{}", op.letter());
        if ops.len() + part.len() > CIGAR_LIMIT {
            return format!("CIGAR {ops}…");
        }
        ops.push_str(&part);
    }
    format!("CIGAR {ops}")
}

fn mate_line(read: &Alignment, mate: Option<&str>, same: bool) -> Option<String> {
    if !read.has(PAIRED) {
        return None;
    }
    if read.has(MATE_UNMAPPED) {
        return Some("Mate unmapped".to_owned());
    }
    let (position, name) = (read.mate?, mate?);
    let mut parts = vec![format!("Mate {name}:{}", grouped(position.start + 1))];
    if position.insert != 0 {
        parts.push(format!(
            "insert {}",
            grouped(position.insert.unsigned_abs())
        ));
    }
    if same {
        let letter = |reverse| if reverse { 'R' } else { 'F' };
        let (own, other) = (letter(read.has(REVERSE)), letter(read.has(MATE_REVERSE)));
        let (left, right) = if read.start <= position.start {
            (own, other)
        } else {
            (other, own)
        };
        parts.push(format!("{left}{right}"));
    }
    if read.has(FIRST) {
        parts.push("first".to_owned());
    } else if read.has(SECOND) {
        parts.push("second".to_owned());
    }
    Some(parts.join(" · "))
}

pub fn pair_lines(read: &Alignment, mate: Option<&str>, same: bool) -> Vec<String> {
    let mut lines: Vec<String> = mate_line(read, mate, same).into_iter().collect();
    let mut tags = Vec::new();
    if let Some(phase) = read.tags.phase {
        tags.push(format!("HP {phase}"));
    }
    if let Some(group) = &read.tags.group {
        tags.push(format!("RG {group}"));
    }
    if !tags.is_empty() {
        lines.push(tags.join(" · "));
    }
    lines
}

fn modification_line(read: &Alignment) -> Option<String> {
    let counts = read.tags.mods.counts();
    if counts.is_empty() {
        return None;
    }
    let parts: Vec<String> = counts
        .into_iter()
        .map(|(name, modified, total)| {
            format!(
                "{name} {} of {}",
                grouped(modified as u64),
                grouped(total as u64)
            )
        })
        .collect();
    Some(format!("Modified {}", parts.join(" · ")))
}

fn allele(text: &str) -> String {
    if text.len() <= ALLELE_LIMIT {
        return text.to_owned();
    }
    let start: String = text.chars().take(ALLELE_KEEP).collect();
    format!("{start}… ({} bp)", grouped(text.len() as u64))
}

/// What a structural variant does, for example "Deletion · 490 bp" or "Breakend · other end
/// chr2:321,682".
fn structure_line(structure: &Structure) -> String {
    let kind = match structure.kind {
        Sv::Deletion => "Deletion",
        Sv::Duplication => "Duplication",
        Sv::Inversion => "Inversion",
        Sv::Insertion => "Insertion",
        Sv::CopyNumber => "Copy number change",
        Sv::Breakend => "Breakend",
    };
    let mut parts = vec![kind.to_owned()];
    if let Some(length) = structure.length {
        parts.push(format!("{} bp", grouped(length)));
    }
    if let Some((contig, position)) = &structure.mate {
        parts.push(format!("other end {contig}:{}", grouped(position + 1)));
    }
    if !structure.precise {
        parts.push("imprecise".to_owned());
    }
    parts.join(" · ")
}

pub fn change(variant: &Variant) -> String {
    if let Some(structure) = &variant.structure {
        return structure_line(structure);
    }
    let alternates: Vec<String> = variant
        .alternates
        .iter()
        .map(|alternate| allele(alternate))
        .collect();
    format!("{} > {}", allele(&variant.reference), alternates.join(", "))
}

pub fn genotype(name: &str, call: Call) -> String {
    let words = match call {
        Call::Missing => "no call",
        Call::Reference => "homozygous reference",
        Call::Het => "heterozygous",
        Call::Alternate => "homozygous alternate",
    };
    format!("{name} · {words}")
}

fn amount(value: f64) -> String {
    if value != 0.0 && value.abs() < 0.001 {
        return format!("{value:.2e}");
    }
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The title of a density, for example "About 520 variants per Mb".
fn density_title(per_megabase: f64, unit: &str) -> String {
    let number = if per_megabase >= 10.0 {
        grouped(per_megabase.round() as u64)
    } else {
        amount(per_megabase)
    };
    format!("About {number} {unit} per Mb")
}

pub fn signal_facts(bin: Bin) -> (String, Option<String>) {
    if bin.max - bin.min <= f32::EPSILON * bin.max.abs().max(1.0) {
        return (format!("Value {}", amount(f64::from(bin.max))), None);
    }
    let range = format!(
        "Min {} · Max {}",
        amount(f64::from(bin.min)),
        amount(f64::from(bin.max))
    );
    (format!("Mean {}", amount(bin.mean())), Some(range))
}

fn signal_content(span: Region, bin: Bin, unit: Option<&str>) -> (String, Region, Vec<String>) {
    if let Some(unit) = unit {
        return (density_title(bin.mean(), unit), span, Vec::new());
    }
    let (title, range) = signal_facts(bin);
    (title, span, range.into_iter().collect())
}

/// The length, the strand and the mismatches of a sequence match or of a PCR product, for example
/// "6 bp · reverse strand · 1 mismatch". `exact` names a match without mismatches.
fn sequence_facts(span: Region, strand: Strand, (wrong, exact): (usize, &str)) -> String {
    let strands = match strand {
        Strand::Forward => "forward strand",
        Strand::Reverse => "reverse strand",
        Strand::Unknown => "both strands",
    };
    let fit = match wrong {
        0 => exact.to_owned(),
        1 => "1 mismatch".to_owned(),
        count => format!("{count} mismatches"),
    };
    format!("{} bp · {strands} · {fit}", grouped(span.len()))
}

/// The card of a PCR product: its size, its strand, its mismatches and its primers.
fn product_content(
    (forward, reverse): (&str, &str),
    span: Region,
    (strand, wrong): (Strand, usize),
) -> (String, Region, Vec<String>) {
    let lines = vec![
        sequence_facts(span, strand, (wrong, "exact primers")),
        format!("Forward {forward}"),
        format!("Reverse {reverse}"),
    ];
    ("PCR product".to_owned(), span, lines)
}

fn content(detail: &Detail) -> (String, Region, Vec<String>) {
    match detail {
        Detail::Signal { span, bin, unit } => signal_content(*span, *bin, *unit),
        Detail::Variant {
            variant,
            sample,
            groups,
        } => {
            let title = if variant.id.is_empty() {
                change(variant)
            } else {
                variant.id.clone()
            };
            let mut lines = vec![change(variant)];
            lines.extend(sample.as_ref().map(|(name, call)| genotype(name, *call)));
            lines.extend(groups.iter().cloned());
            (title, variant.span, lines)
        }
        Detail::Feature(feature) => {
            let title = if feature.name().is_empty() {
                "Feature".to_owned()
            } else {
                feature.name().to_owned()
            };
            let mut lines = vec![facts(feature)];
            lines.extend(coding(feature));
            (title, feature.span(), lines)
        }
        Detail::Read { read, mate, same } => {
            let mut lines = vec![read_facts(read), cigar(read)];
            lines.extend(pair_lines(read, mate.as_deref(), *same));
            lines.extend(modification_line(read));
            (read.name.clone(), read.span(), lines)
        }
        Detail::Junction(junction) => {
            let lines = vec![
                junction_facts(junction),
                format!("Intron {} bp", grouped(junction.span.len())),
            ];
            ("Splice junction".to_owned(), junction.span, lines)
        }
        Detail::Methylation {
            name,
            position,
            group,
            counts,
        } => {
            let mut lines: Vec<String> = group.iter().cloned().collect();
            for (other, modified, total) in counts {
                let reads = format!("{modified} of {total} {}", plural(*total, "read"));
                if other == name {
                    let percent = (modified * 100 + total / 2) / (*total).max(1);
                    lines.insert(0, format!("Modified {reads} · {percent} %"));
                } else {
                    lines.push(format!("{other} {reads}"));
                }
            }
            (
                name.clone(),
                Region::between(*position, position + 1),
                lines,
            )
        }
        Detail::Match {
            pattern,
            span,
            strand,
            bases,
            wrong,
        } => {
            let facts = sequence_facts(*span, *strand, (*wrong, "exact match"));
            (pattern.clone(), *span, vec![facts, bases.clone()])
        }
        Detail::Product {
            forward,
            reverse,
            span,
            strand,
            wrong,
        } => product_content((forward, reverse), *span, (*strand, *wrong)),
        Detail::Link(link) => {
            let title = if link.name.is_empty() {
                "Link".to_owned()
            } else {
                link.name.clone()
            };
            (title, link.span(), link_lines(link))
        }
        Detail::Segment {
            sample,
            segment,
            column,
        } => (sample.clone(), segment.span, segment_lines(segment, column)),
        Detail::Contact(pair) => contact_content(pair),
        Detail::Association { association, name } => association_content(association, name),
    }
}

/// The -log10 p and the p-value of a row of GWAS summary statistics.
fn association_content(association: &Association, name: &str) -> (String, Region, Vec<String>) {
    let value = f64::from(association.value);
    let p = 10_f64.powf(-value);
    let line = format!("-log10 p {} · p {p:.1e}", amount(value));
    let title = if name.is_empty() {
        "Association".to_owned()
    } else {
        name.to_owned()
    };
    let span = Region::between(association.position, association.position + 1);
    (title, span, vec![line])
}

/// The bins, the distance and the value of a pair of bins of a Hi-C map.
fn contact_content(pair: &Pair) -> (String, Region, Vec<String>) {
    let [first, second] = pair.bins;
    let place =
        |region: Region| format!("{}-{}", grouped(region.start() + 1), grouped(region.end()));
    let distance = second.start().abs_diff(first.start());
    let mut lines = vec![
        format!("Bins {} and {}", place(first), place(second)),
        format!(
            "Distance {} bp · {} bp bins",
            grouped(distance),
            grouped(first.len())
        ),
        match (pair.value, pair.ratio) {
            (None, _) => "No contacts".to_owned(),
            (Some(value), true) => format!("Observed/expected {}", amount(f64::from(value))),
            (Some(value), false) => format!("Contacts {}", amount(f64::from(value))),
        },
    ];
    let normalization = &pair.normalization;
    if normalization != genie_formats::hic::NONE && !normalization.is_empty() {
        lines.push(format!("Normalization {normalization}"));
    }
    ("Contact".to_owned(), first.hull(second), lines)
}

fn segment_lines(segment: &Segment, column: &str) -> Vec<String> {
    let mut facts = vec![format!("{} bp", grouped(segment.span.len()))];
    if let Some(markers) = segment.markers {
        facts.push(format!("{} markers", grouped(u64::from(markers))));
    }
    let value = format!("{column} {}", amount(f64::from(segment.value)));
    vec![facts.join(" · "), value]
}

fn link_lines(link: &Interaction) -> Vec<String> {
    let place =
        |region: Region| format!("{}-{}", grouped(region.start() + 1), grouped(region.end()));
    let mut lines = Vec::new();
    match (&link.elsewhere, link.distance()) {
        (Some(contig), _) => lines.push(format!("Other end {contig}:{}", place(link.far))),
        (None, Some(distance)) => {
            lines.push(format!("Ends {} and {}", place(link.near), place(link.far)));
            lines.push(format!("Distance {} bp", grouped(distance)));
        }
        (None, None) => {}
    }
    if let Some(value) = link.value {
        lines.push(format!("Value {}", amount(value)));
    }
    lines
}

fn plural(count: u32, word: &str) -> String {
    if count == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

fn junction_facts(junction: &Junction) -> String {
    let mut facts = vec![if junction.count == 1 {
        "1 read".to_owned()
    } else {
        format!("{} reads", grouped(u64::from(junction.count)))
    }];
    facts.extend(strand(junction.strand).map(str::to_owned));
    facts.join(" · ")
}

fn locus(card: &Card, span: Region) -> Locus {
    Locus {
        name: card.contig.clone(),
        span: Span::Region(span),
    }
}

pub fn plain(card: &Card) -> String {
    let (title, span, lines) = content(&card.detail);
    let fields = card
        .samples
        .iter()
        .chain(&card.fields)
        .map(|field| format!("{}: {}", field.label, field.value));
    let parts = other_parts(&card.detail)
        .iter()
        .map(|part| format!("{PART}: {}", part_words(part)));
    let (notes, others) = annotations(&card.detail);
    [title, locus(card, span).to_string()]
        .into_iter()
        .chain(lines)
        .chain(card.effects.all())
        .chain(notes)
        .chain(others)
        .chain(fields)
        .chain(parts)
        .collect::<Vec<_>>()
        .join("\n")
}

fn more(count: usize) -> String {
    format!("+{count} fields: click to see all")
}

fn short(value: &str) -> String {
    if value.chars().count() <= VALUE_LIMIT {
        return value.to_owned();
    }
    let mut short: String = value.chars().take(VALUE_LIMIT - 1).collect();
    short.push('…');
    short
}

fn wrapped(text: &str, width: f32, character: f32) -> usize {
    let per_line = ((width / character).floor() as usize).max(1);
    text.chars().count().div_ceil(per_line).max(1)
}

/// The items of a field value, each with a menu of what genie can do with it. A field of the
/// sample table is a `sample` field.
fn items<'a>(track: u64, label: &str, value: &str, sample: bool) -> Element<'a, Message> {
    let parts: Vec<String> = fields::items(value).map(str::to_owned).collect();
    if parts.is_empty() {
        return text(value.to_owned())
            .size(11)
            .wrapping(WRAP)
            .width(Length::Fill)
            .into();
    }
    let count = parts.len();
    let mut cell = row![].spacing(4).width(Length::Fill);
    for (index, item) in parts.into_iter().enumerate() {
        let shown = if index + 1 < count {
            format!("{item},")
        } else {
            item.clone()
        };
        let label = label.to_owned();
        cell = cell.push(Context::left(
            text(shown).size(11).wrapping(WRAP),
            move |at| {
                Message::Field(Choice {
                    track,
                    label: label.clone(),
                    item: item.clone(),
                    at,
                    sample,
                })
            },
        ));
    }
    cell.wrap().vertical_spacing(2).into()
}

fn other_parts(detail: &Detail) -> &[Part] {
    match detail {
        Detail::Read { read, .. } => &read.tags.parts,
        _ => &[],
    }
}

fn part_words(part: &Part) -> String {
    let strand = if part.reverse { '-' } else { '+' };
    format!(
        "{}:{} {strand} · MAPQ {}",
        part.contig,
        grouped(part.start + 1),
        part.mapq
    )
}

fn part_row<'a>(part: &Part, pinned: bool) -> Element<'a, Message> {
    let words = part_words(part);
    let words = text(words).size(11).wrapping(WRAP);
    let cell: Element<'a, Message> = if pinned {
        let locus = format!("{}:{}", part.contig, part.start + 1);
        Context::left(words, move |_| Message::Visit(locus.clone())).into()
    } else {
        words.into()
    };
    row![
        text(PART).size(11).style(style::muted).width(LABEL_WIDTH),
        cell,
    ]
    .spacing(FIELD_GAP)
    .into()
}

fn field_row<'a>(
    track: Option<u64>,
    label: String,
    value: String,
    sample: bool,
) -> Element<'a, Message> {
    let cell = match track {
        Some(track) => items(track, &label, &value, sample),
        None => text(value)
            .size(11)
            .wrapping(WRAP)
            .width(Length::Fill)
            .into(),
    };
    row![
        text(label)
            .size(11)
            .style(style::muted)
            .wrapping(WRAP)
            .width(LABEL_WIDTH),
        cell,
    ]
    .spacing(FIELD_GAP)
    .into()
}

fn heading<'a>(title: String, pinned: bool) -> Element<'a, Message> {
    let mut heading = row![
        text(title)
            .font(font::UI_STRONG)
            .size(12)
            .wrapping(WRAP)
            .width(Length::Fill)
    ]
    .align_y(Align::Center);
    if pinned {
        heading = heading.push(
            button(icon::view(Icon::Close, 10.0))
                .width(16)
                .height(16)
                .padding(0)
                .style(style::ghost)
                .on_press(Message::CardClosed),
        );
    }
    heading.into()
}

fn scrolled(body: Element<'_, Message>, height: f32) -> Element<'_, Message> {
    scrollable(body)
        .direction(Direction::Vertical(
            Scrollbar::new()
                .width(4)
                .scroller_width(4)
                .margin(0)
                .spacing(2),
        ))
        .style(style::scrollbar)
        .height(height)
        .into()
}

/// The fields of a card that show: all of them on a pinned card, and the first ones else.
/// The fields of the sample come first, and each row says if it is a field of the sample.
fn shown_fields(card: &Card, pinned: bool) -> (Vec<(String, String, bool)>, usize) {
    let all = card.samples.len() + card.fields.len();
    let shown = if pinned { all } else { all.min(HOVER_FIELDS) };
    let samples = card.samples.iter().map(|field| (field, true));
    let rows = samples
        .chain(card.fields.iter().map(|field| (field, false)))
        .take(shown)
        .map(|(field, sample)| {
            let value = if pinned {
                field.value.clone()
            } else {
                short(&field.value)
            };
            (field.label.clone(), value, sample)
        })
        .collect();
    (rows, all - shown)
}

pub fn view<'a>(card: &Card, pinned: bool, window: Size) -> Element<'a, Message> {
    let (title, span, lines) = content(&card.detail);
    let parts = other_parts(&card.detail);
    let width = if card.fields.is_empty() && card.samples.is_empty() && parts.is_empty() {
        WIDTH
    } else {
        WIDE
    };
    let (rows, hidden) = shown_fields(card, pinned);
    let (notes, others) = annotations(&card.detail);
    let value_width = WIDE - 2.0 * PADDING - LABEL_WIDTH - FIELD_GAP;
    let count = lines.len() + wrapped(&title, width - 2.0 * PADDING, TITLE_CHARACTER) - 1
        + card
            .effects
            .all()
            .chain(notes.iter().cloned())
            .chain(others.iter().cloned())
            .map(|line| wrapped(&line, width - 2.0 * PADDING, CHARACTER))
            .sum::<usize>()
        + parts.len()
        + usize::from(hidden > 0)
        + rows
            .iter()
            .map(|(label, value, _)| {
                wrapped(label, LABEL_WIDTH, CHARACTER).max(wrapped(value, value_width, CHARACTER))
            })
            .sum::<usize>();
    let room = (window.height - 2.0 * EDGE).max(BASE_HEIGHT);
    let height = BASE_HEIGHT + LINE_HEIGHT * count as f32;
    let locus = locus(card, span);
    let mut body = column![
        heading(title, pinned),
        text(locus.to_string())
            .font(font::MONO)
            .size(11)
            .style(style::muted)
            .wrapping(WRAP),
    ]
    .extend(lines.into_iter().map(|line| {
        text(line)
            .size(11)
            .style(style::muted)
            .wrapping(WRAP)
            .into()
    }))
    .extend(
        card.effects
            .lines
            .iter()
            .map(|line| text(line.clone()).size(11).wrapping(WRAP).into()),
    )
    .extend(
        card.effects
            .others()
            .map(|line| text(line).size(11).style(style::muted).into()),
    )
    .extend(
        notes
            .into_iter()
            .map(|line| text(line).size(11).wrapping(WRAP).into()),
    )
    .extend(others.map(|line| text(line).size(11).style(style::muted).into()))
    .extend(rows.into_iter().map(|(label, value, sample)| {
        field_row(pinned.then_some(card.track), label, value, sample)
    }))
    .extend(parts.iter().map(|part| part_row(part, pinned)))
    .spacing(2);
    if hidden > 0 {
        body = body.push(text(more(hidden)).size(11).style(style::muted));
    }
    let body: Element<'a, Message> = if height > room {
        scrolled(body.into(), room - 12.0)
    } else {
        body.into()
    };
    let body = container(body)
        .id("card")
        .width(width)
        .padding([6.0, PADDING])
        .style(style::menu);
    let height = height.min(room);
    let x = (card.at.x + OFFSET_X)
        .min(window.width - width - EDGE)
        .max(0.0);
    let y = if card.at.y + OFFSET_Y + height > window.height {
        (card.at.y - height - EDGE).max(0.0)
    } else {
        card.at.y + OFFSET_Y
    };
    pin(body).x(x).y(y).into()
}

pub fn tip<'a>(name: String, at: Point, window: Size) -> Element<'a, Message> {
    let body = container(text(name).size(11))
        .padding([3, 6])
        .style(style::menu);
    let (x, align) = if at.x + OFFSET_X + TIP_WIDTH + EDGE > window.width {
        (at.x - OFFSET_X - TIP_WIDTH, Align::End)
    } else {
        (at.x + OFFSET_X, Align::Start)
    };
    pin(container(body).width(TIP_WIDTH).align_x(align))
        .x(x.max(0.0))
        .y(at.y + OFFSET_Y)
        .into()
}

#[cfg(test)]
mod tests {
    use genie_core::alignment::{
        Alignment, FIRST, MATE_REVERSE, MATE_UNMAPPED, Mate, Op, PAIRED, REVERSE, SECOND, Tags,
    };
    use genie_core::signal::Bin;
    use genie_core::variant::{Annotation, Annotations, Call, Impact, Structure, Sv, Variant};
    use genie_core::{Feature, Region, Strand};
    use genie_formats::fields::Field;
    use iced::{Point, Size};
    use iced_test::{selector, simulator};

    use super::{
        Card, Changes, Detail, change, cigar, coding, facts, genotype, pair_lines, plain,
        read_facts, signal_facts, view,
    };

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    #[test]
    fn facts_give_length_strand_and_exons() {
        let transcript = Feature::new("a", region(0, 80_000), Strand::Reverse).with_blocks(vec![
            region(0, 10),
            region(100, 200),
            region(79_000, 80_000),
        ]);
        assert_eq!(facts(&transcript), "80,000 bp · reverse strand · 3 exons");
        let simple = Feature::new("b", region(5, 15), Strand::Unknown);
        assert_eq!(facts(&simple), "10 bp");
    }

    #[test]
    fn coding_shows_only_a_partial_coding_part() {
        let transcript =
            Feature::new("a", region(0, 100), Strand::Forward).with_coding(Some(region(9, 90)));
        assert_eq!(coding(&transcript).as_deref(), Some("Coding 10-90"));
        let simple = Feature::new("b", region(0, 100), Strand::Forward);
        assert_eq!(coding(&simple), None);
    }

    fn site(reference: &str, alternates: &[&str], quality: Option<f32>, info: &str) -> Variant {
        Variant {
            id: "rs7".to_owned(),
            span: region(99, 100),
            reference: reference.to_owned(),
            alternates: alternates.iter().map(|alt| (*alt).to_owned()).collect(),
            quality,
            filter: "PASS".to_owned(),
            info: info.to_owned(),
            annotations: Annotations::default(),
            structure: None,
            calls: Vec::new(),
        }
    }

    #[test]
    fn match_cards_show_the_strand_the_mismatches_and_the_bases() {
        let card = |strand, bases: &str, wrong| Card {
            track: u64::MAX,
            contig: "chr1".to_owned(),
            detail: Detail::Match {
                pattern: "GGATCC".to_owned(),
                span: region(10, 16),
                strand,
                bases: bases.to_owned(),
                wrong,
            },
            at: Point::ORIGIN,
            fields: Vec::new(),
            effects: Changes::default(),
            samples: Vec::new(),
        };
        assert_eq!(
            plain(&card(Strand::Forward, "GGATCC", 0)),
            "GGATCC\nchr1:11-16\n6 bp · forward strand · exact match\nGGATCC"
        );
        assert_eq!(
            plain(&card(Strand::Reverse, "GGAtCC", 1)),
            "GGATCC\nchr1:11-16\n6 bp · reverse strand · 1 mismatch\nGGAtCC"
        );
        assert_eq!(
            plain(&card(Strand::Unknown, "GcAtCC", 2)),
            "GGATCC\nchr1:11-16\n6 bp · both strands · 2 mismatches\nGcAtCC"
        );
    }

    #[test]
    fn product_cards_give_the_size_the_strand_and_the_primers() {
        let card = |strand, wrong| Card {
            track: u64::MAX,
            contig: "chr1".to_owned(),
            detail: Detail::Product {
                forward: "ACGTTGCAGGTC".to_owned(),
                reverse: "TTGACCGATGCA".to_owned(),
                span: region(10, 102),
                strand,
                wrong,
            },
            at: Point::ORIGIN,
            fields: Vec::new(),
            effects: Changes::default(),
            samples: Vec::new(),
        };
        let primers = "Forward ACGTTGCAGGTC\nReverse TTGACCGATGCA";
        assert_eq!(
            plain(&card(Strand::Forward, 0)),
            format!("PCR product\nchr1:11-102\n92 bp · forward strand · exact primers\n{primers}")
        );
        assert_eq!(
            plain(&card(Strand::Reverse, 2)),
            format!("PCR product\nchr1:11-102\n92 bp · reverse strand · 2 mismatches\n{primers}")
        );
    }

    #[test]
    fn variants_show_their_change() {
        let snv = site("A", &["G", "T"], Some(50.5), "DP=10;AF=0.5;MQ=60;FS=1.2");
        assert_eq!(change(&snv), "A > G, T");
        let long = site(&"ACGT".repeat(10), &["A"], None, ".");
        assert_eq!(change(&long), "ACGTACGT… (40 bp) > A");
        assert_eq!(genotype("NA1", Call::Het), "NA1 · heterozygous");
    }

    #[test]
    fn structural_variants_say_what_they_change() {
        let structure = |kind, length, mate: Option<(&str, u64)>, precise| Structure {
            kind,
            length,
            mate: mate.map(|(contig, position)| (contig.to_owned(), position)),
            around: [None, None],
            precise,
        };
        let mut deletion = site("N", &["<DEL>"], None, ".");
        deletion.structure = Some(structure(Sv::Deletion, Some(2_345), None, false));
        assert_eq!(change(&deletion), "Deletion · 2,345 bp · imprecise");
        let mut breakend = site("G", &["G]chr2:321682]"], None, ".");
        breakend.structure = Some(structure(Sv::Breakend, None, Some(("chr2", 321_681)), true));
        assert_eq!(change(&breakend), "Breakend · other end chr2:321,682");
        let mut copies = site("A", &["<CN0>"], None, ".");
        copies.structure = Some(structure(Sv::CopyNumber, None, None, true));
        assert_eq!(change(&copies), "Copy number change");
    }

    #[test]
    fn signal_cards_show_the_value_or_the_range() {
        let one = Bin {
            count: 10.0,
            min: 2.5,
            max: 2.5,
            sum: 25.0,
        };
        assert_eq!(signal_facts(one), ("Value 2.5".to_owned(), None));
        let many = Bin {
            count: 4.0,
            min: -1.0,
            max: 7.125,
            sum: 13.0,
        };
        assert_eq!(
            signal_facts(many),
            (
                "Mean 3.25".to_owned(),
                Some("Min -1 · Max 7.125".to_owned())
            )
        );
        let tiny = Bin {
            count: 1.0,
            min: 0.000_25,
            max: 0.000_25,
            sum: 0.000_25,
        };
        assert_eq!(signal_facts(tiny).0, "Value 2.50e-4");
    }

    #[test]
    fn density_cards_count_the_records_per_megabase() {
        let card = |mean: f64| {
            let detail = Detail::Signal {
                span: region(0, 2_000_000),
                bin: Bin {
                    count: 2_000_000.0,
                    min: 0.0,
                    max: 30_000.0,
                    sum: mean * 2_000_000.0,
                },
                unit: Some("variants"),
            };
            let card = Card {
                track: 1,
                contig: "chr1".to_owned(),
                detail,
                at: Point::ORIGIN,
                fields: Vec::new(),
                effects: Changes::default(),
                samples: Vec::new(),
            };
            plain(&card)
        };
        assert_eq!(
            card(12_345.4),
            "About 12,345 variants per Mb\nchr1:1-2,000,000"
        );
        assert_eq!(card(2.25), "About 2.25 variants per Mb\nchr1:1-2,000,000");
    }

    #[test]
    fn pair_lines_show_the_mate_insert_orientation_and_tags() {
        let read = Alignment {
            name: "p1".to_owned(),
            start: 100,
            flags: PAIRED | MATE_REVERSE | FIRST,
            mapq: 60,
            cigar: vec![(Op::Match, 50)],
            bases: Vec::new(),
            qualities: Vec::new(),
            mate: Some(Mate {
                contig: 0,
                start: 4_400,
                insert: 4_350,
            }),
            tags: Tags {
                phase: Some(1),
                group: Some("groupA".to_owned()),
                ..Tags::default()
            },
        };
        assert_eq!(
            pair_lines(&read, Some("chr1"), true),
            [
                "Mate chr1:4,401 · insert 4,350 · FR · first",
                "HP 1 · RG groupA"
            ]
        );
        assert_eq!(
            pair_lines(&read, Some("chr2"), false)[0],
            "Mate chr2:4,401 · insert 4,350 · first"
        );
        let outward = Alignment {
            flags: PAIRED | REVERSE | SECOND,
            tags: Tags::default(),
            ..read.clone()
        };
        assert_eq!(
            pair_lines(&outward, Some("chr1"), true),
            ["Mate chr1:4,401 · insert 4,350 · RF · second"]
        );
        let lost = Alignment {
            flags: PAIRED | MATE_UNMAPPED,
            tags: Tags::default(),
            ..read.clone()
        };
        assert_eq!(pair_lines(&lost, None, false), ["Mate unmapped"]);
        let single = Alignment {
            flags: 0,
            mate: None,
            tags: Tags::default(),
            ..read
        };
        assert_eq!(pair_lines(&single, None, false), [] as [String; 0]);
    }

    fn field(label: &str, value: &str) -> Field {
        Field {
            label: label.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn card_text_has_one_line_per_field() {
        let feature =
            Feature::new("geneA", region(9, 20), Strand::Forward).with_coding(Some(region(11, 15)));
        let card = Card {
            track: 1,
            contig: "chr1".to_owned(),
            detail: Detail::Feature(feature),
            at: Point::ORIGIN,
            fields: vec![
                field("Class", "pELS"),
                field("Highest DNase Z-score", "2.5"),
            ],
            effects: Changes::default(),
            samples: Vec::new(),
        };
        assert_eq!(
            plain(&card),
            "geneA\nchr1:10-20\n11 bp · forward strand\nCoding 12-15\nClass: pELS\nHighest DNase Z-score: 2.5"
        );
    }

    #[test]
    fn variant_cards_show_the_effects_on_transcripts() {
        let effects = [
            "BRCA1 p.Arg1699Trp · c.5095C>T",
            "BRCA1 splice site · c.5074+1G>A",
            "+2 more transcripts",
        ];
        let card = Card {
            track: 1,
            contig: "chr17".to_owned(),
            detail: Detail::Variant {
                variant: site("C", &["T"], None, "."),
                sample: None,
                groups: Vec::new(),
            },
            at: Point::new(100.0, 780.0),
            fields: vec![field("DP", "10")],
            effects: Changes {
                lines: effects[..2].iter().map(|line| (*line).to_owned()).collect(),
                others: 2,
            },
            samples: Vec::new(),
        };
        let text = plain(&card);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[2..],
            ["C > T", effects[0], effects[1], effects[2], "DP: 10"]
        );
        let window = Size::new(1280.0, 800.0);
        for pinned in [false, true] {
            let mut ui = simulator(view(&card, pinned, window));
            let edge = ui.find(selector::id("card")).unwrap().bounds();
            assert!(edge.y + edge.height <= card.at.y, "{pinned} {edge:?}");
            for line in effects {
                let shown = ui.find(line).unwrap().bounds();
                assert!(
                    shown.y + shown.height <= edge.y + edge.height,
                    "{line} {pinned}"
                );
            }
        }
        let tall = Card {
            effects: Changes {
                lines: (0..100)
                    .map(|index| format!("T{index} p.Arg3His"))
                    .collect(),
                others: 1,
            },
            at: Point::new(100.0, 100.0),
            ..card
        };
        assert!(plain(&tall).contains("T99 p.Arg3His\n+1 more transcript\nDP: 10"));
        let window = Size::new(1280.0, 400.0);
        let edge = simulator(view(&tall, true, window))
            .find(selector::id("card"))
            .unwrap()
            .bounds();
        assert!(edge.height <= 400.0 - 8.0, "{edge:?}");
    }

    fn annotation(gene: &str, consequence: &str, impact: Impact, changes: [&str; 2]) -> Annotation {
        Annotation {
            allele: "T".to_owned(),
            consequences: consequence.split('&').map(str::to_owned).collect(),
            impact,
            gene: gene.to_owned(),
            transcript: format!("{gene}-201"),
            biotype: "protein_coding".to_owned(),
            coding: changes[0].to_owned(),
            protein: changes[1].to_owned(),
        }
    }

    #[test]
    fn variant_cards_show_the_annotations_of_the_file() {
        let mut variant = site("C", &["T"], None, ".");
        variant.annotations = Annotations {
            first: vec![
                annotation(
                    "BRCA1",
                    "missense_variant",
                    Impact::Moderate,
                    ["c.5095C>T", "p.Arg1699Trp"],
                ),
                annotation(
                    "NBR2",
                    "intron_variant&non_coding_transcript_variant",
                    Impact::Modifier,
                    ["", ""],
                ),
            ],
            others: 3,
            fields: Vec::new(),
        };
        let lines = [
            "BRCA1 · missense_variant · MODERATE · BRCA1-201:c.5095C>T · p.Arg1699Trp",
            "NBR2 · intron_variant, non_coding_transcript_variant · MODIFIER · NBR2-201",
            "+3 more annotations",
        ];
        let card = Card {
            track: 1,
            contig: "chr17".to_owned(),
            detail: Detail::Variant {
                variant,
                sample: None,
                groups: Vec::new(),
            },
            at: Point::new(100.0, 780.0),
            fields: vec![field("DP", "10")],
            effects: Changes::default(),
            samples: Vec::new(),
        };
        let text = plain(&card);
        let shown: Vec<&str> = text.lines().collect();
        assert_eq!(
            shown[2..],
            ["C > T", lines[0], lines[1], lines[2], "DP: 10"]
        );
        let window = Size::new(1280.0, 800.0);
        for pinned in [false, true] {
            let mut ui = simulator(view(&card, pinned, window));
            let edge = ui.find(selector::id("card")).unwrap().bounds();
            assert!(edge.y + edge.height <= card.at.y, "{pinned} {edge:?}");
            for line in lines {
                let found = ui.find(line).unwrap().bounds();
                assert!(
                    found.y + found.height <= edge.y + edge.height,
                    "{line} {pinned}"
                );
            }
        }
        let Detail::Variant { mut variant, .. } = card.detail else {
            unreachable!();
        };
        variant.alternates.push("G".to_owned());
        variant.annotations.others = 1;
        let both = Card {
            detail: Detail::Variant {
                variant,
                sample: None,
                groups: Vec::new(),
            },
            ..card
        };
        let text = plain(&both);
        assert!(
            text.contains("\nAllele T · BRCA1 · missense_variant"),
            "{text}"
        );
        assert!(text.contains("\n+1 more annotation\n"), "{text}");
    }

    #[test]
    fn cards_with_fields_are_wider_and_scroll_when_tall() {
        let feature = Feature::new("geneA", region(9, 20), Strand::Forward);
        let mut card = Card {
            track: 1,
            contig: "chr1".to_owned(),
            detail: Detail::Feature(feature),
            at: Point::ORIGIN,
            fields: Vec::new(),
            effects: Changes::default(),
            samples: Vec::new(),
        };
        let window = Size::new(1280.0, 400.0);
        let bounds = |card: &Card, pinned: bool| {
            simulator(view(card, pinned, window))
                .find(selector::id("card"))
                .unwrap()
                .bounds()
        };
        assert!((bounds(&card, false).width - 240.0).abs() < 0.5);
        card.fields = (0..60)
            .map(|index| field(&format!("Field {index}"), "x"))
            .collect();
        let hover = bounds(&card, false);
        assert!((hover.width - 320.0).abs() < 0.5, "{hover:?}");
        let pinned = bounds(&card, true);
        assert!(pinned.height > hover.height, "{pinned:?} {hover:?}");
        assert!(pinned.height <= 400.0 - 8.0, "{pinned:?}");
    }

    #[test]
    fn long_words_wrap_inside_the_card() {
        let title = "ENST00000357654.9_BRCA1_201_protein_coding_MANE_Select_transcript_of_BRCA1";
        let label = "molecular_consequence_of_the_variant";
        let value = "NM_007294.4(BRCA1):c.673_680delinsCAGTAT";
        let card = Card {
            track: 1,
            contig: "chr17".to_owned(),
            detail: Detail::Feature(Feature::new(title, region(9, 20), Strand::Forward)),
            at: Point::new(100.0, 780.0),
            fields: vec![field(label, value)],
            effects: Changes::default(),
            samples: Vec::new(),
        };
        let window = Size::new(1280.0, 800.0);
        for pinned in [false, true] {
            let mut ui = simulator(view(&card, pinned, window));
            let edge = ui.find(selector::id("card")).unwrap().bounds();
            assert!(edge.y + edge.height <= card.at.y, "{pinned} {edge:?}");
            for words in [title, label, value] {
                let shown = ui.find(words).unwrap().bounds();
                assert!(shown.height > 20.0, "{words} {pinned} {shown:?}");
                assert!(
                    shown.x + shown.width <= edge.x + edge.width,
                    "{words} {pinned} {shown:?} {edge:?}"
                );
            }
        }
    }

    #[test]
    fn reads_show_length_strand_quality_and_cigar() {
        let read = Alignment {
            name: "read7".to_owned(),
            start: 100,
            flags: REVERSE,
            mapq: 42,
            cigar: vec![(Op::Match, 40), (Op::Deletion, 3), (Op::Match, 60)],
            bases: vec![b'A'; 100],
            qualities: Vec::new(),
            mate: None,
            tags: genie_core::alignment::Tags::default(),
        };
        assert_eq!(read_facts(&read), "100 bp · reverse strand · MAPQ 42");
        assert_eq!(cigar(&read), "CIGAR 40M3D60M");
        let repeat = Alignment { mapq: 0, ..read };
        assert_eq!(
            read_facts(&repeat),
            "100 bp · reverse strand · MAPQ 0 · placed in more than one spot"
        );
    }

    #[test]
    fn long_cigars_end_with_an_ellipsis() {
        let ops = [
            (Op::SoftClip, 30),
            (Op::Match, 221),
            (Op::Insertion, 1),
            (Op::Match, 388),
            (Op::Deletion, 1),
            (Op::Match, 20),
            (Op::Deletion, 1),
            (Op::Match, 85),
            (Op::Insertion, 1),
            (Op::Match, 3),
            (Op::Insertion, 1),
            (Op::Match, 140),
            (Op::SoftClip, 26_637),
        ];
        let read = Alignment {
            name: "long".to_owned(),
            start: 100,
            flags: 0,
            mapq: 60,
            cigar: ops.to_vec(),
            bases: Vec::new(),
            qualities: Vec::new(),
            mate: None,
            tags: genie_core::alignment::Tags::default(),
        };
        assert_eq!(cigar(&read), "CIGAR 30S221M1I388M1D20M1D85M1I3M1I…");
    }
}
