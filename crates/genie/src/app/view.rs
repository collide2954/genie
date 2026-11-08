use std::time::Duration;

use genie_core::pileup::sort_rows;
use genie_core::{Viewport, grouped};
use genie_render::{
    alignments, features, overview, ruler, sequence, signal, translation, variants,
};
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    button, center, column, container, mouse_area, pin, row, scrollable, space, stack, text,
    tooltip,
};
use iced::{Alignment, Border, Element, Length, Padding, mouse};

use crate::card;
use crate::chrome::{self, Controls, LocusField};
use crate::context::Context;
use crate::grip::Grip;
use crate::icon::{self, Icon};
use crate::menu::{self, Place};
use crate::navigator::Navigator;
use crate::paths::file_name;
use crate::reading;
use crate::settings;
use crate::theme::{Appearance, tokens};
use crate::tiles;
use crate::track::{Content, Track};
use crate::{font, scene, style};

use super::{Genie, Lane, Message, Session, Status, Suggestion};

const OVERVIEW_HEIGHT: f32 = 12.0;
const RULER_HEIGHT: f32 = 20.0;
const BASE_HEIGHT: f32 = 16.0;
const EMPTY_HEIGHT: f32 = 20.0;
const EDGE: f32 = 4.0;
const LABEL_SIZE: f32 = 10.0;
const READ_HINT: &str = "Zoom in to see the reads";
pub(super) const VARIANT_HINT: &str = "Zoom in to see the variants";
pub(super) const SIGNAL_HINT: &str = "Zoom in to see the signal";
pub(super) const DROP_HINT: &str = "Drop the file to open it";
pub(super) const ZOOM_HINT: &str = "Zoom in to see the features";

impl Genie {
    pub(super) fn figures(&self) -> Vec<(&'static str, String)> {
        let millis = |duration: Duration| format!("{:.2} ms", duration.as_secs_f64() * 1_000.0);
        let (mut reads, mut features, mut sites, mut values) = (0, 0, 0, 0);
        if let Some(session) = &self.session {
            for track in &session.tracks {
                match track.data(session.contig).map(|data| &data.content) {
                    Some(Content::Reads(pile)) => reads += pile.reads.len(),
                    Some(Content::Features(annotation)) => features += annotation.features.len(),
                    Some(Content::Variants(calls)) => sites += calls.variants.len(),
                    Some(Content::Signal(data)) => values += data.summaries.len(),
                    None => {}
                }
            }
        }
        vec![
            ("update", millis(self.stats.update)),
            ("view", millis(self.stats.view.get())),
            (
                "frame",
                self.stats.frame.map_or_else(|| "idle".to_owned(), millis),
            ),
            (
                "tiles",
                format!("{} of {}", self.tiles.count(), tiles::CAPACITY),
            ),
            ("reads", grouped(reads as u64)),
            ("features", grouped(features as u64)),
            ("variants", grouped(sites as u64)),
            ("signal", grouped(values as u64)),
        ]
    }

    fn diagnostics_view(&self) -> Element<'_, Message> {
        let rows = self.figures().into_iter().map(|(label, value)| {
            row![
                text(label)
                    .font(font::MONO)
                    .size(11)
                    .width(64)
                    .style(style::muted),
                text(value).font(font::MONO).size(11),
            ]
            .into()
        });
        let card = container(column(rows).spacing(2))
            .id("diagnostics")
            .padding([6, 8])
            .style(style::menu);
        container(card)
            .width(Length::Fill)
            .align_right(Length::Fill)
            .padding(Padding {
                top: chrome::BAR_HEIGHT + 8.0,
                right: 8.0,
                ..Padding::ZERO
            })
            .into()
    }

    pub(super) fn view(&self) -> Element<'_, Message> {
        let started = Instant::now();
        let element = self.window_view();
        self.stats.view.set(started.elapsed());
        element
    }

    fn window_view(&self) -> Element<'_, Message> {
        let appearance = match self.settings.appearance {
            Appearance::System => Icon::Monitor,
            Appearance::Light => Icon::Sun,
            Appearance::Dark => Icon::Moon,
        };
        let locus = self.session.as_ref().map(|session| LocusField {
            value: self.draft.clone().unwrap_or_else(|| session.locus()),
            error: self.locus_error.clone(),
            back: session.history.can_go_back(),
            forward: session.history.can_go_forward(),
        });
        let mut layers = stack![column![
            chrome::title_bar(Controls::CURRENT, appearance, locus),
            self.body()
        ]];
        if let Some(menu) = self.suggestion_menu() {
            layers = layers.push(menu);
        }
        if let Some(pinned) = &self.pinned {
            layers = layers.push(card::view(pinned, true, self.window));
        }
        if let Some(hover) = &self.hover
            && self
                .pinned
                .as_ref()
                .is_none_or(|pinned| pinned.detail != hover.detail)
        {
            layers = layers.push(card::view(hover, false, self.window));
        }
        let progress = self
            .session
            .as_ref()
            .and(self.indexing.as_ref())
            .map(|(name, percent)| indexing(name, *percent));
        let notice = self.notice.as_ref().map(|notice| notice.text.as_str());
        if progress.is_some() || notice.is_some() {
            layers = layers.push(toasts(progress, notice));
        }
        let panel = || {
            self.settings_open
                .then(|| settings::panel(self.settings, chrome::settings_inset(Controls::CURRENT)))
        };
        if let Some(menu) = self
            .track_menu()
            .or_else(|| self.reading_menu())
            .or_else(panel)
        {
            let dismiss = mouse_area(space().width(Length::Fill).height(Length::Fill))
                .on_press(Message::MenuDismissed)
                .on_right_press(Message::MenuDismissed);
            layers = layers.push(dismiss).push(menu);
        }
        if self.hovering {
            layers = layers.push(drop_hint());
        }
        if self.diagnostics {
            layers = layers.push(self.diagnostics_view());
        }
        if Controls::CURRENT == Controls::Custom {
            layers = layers.push(chrome::resize_handles());
        }
        layers.into()
    }

    fn reading_menu(&self) -> Option<Element<'_, Message>> {
        let at = self.sequence_menu?;
        let session = self.session.as_ref()?;
        Some(reading::menu(session.reading, at, self.window))
    }

    fn track_menu(&self) -> Option<Element<'_, Message>> {
        let (id, at) = self.menu?;
        let session = self.session.as_ref()?;
        let position = session.tracks.iter().position(|track| track.id == id)?;
        let track = &session.tracks[position];
        let place = Place {
            position,
            count: session.tracks.len(),
            at,
            window: self.window,
            base: session.viewport.to_bp(f64::from(at.x)).floor().max(0.0) as u64,
        };
        Some(menu::view(track, track.mode(session.contig), &place))
    }

    fn suggestion_menu(&self) -> Option<Element<'_, Message>> {
        let session = self.session.as_ref()?;
        let suggestions = self.suggestions();
        if suggestions.is_empty() {
            return None;
        }
        let contigs = session.reference.assembly.contigs();
        let rows = suggestions
            .into_iter()
            .enumerate()
            .map(|(position, suggestion)| {
                let (name, detail) = match suggestion {
                    Suggestion::Contig(index) => (
                        text(contigs[index].name.as_str()).font(font::MONO),
                        grouped(contigs[index].length),
                    ),
                    Suggestion::Feature(named) => (
                        text(named.name),
                        format!(
                            "{}:{}",
                            contigs[named.contig].name,
                            grouped(named.span.start() + 1)
                        ),
                    ),
                };
                let chosen = self.highlight == Some(position);
                button(
                    row![
                        name.size(12),
                        space::horizontal(),
                        text(detail).size(11).style(style::muted),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([3, 8])
                .style(if chosen { style::chosen } else { style::ghost })
                .on_press(Message::SuggestionChosen(position))
                .into()
            });
        let menu = container(column(rows).spacing(1))
            .width(chrome::LOCUS_WIDTH)
            .padding(4)
            .style(style::menu);
        Some(
            pin(menu)
                .x(chrome::locus_x(Controls::CURRENT))
                .y(chrome::BAR_HEIGHT + 2.0)
                .into(),
        )
    }

    fn body(&self) -> Element<'_, Message> {
        if let Some(session) = &self.session {
            return self.browser(session);
        }
        let message = match (&self.status, &self.indexing) {
            (Status::Idle, _) => text("Open a reference sequence to start.").style(style::muted),
            (Status::Opening(_), Some((name, percent))) => {
                text(indexing(name, *percent)).style(style::muted)
            }
            (Status::Opening(name), None) => text(format!("Opening {name}…")).style(style::muted),
            (Status::Failed(error), _) => text(error.as_str()),
        };
        let open = button(text("Open reference").font(font::UI_STRONG).size(13))
            .padding([8, 16])
            .style(style::primary)
            .on_press(Message::Open);
        let mut welcome = column![message, open]
            .spacing(16)
            .align_x(Alignment::Center);
        if let Some(last) = &self.last {
            let tracks = match last.tracks.len() {
                0 => String::new(),
                1 => " · 1 track".to_owned(),
                count => format!(" · {count} tracks"),
            };
            let locus = last
                .locus
                .as_deref()
                .map(|locus| format!(" · {locus}"))
                .unwrap_or_default();
            let detail = format!("{}{locus}{tracks}", file_name(&last.reference));
            welcome = welcome.push(
                button(
                    column![
                        text("Restore last session").size(12),
                        text(detail).size(11).style(style::muted)
                    ]
                    .spacing(2)
                    .align_x(Alignment::Center),
                )
                .padding([6, 12])
                .style(style::ghost)
                .on_press(Message::RestoreLast),
            );
        }
        if !self.recent.paths().is_empty() {
            let files = self.recent.paths().iter().map(|path| {
                let folder = path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default();
                button(
                    row![
                        text(file_name(path)).size(12),
                        text(folder).size(11).style(style::muted)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(style::ghost)
                .on_press(Message::Picked(Some(path.clone())))
                .into()
            });
            welcome = welcome.push(
                column![text("Recent").size(11).style(style::muted)]
                    .extend(files)
                    .spacing(2),
            );
        }
        center(welcome).into()
    }

    pub(super) fn tracks<'a>(&'a self, session: &'a Session, repack: bool) -> Element<'a, Message> {
        let viewport = session.viewport;
        let (contig, visible) = (session.contig, viewport.visible());
        let reading = session.reading;
        let padding = self.settings.density.padding();
        let bases = sequence::scene(
            viewport,
            self.tiles.chunks(contig, visible),
            BASE_HEIGHT,
            reading.reverse,
        );
        let frames = reading.frames();
        let mut strands = column![scene::view(bases, BASE_HEIGHT)];
        if !frames.is_empty() {
            let codons = translation::scene(viewport, self.tiles.chunks(contig, visible), &frames);
            let height = frames.len() as f32 * translation::ROW_HEIGHT;
            strands = strands.push(scene::view(codons, height));
        }
        let sequence = Context::new(
            stack![
                container(strands).padding([padding, 0.0]),
                chrome::tip(
                    label("Sequence"),
                    "Right-click for the strand and reading frames",
                    tooltip::Position::Right,
                ),
            ],
            Message::SequenceMenu,
        );
        let gap = self
            .drop_target()
            .filter(|(from, gap)| gap != from && *gap != from + 1)
            .map(|(_, gap)| gap);
        let line = |at: usize| {
            if gap == Some(at) {
                drop_line()
            } else {
                hairline()
            }
        };
        let mut data = column![container(sequence).id("sequence"), line(0)];
        for (index, track) in session.tracks.iter().enumerate() {
            data = data
                .push(track_view(session, track, repack, padding))
                .push(line(index + 1));
        }
        data.into()
    }

    fn browser<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let viewport = session.viewport;
        let tracks = column![
            container(
                scene::view(overview::scene(viewport, OVERVIEW_HEIGHT), OVERVIEW_HEIGHT)
                    .on_press(Message::OverviewPressed)
                    .on_drag(Message::OverviewDragged)
                    .on_release(Message::OverviewReleased)
            )
            .id("overview"),
            ruler_view(viewport),
            hairline(),
            scrollable(self.tracks(session, false))
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0)
                ))
                .style(style::scrollbar)
                .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                .height(Length::Fill),
        ]
        .height(Length::Fill);
        Navigator::new(tracks, self.width)
            .on_pan(Message::Pan)
            .on_zoom(Message::Zoom)
            .on_resize(Message::Resized)
            .into()
    }
}

fn track_body<'a>(
    session: &Session,
    track: &'a Track,
    shape: Shape,
    repack: bool,
) -> Element<'a, Message> {
    let viewport = session.viewport;
    let contig = session.contig;
    let id = track.id;
    let interactive = |primitives, height| {
        scene::view(primitives, height)
            .on_hover(move |pointer| Message::TrackHovered(id, pointer))
            .on_click(move |pointer| Message::TrackClicked(id, pointer))
    };
    let mode = track.mode(contig);
    if let Some(error) = &track.error {
        return hint(error.clone());
    }
    if viewport.visible().len() > track.source.span() {
        let words = if track.source.has_reads() {
            READ_HINT
        } else if track.source.has_variants() {
            VARIANT_HINT
        } else if track.source.has_signal() {
            SIGNAL_HINT
        } else {
            ZOOM_HINT
        };
        return hint(words.to_owned());
    }
    match track.data(contig).map(|data| &data.content) {
        Some(Content::Signal(data)) => {
            let height = track.height.unwrap_or(shape.height);
            let columns = signal_columns(track, viewport, &data.summaries);
            let Some(scale) = signal_scale(session, track, &columns) else {
                return space().width(Length::Fill).height(height).into();
            };
            let width = viewport.width() as f32;
            let style = track.plot.style;
            let primitives = signal::scene(&columns, scale, style, track.hue, height, width);
            interactive(primitives, height).into()
        }
        Some(Content::Variants(calls)) => {
            let primitives = variants::scene(viewport, &calls.variants, mode);
            interactive(primitives, shape.height).into()
        }
        Some(Content::Features(annotation)) => {
            let primitives = features::scene(
                viewport,
                &annotation.features,
                &annotation.rows,
                mode,
                track.hue,
                track.plot.colors,
            );
            interactive(primitives, shape.height).into()
        }
        Some(Content::Reads(pile)) => {
            let depth = alignments::coverage(
                viewport,
                &pile.coverage,
                |at| pile.base(at),
                alignments::COVERAGE_HEIGHT,
            );
            let mut body = column![scene::view(depth, alignments::COVERAGE_HEIGHT)].spacing(1);
            if mode != features::Mode::Collapsed {
                let (packed, bands);
                let (rows, groups) = if repack {
                    let (rows, _, groups) = pile.packed(viewport.visible());
                    let starts: Vec<usize> = groups.iter().map(|(first, _)| *first).collect();
                    packed = match &track.order {
                        Some((at, _)) => sort_rows(&pile.reads, &rows, &starts, *at),
                        None => rows,
                    };
                    bands = groups;
                    (packed.as_slice(), bands.as_slice())
                } else {
                    (track.rows(pile), pile.groups.as_slice())
                };
                let pairs = track.plot.pairs;
                let primitives =
                    alignments::reads(viewport, &pile.reads, rows, groups, mode, pairs);
                let height = shape.height - alignments::COVERAGE_HEIGHT - 1.0;
                body = body.push(interactive(primitives, height));
            }
            body.into()
        }
        None => space().width(Length::Fill).height(EMPTY_HEIGHT).into(),
    }
}

fn track_view<'a>(
    session: &Session,
    track: &'a Track,
    repack: bool,
    padding: f32,
) -> Element<'a, Message> {
    let id = track.id;
    let shape = shape(session, track, repack);
    let body = track_body(session, track, shape, repack);
    let height = track.height.unwrap_or(shape.height);
    let edge = Grip::new(
        space().width(Length::Fill).height(EDGE),
        move |delta| Message::Lane(id, Lane::Resized(delta)),
        Message::Lane(id, Lane::Released),
    )
    .on_double_click(Message::Lane(id, Lane::Fit))
    .interaction(mouse::Interaction::ResizingVertically);
    let handle = Grip::new(
        chrome::tip(
            label(&track.source.name),
            "Drag to move · Right-click for options",
            tooltip::Position::Right,
        ),
        move |delta| Message::Lane(id, Lane::Moved(delta)),
        Message::Lane(id, Lane::Dropped),
    );
    let mut layers = stack![
        container(body)
            .height(height + 2.0 * padding)
            .padding([padding, 0.0])
            .clip(true),
        handle,
        container(edge)
            .height(Length::Fill)
            .align_bottom(Length::Fill),
    ];
    let hidden = shape.hidden(height);
    if hidden > 0 {
        layers = layers.push(
            container(note(hidden))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
                .padding([1, 4]),
        );
    }
    container(Context::new(layers, move |at| Message::TrackMenu(id, at)))
        .id(format!("lane-{id}"))
        .into()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    height: f32,
    rows: usize,
    fixed: f32,
    row_height: f32,
}

impl Shape {
    const fn plain(height: f32) -> Self {
        Self {
            height,
            rows: 0,
            fixed: height,
            row_height: 0.0,
        }
    }

    fn hidden(self, height: f32) -> usize {
        if self.row_height <= 0.0 {
            return 0;
        }
        let shown = ((height - self.fixed).max(0.0) / self.row_height).floor() as usize;
        self.rows.saturating_sub(shown)
    }
}

fn shape(session: &Session, track: &Track, repack: bool) -> Shape {
    let viewport = session.viewport;
    if track.error.is_some() || viewport.visible().len() > track.source.span() {
        return Shape::plain(EMPTY_HEIGHT);
    }
    let mode = track.mode(session.contig);
    match track.data(session.contig).map(|data| &data.content) {
        Some(Content::Features(annotation)) => {
            let rows = if mode == features::Mode::Collapsed {
                1
            } else {
                annotation.count.max(1)
            };
            let row_height = mode.row_height();
            Shape {
                height: rows as f32 * row_height,
                rows,
                fixed: 0.0,
                row_height,
            }
        }
        Some(Content::Variants(calls)) => {
            if mode == features::Mode::Collapsed || calls.samples == 0 {
                return Shape::plain(variants::SITE_HEIGHT);
            }
            let row_height = variants::row_height(mode);
            let fixed = variants::SITE_HEIGHT + 1.0;
            Shape {
                height: fixed + calls.samples as f32 * row_height,
                rows: calls.samples,
                fixed,
                row_height,
            }
        }
        Some(Content::Reads(_)) if mode == features::Mode::Collapsed => {
            Shape::plain(alignments::COVERAGE_HEIGHT)
        }
        Some(Content::Reads(pile)) => {
            let (count, groups) = if repack {
                let (_, count, groups) = pile.packed(viewport.visible());
                (count, groups.len())
            } else {
                (pile.count, pile.groups.len())
            };
            let count = count.max(1);
            let row_height = alignments::row_height(mode);
            let fixed = alignments::COVERAGE_HEIGHT + 1.0 + groups as f32 * alignments::GROUP_GAP;
            Shape {
                height: fixed + count as f32 * row_height,
                rows: count,
                fixed,
                row_height,
            }
        }
        Some(Content::Signal(_)) => Shape::plain(signal::HEIGHT),
        None if track.source.has_signal() => Shape::plain(signal::HEIGHT),
        None => Shape::plain(EMPTY_HEIGHT),
    }
}

pub(super) fn signal_columns(
    track: &Track,
    viewport: Viewport,
    summaries: &[genie_core::signal::Summary],
) -> Vec<signal::Column> {
    let columns = signal::columns(viewport, summaries);
    if track.source.is_bigbed() {
        signal::density(columns)
    } else {
        columns
    }
}

pub(super) fn signal_scale(
    session: &Session,
    track: &Track,
    columns: &[signal::Column],
) -> Option<signal::Scale> {
    if !track.plot.linked {
        return signal::range(columns);
    }
    session
        .tracks
        .iter()
        .filter(|other| other.plot.linked)
        .filter_map(|other| match other.data(session.contig)?.content {
            Content::Signal(ref data) => {
                signal::range(&signal_columns(other, session.viewport, &data.summaries))
            }
            _ => None,
        })
        .reduce(signal::Scale::union)
}

pub(super) fn body_height(session: &Session, track: &Track) -> f32 {
    track
        .height
        .unwrap_or_else(|| shape(session, track, false).height)
}

fn drop_line<'a>() -> Element<'a, Message> {
    container(space())
        .id("drop")
        .width(Length::Fill)
        .height(2)
        .style(|theme| container::background(tokens(theme).text))
        .into()
}

fn note<'a>(hidden: usize) -> Element<'a, Message> {
    let words = if hidden == 1 {
        "+1 row".to_owned()
    } else {
        format!("+{hidden} rows")
    };
    container(text(words).size(LABEL_SIZE).style(style::muted))
        .padding([0, 4])
        .style(|theme| container::Style {
            background: Some(tokens(theme).canvas.scale_alpha(scene::CHIP).into()),
            border: Border::default().rounded(3),
            ..container::Style::default()
        })
        .into()
}

pub(super) fn ruler_view<'a>(viewport: Viewport) -> Element<'a, Message> {
    container(scene::view(
        ruler::scene(viewport, RULER_HEIGHT),
        RULER_HEIGHT,
    ))
    .id("ruler")
    .into()
}

fn hint<'a>(message: String) -> Element<'a, Message> {
    container(text(message).size(11).style(style::muted))
        .center_x(Length::Fill)
        .height(EMPTY_HEIGHT)
        .align_y(Alignment::Center)
        .into()
}

fn indexing(name: &str, percent: u8) -> String {
    format!("Indexing {name}… {percent} %")
}

fn toasts<'a>(progress: Option<String>, notice: Option<&'a str>) -> Element<'a, Message> {
    let card = |content: Element<'a, Message>| {
        container(content)
            .max_width(360)
            .padding([6, 6])
            .style(style::menu)
    };
    let progress = progress.map(|line| card(text(line).size(12).into()));
    let notice = notice.map(|notice| {
        let close = button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::NoticeDismissed);
        card(
            row![text(notice).size(12).width(Length::Fill), close]
                .spacing(8)
                .align_y(Alignment::Center)
                .into(),
        )
    });
    container(
        column![]
            .push(progress)
            .push(notice)
            .spacing(6)
            .align_x(Alignment::End),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_right(Length::Fill)
    .align_bottom(Length::Fill)
    .padding(12)
    .into()
}

fn label(name: &str) -> Element<'_, Message> {
    container(text(name).size(LABEL_SIZE).style(style::muted))
        .padding([0, 4])
        .style(|theme| container::Style {
            background: Some(tokens(theme).canvas.scale_alpha(scene::CHIP).into()),
            border: Border::default().rounded(3),
            ..container::Style::default()
        })
        .into()
}

pub(super) fn hairline<'a>() -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|theme| container::background(tokens(theme).line))
        .into()
}

fn drop_hint<'a>() -> Element<'a, Message> {
    let frame = container(center(text(DROP_HINT).size(13))).style(|theme| {
        let tokens = tokens(theme);
        container::Style {
            background: Some(tokens.canvas.scale_alpha(0.92).into()),
            border: Border::default().rounded(8).width(1).color(tokens.muted),
            ..container::Style::default()
        }
    });
    container(frame).padding(8).into()
}
