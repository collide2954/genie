use genie_core::alignment::{
    Alignment, FIRST, MATE_REVERSE, MATE_UNMAPPED, PAIRED, REVERSE, SECOND,
};
use genie_core::signal::Bin;
use genie_core::variant::{Call, Variant};
use genie_core::{Feature, Locus, Region, Span, Strand, grouped};
use iced::widget::{button, column, container, pin, row, text};
use iced::{Alignment as Align, Element, Length, Point, Size};

use crate::app::Message;
use crate::icon::{self, Icon};
use crate::{font, style};

pub const WIDTH: f32 = 240.0;
const BASE_HEIGHT: f32 = 52.0;
const LINE_HEIGHT: f32 = 16.0;
const OFFSET_X: f32 = 12.0;
const OFFSET_Y: f32 = 16.0;
const EDGE: f32 = 4.0;
const TIP_WIDTH: f32 = 120.0;
const ALLELE_LIMIT: usize = 12;
const ALLELE_KEEP: usize = 8;
const INFO_ENTRIES: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Feature(Feature),
    Read {
        read: Alignment,
        mate: Option<String>,
        same: bool,
    },
    Variant {
        variant: Variant,
        sample: Option<(String, Call)>,
    },
    Signal {
        span: Region,
        bin: Bin,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub track: u64,
    pub contig: String,
    pub detail: Detail,
    pub at: Point,
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
    facts.join(" · ")
}

pub fn cigar(read: &Alignment) -> String {
    let mut text = String::from("CIGAR ");
    for (op, length) in &read.cigar {
        text.push_str(&length.to_string());
        text.push(op.letter());
    }
    text
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

fn allele(text: &str) -> String {
    if text.len() <= ALLELE_LIMIT {
        return text.to_owned();
    }
    let start: String = text.chars().take(ALLELE_KEEP).collect();
    format!("{start}… ({} bp)", grouped(text.len() as u64))
}

pub fn change(variant: &Variant) -> String {
    let alternates: Vec<String> = variant
        .alternates
        .iter()
        .map(|alternate| allele(alternate))
        .collect();
    format!("{} > {}", allele(&variant.reference), alternates.join(", "))
}

pub fn variant_facts(variant: &Variant) -> String {
    let mut facts = vec![change(variant)];
    facts.extend(variant.quality.map(|quality| format!("QUAL {quality}")));
    if variant.filter != "." {
        facts.push(variant.filter.clone());
    }
    facts.join(" · ")
}

pub fn info(variant: &Variant) -> Option<String> {
    let entries: Vec<&str> = variant
        .info
        .split(';')
        .filter(|entry| !entry.is_empty() && *entry != ".")
        .take(INFO_ENTRIES)
        .collect();
    (!entries.is_empty()).then(|| entries.join(" · "))
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

fn content(detail: &Detail) -> (String, Region, Vec<String>) {
    match detail {
        Detail::Signal { span, bin } => {
            let (title, range) = signal_facts(*bin);
            (title, *span, range.into_iter().collect())
        }
        Detail::Variant { variant, sample } => {
            let title = if variant.id.is_empty() {
                change(variant)
            } else {
                variant.id.clone()
            };
            let mut lines = vec![variant_facts(variant)];
            lines.extend(info(variant));
            lines.extend(sample.as_ref().map(|(name, call)| genotype(name, *call)));
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
            (read.name.clone(), read.span(), lines)
        }
    }
}

fn locus(card: &Card, span: Region) -> Locus {
    Locus {
        name: card.contig.clone(),
        span: Span::Region(span),
    }
}

pub fn plain(card: &Card) -> String {
    let (title, span, lines) = content(&card.detail);
    [title, locus(card, span).to_string()]
        .into_iter()
        .chain(lines)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn view<'a>(card: &Card, pinned: bool, window: Size) -> Element<'a, Message> {
    let (title, span, lines) = content(&card.detail);
    let height = BASE_HEIGHT + LINE_HEIGHT * lines.len() as f32;
    let locus = locus(card, span);
    let mut heading = row![
        text(title)
            .font(font::UI_STRONG)
            .size(12)
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
    let body = column![
        heading,
        text(locus.to_string())
            .font(font::MONO)
            .size(11)
            .style(style::muted),
    ]
    .extend(
        lines
            .into_iter()
            .map(|line| text(line).size(11).style(style::muted).into()),
    )
    .spacing(2);
    let body = container(body)
        .width(WIDTH)
        .padding([6, 8])
        .style(style::menu);
    let x = (card.at.x + OFFSET_X)
        .min(window.width - WIDTH - EDGE)
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
    use genie_core::variant::{Call, Variant};
    use genie_core::{Feature, Region, Strand};
    use iced::Point;

    use super::{
        Card, Detail, change, cigar, coding, facts, genotype, info, pair_lines, plain, read_facts,
        signal_facts, variant_facts,
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
            calls: Vec::new(),
        }
    }

    #[test]
    fn variants_show_change_quality_filter_and_info() {
        let snv = site("A", &["G", "T"], Some(50.5), "DP=10;AF=0.5;MQ=60;FS=1.2");
        assert_eq!(variant_facts(&snv), "A > G, T · QUAL 50.5 · PASS");
        assert_eq!(info(&snv).as_deref(), Some("DP=10 · AF=0.5 · MQ=60"));
        let long = site(&"ACGT".repeat(10), &["A"], None, ".");
        assert_eq!(change(&long), "ACGTACGT… (40 bp) > A");
        assert_eq!(info(&long), None);
        assert_eq!(genotype("NA1", Call::Het), "NA1 · heterozygous");
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

    #[test]
    fn card_text_has_one_line_per_field() {
        let feature =
            Feature::new("geneA", region(9, 20), Strand::Forward).with_coding(Some(region(11, 15)));
        let card = Card {
            track: 1,
            contig: "chr1".to_owned(),
            detail: Detail::Feature(feature),
            at: Point::ORIGIN,
        };
        assert_eq!(
            plain(&card),
            "geneA\nchr1:10-20\n11 bp · forward strand\nCoding 12-15"
        );
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
    }
}
