use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;
use iced::widget::{Column, button, column, container, pin, row, space, text};
use iced::{Element, Length, Point, Size};

use crate::app::Message;
use crate::style;
use crate::theme::tokens;
use crate::track::{Change, Toggle, Track};

pub const WIDTH: f32 = 168.0;
const FEATURES_HEIGHT: f32 = 192.0;
const READS_HEIGHT: f32 = 257.0;
const VARIANTS_HEIGHT: f32 = 159.0;
const SIGNAL_HEIGHT: f32 = 200.0;
const STYLES: [(&str, Style); 2] = [("Bars", Style::Bars), ("Line", Style::Line)];
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
    } else if track.source.has_variants() {
        VARIANTS_HEIGHT
    } else if track.source.has_signal() {
        SIGNAL_HEIGHT
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

pub fn item(label: &str, message: Message, chosen: bool, enabled: bool) -> Element<'_, Message> {
    button(text(label).size(12))
        .width(Length::Fill)
        .padding([3, 8])
        .style(if chosen { style::chosen } else { style::ghost })
        .on_press_maybe(enabled.then_some(message))
        .into()
}

pub fn card(
    content: Column<'_, Message>,
    at: Point,
    window: Size,
    height: f32,
) -> Element<'_, Message> {
    let card = container(content)
        .id("menu")
        .width(WIDTH)
        .padding(4)
        .style(style::menu);
    let x = at.x.min(window.width - WIDTH - EDGE).max(0.0);
    let y = if at.y + height > window.height {
        (at.y - height).max(0.0)
    } else {
        at.y
    };
    pin(card).x(x).y(y).into()
}

pub fn view<'a>(track: &Track, mode: Mode, place: &Place) -> Element<'a, Message> {
    let id = track.id;
    let item = |label: &'a str, change: Change, chosen: bool, enabled: bool| {
        item(label, Message::TrackChanged(id, change), chosen, enabled)
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
    let mut content =
        if track.source.has_signal() {
            let plot = track.plot;
            column![]
                .spacing(1)
                .extend(STYLES.map(|(label, style)| {
                    item(label, Change::Style(style), plot.style == style, true)
                }))
                .push(separator())
                .push(item("Shared scale", Change::Link, plot.linked, true))
                .push(separator())
        } else {
            column![column(modes).spacing(1), separator()].spacing(1)
        };
    if track.source.has_reads() {
        let options = track.options;
        content = content
            .extend(TOGGLES.map(|(label, toggle)| {
                item(label, Change::Toggle(toggle), options.is_on(toggle), true)
            }))
            .push(item(
                "Color unusual pairs",
                Change::Pairs,
                track.plot.pairs,
                true,
            ))
            .push(separator());
    } else if !track.source.has_variants() {
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
    card(content, place.at, place.window, height(track))
}

pub fn separator<'a>() -> Element<'a, Message> {
    container(
        container(space())
            .width(Length::Fill)
            .height(1)
            .style(|theme| container::background(tokens(theme).line)),
    )
    .padding([3, 4])
    .into()
}
