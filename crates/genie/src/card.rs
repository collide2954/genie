use genie_core::{Feature, Locus, Span, Strand, grouped};
use iced::widget::{button, column, container, pin, row, text};
use iced::{Alignment, Element, Length, Point, Size};

use crate::app::Message;
use crate::icon::{self, Icon};
use crate::{font, style};

pub const WIDTH: f32 = 240.0;
const HEIGHT: f32 = 84.0;
const OFFSET_X: f32 = 12.0;
const OFFSET_Y: f32 = 16.0;
const EDGE: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub track: u64,
    pub contig: String,
    pub feature: Feature,
    pub at: Point,
}

pub fn facts(feature: &Feature) -> String {
    let mut facts = vec![format!("{} bp", grouped(feature.span().len()))];
    match feature.strand() {
        Strand::Forward => facts.push("forward strand".to_owned()),
        Strand::Reverse => facts.push("reverse strand".to_owned()),
        Strand::Unknown => {}
    }
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

pub fn view<'a>(card: &Card, pinned: bool, window: Size) -> Element<'a, Message> {
    let feature = &card.feature;
    let title = if feature.name().is_empty() {
        "Feature".to_owned()
    } else {
        feature.name().to_owned()
    };
    let locus = Locus {
        name: card.contig.clone(),
        span: Span::Region(feature.span()),
    };
    let mut heading = row![
        text(title)
            .font(font::UI_STRONG)
            .size(12)
            .width(Length::Fill)
    ]
    .align_y(Alignment::Center);
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
    let mut lines = column![
        heading,
        text(locus.to_string())
            .font(font::MONO)
            .size(11)
            .style(style::muted),
        text(facts(feature)).size(11).style(style::muted),
    ]
    .spacing(2);
    if let Some(coding) = coding(feature) {
        lines = lines.push(text(coding).size(11).style(style::muted));
    }
    let body = container(lines)
        .width(WIDTH)
        .padding([6, 8])
        .style(style::menu);
    let x = (card.at.x + OFFSET_X)
        .min(window.width - WIDTH - EDGE)
        .max(0.0);
    let y = if card.at.y + OFFSET_Y + HEIGHT > window.height {
        (card.at.y - HEIGHT - EDGE).max(0.0)
    } else {
        card.at.y + OFFSET_Y
    };
    pin(body).x(x).y(y).into()
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand};

    use super::{coding, facts};

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
}
