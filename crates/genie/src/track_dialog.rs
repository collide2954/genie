use genie_render::features::Mode;
use genie_render::scene::Hue;
use iced::widget::{
    Column, Row, button, center, column, container, opaque, row, space, text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::app::{Drafting, Lane, Message, Ranging};
use crate::icon::{self, Icon};
use crate::menu::{self, GROUPS, MODES, STYLES, TOGGLES};
use crate::track::{Change, Track};
use crate::{font, style};

const WIDTH: f32 = 440.0;
const LABEL: f32 = 64.0;
const SWATCH: f32 = 18.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: u64,
    pub name: String,
    pub height: String,
}

impl Draft {
    pub fn of(track: &Track) -> Self {
        Self {
            id: track.id,
            name: track.name.clone(),
            height: track
                .height
                .map(|height| format!("{height:.0}"))
                .unwrap_or_default(),
        }
    }
}

fn line<'a>(label: &'static str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        text(label).size(11).style(style::muted).width(LABEL),
        content.into()
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn choices<'a>(items: impl IntoIterator<Item = (&'static str, Message, bool)>) -> Row<'a, Message> {
    Row::new().spacing(2).extend(
        items
            .into_iter()
            .map(|(label, message, chosen)| menu::item(label, message, chosen, true)),
    )
}

fn swatches<'a>(id: u64, current: Hue) -> Element<'a, Message> {
    Row::new()
        .spacing(6)
        .extend(Hue::ALL.map(|hue| {
            button(space())
                .width(SWATCH)
                .height(SWATCH)
                .padding(0)
                .style(move |theme, status| style::swatch(theme, status, hue, hue == current))
                .on_press(Message::TrackChanged(id, Change::Hue(hue)))
                .into()
        }))
        .into()
}

fn field<'a>(
    placeholder: &str,
    value: &str,
    edit: fn(String) -> Drafting,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .size(12)
        .padding([2, 6])
        .style(style::locus)
        .on_input(move |text| Message::TrackDraft(edit(text)))
}

fn read_lines<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let (id, options, look) = (track.id, track.options, track.plot.look);
    let change = move |change| Message::TrackChanged(id, change);
    let toggles = TOGGLES
        .map(|(label, toggle)| (label, change(Change::Toggle(toggle)), options.is_on(toggle)));
    let colors = [
        ("Color unusual pairs", change(Change::Pairs), look.pairs),
        ("Color by modification", change(Change::Mods), look.mods),
    ];
    let groups = GROUPS.map(|(label, grouping)| {
        (
            label,
            change(Change::Group(grouping)),
            options.group == grouping,
        )
    });
    let mut lines = vec![
        line("Reads", choices(toggles)),
        line("Colors", choices(colors)),
    ];
    let modifications = menu::modification_items(track);
    if !modifications.is_empty() {
        lines.push(line("Show", Row::new().spacing(2).extend(modifications)));
    }
    lines.push(line("Group", choices(groups)));
    lines
}

fn signal_lines<'a>(track: &Track, range: Option<&(String, String)>) -> Vec<Element<'a, Message>> {
    let (id, plot) = (track.id, track.plot);
    let change = move |change| Message::TrackChanged(id, change);
    let styles =
        STYLES.map(|(label, style)| (label, change(Change::Style(style)), plot.style == style));
    let scale = row![
        menu::item("Shared scale", change(Change::Link), plot.linked, true),
        menu::item(
            "Auto range",
            Message::Range(id, Ranging::Auto),
            plot.range.is_none(),
            true
        ),
    ]
    .spacing(2);
    let mut lines = vec![
        line("Style", choices(styles)),
        line("Scale", scale),
        line("Range", menu::range_fields(id, range)),
    ];
    if track.source.is_bedmethyl() {
        lines.push(line(
            "Methyl",
            Column::new().spacing(1).extend(menu::methyl_items(track)),
        ));
    }
    lines
}

pub fn view<'a>(
    track: &Track,
    mode: Mode,
    draft: &Draft,
    range: Option<&(String, String)>,
) -> Element<'a, Message> {
    let id = track.id;
    let change = move |change| Message::TrackChanged(id, change);
    let close = button(icon::view(Icon::Close, 10.0))
        .width(20)
        .height(20)
        .padding(0)
        .style(style::ghost)
        .on_press(Message::MenuDismissed);
    let title = row![
        text(track.name.clone()).font(font::UI_STRONG).size(13),
        space::horizontal(),
        close
    ]
    .align_y(Alignment::Center);
    let height = row![
        field("Auto", &draft.height, Drafting::Height).width(64),
        text("px").size(11).style(style::muted),
        menu::item(
            "Fit rows",
            Message::Lane(id, Lane::Fit),
            false,
            track.height.is_some()
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut body = Column::new()
        .spacing(8)
        .push(line("Name", field("Name", &draft.name, Drafting::Name)))
        .push(line("Color", swatches(id, track.hue)));
    if !track.source.has_signal() {
        let modes =
            MODES.map(|(label, choice)| (label, change(Change::Mode(choice)), choice == mode));
        body = body.push(line("Rows", choices(modes)));
    }
    body = body.push(line("Height", height));
    if track.source.has_reads() {
        body = body.extend(read_lines(track));
    }
    if track.source.has_signal() {
        body = body.extend(signal_lines(track, range));
    }
    if track.source.has_colors() {
        let colors = menu::item(
            "Use file colors",
            change(Change::Colors),
            track.plot.colors,
            true,
        );
        body = body.push(line("File", colors));
    }
    let filter = menu::filter_items(track);
    if !filter.is_empty() {
        body = body.push(Column::new().spacing(1).extend(filter));
    }
    let card = container(column![title, body].spacing(12))
        .id("track-dialog")
        .width(WIDTH)
        .padding(16)
        .style(style::menu);
    center(opaque(card)).width(Length::Fill).into()
}
