use std::time::Duration;

use genie_core::pileup::sort_rows;
use genie_core::{Viewport, grouped};
use genie_render::scene::{Hue, Paint, Primitive};
use genie_render::{
    alignments, features, overview, ruler, sequence, signal, translation, variants,
};
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, mouse_area, pin, row, scrollable, space, stack,
    text, text_input, tooltip,
};
use iced::{Alignment, Border, Element, Length, Padding, Point, Theme, mouse};

use crate::card;
use crate::chrome::{self, Controls, LocusField};
use crate::context::Context;
use crate::escape::Escape;
use crate::genome;
use crate::grip::Grip;
use crate::icon::{self, Icon};
use crate::menu::{self, Place};
use crate::navigator::Navigator;
use crate::reading;
use crate::recent::Entry;
use crate::settings;
use crate::theme::{Appearance, tokens};
use crate::tiles;
use crate::track::{Change, Content, Track};
use crate::{font, scene, style};

use super::tracks::TRACK_NAME;
use super::{
    Genie, Lane, Message, PANEL_GAP, Paneling, Popup, Renaming, Session, Status, Suggestion, Sweep,
};

const OVERVIEW_HEIGHT: f32 = 12.0;
pub(super) const HEADER_HEIGHT: f32 = 20.0;
const NAME_WIDTH: f32 = 160.0;
pub(super) const RULER_HEIGHT: f32 = 20.0;
pub(super) const BASE_HEIGHT: f32 = 16.0;
pub(super) const EMPTY_HEIGHT: f32 = 20.0;
const EDGE: f32 = 4.0;
pub(super) const LABEL_SIZE: f32 = 10.0;
const READ_HINT: &str = "Zoom in to see the reads";
pub(super) const VARIANT_HINT: &str = "Zoom in to see the variants";
pub(super) const SIGNAL_HINT: &str = "Zoom in to see the signal";
pub(super) const DROP_HINT: &str = "Drop the file to open it";
pub(super) const ZOOM_HINT: &str = "Zoom in to see the features";
const CROWDED_HINT: &str = "There is too much to draw here. Zoom in.";
const DRAW_LIMIT: usize = 200_000;
pub(super) const WELCOME: &str = "Pick a genome or open a reference.";
const SPECIES_WIDTH: f32 = 72.0;
const MENU_ROW: f32 = 24.0;
const GENOME_WIDTH: f32 = 64.0;

fn genomes<'a>() -> Element<'a, Message> {
    let rows = genome::all().into_iter().map(|genome| {
        button(
            row![
                text(genome.species).size(12).width(SPECIES_WIDTH),
                text(genome.label).size(12).width(GENOME_WIDTH),
                text(genome.assembly).size(11).style(style::muted)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([3, 8])
        .style(style::ghost)
        .on_press(Message::Genome(genome.id))
        .into()
    });
    column![text("Genomes").size(11).style(style::muted)]
        .extend(rows)
        .spacing(2)
        .into()
}

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
        let locus = self.session.as_ref().map(|session| {
            let reference = &session.reference;
            let about = match reference.genome.as_deref().and_then(genome::find) {
                Some(genome) => format!("{} {}", genome.species, genome.assembly),
                None => reference.location.to_string(),
            };
            let genome = reference.label();
            LocusField {
                value: self.draft.clone().unwrap_or_else(|| session.locus()),
                error: self.locus_error.clone(),
                back: session.history.can_go_back(),
                forward: session.history.can_go_forward(),
                genome,
                about,
            }
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
        if let (Some(pointer), Some(name)) = (self.strip, self.band_tip()) {
            layers = layers.push(card::tip(name, pointer.at, self.window));
        }
        let progress = self
            .session
            .as_ref()
            .and(self.indexing.as_ref())
            .map(|(name, percent)| indexing(name, *percent))
            .or_else(|| self.download_text());
        let notice = self.notice.as_ref().map(|notice| notice.text.as_str());
        if progress.is_some() || notice.is_some() {
            layers = layers.push(toasts(progress, notice));
        }
        let panel = || {
            (self.popup == Some(Popup::Settings))
                .then(|| settings::panel(self.settings, chrome::settings_inset(Controls::CURRENT)))
        };
        if let Some(menu) = self
            .track_menu()
            .or_else(|| self.reading_menu())
            .or_else(|| self.copy_menu())
            .or_else(panel)
            .or_else(|| self.address_view())
            .or_else(|| self.genome_menu())
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

    fn genome_menu(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Genomes) {
            return None;
        }
        let current = self
            .session
            .as_ref()
            .and_then(|session| session.reference.genome.clone());
        let genomes = genome::all();
        let count = genomes.len();
        let items = genomes.into_iter().map(|genome| {
            let chosen = current.as_deref() == Some(genome.id.as_str());
            menu::item(
                format!("{} · {}", genome.label, genome.species),
                Message::Genome(genome.id),
                chosen,
                true,
            )
        });
        let offline = self.offline_items();
        let extra = offline.len();
        let mut content = Column::new()
            .extend(items)
            .push(menu::separator())
            .push(menu::item("Open reference…", Message::Open, false, true));
        if extra > 0 {
            content = content.push(menu::separator()).extend(offline);
        }
        let at = Point::new(
            chrome::genome_x(Controls::CURRENT),
            chrome::BAR_HEIGHT + 1.0,
        );
        let height = (count + extra + 1) as f32 * MENU_ROW + 2.0 * MENU_ROW;
        Some(menu::card(content, at, self.window, height))
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
        let (panel, x) = session.locate(at.x, self.width);
        let (contig, viewport) = session.view(panel);
        let place = Place {
            position,
            count: session.tracks.len(),
            at,
            window: self.window,
            panel,
            base: viewport.to_bp(f64::from(x)).floor().max(0.0) as u64,
        };
        let mode = track.mode_at(panel, contig);
        let links = links(session, track);
        Some(menu::view(
            track,
            mode,
            &place,
            self.range_draft.as_ref(),
            links,
        ))
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
            let width = self.sidebar_width();
            if width < 0.5 {
                return self.browser(session);
            }
            let line = container(space())
                .width(1)
                .height(Length::Fill)
                .style(|theme| container::background(tokens(theme).line));
            let panel = container(self.sidebar_view(session))
                .width(width)
                .height(Length::Fill)
                .clip(true);
            return row![self.browser(session), line, panel].into();
        }
        let message = match (&self.status, &self.indexing) {
            (Status::Idle, _) => text(WELCOME).style(style::muted),
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
            let name = last
                .genome
                .as_deref()
                .and_then(genome::find)
                .map_or_else(|| last.reference.name(), |genome| genome.label);
            let detail = format!("{name}{locus}{tracks}");
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
        let mut lists = row![genomes()].spacing(48);
        if !self.recent.entries().is_empty() {
            let files = self.recent.entries().iter().filter_map(|entry| {
                let (name, place, message) = match entry {
                    Entry::Location(location) => (
                        location.name(),
                        location.place(),
                        Message::Reopen(location.clone()),
                    ),
                    Entry::Genome(id) => {
                        let genome = genome::find(id)?;
                        (
                            genome.label,
                            format!("{} {}", genome.species, genome.assembly),
                            Message::Genome(genome.id),
                        )
                    }
                };
                let row = row![
                    text(name).size(12),
                    text(place).size(11).style(style::muted)
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                Some(
                    button(row)
                        .padding([3, 8])
                        .style(style::ghost)
                        .on_press(message)
                        .into(),
                )
            });
            lists = lists.push(
                column![text("Recent").size(11).style(style::muted)]
                    .extend(files)
                    .spacing(2),
            );
        }
        center(welcome.push(lists)).into()
    }

    pub(super) fn tracks<'a>(&'a self, session: &'a Session, repack: bool) -> Element<'a, Message> {
        let reading = session.reading;
        let padding = self.settings.density.padding();
        let frames = reading.frames();
        let codon_height = frames.len() as f32 * translation::ROW_HEIGHT;
        let strands = across(session.count(), BASE_HEIGHT + codon_height, |panel| {
            let (contig, viewport) = session.view(panel);
            let visible = viewport.visible();
            let bases = sequence::scene(
                viewport,
                self.tiles.chunks(contig, visible),
                BASE_HEIGHT,
                reading.reverse,
            );
            let mut strands = column![scene::view(bases, BASE_HEIGHT)];
            if !frames.is_empty() {
                let chunks = self.tiles.chunks(contig, visible);
                let codons = translation::scene(viewport, chunks, &frames);
                strands = strands.push(scene::view(codons, codon_height));
            }
            strands.into()
        });
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
            if track.over.is_some() {
                continue;
            }
            let draft = self
                .renaming
                .as_ref()
                .filter(|(id, _)| !repack && *id == track.id)
                .map(|(_, draft)| draft.as_str());
            data = data
                .push(track_view(session, track, repack, padding, draft))
                .push(line(index + 1));
        }
        data.into()
    }

    fn ruler_at<'a>(&self, session: &Session, panel: usize) -> Element<'a, Message> {
        let (_, viewport) = session.view(panel);
        let mut primitives = Vec::new();
        if let Some((swept, start, end)) = self.sweep
            && swept == panel
        {
            let from = viewport.to_px(start.min(end)) as f32;
            let to = viewport.to_px(start.max(end)) as f32;
            primitives.push(Primitive::Rect {
                x: from,
                y: 0.0,
                width: to - from,
                height: RULER_HEIGHT,
                paint: Paint::Selection,
            });
        }
        primitives.extend(ruler::scene(viewport, RULER_HEIGHT));
        let ruler = scene::view(primitives, RULER_HEIGHT)
            .on_press(move |share| Message::Ruler(panel, Sweep::Pressed(share)))
            .on_drag(move |share| Message::Ruler(panel, Sweep::Dragged(share)))
            .on_release(Message::Ruler(panel, Sweep::Released))
            .crosshair();
        if panel > 0 {
            return container(ruler).id(format!("ruler-{panel}")).into();
        }
        container(Context::new(ruler, |at| Message::CopyMenu(None, at)))
            .id("ruler")
            .into()
    }

    fn overview(session: &Session) -> Element<'_, Message> {
        let bands = session.reference.bands(session.contig);
        let strip = scene::view(
            overview::scene(session.viewport, OVERVIEW_HEIGHT, bands),
            OVERVIEW_HEIGHT,
        )
        .on_press(|share| Message::Overview(Sweep::Pressed(share)))
        .on_drag(|share| Message::Overview(Sweep::Dragged(share)))
        .on_release(Message::Overview(Sweep::Released))
        .on_hover(Message::Strip);
        container(strip).id("overview").into()
    }

    fn browser<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let count = session.count();
        let mut top = column![];
        if count == 1 {
            top = top.push(Self::overview(session));
        } else {
            top = top
                .push(across(count, HEADER_HEIGHT, |panel| header(session, panel)))
                .push(hairline());
        }
        top = top.push(across(count, RULER_HEIGHT, |panel| {
            self.ruler_at(session, panel)
        }));
        if count == 1 {
            top = top.push(self.bands(session));
        }
        let tracks = top
            .push(hairline())
            .push(
                scrollable(self.tracks(session, false))
                    .direction(Direction::Vertical(
                        Scrollbar::new().width(6).scroller_width(6).margin(0),
                    ))
                    .style(style::scrollbar)
                    .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                    .height(Length::Fill),
            )
            .height(Length::Fill);
        Navigator::new(tracks, self.width)
            .panels(count, PANEL_GAP)
            .on_pan(Message::Pan)
            .on_zoom(Message::Zoom)
            .on_resize(Message::Resized)
            .into()
    }
}

pub(super) fn across<'a>(
    count: usize,
    height: f32,
    each: impl Fn(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    if count == 1 {
        return each(0);
    }
    let mut columns = row![];
    for panel in 0..count {
        if panel > 0 {
            columns = columns.push(
                container(space())
                    .width(PANEL_GAP)
                    .height(height)
                    .style(|theme| container::background(tokens(theme).line)),
            );
        }
        columns = columns.push(container(each(panel)).width(Length::FillPortion(1)));
    }
    columns.into()
}

fn header(session: &Session, panel: usize) -> Element<'_, Message> {
    row![
        button(text(session.locus_at(panel)).size(11).font(font::MONO))
            .padding([2, 6])
            .width(Length::Fill)
            .style(style::ghost)
            .on_press(Message::Panel(panel, Paneling::Only)),
        button(icon::view(Icon::Close, 10.0))
            .width(16)
            .height(16)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Panel(panel, Paneling::Close)),
    ]
    .height(HEADER_HEIGHT)
    .padding([0, 4])
    .align_y(Alignment::Center)
    .into()
}

pub(super) enum Body {
    Hint(String),
    Space(f32),
    Scene(Vec<Primitive>, f32),
    Reads {
        coverage: Vec<Primitive>,
        reads: Option<(Vec<Primitive>, f32)>,
    },
}

fn track_body<'a>(
    session: &Session,
    track: &'a Track,
    panel: usize,
    shape: Shape,
    repack: bool,
) -> Element<'a, Message> {
    let id = track.id;
    let interactive = |primitives, height| {
        scene::view(primitives, height)
            .on_hover(move |pointer| Message::TrackHovered(id, panel, pointer))
            .on_click(move |pointer| Message::TrackClicked(id, panel, pointer))
    };
    match body(session, track, panel, shape, repack) {
        Body::Hint(words) => hint(words),
        Body::Space(height) => space().width(Length::Fill).height(height).into(),
        Body::Scene(primitives, height) => interactive(primitives, height).into(),
        Body::Reads { coverage, reads } => {
            let mut body = column![scene::view(coverage, alignments::COVERAGE_HEIGHT)].spacing(1);
            if let Some((primitives, height)) = reads {
                body = body.push(interactive(primitives, height));
            }
            body.into()
        }
    }
}

pub(super) fn body(
    session: &Session,
    track: &Track,
    panel: usize,
    shape: Shape,
    repack: bool,
) -> Body {
    let body = drawing(session, track, panel, shape, repack);
    let count = match &body {
        Body::Scene(primitives, _) => primitives.len(),
        Body::Reads { coverage, reads } => {
            coverage.len() + reads.as_ref().map_or(0, |(primitives, _)| primitives.len())
        }
        Body::Hint(_) | Body::Space(_) => 0,
    };
    if count > DRAW_LIMIT {
        return Body::Hint(CROWDED_HINT.to_owned());
    }
    body
}

fn drawing(session: &Session, track: &Track, panel: usize, shape: Shape, repack: bool) -> Body {
    let (contig, viewport) = session.view(panel);
    let slot = track.slot(panel);
    let mode = track.mode_at(panel, contig);
    if let Some(error) = &slot.error {
        return Body::Hint(error.clone());
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
        return Body::Hint(words.to_owned());
    }
    match slot.data(contig).map(|data| &data.content) {
        Some(Content::Signal(data)) => {
            let height = track.height.unwrap_or(shape.height);
            let columns = signal_columns(track, viewport, &data.summaries);
            let Some(scale) = signal_scale(session, track, panel, &columns) else {
                return Body::Space(height);
            };
            let width = viewport.width() as f32;
            let members = members(session, track);
            let primitives = if members.is_empty() {
                signal::scene(&columns, scale, track.plot.style, track.hue, height, width)
            } else {
                let others: Vec<(Vec<signal::Column>, Hue)> = members
                    .iter()
                    .filter_map(|member| {
                        Some((member_columns(session, member, panel)?, member.hue))
                    })
                    .collect();
                let mut layers: Vec<(&[signal::Column], Hue)> = vec![(&columns, track.hue)];
                layers.extend(
                    others
                        .iter()
                        .map(|(columns, hue)| (columns.as_slice(), *hue)),
                );
                signal::traces(&layers, scale, height, width)
            };
            Body::Scene(primitives, height)
        }
        Some(Content::Variants(calls)) => {
            let primitives = variants::scene(viewport, &calls.variants, mode);
            Body::Scene(primitives, shape.height)
        }
        Some(Content::Features(annotation)) => {
            let packed;
            let rows = if repack {
                packed = annotation.packed(viewport.visible()).0;
                packed.as_slice()
            } else {
                annotation.rows.as_slice()
            };
            let primitives = features::scene(
                viewport,
                &annotation.features,
                rows,
                mode,
                track.hue,
                track.plot.colors,
            );
            Body::Scene(primitives, shape.height)
        }
        Some(Content::Reads(pile)) => {
            let coverage = alignments::coverage(
                viewport,
                &pile.coverage,
                |at| pile.base(at),
                alignments::COVERAGE_HEIGHT,
            );
            let mut reads = None;
            if mode != features::Mode::Collapsed {
                let (packed, bands);
                let (rows, groups) = if repack {
                    let (rows, _, groups) = pile.packed(viewport.visible());
                    let starts: Vec<usize> = groups.iter().map(|(first, _)| *first).collect();
                    packed = match &slot.order {
                        Some((at, _)) => sort_rows(&pile.reads, &rows, &starts, *at),
                        None => rows,
                    };
                    bands = groups;
                    (packed.as_slice(), bands.as_slice())
                } else {
                    (slot.rows(pile), pile.groups.as_slice())
                };
                let pairs = track.plot.pairs;
                let primitives =
                    alignments::reads(viewport, &pile.reads, rows, groups, mode, pairs);
                let height = shape.height - alignments::COVERAGE_HEIGHT - 1.0;
                reads = Some((primitives, height));
            }
            Body::Reads { coverage, reads }
        }
        None => Body::Space(EMPTY_HEIGHT),
    }
}

fn track_view<'a>(
    session: &'a Session,
    track: &'a Track,
    repack: bool,
    padding: f32,
    draft: Option<&'a str>,
) -> Element<'a, Message> {
    let id = track.id;
    let count = session.count();
    let (shapes, tallest) = shapes(session, track, repack);
    let height = track.height.unwrap_or(tallest.height);
    let shape = tallest;
    let body = across(count, height, |panel| {
        track_body(session, track, panel, shapes[panel], repack)
    });
    let edge = Grip::new(
        space().width(Length::Fill).height(EDGE),
        move |delta| Message::Lane(id, Lane::Resized(delta)),
        Message::Lane(id, Lane::Released),
    )
    .on_double_click(Message::Lane(id, Lane::Fit))
    .interaction(mouse::Interaction::ResizingVertically);
    let handle: Element<'a, Message> = match draft {
        Some(draft) => Escape::new(
            text_input("Name", draft)
                .id(TRACK_NAME)
                .size(LABEL_SIZE)
                .padding([0, 4])
                .width(NAME_WIDTH)
                .style(style::locus)
                .on_input(|text| Message::Rename(Renaming::Draft(text)))
                .on_submit(Message::Rename(Renaming::Done)),
            Message::Rename(Renaming::Cancel),
        )
        .into(),
        None => Grip::new(
            chrome::tip(
                name_label(session, track),
                "Drag to move · Double-click to rename · Right-click for options",
                tooltip::Position::Right,
            ),
            move |delta| Message::Lane(id, Lane::Moved(delta)),
            Message::Lane(id, Lane::Dropped),
        )
        .on_double_click(Message::Rename(Renaming::Start(id)))
        .into(),
    };
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
pub(super) struct Shape {
    pub(super) height: f32,
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

    pub(super) fn hidden(self, height: f32) -> usize {
        if self.row_height <= 0.0 {
            return 0;
        }
        let shown = ((height - self.fixed).max(0.0) / self.row_height).floor() as usize;
        self.rows.saturating_sub(shown)
    }
}

pub(super) fn shapes(session: &Session, track: &Track, repack: bool) -> (Vec<Shape>, Shape) {
    let shapes: Vec<Shape> = (0..session.count())
        .map(|panel| shape(session, track, panel, repack))
        .collect();
    let tallest = shapes
        .iter()
        .copied()
        .max_by(|a, b| a.height.total_cmp(&b.height))
        .unwrap_or(Shape::plain(EMPTY_HEIGHT));
    (shapes, tallest)
}

fn shape(session: &Session, track: &Track, panel: usize, repack: bool) -> Shape {
    let (contig, viewport) = session.view(panel);
    let slot = track.slot(panel);
    if slot.error.is_some() || viewport.visible().len() > track.source.span() {
        return Shape::plain(EMPTY_HEIGHT);
    }
    let mode = track.mode_at(panel, contig);
    match slot.data(contig).map(|data| &data.content) {
        Some(Content::Features(annotation)) => {
            let rows = if mode == features::Mode::Collapsed {
                1
            } else if repack {
                annotation.packed(viewport.visible()).1.max(1)
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
            let rows = variants::rows(calls.samples);
            Shape {
                height: fixed + rows as f32 * row_height,
                rows,
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
    panel: usize,
    columns: &[signal::Column],
) -> Option<signal::Scale> {
    if let Some(range) = track.plot.range {
        return Some(range);
    }
    let members = members(session, track);
    if !members.is_empty() {
        return members
            .iter()
            .filter_map(|member| signal::range(&member_columns(session, member, panel)?))
            .chain(signal::range(columns))
            .reduce(signal::Scale::union);
    }
    if !track.plot.linked {
        return signal::range(columns);
    }
    session
        .tracks
        .iter()
        .filter(|other| other.plot.linked)
        .filter_map(|other| signal::range(&member_columns(session, other, panel)?))
        .reduce(signal::Scale::union)
}

pub(super) fn members<'a>(session: &'a Session, track: &Track) -> Vec<&'a Track> {
    session
        .tracks
        .iter()
        .filter(|other| other.over == Some(track.id))
        .collect()
}

fn member_columns(session: &Session, member: &Track, panel: usize) -> Option<Vec<signal::Column>> {
    let (contig, viewport) = session.view(panel);
    let Content::Signal(data) = &member.slot(panel).data(contig)?.content else {
        return None;
    };
    Some(signal_columns(member, viewport, &data.summaries))
}

pub(super) fn links(session: &Session, track: &Track) -> (&'static str, Vec<(String, Message)>) {
    if !track.source.has_signal() || track.over.is_some() {
        return ("", Vec::new());
    }
    let members = members(session, track);
    if !members.is_empty() {
        let items = members
            .iter()
            .map(|member| {
                let change = Message::TrackChanged(member.id, Change::Separate);
                (member.name.clone(), change)
            })
            .collect();
        return ("Separate", items);
    }
    let items = session
        .tracks
        .iter()
        .filter(|other| other.id != track.id && other.over.is_none() && other.source.has_signal())
        .map(|other| {
            let change = Message::TrackChanged(track.id, Change::Overlay(other.id));
            (other.name.clone(), change)
        })
        .collect();
    ("Overlay on", items)
}

pub(super) fn body_height(session: &Session, track: &Track) -> f32 {
    track.height.unwrap_or_else(|| {
        (0..session.count())
            .map(|panel| shape(session, track, panel, false).height)
            .fold(0.0, f32::max)
    })
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

pub(super) fn ruler_view<'a>(session: &Session, panel: usize) -> Element<'a, Message> {
    let (_, viewport) = session.view(panel);
    let ruler = container(scene::view(
        ruler::scene(viewport, RULER_HEIGHT),
        RULER_HEIGHT,
    ));
    if panel > 0 {
        return ruler.id(format!("ruler-{panel}")).into();
    }
    ruler.id("ruler").into()
}

pub(super) fn panel_label<'a>(session: &Session, panel: usize) -> Element<'a, Message> {
    container(text(session.locus_at(panel)).size(11).font(font::MONO))
        .padding([0, 10])
        .height(HEADER_HEIGHT)
        .align_y(Alignment::Center)
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
        .style(chip)
        .into()
}

fn name_label<'a>(session: &'a Session, track: &'a Track) -> Element<'a, Message> {
    let members = members(session, track);
    if members.is_empty() {
        return label(&track.name);
    }
    let names = std::iter::once(track).chain(members).map(|track| {
        let hue = track.hue;
        text(track.name.as_str())
            .size(LABEL_SIZE)
            .style(move |theme: &Theme| text::Style {
                color: Some(crate::theme::data(theme).hues[hue as usize]),
            })
            .into()
    });
    container(row(names).spacing(6))
        .padding([0, 4])
        .style(chip)
        .into()
}

fn chip(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(tokens(theme).canvas.scale_alpha(scene::CHIP).into()),
        border: Border::default().rounded(3),
        ..container::Style::default()
    }
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
