use genie_formats::fields::{self, Rule, Test};
use iced::widget::{Column, button, container, row, text, text_input};
use iced::{Alignment, Element, Length};

use crate::track::{Change, Grouping, tag};
use crate::{font, menu, style};

use super::{Choice, Filtering, Genie, Message};

const NUMBER: &str = "field-number";
const WIDTH: f32 = 240.0;
const HEADING: f32 = 21.0;
const ROW: f32 = 22.6;
const SEPARATOR: f32 = 8.0;
const PADDING: f32 = 8.0;

fn choice<'a>(label: String, message: Message, chosen: bool) -> Element<'a, Message> {
    button(text(label).size(12).wrapping(text::Wrapping::None))
        .width(Length::Fill)
        .padding([3, 8])
        .clip(true)
        .style(if chosen { style::chosen } else { style::ghost })
        .on_press(message)
        .into()
}

impl Genie {
    pub(super) fn choose_field(&mut self, choice: Choice) {
        self.close_menus();
        self.field_draft = fields::number(&choice.item).map(|_| choice.item.clone());
        self.field = Some(choice);
    }

    pub(super) fn field_menu(&self) -> Option<Element<'_, Message>> {
        let picked = self.field.as_ref()?;
        let session = self.session.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == picked.track)?;
        let id = picked.track;
        let add = |test| {
            Message::Filter(
                id,
                Filtering::Add(Rule {
                    label: picked.label.clone(),
                    test,
                }),
            )
        };
        let item = &picked.item;
        let mut rows = vec![
            choice(
                format!("Keep only {item}"),
                add(Test::Keep(item.clone())),
                false,
            ),
            choice(format!("Hide {item}"), add(Test::Hide(item.clone())), false),
        ];
        if fields::number(item).is_some() {
            let draft = self.field_draft.as_deref().unwrap_or(item);
            let typed = fields::number(draft);
            let input = text_input("Number", draft)
                .id(NUMBER)
                .font(font::MONO)
                .size(12)
                .padding([3, 6])
                .width(Length::Fill)
                .style(style::locus)
                .on_input(move |text| Message::Filter(id, Filtering::Draft(text)));
            let bound = |words: &'static str, test: fn(f64) -> Test| {
                button(text(words).size(12).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(style::ghost)
                    .on_press_maybe(typed.map(|number| add(test(number))))
            };
            rows.push(
                row![
                    input,
                    bound("At least", Test::AtLeast),
                    bound("At most", Test::AtMost)
                ]
                .spacing(2)
                .padding([0, 4])
                .align_y(Alignment::Center)
                .into(),
            );
        }
        let mut height = HEADING + rows.len() as f32 * ROW + PADDING;
        let mut content = Column::new()
            .spacing(1)
            .push(
                container(text(picked.label.clone()).size(11).style(style::muted))
                    .padding([3, 8])
                    .height(HEADING - 1.0),
            )
            .extend(rows);
        if track.source.has_features() {
            let chosen = track.filter.label.as_deref() == Some(picked.label.as_str());
            let label = Message::Filter(id, Filtering::Label(Some(picked.label.clone())));
            content = content.push(menu::separator()).push(choice(
                "Label with this field".to_owned(),
                label,
                chosen,
            ));
            height += SEPARATOR + ROW;
        }
        if let Some(name) = tag(&picked.label).filter(|_| track.source.has_reads()) {
            let options = track.options;
            let group = Message::TrackChanged(id, Change::Group(Grouping::Tag(name)));
            let color = Message::TrackChanged(id, Change::Color(name));
            content = content
                .push(menu::separator())
                .push(choice(
                    "Group by this tag".to_owned(),
                    group,
                    options.group == Grouping::Tag(name),
                ))
                .push(choice(
                    "Color by this tag".to_owned(),
                    color,
                    options.color == Some(name),
                ));
            height += SEPARATOR + 2.0 * ROW;
        }
        Some(menu::sized(
            content,
            picked.at,
            self.window,
            (WIDTH, height),
        ))
    }
}
