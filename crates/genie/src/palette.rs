use iced::widget::{Column, button, column, container, opaque, row, space, text, text_input};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{Message, Paletting};
use crate::style;

pub const LIMIT: usize = 12;
pub const INPUT: &str = "palette-input";
const WIDTH: f32 = 520.0;
const TOP: f32 = 64.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub query: String,
    pub selected: usize,
}

pub fn view<'a>(query: &str, found: &[&Action], selected: usize) -> Element<'a, Message> {
    let input = text_input("Type an action, a track or a file", query)
        .id(INPUT)
        .size(13)
        .padding([6, 8])
        .style(style::locus)
        .on_input(|query| Message::Palette(Paletting::Query(query)))
        .on_submit(Message::Palette(Paletting::Run));
    let rows = found.iter().enumerate().map(|(index, action)| {
        let line = row![
            text(action.label.clone()).size(12),
            space::horizontal(),
            text(action.detail.clone())
                .size(11)
                .style(style::muted)
                .wrapping(text::Wrapping::None)
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        button(line)
            .width(Length::Fill)
            .padding([4, 8])
            .clip(true)
            .style(if index == selected {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::Palette(Paletting::Choose(index)))
            .into()
    });
    let list: Element<'a, Message> = if found.is_empty() {
        container(text("Nothing found").size(12).style(style::muted))
            .padding([4, 8])
            .into()
    } else {
        Column::new().spacing(1).extend(rows).into()
    };
    let card = container(column![input, list].spacing(6))
        .id("palette")
        .width(WIDTH)
        .padding(8)
        .style(style::menu);
    container(opaque(card))
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding {
            top: TOP,
            ..Padding::ZERO
        })
        .into()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub label: String,
    pub detail: String,
    pub message: Message,
}

impl Action {
    pub fn new(label: impl Into<String>, detail: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            message,
        }
    }
}

pub fn found<'a>(actions: &'a [Action], query: &str) -> Vec<&'a Action> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    actions
        .iter()
        .filter(|action| {
            let haystack = format!("{} {}", action.label, action.detail).to_lowercase();
            words.iter().all(|word| haystack.contains(word.as_str()))
        })
        .take(LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Action, LIMIT, found};
    use crate::app::Message;

    fn actions() -> Vec<Action> {
        vec![
            Action::new("Group by phase", "reads.bam", Message::AppMenu),
            Action::new("Group by strand", "reads.bam", Message::AppMenu),
            Action::new("Show duplicates", "reads.bam", Message::AppMenu),
            Action::new("Zoom in", "+", Message::AppMenu),
        ]
    }

    fn labels(actions: &[Action], query: &str) -> Vec<String> {
        found(actions, query)
            .into_iter()
            .map(|action| action.label.clone())
            .collect()
    }

    #[test]
    fn every_word_must_match_in_any_order() {
        let actions = actions();
        assert_eq!(labels(&actions, "phase group"), ["Group by phase"]);
        assert_eq!(labels(&actions, "READS dup"), ["Show duplicates"]);
        assert_eq!(labels(&actions, "zoom"), ["Zoom in"]);
        assert_eq!(labels(&actions, "zzz"), Vec::<String>::new());
        assert_eq!(labels(&actions, "  ").len(), 4);
        let many: Vec<Action> = (0..40)
            .map(|index| Action::new(format!("Item {index}"), "", Message::AppMenu))
            .collect();
        assert_eq!(found(&many, "item").len(), LIMIT);
    }
}
