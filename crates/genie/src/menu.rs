use genie_core::grouped;
use genie_formats::fields::{Rule, Test};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;
use iced::widget::{Column, button, column, container, pin, row, space, text, text_input};
use iced::{Alignment, Element, Length, Point, Size};

use crate::app::{Filtering, Message, Ranging, Renaming};
use crate::chrome::keys;
use crate::icon::{self, Icon};
use crate::style;
use crate::theme::tokens;
use crate::track::{Change, Grouping, Toggle, Track};

pub const WIDTH: f32 = 168.0;
const FEATURES_HEIGHT: f32 = 267.5;
const COLORED_HEIGHT: f32 = 290.5;
const READS_HEIGHT: f32 = 386.5;
const VARIANTS_HEIGHT: f32 = 234.5;
const SIGNAL_HEIGHT: f32 = 271.5;
const HEADING_HEIGHT: f32 = 21.0;
const LINK_HEIGHT: f32 = 22.6;
const SEPARATOR_HEIGHT: f32 = 8.0;
const FILTER_WIDTH: f32 = 260.0;
pub const STYLES: [(&str, Style); 2] = [("Bars", Style::Bars), ("Line", Style::Line)];
const EDGE: f32 = 4.0;
pub const MODES: [(&str, Mode); 3] = [
    ("Expanded", Mode::Expanded),
    ("Squished", Mode::Squished),
    ("Collapsed", Mode::Collapsed),
];
pub const GROUPS: [(&str, Grouping); 2] = [
    ("Group by phase", Grouping::Phase),
    ("Group by strand", Grouping::Strand),
];
pub const TOGGLES: [(&str, Toggle); 3] = [
    ("Show duplicates", Toggle::Duplicates),
    ("Show soft clips", Toggle::Clips),
    ("Down-sample", Toggle::Downsample),
];

pub fn height(track: &Track, links: usize) -> f32 {
    let extra = if links == 0 {
        0.0
    } else {
        HEADING_HEIGHT + links as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT
    };
    let filter = &track.filter;
    let rules = if filter.is_empty() {
        0.0
    } else {
        let rows = filter.rules.len() + usize::from(filter.label.is_some());
        HEADING_HEIGHT + rows as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT
    };
    LINK_HEIGHT + SEPARATOR_HEIGHT + kind_height(track) + extra + rules
}

pub fn describe(rule: &Rule) -> String {
    let label = &rule.label;
    match &rule.test {
        Test::Keep(value) => format!("{label} = {value}"),
        Test::Hide(value) => format!("{label} ≠ {value}"),
        Test::AtLeast(bound) => format!("{label} ≥ {bound}"),
        Test::AtMost(bound) => format!("{label} ≤ {bound}"),
    }
}

fn heading(words: &str) -> Element<'_, Message> {
    container(text(words).size(11).style(style::muted))
        .padding([3, 8])
        .height(HEADING_HEIGHT - 1.0)
        .into()
}

pub fn filter_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let (id, filter) = (track.id, &track.filter);
    if filter.is_empty() {
        return Vec::new();
    }
    let mut items = vec![heading("Filter")];
    items.extend(
        filter
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| rule_row(id, index, rule)),
    );
    if filter.label.is_some() {
        items.push(link(
            "Label with name".to_owned(),
            Message::Filter(id, Filtering::Label(None)),
        ));
    }
    items.push(separator());
    items
}

fn rule_row<'a>(id: u64, index: usize, rule: &Rule) -> Element<'a, Message> {
    let words = container(text(describe(rule)).size(12).wrapping(text::Wrapping::None))
        .width(Length::Fill)
        .clip(true);
    let remove = container(
        button(icon::view(Icon::Close, 10.0))
            .width(16)
            .height(16)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Filter(id, Filtering::Remove(index))),
    )
    .id(format!("rule-{index}"));
    row![words, remove]
        .spacing(4)
        .padding([3, 8])
        .align_y(Alignment::Center)
        .into()
}

fn link<'a>(label: String, message: Message) -> Element<'a, Message> {
    button(text(label).size(12).wrapping(text::Wrapping::None))
        .width(Length::Fill)
        .padding([3, 8])
        .clip(true)
        .style(style::ghost)
        .on_press(message)
        .into()
}

fn modification_choices(track: &Track) -> Vec<String> {
    let names = track.modification_names();
    let wanted = track.source.is_bedmethyl() || track.plot.look.mods;
    if wanted && names.len() > 1 {
        names
    } else {
        Vec::new()
    }
}

pub fn modification_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let shown = track.shown_modification();
    modification_choices(track)
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let chosen = shown.as_ref() == Some(&name);
            item(
                format!("Show {name}"),
                Message::TrackChanged(track.id, Change::Modification(index)),
                chosen,
                true,
            )
        })
        .collect()
}

pub fn methyl_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let mut items = modification_items(track);
    items.push(item(
        "Join CpG strands",
        Message::TrackChanged(track.id, Change::Toggle(Toggle::Join)),
        track.options.is_on(Toggle::Join),
        true,
    ));
    items.push(separator());
    items
}

fn kind_height(track: &Track) -> f32 {
    let choices = modification_choices(track).len() as f32 * LINK_HEIGHT;
    if track.source.has_reads() {
        READS_HEIGHT + choices
    } else if track.source.has_variants() {
        VARIANTS_HEIGHT
    } else if track.source.is_bedmethyl() {
        SIGNAL_HEIGHT + choices + LINK_HEIGHT + SEPARATOR_HEIGHT
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
    pub panel: usize,
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

pub fn keyed<'a>(
    label: impl text::IntoFragment<'a>,
    keys: String,
    message: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let content = row![
        text(label).size(12),
        space::horizontal(),
        text(keys).size(11).style(style::muted)
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    button(content)
        .width(Length::Fill)
        .padding([3, 8])
        .style(style::ghost)
        .on_press_maybe(enabled.then_some(message))
        .into()
}

pub fn card(
    content: Column<'_, Message>,
    at: Point,
    window: Size,
    height: f32,
) -> Element<'_, Message> {
    sized(content, at, window, (WIDTH, height))
}

pub fn sized(
    content: Column<'_, Message>,
    at: Point,
    window: Size,
    (width, height): (f32, f32),
) -> Element<'_, Message> {
    let card = container(content)
        .id("menu")
        .width(width)
        .padding(4)
        .style(style::menu);
    let x = at.x.min(window.width - width - EDGE).max(0.0);
    let y = if at.y + height > window.height {
        (at.y - height).max(0.0)
    } else {
        at.y
    };
    pin(card).x(x).y(y).into()
}

pub fn range_fields<'a>(id: u64, range: Option<&(String, String)>) -> Element<'a, Message> {
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

fn signal_items<'a>(track: &Track, range: Option<&(String, String)>) -> Column<'a, Message> {
    let (id, plot) = (track.id, track.plot);
    let item = |label: &'a str, change: Change, chosen: bool| {
        item(label, Message::TrackChanged(id, change), chosen, true)
    };
    column![]
        .spacing(1)
        .extend(STYLES.map(|(label, style)| item(label, Change::Style(style), plot.style == style)))
        .push(separator())
        .push(item("Shared scale", Change::Link, plot.linked))
        .push(range_fields(id, range))
        .push(self::item(
            "Auto range",
            Message::Range(id, Ranging::Auto),
            plot.range.is_none(),
            true,
        ))
        .push(separator())
}

fn read_items<'a>(track: &Track, place: &Place) -> Vec<Element<'a, Message>> {
    let (id, options, look) = (track.id, track.options, track.plot.look);
    let item = |label: &'a str, change: Change, chosen: bool| {
        item(label, Message::TrackChanged(id, change), chosen, true)
    };
    let mut items: Vec<Element<'a, Message>> = TOGGLES
        .map(|(label, toggle)| item(label, Change::Toggle(toggle), options.is_on(toggle)))
        .into_iter()
        .collect();
    items.push(item("Color unusual pairs", Change::Pairs, look.pairs));
    items.push(item("Color by modification", Change::Mods, look.mods));
    items.extend(modification_items(track));
    items.push(separator());
    items.push(self::item(
        format!("Sort by base at {}", grouped(place.base + 1)),
        Message::TrackChanged(id, Change::Sort(place.panel, place.base)),
        false,
        true,
    ));
    items.push(separator());
    items.extend(
        GROUPS.map(|(label, grouping)| {
            item(label, Change::Group(grouping), options.group == grouping)
        }),
    );
    items.push(separator());
    items
}

pub fn view<'a>(
    track: &Track,
    mode: Mode,
    place: &Place,
    range: Option<&(String, String)>,
    (heading, links): (&'static str, Vec<(String, Message)>),
) -> Element<'a, Message> {
    let id = track.id;
    let count = links.len();
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
    let kind = if track.source.is_bedmethyl() {
        signal_items(track, range).extend(methyl_items(track))
    } else if track.source.has_signal() {
        signal_items(track, range)
    } else {
        column![column(modes).spacing(1), separator()].spacing(1)
    };
    let settings = self::item("Track settings…", Message::TrackSettings(id), false, true);
    let mut content = column![settings, separator(), kind].spacing(1);
    if track.source.has_reads() {
        content = content.extend(read_items(track, place));
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
    if count > 0 {
        content = content
            .push(self::heading(heading))
            .extend(
                links
                    .into_iter()
                    .map(|(label, message)| link(label, message)),
            )
            .push(separator());
    }
    let content = content
        .extend(filter_items(track))
        .extend(jumps(track))
        .extend([
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
    let width = if track.filter.is_empty() {
        WIDTH
    } else {
        FILTER_WIDTH
    };
    sized(
        content,
        place.at,
        place.window,
        (width, height(track, count)),
    )
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
        keyed(
            format!("Next {noun}"),
            keys('F'),
            Message::Jump(track.id, true),
            true,
        ),
        keyed(
            format!("Previous {noun}"),
            keys('B'),
            Message::Jump(track.id, false),
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
