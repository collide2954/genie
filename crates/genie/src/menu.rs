use genie_render::features::Mode;
use genie_render::scene::Hue;
use iced::widget::{button, column, container, pin, row, space, text};
use iced::{Element, Length, Point, Size};

use crate::app::Message;
use crate::style;
use crate::theme::tokens;
use crate::track::{Change, Toggle, Track};

pub const WIDTH: f32 = 168.0;
const FEATURES_HEIGHT: f32 = 192.0;
const READS_HEIGHT: f32 = 235.0;
const EDGE: f32 = 4.0;
const MODES: [(&str, Mode); 3] = [
    ("Expanded", Mode::Expanded),
    ("Squished", Mode::Squished),
    ("Collapsed", Mode::Collapsed),
];
const TOGGLES: [(&str, Toggle); 3] = [
    ("Show duplicates", Toggle::Duplicates),
    ("Show soft clips", Toggle::Clips),
    ("Down-sample", Toggle::Downsample),
];

pub fn height(track: &Track) -> f32 {
    if track.source.has_reads() {
        READS_HEIGHT
    } else {
        FEATURES_HEIGHT
    }
}

pub struct Place {
    pub position: usize,
    pub count: usize,
    pub at: Point,
    pub window: Size,
}

pub fn view<'a>(track: &Track, mode: Mode, place: &Place) -> Element<'a, Message> {
    let id = track.id;
    let item =
        |label: &'a str, change: Change, chosen: bool, enabled: bool| -> Element<'a, Message> {
            button(text(label).size(12))
                .width(Length::Fill)
                .padding([3, 8])
                .style(if chosen { style::chosen } else { style::ghost })
                .on_press_maybe(enabled.then_some(Message::TrackChanged(id, change)))
                .into()
        };
    let modes =
        MODES.map(|(label, choice)| item(label, Change::Mode(choice), choice == mode, true));
    let current = track.hue;
    let swatches = Hue::ALL.map(|hue| {
        button(space())
            .width(18)
            .height(18)
            .padding(0)
            .style(move |theme, status| style::swatch(theme, status, hue, hue == current))
            .on_press(Message::TrackChanged(id, Change::Hue(hue)))
            .into()
    });
    let mut content = column![column(modes).spacing(1), separator()].spacing(1);
    if track.source.has_reads() {
        let options = track.options;
        content = content
            .extend(TOGGLES.map(|(label, toggle)| {
                item(label, Change::Toggle(toggle), options.is_on(toggle), true)
            }))
            .push(separator());
    } else {
        content = content
            .push(container(row(swatches).spacing(6).padding([3, 8])).id("swatches"))
            .push(separator());
    }
    let content = content.extend([
        item("Move up", Change::Up, false, place.position > 0),
        item(
            "Move down",
            Change::Down,
            false,
            place.position + 1 < place.count,
        ),
        separator(),
        item("Remove", Change::Remove, false, true),
    ]);
    let card = container(content)
        .id("menu")
        .width(WIDTH)
        .padding(4)
        .style(style::menu);
    let height = height(track);
    let x = place.at.x.min(place.window.width - WIDTH - EDGE).max(0.0);
    let y = if place.at.y + height > place.window.height {
        (place.at.y - height).max(0.0)
    } else {
        place.at.y
    };
    pin(card).x(x).y(y).into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(
        container(space())
            .width(Length::Fill)
            .height(1)
            .style(|theme| container::background(tokens(theme).line)),
    )
    .padding([3, 4])
    .into()
}
