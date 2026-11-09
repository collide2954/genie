use genie_core::grouped;
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;
use iced::widget::{Column, button, column, container, pin, row, space, text, text_input};
use iced::{Alignment, Element, Length, Point, Size};

use crate::app::{Message, Ranging, Renaming};
use crate::style;
use crate::theme::tokens;
use crate::track::{Change, Grouping, Toggle, Track};

pub const WIDTH: f32 = 168.0;
const FEATURES_HEIGHT: f32 = 267.5;
const COLORED_HEIGHT: f32 = 290.5;
const READS_HEIGHT: f32 = 363.5;
const VARIANTS_HEIGHT: f32 = 234.5;
const SIGNAL_HEIGHT: f32 = 271.5;
const STYLES: [(&str, Style); 2] = [("Bars", Style::Bars), ("Line", Style::Line)];
const EDGE: f32 = 4.0;
const MODES: [(&str, Mode); 3] = [
    ("Expanded", Mode::Expanded),
    ("Squished", Mode::Squished),
    ("Collapsed", Mode::Collapsed),
];
const GROUPS: [(&str, Grouping); 2] = [
    ("Group by phase", Grouping::Phase),
    ("Group by strand", Grouping::Strand),
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
    } else if track.source.has_colors() {
        COLORED_HEIGHT
    } else {
        FEATURES_HEIGHT
    }
}

pub struct Place {
    pub position: usize,
    pub count: usize,
    pub at: Point,
    pub window: Size,
    pub base: u64,
}

pub fn item<'a>(
    label: impl text::IntoFragment<'a>,
    message: Message,
    chosen: bool,
    enabled: bool,
) -> Element<'a, Message> {
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

fn range_fields<'a>(id: u64, range: Option<&(String, String)>) -> Element<'a, Message> {
    let (min, max) = range.map_or(("", ""), |(min, max)| (min.as_str(), max.as_str()));
    let field = |placeholder: &str, value: &str, edit: fn(String) -> Ranging| {
        text_input(placeholder, value)
            .size(12)
            .padding([2, 4])
            .style(style::locus)
            .on_input(move |text| Message::Range(id, edit(text)))
            .on_submit(Message::Range(id, Ranging::Fix))
    };
    container(
        row![
            field("Min", min, Ranging::Min),
            text("to").size(12).style(style::muted),
            field("Max", max, Ranging::Max),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 8])
    .into()
}

pub fn view<'a>(
    track: &Track,
    mode: Mode,
    place: &Place,
    range: Option<&(String, String)>,
) -> Element<'a, Message> {
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
                .push(range_fields(id, range))
                .push(self::item(
                    "Auto range",
                    Message::Range(id, Ranging::Auto),
                    plot.range.is_none(),
                    true,
                ))
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
            .push(separator())
            .push(self::item(
                format!("Sort by base at {}", grouped(place.base + 1)),
                Message::TrackChanged(id, Change::Sort(place.base)),
                false,
                true,
            ))
            .push(separator())
            .extend(GROUPS.map(|(label, grouping)| {
                item(
                    label,
                    Change::Group(grouping),
                    options.group == grouping,
                    true,
                )
            }))
            .push(separator());
    } else if !track.source.has_variants() {
        if track.source.has_colors() {
            content = content.push(item(
                "Use file colors",
                Change::Colors,
                track.plot.colors,
                true,
            ));
        }
        content = content
            .push(container(row(swatches).spacing(6).padding([3, 8])).id("swatches"))
            .push(separator());
    }
    let content = content.extend(jumps(track)).extend([
        self::item("Rename", Message::Rename(Renaming::Start(id)), false, true),
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

fn jumps<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    if !track.source.can_jump() {
        return Vec::new();
    }
    let noun = if track.source.has_variants() {
        "variant"
    } else {
        "feature"
    };
    vec![
        item(
            format!("Next {noun}"),
            Message::Jump(track.id, true),
            false,
            true,
        ),
        item(
            format!("Previous {noun}"),
            Message::Jump(track.id, false),
            false,
            true,
        ),
        separator(),
    ]
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
