use genie_core::grouped;
use genie_formats::fields::{Rule, Test};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;
use iced::widget::{Column, button, column, container, opaque, pin, row, space, text, text_input};
use iced::{Alignment, Element, Length, Point, Size};

use crate::app::{
    Filtering, Lifting, Message, Ranging, Renaming, Sampling, Tabling, active, arranging_words,
};
use crate::chrome::keys;
use crate::icon::{self, Icon};
use crate::style;
use crate::theme::tokens;
use crate::track::{Change, Format, Grouping, Pick, Toggle, Track, tag_name};

pub const WIDTH: f32 = 168.0;
const FEATURES_HEIGHT: f32 = 267.5;
const COLORED_HEIGHT: f32 = 290.5;
const READS_HEIGHT: f32 = 522.7;
const VARIANTS_HEIGHT: f32 = 234.5;
const SIGNAL_HEIGHT: f32 = 271.5;
const HEADING_HEIGHT: f32 = 21.0;
const LINK_HEIGHT: f32 = 22.6;
const SEPARATOR_HEIGHT: f32 = 8.0;
const SWATCHES_HEIGHT: f32 = 24.0;
/// The kind items of a track without modes, swatches or jumps to the next feature, each with its
/// separator: the items that every track menu has.
const PLAIN_HEIGHT: f32 =
    FEATURES_HEIGHT - 5.0 * LINK_HEIGHT - 3.0 * SEPARATOR_HEIGHT - SWATCHES_HEIGHT;
/// The row of swatches with the space of the menu under it.
const SWATCH_ROW: f32 = SWATCHES_HEIGHT + 1.0;
/// The swatches and their separator.
const COLORS_HEIGHT: f32 = SWATCH_ROW + SEPARATOR_HEIGHT;
/// The three modes and their separator.
const MODES_HEIGHT: f32 = 3.0 * LINK_HEIGHT + SEPARATOR_HEIGHT;
/// The row of the fields of a range, with the space of the menu under it.
const RANGE_ROW: f32 = 25.6;
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
pub const TOGGLES: [(&str, Toggle); 6] = [
    ("Show duplicates", Toggle::Duplicates),
    ("Show soft clips", Toggle::Clips),
    ("Down-sample", Toggle::Downsample),
    ("Show secondary reads", Toggle::Secondary),
    ("Show supplementary reads", Toggle::Supplementary),
    ("Show QC-failed reads", Toggle::Failed),
];
/// The lowest MAPQ that a read track can ask for.
pub const MAPQ_FLOORS: [(&str, u8); 5] = [("0", 0), ("1", 1), ("10", 10), ("20", 20), ("30", 30)];

/// A row that sets the lowest MAPQ of the reads that show.
pub fn mapq_row<'a>(track: &Track) -> Element<'a, Message> {
    let (id, floor) = (track.id, track.options.mapq);
    let choices = MAPQ_FLOORS.map(|(label, value)| {
        button(text(label).size(12).wrapping(text::Wrapping::None))
            .padding([3, 4])
            .style(if value == floor {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::TrackChanged(id, Change::Mapq(value)))
            .into()
    });
    row![text("MAPQ ≥").size(12).wrapping(text::Wrapping::None)]
        .push(space::horizontal())
        .extend(choices)
        .spacing(1)
        .padding(iced::Padding {
            left: 8.0,
            ..iced::Padding::ZERO
        })
        .align_y(Alignment::Center)
        .into()
}

/// The fewest reads of the splice junctions that a Sashimi plot shows as arcs.
pub const JUNCTION_FLOORS: [(&str, u32); 5] =
    [("1", 1), ("2", 2), ("5", 5), ("10", 10), ("50", 50)];

/// A row that sets the fewest reads of the junctions that a Sashimi plot shows.
fn junction_row<'a>(track: &Track) -> Element<'a, Message> {
    let (id, floor) = (track.id, track.plot.junction_reads);
    let choices = JUNCTION_FLOORS.map(|(label, value)| {
        button(text(label).size(12).wrapping(text::Wrapping::None))
            .padding([3, 4])
            .style(if value == floor {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::TrackChanged(id, Change::JunctionReads(value)))
            .into()
    });
    row![text("Arcs ≥").size(12).wrapping(text::Wrapping::None)]
        .push(space::horizontal())
        .extend(choices)
        .spacing(1)
        .padding(iced::Padding {
            left: 8.0,
            ..iced::Padding::ZERO
        })
        .align_y(Alignment::Center)
        .into()
}

/// Items of a track menu under a heading, such as the other tracks to overlay or to compare with.
pub struct Section {
    pub heading: &'static str,
    pub items: Vec<(String, Message)>,
}

pub fn height(track: &Track, sections: &[Section]) -> f32 {
    let extra: f32 = sections
        .iter()
        .filter(|section| !section.items.is_empty())
        .map(|section| HEADING_HEIGHT + section.items.len() as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT)
        .sum();
    let filter = &track.filter;
    let rules = if filter.is_empty() {
        0.0
    } else {
        let rows = filter.rules.len() + usize::from(filter.label.is_some());
        HEADING_HEIGHT + rows as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT
    };
    let arranged = active(track).len();
    let samples = if arranged == 0 {
        0.0
    } else {
        arranged as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT
    };
    let table = if track.source.has_variants() {
        LINK_HEIGHT
    } else {
        0.0
    };
    2.0 * LINK_HEIGHT
        + table
        + SEPARATOR_HEIGHT
        + kind_height(track)
        + lift_height(track)
        + extra
        + rules
        + samples
}

/// The fields that group, sort and color the samples of a track, to turn them off.
fn sample_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let arranged = active(track);
    if arranged.is_empty() {
        return Vec::new();
    }
    let id = track.id;
    let mut items: Vec<Element<'a, Message>> = arranged
        .into_iter()
        .map(|arranging| {
            let words = arranging_words(&arranging);
            item(
                words,
                Message::Samples(Sampling::Arrange(id, arranging)),
                true,
                true,
            )
        })
        .collect();
    items.push(separator());
    items
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

/// The normalizations of a Hi-C map, observed/expected ratios, and the virtual 4C from the base
/// under the pointer.
pub fn contact_items<'a>(track: &Track, base: u64) -> Vec<Element<'a, Message>> {
    let (names, place) = track
        .source
        .normalizations(track.options.contacts.normalization);
    let mut items: Vec<Element<'a, Message>> = names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let label = if name == genie_formats::hic::NONE {
                "No normalization".to_owned()
            } else {
                format!("{name} normalization")
            };
            let change = Change::Normalization(u8::try_from(index).ok()?);
            Some(item(
                label,
                Message::TrackChanged(track.id, change),
                index == place,
                true,
            ))
        })
        .collect();
    items.push(item(
        "Observed/expected",
        Message::TrackChanged(track.id, Change::Ratio),
        track.options.contacts.ratio,
        true,
    ));
    let (label, viewpoint) = if track.plot.viewpoint.is_some() {
        ("Stop virtual 4C", None)
    } else {
        ("Virtual 4C from here", Some(base))
    };
    items.push(item(
        label,
        Message::TrackChanged(track.id, Change::Viewpoint(viewpoint)),
        false,
        true,
    ));
    items.push(separator());
    items
}

/// The columns of numbers of a table with a tabix index, and its rows.
pub fn column_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    let Format::Columns(file) = &track.source.format else {
        return Vec::new();
    };
    let shown = file.shown(track.options.table);
    let change = |pick| Message::TrackChanged(track.id, Change::Column(pick));
    let mut items: Vec<Element<'a, Message>> = file
        .columns
        .numbers
        .iter()
        .filter_map(|&column| {
            let name = file.columns.names.get(column)?;
            let pick = Pick::Column(u16::try_from(column).ok()?);
            Some(item(
                format!("Show {name}"),
                change(pick),
                shown == Some(column),
                true,
            ))
        })
        .collect();
    items.push(item("Show rows", change(Pick::Rows), shown.is_none(), true));
    items.push(separator());
    items
}

/// The number of the column items of a table.
fn column_choices(track: &Track) -> usize {
    match &track.source.format {
        Format::Columns(file) => file.columns.numbers.len() + 1,
        _ => 0,
    }
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

/// True when the read menu offers the methylation difference of the groups of reads: the reads
/// show their modifications and are in groups.
fn compares_groups(track: &Track) -> bool {
    track.plot.look.mods && track.options.group != Grouping::None
}

fn kind_height(track: &Track) -> f32 {
    let choices = modification_choices(track).len() as f32 * LINK_HEIGHT;
    if track.source.has_reads() {
        let sashimi = if track.plot.sashimi { 3.0 } else { 1.0 };
        let difference = f32::from(u8::from(compares_groups(track)));
        READS_HEIGHT + choices + (tag_items(track) as f32 + sashimi + difference) * LINK_HEIGHT
    } else if track.source.has_variants() {
        VARIANTS_HEIGHT
    } else if track.source.is_bedmethyl() {
        SIGNAL_HEIGHT + choices + LINK_HEIGHT + SEPARATOR_HEIGHT
    } else if track.source.has_signal() {
        SIGNAL_HEIGHT
    } else if track.source.has_links() {
        // The file colors and the swatches.
        PLAIN_HEIGHT + LINK_HEIGHT + COLORS_HEIGHT
    } else if track.source.has_junctions() {
        PLAIN_HEIGHT
    } else if track.source.has_contacts() {
        // The normalizations and observed/expected, with their separator, in a column of their
        // own that has one space less.
        // The normalizations, observed/expected and the virtual 4C.
        let choices = track.source.normalizations(None).0.len() as f32 + 2.0;
        PLAIN_HEIGHT + choices * LINK_HEIGHT + SEPARATOR_HEIGHT - 1.0
    } else if track.source.has_columns() {
        // The columns and the rows, with their separator, in a column of their own that has one
        // space less.
        let choices = column_choices(track) as f32 * LINK_HEIGHT + SEPARATOR_HEIGHT - 1.0;
        PLAIN_HEIGHT + choices + MODES_HEIGHT + COLORS_HEIGHT
    } else if track.source.has_segments() {
        // The range and the default range, with their separator.
        let range = RANGE_ROW + LINK_HEIGHT + SEPARATOR_HEIGHT;
        PLAIN_HEIGHT + MODES_HEIGHT + range + COLORS_HEIGHT
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
    pin(opaque(card)).x(x).y(y).into()
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

/// The range of a segment track: the log2 values that get the full blue and red.
pub fn segment_items<'a>(track: &Track, range: Option<&(String, String)>) -> Column<'a, Message> {
    let id = track.id;
    column![
        range_fields(id, range),
        item(
            "Default range (±1.5)",
            Message::Range(id, Ranging::Auto),
            track.plot.range.is_none(),
            true
        ),
    ]
    .spacing(1)
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
    items.push(mapq_row(track));
    items.push(separator());
    items.push(item("Color unusual pairs", Change::Pairs, look.pairs));
    items.push(item(
        "View as pairs",
        Change::Toggle(Toggle::Mates),
        options.is_on(Toggle::Mates),
    ));
    items.push(item("Color by modification", Change::Mods, look.mods));
    items.extend(modification_items(track));
    if compares_groups(track) {
        let difference = look.difference;
        items.push(item(
            "Methylation difference",
            Change::Difference,
            difference,
        ));
    }
    items.push(item("Sashimi plot", Change::Sashimi, track.plot.sashimi));
    if track.plot.sashimi {
        items.push(junction_row(track));
        items.push(item("Shared scale", Change::Link, track.plot.linked));
    }
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
    if let Grouping::Tag(name) = options.group {
        let change = Change::Group(Grouping::Tag(name));
        let words = format!("Group by {}", tag_name(name));
        items.push(self::item(
            words,
            Message::TrackChanged(id, change),
            true,
            true,
        ));
    }
    if let Some(name) = options.color {
        let words = format!("Color by {}", tag_name(name));
        let change = Message::TrackChanged(id, Change::Color(name));
        items.push(self::item(words, change, true, true));
    }
    items.push(separator());
    items
}

/// The number of menu items for a tag that groups or colors the reads of a track.
fn tag_items(track: &Track) -> usize {
    usize::from(matches!(track.options.group, Grouping::Tag(_)))
        + usize::from(track.options.color.is_some())
}

pub fn view<'a>(
    track: &Track,
    mode: Mode,
    place: &Place,
    range: Option<&(String, String)>,
    sections: Vec<Section>,
) -> Element<'a, Message> {
    let id = track.id;
    let size = height(track, &sections);
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
    } else if track.source.has_links() || track.source.has_junctions() {
        column![]
    } else if track.source.has_contacts() {
        column(contact_items(track, place.base)).spacing(1)
    } else if track.source.has_columns() {
        column(column_items(track))
            .push(column(modes).spacing(1))
            .push(separator())
            .spacing(1)
    } else if track.source.has_segments() {
        column![
            column(modes).spacing(1),
            separator(),
            segment_items(track, range),
            separator()
        ]
        .spacing(1)
    } else {
        column![column(modes).spacing(1), separator()].spacing(1)
    };
    let mut content = top_items(track).push(kind).extend(sample_items(track));
    if track.source.has_reads() {
        content = content.extend(read_items(track, place));
    } else if !track.source.has_variants()
        && !track.source.has_contacts()
        && !track.source.has_junctions()
    {
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
    for section in sections
        .into_iter()
        .filter(|section| !section.items.is_empty())
    {
        content = content
            .push(self::heading(section.heading))
            .extend(
                section
                    .items
                    .into_iter()
                    .map(|(label, message)| link(label, message)),
            )
            .push(separator());
    }
    let content = content
        .extend(filter_items(track))
        .extend(jumps(track))
        .extend(lift_items(track))
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
            item("Hide", Change::Visible(false), false, true),
            item("Remove", Change::Remove, false, true),
        ]);
    let width = if track.filter.is_empty() {
        WIDTH
    } else {
        FILTER_WIDTH
    };
    sized(content, place.at, place.window, (width, size))
}

/// The items at the top of the track menu: the settings dialog, and the variant table of a
/// variant track.
fn top_items<'a>(track: &Track) -> Column<'a, Message> {
    let id = track.id;
    let settings = item("Track settings…", Message::TrackSettings(id), false, true);
    let mut items = column![settings].spacing(1);
    if track.source.has_variants() {
        let open = Message::Table(Tabling::Open(id));
        items = items.push(item("Variant table", open, false, true));
    }
    items.push(separator())
}

/// Whether genie can move the data of a track from another assembly to the assembly in view.
fn can_lift(track: &Track) -> bool {
    !track.source.has_reads() && !track.source.has_contacts()
}

fn lift_items<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    if !can_lift(track) {
        return Vec::new();
    }
    let (label, lifting) = if track.lift.is_some() {
        ("Remove lift", Lifting::Stop(track.id))
    } else {
        ("Lift over…", Lifting::Pick(Some(track.id)))
    };
    vec![
        item(label, Message::Lift(lifting), false, true),
        separator(),
    ]
}

/// The height of the lift items of a track, less the items to go to the next feature that a
/// lift takes away.
fn lift_height(track: &Track) -> f32 {
    if !can_lift(track) {
        return 0.0;
    }
    let jumps = if track.lift.is_some() && track.source.can_jump() {
        2.0 * LINK_HEIGHT + SEPARATOR_HEIGHT
    } else {
        0.0
    };
    LINK_HEIGHT + SEPARATOR_HEIGHT - jumps
}

fn jumps<'a>(track: &Track) -> Vec<Element<'a, Message>> {
    if !track.source.can_jump() || track.lift.is_some() {
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
