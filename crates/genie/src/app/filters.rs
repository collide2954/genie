use genie_formats::fields::{self, Rule, Test};
use iced::widget::{Column, button, container, text};
use iced::{Element, Length};

use crate::{menu, style};

use super::{Filtering, Genie, Message};

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
        if let Some(number) = fields::number(item) {
            rows.push(choice(
                format!("At least {item}"),
                add(Test::AtLeast(number)),
                false,
            ));
            rows.push(choice(
                format!("At most {item}"),
                add(Test::AtMost(number)),
                false,
            ));
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
        if !track.source.has_variants() {
            let chosen = track.filter.label.as_deref() == Some(picked.label.as_str());
            let label = Message::Filter(id, Filtering::Label(Some(picked.label.clone())));
            content = content.push(menu::separator()).push(choice(
                "Label with this field".to_owned(),
                label,
                chosen,
            ));
            height += SEPARATOR + ROW;
        }
        Some(menu::sized(
            content,
            picked.at,
            self.window,
            (WIDTH, height),
        ))
    }
}
