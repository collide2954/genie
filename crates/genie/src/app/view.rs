use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use genie_core::isoform::Isoform;
use genie_core::pileup::sort_rows;
use genie_core::{Feature, Region, Strand, Viewport, grouped, trio};
use genie_formats::fields::{Rule, Test};
use genie_render::scene::{Hue, Paint, Primitive};
use genie_render::{
    alignments, contacts, features, fractions, ld, links, manhattan, matches, overview, ratio,
    ruler, samples, sashimi, segments, sequence, signal, translation, variants,
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
use crate::menu::{self, Place, Section};
use crate::navigator::Navigator;
use crate::reading;
use crate::recent::Entry;
use crate::settings;
use crate::theme::{Appearance, tokens};
use crate::tiles;
use crate::track::{self, Calls, Change, Content, GroupKey, Loaded, Pile, Track, junctions};
use crate::{font, scene, style};

use super::tracks::TRACK_NAME;
use super::{
    Celling, Filtering, Genie, Lane, Message, Notice, PANEL_GAP, Paneling, Popup, Preparation,
    Preparing, Renaming, Session, Status, Suggestion, Sweep, whole,
};

const OVERVIEW_HEIGHT: f32 = 12.0;
pub(super) const HEADER_HEIGHT: f32 = 20.0;
const NAME_WIDTH: f32 = 160.0;
const TRACK_BUTTON: f32 = 16.0;
const TRACK_ICON: f32 = 12.0;
pub(super) const RULER_HEIGHT: f32 = 20.0;
pub(super) const BASE_HEIGHT: f32 = 16.0;
pub(super) const EMPTY_HEIGHT: f32 = 20.0;
const EDGE: f32 = 4.0;
pub(super) const LABEL_SIZE: f32 = 10.0;
const READ_HINT: &str = "Zoom in to see the reads";
pub(super) const VARIANT_HINT: &str = "Zoom in to see the variants";
pub(super) const SIGNAL_HINT: &str = "Zoom in to see the signal";
pub(super) const LINK_HINT: &str = "Zoom in to see the links";
pub(super) const DROP_HINT: &str = "Drop the file to open it";
pub(super) const ZOOM_HINT: &str = "Zoom in to see the features";
const CROWDED_HINT: &str = "There is too much to draw here. Zoom in.";
const FILTER_HINT: &str = "Zoom in to use the filter";
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
    let other = button(text("Other NCBI assemblies…").size(12))
        .padding([3, 8])
        .style(style::ghost)
        .on_press(Message::Assemblies(super::Assembling::Open));
    column![text("Genomes").size(11).style(style::muted)]
        .extend(rows)
        .push(other)
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
                    Some(Content::Links(links)) => features += links.len(),
                    Some(Content::Junctions(found)) => features += found.junctions.len(),
                    Some(Content::Segments(copies)) => features += copies.segments.len(),
                    Some(Content::Contacts(contacts)) => values += contacts.map.contacts.len(),
                    Some(Content::Associations(found)) => sites += found.len(),
                    Some(Content::Variants(calls)) => sites += calls.variants.len(),
                    Some(Content::Signal(data)) => values += data.summaries.len(),
                    Some(Content::Cells(lanes)) => values += lanes.lanes.len(),
                    Some(Content::Ratio(found)) => values += found.bins.len(),
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
                room: chrome::locus_room(Controls::CURRENT, self.window.width),
                narrow: chrome::narrow(Controls::CURRENT, self.window.width),
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
            let gnomad = self.gnomad.part(pinned);
            layers = layers.push(card::view_with(pinned, true, self.window, gnomad));
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
        let notice = self.notice.as_ref();
        if progress.is_some() || notice.is_some() {
            layers = layers.push(toasts(progress, notice));
        }
        let panel = || {
            (self.popup == Some(Popup::Settings)).then(|| {
                let inset = chrome::settings_inset(Controls::CURRENT);
                let updates = settings::Updates {
                    automatic: self.checks.automatic,
                    line: self.version_line(),
                    download: matches!(self.release, crate::update::State::Available(_)),
                };
                settings::panel(self.settings, inset, self.store(), updates)
            })
        };
        if let Some(menu) = self
            .track_menu()
            .or_else(|| self.reading_menu())
            .or_else(|| self.copy_menu())
            .or_else(|| self.folder_menu())
            .or_else(panel)
            .or_else(|| self.address_view())
            .or_else(|| self.genome_menu())
            .or_else(|| self.catalog_menu())
            .or_else(|| self.field_menu())
            .or_else(|| (self.popup == Some(Popup::Help)).then(crate::help::view))
            .or_else(|| self.app_menu())
            .or_else(|| self.dialog())
            .or_else(|| self.palette_view())
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

    /// The open dialog, if one is open.
    fn dialog(&self) -> Option<Element<'_, Message>> {
        self.track_dialog()
            .or_else(|| self.export_dialog())
            .or_else(|| self.assembly_dialog())
            .or_else(|| self.blat_dialog())
            .or_else(|| self.enzyme_dialog())
            .or_else(|| self.motif_dialog_view())
            .or_else(|| self.primer_dialog())
            .or_else(|| self.hub_dialog())
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
        let ncbi = Message::Assemblies(super::Assembling::Open);
        let mut content = Column::new()
            .extend(items)
            .push(menu::separator())
            .push(menu::item("Open reference…", Message::Open, false, true))
            .push(menu::item("NCBI assembly…", ncbi, false, true));
        if extra > 0 {
            content = content.push(menu::separator()).extend(offline);
        }
        let at = Point::new(
            chrome::genome_x(Controls::CURRENT),
            chrome::BAR_HEIGHT + 1.0,
        );
        let height = (count + extra + 2) as f32 * MENU_ROW + 2.0 * MENU_ROW;
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

    /// The tracks with the variant table under them, the tracks panel at their left and the
    /// region list at their right.
    fn session_body<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let main = if let Some(plot) = session.shown_plot() {
            super::dotplot::plot_view(session, plot)
        } else {
            let mut main = column![self.browser(session)];
            if let Some(bar) = self.review_bar(session) {
                main = main.push(hairline()).push(bar);
            }
            let table = self
                .table_view(session)
                .or_else(|| self.dmr_view(session))
                .or_else(|| self.isoform_view(session))
                .or_else(|| self.splice_view(session));
            if let Some(table) = table {
                main = main.push(table);
            }
            main.into()
        };
        let line = || {
            container(space())
                .width(1)
                .height(Length::Fill)
                .style(|theme| container::background(tokens(theme).line))
        };
        let mut body = row![];
        if let Some(list) = self.track_list_view(session) {
            let list = container(list)
                .width(super::track_list::WIDTH)
                .height(Length::Fill)
                .clip(true);
            body = body.push(list).push(line());
        }
        body = body.push(main);
        let width = self.sidebar_width();
        if width >= 0.5 {
            let panel = container(self.sidebar_view(session))
                .width(width)
                .height(Length::Fill)
                .clip(true);
            body = body.push(line()).push(panel);
        }
        body.into()
    }

    fn body(&self) -> Element<'_, Message> {
        if let Some(session) = &self.session {
            return self.session_body(session);
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
        let padding = self.settings.density.padding();
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
        let mut data = if session.genome {
            column![line(0)]
        } else {
            column![self.sequence_row(session), line(0)]
        };
        for (index, track) in session.tracks.iter().enumerate() {
            if track.over.is_some() || track.hidden {
                continue;
            }
            let draft = self
                .renaming
                .as_ref()
                .filter(|(id, _)| !repack && *id == track.id)
                .map(|(_, draft)| draft.as_str());
            data = data
                .push(track_view(
                    session,
                    track,
                    (repack, padding),
                    draft,
                    !repack && self.lane_hover == Some(track.id),
                ))
                .push(line(index + 1));
        }
        if !repack {
            for item in &session.preparing {
                data = data.push(preparing_lane(item)).push(hairline());
            }
        }
        data.into()
    }

    /// The bases of the reference and their reading frames.
    fn sequence_row<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let reading = session.reading;
        let padding = self.settings.density.padding();
        let frames = reading.frames();
        let codon_height = frames.len() as f32 * translation::ROW_HEIGHT;
        let strands = across(&session.portions(), BASE_HEIGHT + codon_height, |panel| {
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
        container(sequence).id("sequence").into()
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
        if !session.genome {
            primitives.extend(ruler::scene(viewport, RULER_HEIGHT));
        }
        let ruler = scene::view(primitives, RULER_HEIGHT)
            .on_press(move |share| Message::Ruler(panel, Sweep::Pressed(share)))
            .on_drag(move |share| Message::Ruler(panel, Sweep::Dragged(share)))
            .on_release(Message::Ruler(panel, Sweep::Released))
            .crosshair();
        if session.genome {
            let (contig, _) = session.view(panel);
            let contig = &session.reference.assembly.contigs()[contig].name;
            let name = whole::name(contig, viewport.width() as f32).unwrap_or_default();
            let label = container(text(name).size(11).style(style::muted))
                .center_x(Length::Fill)
                .center_y(RULER_HEIGHT);
            return container(stack![ruler, label])
                .id(format!("ruler-{panel}"))
                .into();
        }
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
        let portions = session.portions();
        let mut top = column![];
        if count == 1 {
            top = top.push(Self::overview(session));
        } else if !session.genome {
            top = top
                .push(across(&portions, HEADER_HEIGHT, |panel| {
                    header(session, panel)
                }))
                .push(hairline());
        }
        top = top.push(across(&portions, RULER_HEIGHT, |panel| {
            self.ruler_at(session, panel)
        }));
        if count == 1 {
            top = top.push(self.bands(session));
        }
        if let Some(bar) = self.find_bar() {
            top = top.push(hairline()).push(bar);
            if !session.genome {
                top = top.push(across(&portions, matches::HEIGHT, |panel| {
                    self.find_lane(session, panel)
                }));
            }
        }
        if self.settings.tips {
            top = top.push(hairline()).push(crate::help::strip());
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

/// A row of panels with a line between them. Each panel gets its share of the width.
pub(super) fn across<'a>(
    portions: &[u16],
    height: f32,
    each: impl Fn(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    if portions.len() <= 1 {
        return each(0);
    }
    let mut columns = row![];
    for (panel, portion) in portions.iter().enumerate() {
        if panel > 0 {
            columns = columns.push(
                container(space())
                    .width(PANEL_GAP)
                    .height(height)
                    .style(|theme| container::background(tokens(theme).line)),
            );
        }
        columns = columns.push(
            container(each(panel))
                .width(Length::FillPortion(*portion))
                .clip(true),
        );
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
    let genome = session.genome;
    let interactive = |primitives, height| {
        let view = scene::view(primitives, height)
            .on_hover(move |pointer| Message::TrackHovered(id, panel, pointer));
        if genome {
            // A drag selects a region of the chromosome, and a click opens it, as on the ruler.
            view.on_press(move |share| Message::Ruler(panel, Sweep::Pressed(share)))
                .on_drag(move |share| Message::Ruler(panel, Sweep::Dragged(share)))
                .on_release(Message::Ruler(panel, Sweep::Released))
                .crosshair()
        } else {
            view.on_click(move |pointer| Message::TrackClicked(id, panel, pointer))
        }
    };
    let content = match body(session, track, panel, shape, repack) {
        Body::Hint(words) => hint(words),
        Body::Space(height) => space().width(Length::Fill).height(height).into(),
        Body::Scene(mut primitives, height) => {
            if session.genome {
                primitives.retain(|primitive| !matches!(primitive, Primitive::Label { .. }));
            }
            interactive(primitives, height).into()
        }
        Body::Reads { coverage, reads } => {
            let mut body = column![scene::view(coverage, alignments::COVERAGE_HEIGHT)].spacing(1);
            if let Some((primitives, height)) = reads {
                body = body.push(interactive(primitives, height));
            }
            body.into()
        }
    };
    match count_chip(session, track, panel).filter(|_| !repack) {
        Some(chip) => stack![
            content,
            container(chip)
                .width(Length::Fill)
                .align_right(Length::Fill)
                .padding(edge_padding()),
        ]
        .into(),
        None => content,
    }
}

fn in_view(data: &Loaded, visible: Region) -> Option<usize> {
    match &data.content {
        Content::Features(annotation) => Some(
            annotation
                .features
                .iter()
                .filter(|feature| feature.span().intersection(visible).is_some())
                .count(),
        ),
        Content::Variants(calls) => Some(
            calls
                .variants
                .iter()
                .filter(|variant| variant.span.intersection(visible).is_some())
                .count(),
        ),
        Content::Links(links) => Some(
            links
                .iter()
                .filter(|link| link.span().overlaps(visible))
                .count(),
        ),
        Content::Junctions(found) => Some(
            found
                .junctions
                .iter()
                .filter(|junction| junction.span.overlaps(visible))
                .count(),
        ),
        Content::Reads(_)
        | Content::Signal(_)
        | Content::Segments(_)
        | Content::Contacts(_)
        | Content::Associations(_)
        | Content::Cells(_)
        | Content::Ratio(_) => None,
    }
}

/// The chip at the right of a track: the comparison of a signal track, or the number of the
/// features that a filter keeps. A click opens the track menu.
fn count_chip<'a>(session: &Session, track: &Track, panel: usize) -> Option<Element<'a, Message>> {
    let id = track.id;
    if let Some(math) = track.plot.math {
        let other = session.tracks.iter().find(|other| other.id == math.other)?;
        let words = format!("{} to {}", math.operation.label(), other.name);
        return Some(Context::left(label(words), move |at| Message::TrackMenu(id, at)).into());
    }
    if track.filter.rules.is_empty() {
        return None;
    }
    let (contig, viewport) = session.view(panel);
    let visible = viewport.visible();
    let slot = track.slot(panel);
    let all = slot
        .loaded
        .as_ref()
        .filter(|(loaded, _)| *loaded == contig)?;
    let counts = match &all.1.content {
        Content::Signal(signal) => signal.kept.as_ref().map(|kept| kept.counts(visible)),
        _ => slot
            .data(contig)
            .and_then(|data| in_view(data, visible))
            .zip(in_view(&all.1, visible)),
    };
    let words = counts.map_or_else(
        || FILTER_HINT.to_owned(),
        |(shown, total)| format!("{} of {}", grouped(shown as u64), grouped(total as u64)),
    );
    Some(Context::left(label(words), move |at| Message::TrackMenu(id, at)).into())
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

/// The bars or lines of a signal track, with the tracks that overlay it.
fn signal_body(
    session: &Session,
    track: &Track,
    panel: usize,
    summaries: &[genie_core::signal::Summary],
    height: f32,
) -> Body {
    let (_, viewport) = session.view(panel);
    let columns = shown_columns(session, track, panel, summaries);
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
            .filter_map(|member| Some((member_columns(session, member, panel)?, member.hue)))
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
        } else if track.source.has_links() {
            LINK_HINT
        } else {
            ZOOM_HINT
        };
        return Body::Hint(words.to_owned());
    }
    let baf = slot.data(contig).filter(|data| data.options.baf.is_some());
    match slot.data(contig).map(|data| &data.content) {
        Some(Content::Signal(_)) if track.plot.math.is_some_and(is_methylation) => {
            methylation_body(session, track, panel)
        }
        Some(Content::Signal(data)) => {
            if let Some(loaded) = baf {
                return density_body(session, track, panel, (&data.summaries, loaded));
            }
            let height = track.height.unwrap_or(shape.height);
            signal_body(session, track, panel, &data.summaries, height)
        }
        Some(Content::Variants(calls)) => {
            let height = shape.height;
            variants_body(session, track, panel, (calls, baf), height)
        }
        Some(Content::Links(found)) => {
            let height = track.height.unwrap_or(shape.height);
            let look = (track.hue, track.plot.colors);
            Body::Scene(links::scene(viewport, found, look, height), height)
        }
        Some(Content::Junctions(found)) => {
            let height = track.height.unwrap_or(shape.height);
            let arcs = alignments::arcs_in(viewport, &found.junctions, height);
            Body::Scene(arcs, height)
        }
        Some(Content::Segments(copies)) => {
            let names = track.source.samples();
            let range = segment_range(track);
            let primitives =
                segments::scene(viewport, &copies.segments, names, mode, range, &track.rows);
            Body::Scene(primitives, shape.height)
        }
        Some(Content::Contacts(contacts)) => {
            let (map, total) = contact_heights(track);
            let full = track.plot.range.map(|range| range.max);
            let mut primitives = contacts::scene(viewport, &contacts.map, map, full);
            if let Some(viewpoint) = track.plot.viewpoint {
                let lane = contacts::viewpoint_lane(viewport, &contacts.map, viewpoint, track.hue);
                let top = map + VIEWPOINT_GAP;
                primitives.extend(lane.into_iter().map(|primitive| primitive.lowered(top)));
            }
            Body::Scene(primitives, total)
        }
        Some(Content::Associations(found)) => {
            let faint = session.genome && panel % 2 == 1;
            manhattan_body((session, panel), found, track, (shape.height, faint))
        }
        Some(Content::Features(annotation)) => {
            features_body(viewport, track, annotation, mode, (shape, repack))
        }
        Some(Content::Reads(pile)) => reads_body(session, track, panel, pile, (shape, repack)),
        Some(Content::Cells(lanes)) => {
            let primitives = genie_render::cells::scene(viewport, &lanes.depths, &lanes.lanes)
                .into_iter()
                .map(|primitive| primitive.lowered(genie_render::cells::HEADER))
                .collect();
            Body::Scene(primitives, shape.height)
        }
        Some(Content::Ratio(found)) => {
            let range = track.plot.range.map_or(ratio::RANGE, |range| range.max);
            let data = (&found.bins[..], &found.segments[..]);
            Body::Scene(
                ratio::scene(viewport, data, range, shape.height),
                shape.height,
            )
        }
        None => Body::Space(EMPTY_HEIGHT),
    }
}

/// The features of a track in their rows, packed again for the view when the rows change with
/// it, and a note when some rows do not show.
fn features_body(
    viewport: Viewport,
    track: &Track,
    annotation: &track::Annotation,
    mode: features::Mode,
    (shape, repack): (Shape, bool),
) -> Body {
    let packed;
    let (rows, count) = if repack {
        packed = annotation.packed(viewport.visible());
        (packed.0.as_slice(), packed.1)
    } else {
        (annotation.rows.as_slice(), annotation.count)
    };
    let mut primitives = features::scene(
        viewport,
        &annotation.features,
        rows,
        mode,
        track.hue,
        track.plot.colors,
    );
    primitives.extend(features::more_rows(count, mode));
    Body::Scene(primitives, shape.height)
}

/// The sites of a variant track, its breakend arcs, the calls of its samples and, with its data
/// of B-allele frequencies, the lane of the frequencies between the sites and the calls.
fn variants_body(
    session: &Session,
    track: &Track,
    panel: usize,
    (calls, baf): (&Calls, Option<&Loaded>),
    height: f32,
) -> Body {
    let (contig, viewport) = session.view(panel);
    let mode = track.mode_at(panel, contig);
    let arcs = variants::breakend_height(&calls.variants);
    let mut primitives = if arcs > 0.0 {
        let name = &session.reference.assembly.contigs()[contig].name;
        variants::Breakends::new(&calls.variants, name).scene(viewport, track.hue)
    } else {
        Vec::new()
    };
    let (found, rows) = (&calls.variants, &track.rows);
    let strip = review_strip(track);
    if strip > 0.0 {
        let verdicts: Vec<_> = found
            .iter()
            .map(|variant| {
                track
                    .review_of(contig, variant)
                    .map(|review| review.verdict)
            })
            .collect();
        let marks = variants::review_marks(viewport, found, &verdicts);
        primitives.extend(marks.into_iter().map(|primitive| primitive.lowered(arcs)));
    }
    let top = arcs + strip;
    let trios = track.trios_on(contig);
    let mut marks = variants::trio_marks(viewport, found, mode, rows, &trios);
    marks.extend(variants::scene(viewport, found, mode, rows));
    let lane = baf.map_or(0.0, fraction_height);
    let linked = ld_data(track, panel, contig);
    let ld = if linked.is_some() { ld::HEIGHT } else { 0.0 };
    primitives.extend(
        variants::rows_lowered(marks, lane + ld)
            .into_iter()
            .map(|primitive| primitive.lowered(top)),
    );
    let under = top + variants::SITE_HEIGHT;
    if let Some(data) = baf {
        let shown = fraction_lane(session, track, panel, data);
        primitives.extend(shown.into_iter().map(|primitive| primitive.lowered(under)));
    }
    if let Some(data) = linked {
        let shown = ld_lane(session, track, panel, data);
        let below = under + lane;
        primitives.extend(shown.into_iter().map(|primitive| primitive.lowered(below)));
    }
    Body::Scene(primitives, height)
}

/// The loaded data of a variant track with LD, also the variants that its filter hides, for
/// the LD lane.
pub(super) fn ld_data(track: &Track, panel: usize, contig: usize) -> Option<&Loaded> {
    let (loaded, data) = track.slot(panel).loaded.as_ref()?;
    let variants = matches!(data.content, Content::Variants(_));
    (*loaded == contig && variants && data.options.ld.is_some()).then_some(data)
}

/// The height of the LD lane of a variant track, or 0 without the lane.
pub(super) fn ld_height(track: &Track, panel: usize, contig: usize) -> f32 {
    if ld_data(track, panel, contig).is_some() {
        ld::HEIGHT
    } else {
        0.0
    }
}

/// The starts of the loaded variants of LD data, in the order of its matrix.
pub(super) fn ld_starts(data: &Loaded) -> Vec<u64> {
    match &data.content {
        Content::Variants(calls) => calls
            .variants
            .iter()
            .map(|variant| variant.span.start())
            .collect(),
        _ => Vec::new(),
    }
}

/// The LD lane of a variant track, with the samples of its LD.
fn ld_lane(session: &Session, track: &Track, panel: usize, data: &Loaded) -> Vec<Primitive> {
    let (_, viewport) = session.view(panel);
    let samples = track.ld_group.as_deref().unwrap_or("all samples");
    let label = format!("LD r² · {samples}");
    ld::lane(viewport, &ld_starts(data), data.ld.as_deref(), &label)
}

/// The LocusZoom colors of the dots of a GWAS track: the r² of each association with the lead
/// from the first variant track with LD, by the places of its loaded variants, and the place of
/// the lead. The lead is the dot that a card made the lead, when it is in view, or else the top
/// dot in view with a variant.
pub(super) fn locus_zoom(
    session: &Session,
    track: &Track,
    panel: usize,
    found: &[genie_core::association::Association],
) -> Option<(Vec<Option<f32>>, usize)> {
    let (contig, viewport) = session.view(panel);
    let visible = viewport.visible();
    let data = session
        .tracks
        .iter()
        .filter(|other| !other.hidden)
        .find_map(|other| ld_data(other, panel, contig))?;
    let matrix = data.ld.as_deref()?;
    let mut places = std::collections::HashMap::new();
    for (index, start) in ld_starts(data).into_iter().enumerate() {
        places.entry(start).or_insert(index);
    }
    let variant_of = |position: u64| places.get(&position).copied();
    let shown = |position: u64| visible.contains(position);
    let picked = track.lead.and_then(|lead| {
        found
            .iter()
            .position(|association| association.position == lead && shown(lead))
    });
    let lead = picked.or_else(|| {
        found
            .iter()
            .enumerate()
            .filter(|(_, association)| {
                shown(association.position) && variant_of(association.position).is_some()
            })
            .max_by(|a, b| a.1.value.total_cmp(&b.1.value))
            .map(|(index, _)| index)
    })?;
    let lead_variant = variant_of(found[lead].position)?;
    let r2 = found
        .iter()
        .map(|association| matrix.get(lead_variant, variant_of(association.position)?))
        .collect();
    Some((r2, lead))
}

/// The height of the strip of review marks over the sites of a variant track: none until the
/// track has a mark.
pub(super) fn review_strip(track: &Track) -> f32 {
    if track.reviews.is_empty() {
        0.0
    } else {
        variants::REVIEW_HEIGHT
    }
}

/// The density of a variant track in a wide view, with the lane of its B-allele frequencies
/// under it.
fn density_body(
    session: &Session,
    track: &Track,
    panel: usize,
    (summaries, data): (&[genie_core::signal::Summary], &Loaded),
) -> Body {
    let (density, total) = density_heights(track, fraction_height(data));
    let mut primitives = match signal_body(session, track, panel, summaries, density) {
        Body::Scene(primitives, _) => primitives,
        Body::Hint(_) | Body::Space(_) | Body::Reads { .. } => Vec::new(),
    };
    let shown = fraction_lane(session, track, panel, data);
    primitives.extend(
        shown
            .into_iter()
            .map(|primitive| primitive.lowered(density)),
    );
    Body::Scene(primitives, total)
}

/// The height of the lane of the B-allele frequencies of a variant track, or 0 without the
/// lane.
pub(super) fn fraction_height(data: &Loaded) -> f32 {
    if data.options.baf.is_some() {
        fractions::HEIGHT
    } else {
        0.0
    }
}

/// The lowest density of a variant track over the lane of its B-allele frequencies.
const LOWEST_DENSITY: f32 = 12.0;

/// The heights of the density of a variant track and of its whole body, with the lane of its
/// B-allele frequencies under the density. A track height that somebody sets is the height of
/// the whole body.
pub(super) fn density_heights(track: &Track, lane: f32) -> (f32, f32) {
    let total = track.height.unwrap_or(signal::HEIGHT + lane);
    (
        (total - lane).max(LOWEST_DENSITY),
        total.max(LOWEST_DENSITY + lane),
    )
}

/// The lane of the B-allele frequencies of a variant track, with the name of its sample.
fn fraction_lane(session: &Session, track: &Track, panel: usize, data: &Loaded) -> Vec<Primitive> {
    let (_, viewport) = session.view(panel);
    let sample = data
        .options
        .baf
        .and_then(|index| track.source.samples().get(index));
    let label = sample.map_or_else(String::new, |name| format!("B-allele frequency · {name}"));
    fractions::lane(viewport, data.fractions.as_deref(), track.hue, &label)
}

/// The coverage and the reads of a read track, or its Sashimi plot.
fn reads_body(
    session: &Session,
    track: &Track,
    panel: usize,
    pile: &Pile,
    (shape, repack): (Shape, bool),
) -> Body {
    let (contig, viewport) = session.view(panel);
    let looks = shown_splicing(session, track, panel)
        .map_or_else(Vec::new, |shown| splice_looks(&pile.junctions, &shown.1));
    if track.plot.sashimi {
        let top = sashimi_top(session, track, panel);
        let look = (track.hue, track.plot.junction_reads, top);
        let (coverage, junctions) = (&pile.coverage, &pile.junctions);
        let primitives = sashimi::scene_looked(viewport, coverage, junctions, look, &looks);
        return Body::Scene(primitives, sashimi::HEIGHT);
    }
    if track.plot.isoforms {
        return isoform_body(session, track, panel, pile);
    }
    if let Some(lanes) = pile.cells.as_ref().filter(|_| track.plot.cells) {
        return cell_body(session, panel, pile, lanes);
    }
    let mode = track.mode_at(panel, contig);
    let coverage = alignments::coverage(
        viewport,
        &pile.coverage,
        |at| pile.base(at),
        alignments::COVERAGE_HEIGHT,
    );
    let height = shape.height - alignments::COVERAGE_HEIGHT - 1.0;
    let shown = mode != features::Mode::Collapsed
        || !pile.junctions.is_empty()
        || lanes_height(track, pile) > 0.0;
    let reads = shown.then(|| {
        let place = (panel, viewport, mode);
        (read_lane(track, place, pile, &looks, repack), height)
    });
    Body::Reads { coverage, reads }
}

/// The pile of a read track in a panel, and the read track that it compares its splicing with
/// and its pile, when the two have reads in view.
pub(super) fn splicing_piles<'a>(
    session: &'a Session,
    track: &'a Track,
    panel: usize,
) -> Option<(&'a Pile, Option<(&'a Track, &'a Pile)>)> {
    let (contig, _) = session.view(panel);
    let pile = |track: &'a Track| match &track.slot(panel).data(contig)?.content {
        Content::Reads(pile) => Some(&**pile),
        _ => None,
    };
    let other = match track.plot.splicing {
        Some(track::Splice::Track(id)) => {
            let other = session.tracks.iter().find(|other| other.id == id)?;
            Some((other, pile(other)?))
        }
        _ => None,
    };
    Some((pile(track)?, other))
}

/// The splicing difference that a read track shows in a panel.
pub(super) fn shown_splicing(
    session: &Session,
    track: &Track,
    panel: usize,
) -> Option<Arc<track::SpliceDifference>> {
    track.plot.splicing?;
    let (pile, other) = splicing_piles(session, track, panel)?;
    track.splicing_difference(pile, other)
}

/// The looks of the arcs of junctions for a splicing difference: the color of the difference
/// for a junction that differs, grey for the others, and the difference in points as the words.
/// A junction without a difference is grey, without words.
pub(super) fn splice_looks(
    junctions: &[genie_core::pileup::Junction],
    differences: &[genie_core::splicing::Difference],
) -> Vec<Option<alignments::ArcLook>> {
    let place = |span: Region| (span.start(), span.end());
    junctions
        .iter()
        .map(|junction| {
            let at = differences
                .binary_search_by_key(&place(junction.span), |change| place(change.span))
                .ok();
            let found = at.map(|at| &differences[at]);
            let look = match found.and_then(|change| Some((change, change.points()?))) {
                Some((change, points)) => alignments::ArcLook {
                    paint: if change.differs() {
                        Paint::Change(points.round().clamp(-100.0, 100.0) as i8)
                    } else {
                        Paint::Muted
                    },
                    words: format!("{points:+.0}").replace('-', "\u{2212}"),
                },
                None => alignments::ArcLook {
                    paint: Paint::Muted,
                    words: String::new(),
                },
            };
            Some(look)
        })
        .collect()
}

/// The least reads of an isoform with a row of its own.
const ISOFORM_READS: usize = 2;
/// An isoform with a row of its own has at least one read in this number of the spliced reads in
/// view: 1 %.
const ISOFORM_SHARE: usize = 100;
/// How far apart two junctions of one isoform can be, in bases.
pub(super) const SLACK: u64 = 5;
/// How far an end of a junction of a read moves to the end of an intron of an open transcript,
/// in bases.
const SNAP: u64 = 20;
/// The height of the line under the isoforms that counts the other reads.
const OTHERS_HEIGHT: f32 = 16.0;

/// The isoforms of a read track in a panel, as features for the feature lane: each with the name
/// of the transcript of an open feature track that has the same introns, with its ID when the
/// track shows a gene name in place of it, or "novel" with a number, and its reads in view. The
/// line under them counts the other reads.
pub(super) struct Isoforms {
    pub features: Vec<Feature>,
    pub rows: Vec<usize>,
    pub others: String,
}

/// The introns of a transcript: the parts between its exons.
fn introns(feature: &Feature) -> Vec<Region> {
    feature
        .blocks()
        .windows(2)
        .map(|pair| Region::between(pair[0].end(), pair[1].start()))
        .collect()
}

/// The transcripts of the open feature tracks in a panel: their features with two exons or more.
fn transcripts(session: &Session, panel: usize, contig: usize) -> Vec<&Feature> {
    session
        .tracks
        .iter()
        .filter_map(|track| match &track.slot(panel).data(contig)?.content {
            Content::Features(annotation) => Some(&annotation.features),
            _ => None,
        })
        .flatten()
        .filter(|feature| feature.blocks().len() > 1)
        .collect()
}

/// The isoforms of all reads of a pile, the most reads first, after the ends of their junctions
/// snap to the introns of the transcripts. genie finds them again only when the introns change.
fn all_isoforms(pile: &Pile, transcripts: &[&Feature]) -> Arc<Vec<Isoform<GroupKey>>> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut known: Vec<Region> = transcripts
        .iter()
        .flat_map(|transcript| introns(transcript))
        .collect();
    known.sort_unstable_by_key(|intron| (intron.start(), intron.end()));
    known.dedup();
    let mut hasher = DefaultHasher::new();
    known.hash(&mut hasher);
    pile.isoforms.get_or(hasher.finish(), || {
        let snapped = genie_core::isoform::snap(&pile.chains.0, &known, SNAP);
        genie_core::isoform::isoforms(&snapped, SLACK, 1).0
    })
}

/// The introns of each transcript of the open feature tracks in a panel.
pub(super) fn transcript_introns(
    session: &Session,
    panel: usize,
    contig: usize,
) -> Vec<Vec<Region>> {
    transcripts(session, panel, contig)
        .into_iter()
        .map(introns)
        .collect()
}

/// The height of the isoforms of a pile under its coverage: a row for each isoform, and the line
/// of the other reads.
fn isoforms_height(session: &Session, panel: usize, pile: &Pile) -> f32 {
    let rows = isoforms(session, panel, pile).features.len();
    rows as f32 * features::Mode::Expanded.row_height() + OTHERS_HEIGHT
}

/// An isoform in view of a read track: its place in the isoforms of the pile, its name, its ID
/// and its gene for exports, its strand, and its reads in view, also for each group of the
/// reads.
pub(super) struct Seen {
    pub index: usize,
    pub name: String,
    pub id: String,
    pub gene: String,
    pub strand: Strand,
    pub reads: usize,
    pub groups: Vec<(GroupKey, usize)>,
}

/// The isoforms in view of a read track: the isoforms of the pile, the isoforms with reads in
/// view with the most reads first, the least reads of an isoform with a row in the lane, and the
/// reads in view without a splice junction.
pub(super) struct InView {
    pub all: Arc<Vec<Isoform<GroupKey>>>,
    pub seen: Vec<Seen>,
    pub least: usize,
    pub unspliced: usize,
}

/// The name, the ID, the gene and the strand of an isoform.
struct Named {
    name: String,
    id: String,
    gene: String,
    strand: Strand,
}

/// The name, the ID, the gene and the strand of an isoform: those of the transcript with the
/// same introns, with an ID before one without. Else the isoform is novel: "novel" with the next
/// number of the gene of the transcript that has the most of its junctions, or of no gene. The
/// isoform keeps the strand of its reads when they give one.
fn isoform_name(
    transcripts: &[&Feature],
    isoform: &Isoform<GroupKey>,
    novel: &mut HashMap<String, usize>,
) -> Named {
    let strand = |transcript: &Feature| match isoform.strand {
        Strand::Unknown => transcript.strand(),
        strand => strand,
    };
    let same = || {
        transcripts.iter().copied().filter(|transcript| {
            genie_core::isoform::agree(&introns(transcript), &isoform.junctions, SLACK)
        })
    };
    let with_id = same().find_map(|transcript| {
        let id = transcript.id().filter(|id| *id != transcript.name())?;
        Some(Named {
            name: format!("{} {id}", transcript.name()),
            id: id.to_owned(),
            gene: transcript.name().to_owned(),
            strand: strand(transcript),
        })
    });
    let known = with_id.or_else(|| {
        same().next().map(|transcript| Named {
            name: transcript.name().to_owned(),
            id: transcript.name().to_owned(),
            gene: transcript.name().to_owned(),
            strand: strand(transcript),
        })
    });
    if let Some(named) = known {
        return named;
    }
    let closest = transcripts
        .iter()
        .copied()
        .map(|transcript| {
            let shared =
                genie_core::isoform::shared(&introns(transcript), &isoform.junctions, SLACK);
            (shared, transcript)
        })
        .filter(|(shared, _)| *shared > 0)
        .min_by_key(|(shared, _)| Reverse(*shared))
        .map(|(_, transcript)| transcript);
    let gene = closest.map(|transcript| transcript.name().to_owned());
    let number = novel.entry(gene.clone().unwrap_or_default()).or_default();
    *number += 1;
    let number = *number;
    match (gene, closest) {
        (Some(gene), Some(transcript)) => Named {
            name: format!("{gene} novel {number}"),
            id: format!(
                "{}_novel_{number}",
                gene.split_whitespace().collect::<Vec<_>>().join("_")
            ),
            strand: strand(transcript),
            gene,
        },
        _ => Named {
            name: format!("novel {number}"),
            id: format!("novel_{number}"),
            gene: format!("novel_{number}"),
            strand: isoform.strand,
        },
    }
}

/// The isoforms of a pile with reads in a region of a contig, with the names of the transcripts
/// of the open feature tracks of a panel.
pub(super) fn isoforms_in_view(
    session: &Session,
    panel: usize,
    pile: &Pile,
    (contig, visible): (usize, Region),
) -> InView {
    let in_sight = |span: &Region| span.intersection(visible).is_some();
    let transcripts = transcripts(session, panel, contig);
    let all = all_isoforms(pile, &transcripts);
    let mut seen: Vec<Seen> = Vec::new();
    for (index, isoform) in all.iter().enumerate() {
        if !in_sight(&isoform.span) {
            continue;
        }
        let mut groups: Vec<(GroupKey, usize)> = Vec::new();
        for read in isoform.reads.iter().filter(|read| in_sight(&read.span)) {
            match groups.iter_mut().find(|(key, _)| *key == read.group) {
                Some((_, count)) => *count += 1,
                None => groups.push((read.group, 1)),
            }
        }
        let reads: usize = groups.iter().map(|(_, count)| count).sum();
        if reads > 0 {
            groups.sort_unstable();
            seen.push(Seen {
                index,
                name: String::new(),
                id: String::new(),
                gene: String::new(),
                strand: isoform.strand,
                reads,
                groups,
            });
        }
    }
    seen.sort_by_key(|seen| Reverse(seen.reads));
    let spliced: usize = seen.iter().map(|seen| seen.reads).sum();
    let mut novel = HashMap::new();
    for seen in &mut seen {
        let named = isoform_name(&transcripts, &all[seen.index], &mut novel);
        (seen.name, seen.id, seen.gene, seen.strand) =
            (named.name, named.id, named.gene, named.strand);
    }
    InView {
        least: ISOFORM_READS.max(spliced.div_ceil(ISOFORM_SHARE)),
        seen,
        unspliced: pile.chains.1.iter().filter(|read| in_sight(read)).count(),
        all,
    }
}

/// "1 read" or the number of reads.
fn reads_text(count: usize) -> String {
    match count {
        1 => "1 read".to_owned(),
        count => format!("{} reads", grouped(count as u64)),
    }
}

/// The isoforms in view of a read track in a panel that have 2 reads or more and 1 % or more of
/// the spliced reads in view, and the line that counts the other isoforms and reads in view.
pub(super) fn isoforms(session: &Session, panel: usize, pile: &Pile) -> Isoforms {
    let (contig, viewport) = session.view(panel);
    let shown = isoforms_in_view(session, panel, pile, (contig, viewport.visible()));
    let (lane, few): (Vec<&Seen>, Vec<&Seen>) = shown
        .seen
        .iter()
        .partition(|seen| seen.reads >= shown.least);
    let features: Vec<Feature> = lane
        .iter()
        .map(|seen| {
            let isoform = &shown.all[seen.index];
            let name = format!("{} · {}", seen.name, reads_text(seen.reads));
            Feature::new(name, isoform.span, seen.strand).with_blocks(isoform.exons())
        })
        .collect();
    let mut parts = Vec::new();
    let few_reads: usize = few.iter().map(|seen| seen.reads).sum();
    match few.len() {
        0 => {}
        1 => parts.push(format!("1 more isoform with {}", reads_text(few_reads))),
        count => parts.push(format!(
            "{} more isoforms with {}",
            grouped(count as u64),
            reads_text(few_reads)
        )),
    }
    if shown.unspliced > 0 {
        parts.push(format!(
            "{} without a splice junction",
            reads_text(shown.unspliced)
        ));
    }
    let others = if features.is_empty() && parts.is_empty() {
        "No spliced reads in view.".to_owned()
    } else {
        parts.join(" · ")
    };
    Isoforms {
        rows: (0..features.len()).collect(),
        features,
        others,
    }
}

/// The coverage of a read track and the lanes of its cell types under it.
fn cell_body(session: &Session, panel: usize, pile: &Pile, lanes: &track::CellLanes) -> Body {
    let (_, viewport) = session.view(panel);
    let coverage = alignments::coverage(
        viewport,
        &pile.coverage,
        |at| pile.base(at),
        alignments::COVERAGE_HEIGHT,
    );
    let primitives = genie_render::cells::scene(viewport, &lanes.depths, &lanes.lanes);
    let height = genie_render::cells::height(lanes.lanes.len());
    Body::Reads {
        coverage,
        reads: Some((primitives, height)),
    }
}

/// The coverage of a read track and its isoforms under it, each as a transcript with its reads.
fn isoform_body(session: &Session, track: &Track, panel: usize, pile: &Pile) -> Body {
    let (_, viewport) = session.view(panel);
    let coverage = alignments::coverage(
        viewport,
        &pile.coverage,
        |at| pile.base(at),
        alignments::COVERAGE_HEIGHT,
    );
    let shown = isoforms(session, panel, pile);
    let mode = features::Mode::Expanded;
    let mut primitives = features::scene(
        viewport,
        &shown.features,
        &shown.rows,
        mode,
        track.hue,
        true,
    );
    let rows = shown.features.len() as f32 * mode.row_height();
    if !shown.others.is_empty() {
        primitives.push(Primitive::Text {
            x: 4.0,
            y: rows + OTHERS_HEIGHT / 2.0,
            content: std::borrow::Cow::Owned(shown.others),
            size: 10.0,
            paint: genie_render::scene::Paint::Muted,
            font: genie_render::scene::Font::Ui,
            align: genie_render::scene::Align::Start,
        });
    }
    Body::Reads {
        coverage,
        reads: Some((primitives, rows + OTHERS_HEIGHT)),
    }
}

/// The depth at the top of the Sashimi plot of a read track: the deepest coverage in view of the
/// track, or of all the Sashimi plots with a shared scale.
fn sashimi_top(session: &Session, track: &Track, panel: usize) -> u32 {
    let (contig, viewport) = session.view(panel);
    let depth = |track: &Track| match track.slot(panel).data(contig).map(|data| &data.content) {
        Some(Content::Reads(pile)) => sashimi::deepest(viewport, &pile.coverage),
        _ => 0,
    };
    if !track.plot.linked {
        return depth(track);
    }
    session
        .tracks
        .iter()
        .filter(|other| other.plot.sashimi && other.plot.linked)
        .map(depth)
        .max()
        .unwrap_or(0)
}

/// The dots of a GWAS track. `faint` gives a lighter color to the dots of every second
/// chromosome of the whole-genome view.
fn manhattan_body(
    (session, panel): (&Session, usize),
    found: &[genie_core::association::Association],
    track: &Track,
    (height, faint): (f32, bool),
) -> Body {
    let (_, viewport) = session.view(panel);
    let height = track.height.unwrap_or(height);
    let zoom = locus_zoom(session, track, panel, found);
    let zoom = zoom
        .as_ref()
        .map(|(r2, lead)| manhattan::Zoom { r2, lead: *lead });
    let primitives = manhattan::scene(viewport, found, height, (track.hue, faint), zoom);
    Body::Scene(primitives, height)
}

/// The lanes of a read track over its reads, its junction arcs with the looks of a splicing
/// difference, and its reads.
fn read_lane(
    track: &Track,
    (panel, viewport, mode): (usize, Viewport, features::Mode),
    pile: &Pile,
    looks: &[Option<alignments::ArcLook>],
    repack: bool,
) -> Vec<Primitive> {
    let (_, lanes) = track.methylation_lanes(pile);
    let mut above = alignments::methylation_height(lanes.len());
    let mut lane = alignments::methylation(viewport, &lanes);
    if let Some(((a, b), differences)) = track.methylation_difference(pile) {
        let label = format!("{a} − {b}");
        let bars = genie_render::difference::lane(viewport, &differences, &label);
        lane.extend(bars.into_iter().map(|primitive| primitive.lowered(above)));
        above += genie_render::difference::HEIGHT + 1.0;
    }
    let height = alignments::JUNCTION_HEIGHT;
    let arcs = alignments::arcs_looked(viewport, &pile.junctions, height, looks);
    lane.extend(arcs.into_iter().map(|primitive| primitive.lowered(above)));
    if mode == features::Mode::Collapsed {
        return lane;
    }
    let slot = track.slot(panel);
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
    let top = above + pile.arc_height();
    let look = (track.plot.look, pile.hues.as_slice(), pile.mates);
    let reads = alignments::painted(viewport, &pile.reads, rows, groups, mode, look);
    lane.extend(reads.into_iter().map(|primitive| primitive.lowered(top)));
    lane
}

pub(super) fn lanes_height(track: &Track, pile: &Pile) -> f32 {
    let difference = if track.methylation_difference(pile).is_some() {
        genie_render::difference::HEIGHT + 1.0
    } else {
        0.0
    };
    alignments::methylation_height(track.methylation_lanes(pile).1.len()) + difference
}

fn track_view<'a>(
    session: &'a Session,
    track: &'a Track,
    (repack, padding): (bool, f32),
    draft: Option<&'a str>,
    hovered: bool,
) -> Element<'a, Message> {
    let id = track.id;
    let (shapes, tallest) = shapes(session, track, repack);
    let height = track.height.unwrap_or(tallest.height);
    let shape = tallest;
    let body = match genome_hint(session, track, &shapes, repack) {
        Some(words) => hint(words),
        None => across(&session.portions(), height, |panel| {
            track_body(session, track, panel, shapes[panel], repack)
        }),
    };
    let edge = Grip::new(
        space().width(Length::Fill).height(EDGE),
        move |delta| Message::Lane(id, Lane::Resized(delta)),
        Message::Lane(id, Lane::Released),
    )
    .on_double_click(Message::Lane(id, Lane::Fit))
    .interaction(mouse::Interaction::ResizingVertically);
    let handle: Element<'a, Message> = if let Some(draft) = draft {
        Escape::new(
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
        .into()
    } else {
        let name = Grip::new(
            chrome::tip(
                name_label(session, track),
                "Drag to move · Double-click to rename · Right-click for options",
                tooltip::Position::Right,
            ),
            move |delta| Message::Lane(id, Lane::Moved(delta)),
            Message::Lane(id, Lane::Dropped),
        )
        .on_double_click(Message::Rename(Renaming::Start(id)));
        if hovered {
            row![name, track_buttons(id)]
                .spacing(2)
                .align_y(Alignment::Center)
                .into()
        } else {
            name.into()
        }
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
                .padding(edge_padding()),
        );
    }
    if let Some(range) = genome_range(session, track) {
        let inset = Padding {
            top: padding,
            right: genie_render::scene::LABEL_INSET,
            ..Padding::ZERO
        };
        layers = layers.push(
            container(text(range).size(signal::RANGE_SIZE).style(style::muted))
                .width(Length::Fill)
                .align_right(Length::Fill)
                .padding(inset),
        );
    }
    let lane = container(Context::new(layers, move |at| Message::TrackMenu(id, at)))
        .id(format!("lane-{id}"));
    mouse_area(lane)
        .on_enter(Message::Lane(id, Lane::Entered))
        .on_exit(Message::Lane(id, Lane::Left))
        .into()
}

/// The hint of a track in the whole-genome view when no chromosome shows data, once for all.
fn genome_hint(session: &Session, track: &Track, shapes: &[Shape], repack: bool) -> Option<String> {
    if !session.genome {
        return None;
    }
    let mut words = None;
    for (panel, shape) in shapes.iter().enumerate() {
        match body(session, track, panel, *shape, repack) {
            Body::Hint(text) => {
                words.get_or_insert(text);
            }
            Body::Space(_) => {}
            Body::Scene(..) | Body::Reads { .. } => return None,
        }
    }
    words
}

/// The range of a signal in the whole-genome view, which all chromosomes share.
fn genome_range(session: &Session, track: &Track) -> Option<String> {
    if !session.genome {
        return None;
    }
    (0..session.count()).find_map(|panel| {
        let (contig, _) = session.view(panel);
        let Content::Signal(data) = &track.slot(panel).data(contig)?.content else {
            return None;
        };
        let columns = shown_columns(session, track, panel, &data.summaries);
        signal_scale(session, track, panel, &columns).map(signal::label)
    })
}

fn track_buttons<'a>(id: u64) -> Element<'a, Message> {
    let more = Context::left(
        container(icon::view(Icon::More, TRACK_ICON))
            .width(TRACK_BUTTON)
            .height(TRACK_BUTTON)
            .style(chip),
        move |at| Message::TrackMenu(id, at),
    );
    let remove = button(icon::view(Icon::Close, TRACK_ICON - 2.0))
        .width(TRACK_BUTTON)
        .height(TRACK_BUTTON)
        .padding(0)
        .style(style::chip_button)
        .on_press(Message::TrackChanged(id, Change::Remove));
    row![
        container(chrome::tip(more, "Track menu", tooltip::Position::Bottom))
            .id(format!("more-{id}")),
        container(chrome::tip(
            remove,
            "Remove the track",
            tooltip::Position::Bottom
        ))
        .id(format!("remove-{id}")),
    ]
    .spacing(2)
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

/// The shape of a read track: its Sashimi plot, its isoforms, the lanes of its cell types, or
/// its reads in a mode, packed again for the view when `repack` is on.
fn read_shape(
    session: &Session,
    track: &Track,
    (panel, mode, repack): (usize, features::Mode, bool),
    pile: &Pile,
) -> Shape {
    let (_, viewport) = session.view(panel);
    if track.plot.sashimi {
        return Shape::plain(sashimi::HEIGHT);
    }
    if track.plot.isoforms {
        let isoforms = isoforms_height(session, panel, pile);
        return Shape::plain(alignments::COVERAGE_HEIGHT + 1.0 + isoforms);
    }
    if let Some(cells) = pile.cells.as_ref().filter(|_| track.plot.cells) {
        let lanes = genie_render::cells::height(cells.lanes.len());
        return Shape::plain(alignments::COVERAGE_HEIGHT + 1.0 + lanes);
    }
    if mode == features::Mode::Collapsed {
        let above = lanes_height(track, pile) + pile.arc_height();
        return if above > 0.0 {
            Shape::plain(alignments::COVERAGE_HEIGHT + 1.0 + above)
        } else {
            Shape::plain(alignments::COVERAGE_HEIGHT)
        };
    }
    let (count, groups) = if repack {
        let (_, count, groups) = pile.packed(viewport.visible());
        (count, groups.len())
    } else {
        (pile.count, pile.groups.len())
    };
    let count = count.max(1);
    let row_height = alignments::row_height(mode);
    let fixed = alignments::COVERAGE_HEIGHT
        + 1.0
        + lanes_height(track, pile)
        + pile.arc_height()
        + groups as f32 * alignments::GROUP_GAP;
    Shape {
        height: fixed + count as f32 * row_height,
        rows: count,
        fixed,
        row_height,
    }
}

fn shape(session: &Session, track: &Track, panel: usize, repack: bool) -> Shape {
    let (contig, viewport) = session.view(panel);
    let slot = track.slot(panel);
    if slot.error.is_some() || viewport.visible().len() > track.source.span() {
        return Shape::plain(EMPTY_HEIGHT);
    }
    let mode = track.mode_at(panel, contig);
    let lane = slot.data(contig).map_or(0.0, fraction_height);
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
            let shown = rows.min(features::MOST_ROWS);
            let note = if rows > shown {
                features::NOTE_HEIGHT
            } else {
                0.0
            };
            Shape {
                height: shown as f32 * row_height + note,
                rows: shown,
                fixed: 0.0,
                row_height,
            }
        }
        Some(Content::Variants(calls)) => {
            let arcs = variants::breakend_height(&calls.variants) + review_strip(track);
            let ld = ld_height(track, panel, contig);
            let above = arcs + variants::SITE_HEIGHT + lane + ld;
            if mode == features::Mode::Collapsed || calls.samples == 0 {
                return Shape::plain(above);
            }
            let row_height = variants::row_height(mode);
            let rows = &track.rows;
            let fixed = above + 1.0 + rows.groups.len() as f32 * samples::GROUP_GAP;
            Shape {
                height: fixed + rows.len() as f32 * row_height,
                rows: rows.len(),
                fixed,
                row_height,
            }
        }
        Some(Content::Reads(pile)) => read_shape(session, track, (panel, mode, repack), pile),
        Some(Content::Segments(copies)) => {
            let rows = track.rows.len().max(1);
            let row_height = segments::row_height(mode, copies.samples);
            let fixed =
                segments::top(&track.rows) + track.rows.groups.len() as f32 * samples::GROUP_GAP;
            Shape {
                height: fixed + rows as f32 * row_height,
                rows,
                fixed,
                row_height,
            }
        }
        Some(Content::Signal(_)) if lane > 0.0 => Shape::plain(density_heights(track, lane).1),
        Some(Content::Signal(_)) => Shape::plain(signal::HEIGHT),
        Some(Content::Links(_)) => Shape::plain(links::HEIGHT),
        Some(Content::Junctions(_)) => Shape::plain(junctions::HEIGHT),
        Some(Content::Contacts(_)) => Shape::plain(contact_heights(track).1),
        Some(Content::Associations(_)) => Shape::plain(manhattan::HEIGHT),
        Some(Content::Cells(lanes)) => {
            let lanes = genie_render::cells::height(lanes.lanes.len());
            Shape::plain(genie_render::cells::HEADER + lanes)
        }
        Some(Content::Ratio(_)) => Shape::plain(track.height.unwrap_or(ratio::HEIGHT)),
        None if track.source.has_ratio() => Shape::plain(track.height.unwrap_or(ratio::HEIGHT)),
        None if track.source.has_signal() => Shape::plain(signal::HEIGHT),
        None if track.source.has_links() => Shape::plain(links::HEIGHT),
        None if track.source.has_junctions() => Shape::plain(junctions::HEIGHT),
        None if track.source.has_contacts() => Shape::plain(contact_heights(track).1),
        None if track.source.has_associations() => Shape::plain(manhattan::HEIGHT),
        None => Shape::plain(EMPTY_HEIGHT),
    }
}

/// The space between the map of a Hi-C track and the lane of its virtual 4C.
pub(super) const VIEWPOINT_GAP: f32 = 1.0;
/// The lowest map of a Hi-C track with a virtual 4C.
const LOWEST_MAP: f32 = 24.0;

/// The heights of the map of a Hi-C track and of its whole body. With a viewpoint, the lane of
/// the virtual 4C is under the map. A track height that somebody sets is the height of the whole
/// body.
pub(super) fn contact_heights(track: &Track) -> (f32, f32) {
    let lane = if track.plot.viewpoint.is_some() {
        VIEWPOINT_GAP + contacts::VIEWPOINT_HEIGHT
    } else {
        0.0
    };
    let total = track.height.unwrap_or(contacts::HEIGHT + lane);
    ((total - lane).max(LOWEST_MAP), total.max(LOWEST_MAP + lane))
}

/// The log2 values that get the full colors of a segment track: its own range, or ±1.5.
pub(super) fn segment_range(track: &Track) -> (f32, f32) {
    track
        .plot
        .range
        .map_or(segments::RANGE, |range| (range.min, range.max))
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
    let own = signal::range(columns);
    // The whole-genome view gives all chromosomes one scale.
    let panels: Vec<usize> = if session.genome {
        (0..session.count())
            .filter(|other| *other != panel)
            .collect()
    } else {
        Vec::new()
    };
    let ranges = |others: &[&Track]| -> Option<signal::Scale> {
        let elsewhere = panels
            .iter()
            .filter_map(|other| signal::range(&member_columns(session, track, *other)?));
        let members = others.iter().flat_map(|member| {
            std::iter::once(panel)
                .chain(panels.iter().copied())
                .filter_map(|other| signal::range(&member_columns(session, member, other)?))
        });
        own.into_iter()
            .chain(elsewhere)
            .chain(members)
            .reduce(signal::Scale::union)
    };
    let members = members(session, track);
    if !members.is_empty() {
        return ranges(&members);
    }
    if !track.plot.linked {
        return ranges(&[]);
    }
    let linked: Vec<&Track> = session
        .tracks
        .iter()
        .filter(|other| other.plot.linked && other.id != track.id)
        .collect();
    ranges(&linked)
}

pub(super) fn members<'a>(session: &'a Session, track: &Track) -> Vec<&'a Track> {
    session
        .tracks
        .iter()
        .filter(|other| other.over == Some(track.id) && !other.hidden)
        .collect()
}

fn member_columns(session: &Session, member: &Track, panel: usize) -> Option<Vec<signal::Column>> {
    let (contig, _) = session.view(panel);
    let Content::Signal(data) = &member.slot(panel).data(contig)?.content else {
        return None;
    };
    Some(shown_columns(session, member, panel, &data.summaries))
}

/// The lane of the methylation difference of the groups of a bedMethyl track and the track that
/// it compares with.
fn methylation_body(session: &Session, track: &Track, panel: usize) -> Body {
    let (_, viewport) = session.view(panel);
    let Some(((a, b), differences)) = group_difference(session, track, panel) else {
        return Body::Space(genie_render::difference::HEIGHT);
    };
    let label = format!("{a} − {b}");
    let bars = genie_render::difference::lane(viewport, &differences, &label);
    Body::Scene(bars, genie_render::difference::HEIGHT)
}

fn is_methylation(math: crate::track::Math) -> bool {
    math.operation == signal::Operation::Methylation
}

/// The bedMethyl tracks of the group of a track, and the name of the group: the bedMethyl tracks
/// of its folder, or the track alone when it has no folder or shares it with the other track.
fn methyl_group<'a>(
    session: &'a Session,
    track: &'a Track,
    other: &Track,
) -> (String, Vec<&'a Track>) {
    match &track.folder {
        Some(folder) if other.folder.as_ref() != Some(folder) => {
            let members = session
                .tracks
                .iter()
                .filter(|member| member.folder.as_ref() == Some(folder))
                .filter(|member| member.source.is_bedmethyl())
                .collect();
            (folder.clone(), members)
        }
        _ => (track.name.clone(), vec![track]),
    }
}

/// The methylation difference of the group of a bedMethyl track against the group of the track
/// that it compares with, in a panel: the names of the groups and the difference at each site.
/// The counts of the tracks of a group add up. None for other comparisons.
pub(super) fn group_difference(
    session: &Session,
    track: &Track,
    panel: usize,
) -> Option<((String, String), Vec<genie_core::methylation::Difference>)> {
    let math = track.plot.math.filter(|math| is_methylation(*math))?;
    let other = session.tracks.iter().find(|other| other.id == math.other)?;
    if !track.source.is_bedmethyl() || !other.source.is_bedmethyl() {
        return None;
    }
    let (contig, _) = session.view(panel);
    let sites = |tracks: &[&Track]| {
        let sets: Vec<Vec<genie_core::methylation::Site>> = tracks
            .iter()
            .filter_map(|member| match &member.slot(panel).data(contig)?.content {
                Content::Signal(signal) => Some(signal.sites.clone()),
                _ => None,
            })
            .collect();
        genie_core::methylation::pooled(&sets)
    };
    let (first, mine) = methyl_group(session, track, other);
    let (second, theirs) = methyl_group(session, other, track);
    let differences = genie_core::methylation::difference(&sites(&mine), &sites(&theirs));
    Some(((first, second), differences))
}

/// The methylation difference that a track shows in a panel: of the first two groups of its
/// reads, or of its group of bedMethyl tracks and the group of the track that it compares with.
/// The names of the groups and the difference at each site.
pub(super) fn shown_difference(
    session: &Session,
    track: &Track,
    panel: usize,
) -> Option<((String, String), Vec<genie_core::methylation::Difference>)> {
    if let Some(found) = group_difference(session, track, panel) {
        return Some(found);
    }
    let (contig, _) = session.view(panel);
    let Content::Reads(pile) = &track.slot(panel).data(contig)?.content else {
        return None;
    };
    track.methylation_difference(pile)
}

/// What the methylation difference of a track in a panel comes from, to know when it changes:
/// the settings of the track and the loaded data of the tracks of its groups.
pub(super) fn difference_inputs(session: &Session, track: &Track, panel: usize) -> Vec<usize> {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

    let (contig, _) = session.view(panel);
    let pointer = |member: &Track| {
        member
            .slot(panel)
            .data(contig)
            .map_or(0, |data| std::ptr::from_ref(data) as usize)
    };
    let modification = BuildHasherDefault::<DefaultHasher>::default().hash_one(&track.modification);
    let mut found = vec![
        usize::from(track.plot.look.difference),
        usize::from(track.plot.look.mods),
        modification as usize,
    ];
    let other = track
        .plot
        .math
        .filter(|math| is_methylation(*math))
        .and_then(|math| session.tracks.iter().find(|other| other.id == math.other));
    if let Some(other) = other {
        let (_, mine) = methyl_group(session, track, other);
        let (_, theirs) = methyl_group(session, other, track);
        found.extend(mine.into_iter().map(pointer));
        found.push(0);
        found.extend(theirs.into_iter().map(pointer));
    } else {
        found.push(pointer(track));
    }
    found
}

/// The columns that a signal track shows in a panel: its own values, or their comparison with
/// the values of another track. A comparison shows nothing until both tracks have values.
pub(super) fn shown_columns(
    session: &Session,
    track: &Track,
    panel: usize,
    summaries: &[genie_core::signal::Summary],
) -> Vec<signal::Column> {
    let (contig, viewport) = session.view(panel);
    let own = signal_columns(track, viewport, summaries);
    let Some(math) = track.plot.math else {
        return own;
    };
    let other = session.tracks.iter().find(|other| other.id == math.other);
    let Some((other, Content::Signal(data))) = other.and_then(|other| {
        let content = &other.slot(panel).data(contig)?.content;
        Some((other, content))
    }) else {
        return Vec::new();
    };
    let theirs = signal_columns(other, viewport, &data.summaries);
    signal::compare(viewport, &own, &theirs, math.operation)
}

/// The filters of a family track: the variants with a de novo call or another Mendelian error in
/// a trio. The filters that the track already has do not show.
fn trio_section(track: &Track) -> Option<Section> {
    if track.trios.is_empty() {
        return None;
    }
    let choices = [
        ("Only de novo variants", trio::DE_NOVO),
        ("Only Mendelian errors", trio::MENDELIAN_ERROR),
    ];
    let items = choices
        .into_iter()
        .map(|(words, kind)| {
            let rule = Rule {
                label: track::INHERITANCE.to_owned(),
                test: Test::Keep(kind.to_owned()),
            };
            (words, rule)
        })
        .filter(|(_, rule)| !track.filter.rules.contains(rule))
        .map(|(words, rule)| {
            let add = Message::Filter(track.id, Filtering::Add(rule));
            (words.to_owned(), add)
        })
        .collect();
    Some(Section::new("Trios", items))
}

/// The items of the menu of a read track that give haplotypes to its reads without HP tags
/// from the phased SNVs of a variant track: the variant tracks and, under the one in use, its
/// samples. A chosen variant track stops the phase.
/// The most columns of a cell table that the menu of a track lists.
const CELL_COLUMNS: usize = 8;

/// The columns of the cell table of a track that can give cell types, with the column in use.
fn cell_section(track: &Track) -> Option<Section> {
    let cells = track.cells.as_ref()?;
    let mut section = Section::new("Cell types by", Vec::new());
    let mut columns: Vec<usize> = cells.columns.iter().copied().take(CELL_COLUMNS).collect();
    if !columns.contains(&cells.column) {
        columns.push(cells.column);
    }
    for column in columns {
        if column == cells.column {
            section.chosen.push(section.items.len());
        }
        let message = Message::Cells(Celling::Column(track.id, column));
        section
            .items
            .push((cells.table.columns[column].clone(), message));
    }
    Some(section)
}

/// The other read tracks that the depth of a read track can compare with: each one adds the
/// track of the ratio of the two depths, or removes it when it is open.
fn depth_section(session: &Session, track: &Track) -> Option<Section> {
    if !track.source.has_reads() || track.lift.is_some() {
        return None;
    }
    let mut section = Section::new("Compare depth with", Vec::new());
    for other in session
        .tracks
        .iter()
        .filter(|other| other.id != track.id && other.source.has_reads() && other.lift.is_none())
    {
        let pair = Some((track.id, other.id));
        if session
            .tracks
            .iter()
            .any(|ratio| ratio.options.depths == pair)
        {
            section.chosen.push(section.items.len());
        }
        let message = Message::Depth(track.id, other.id);
        section.items.push((other.name.clone(), message));
    }
    (!section.items.is_empty()).then_some(section)
}

/// The other read tracks that a read track can compare its splicing with, with the one that it
/// compares with chosen.
fn splice_section(session: &Session, track: &Track) -> Option<Section> {
    if !track.source.has_reads() || track.lift.is_some() {
        return None;
    }
    let mut section = Section::new("Compare splicing with", Vec::new());
    for other in session
        .tracks
        .iter()
        .filter(|other| other.id != track.id && other.source.has_reads() && other.lift.is_none())
    {
        let splice = track::Splice::Track(other.id);
        if track.plot.splicing == Some(splice) {
            section.chosen.push(section.items.len());
        }
        let message = Message::TrackChanged(track.id, Change::Splicing(splice));
        section.items.push((other.name.clone(), message));
    }
    (!section.items.is_empty()).then_some(section)
}

fn phase_section(session: &Session, track: &Track) -> Option<Section> {
    if !track.source.has_reads() || track.lift.is_some() {
        return None;
    }
    let (id, phase) = (track.id, track.options.phase);
    let mut section = Section::new("Phase by", Vec::new());
    for variants in session
        .tracks
        .iter()
        .filter(|other| track::can_phase(other))
    {
        let used = phase.filter(|phase| phase.track == variants.id);
        let change = match used {
            Some(_) => None,
            None => Some(track::Phase {
                track: variants.id,
                sample: track::phase_sample(&track.source, &variants.source),
            }),
        };
        if used.is_some() {
            section.chosen.push(section.items.len());
        }
        let message = Message::TrackChanged(id, Change::Phase(change));
        section.items.push((variants.name.clone(), message));
        let samples = variants.source.samples();
        let Some(used) = used.filter(|_| samples.len() > 1) else {
            continue;
        };
        for index in menu::sample_choices(samples.len(), used.sample) {
            if index == used.sample {
                section.chosen.push(section.items.len());
            }
            section.indented.push(section.items.len());
            let change = Change::Phase(Some(track::Phase {
                sample: index,
                ..used
            }));
            let message = Message::TrackChanged(id, change);
            section.items.push((samples[index].clone(), message));
        }
    }
    Some(section)
}

/// The folder items of a track menu: the other folders of the tracks panel, a new folder, and
/// out of the folder of the track.
fn folder_section(session: &Session, track: &Track) -> Section {
    use super::track_list::Foldering;

    let id = track.id;
    let mut items: Vec<(String, Message)> = super::track_list::folders(session)
        .into_iter()
        .filter(|name| track.folder.as_ref() != Some(name))
        .map(|name| {
            let add = Message::Folder(name.clone(), Foldering::Add(id));
            (name, add)
        })
        .collect();
    let new = Message::TrackList(super::Listing::Name(id));
    items.push(("New folder…".to_owned(), new));
    if let Some(folder) = &track.folder {
        let out = Message::Folder(folder.clone(), Foldering::Take(id));
        items.push(("Out of the folder".to_owned(), out));
    }
    Section::new("Folder", items)
}

/// The sections of a track menu: the links of the kind of the track, then its folder.
pub(super) fn links(session: &Session, track: &Track) -> Vec<Section> {
    let mut sections = kind_links(session, track);
    sections.push(folder_section(session, track));
    sections
}

/// The sections of a track menu for the kind of the track: the trio filters of a family track, or
/// for a signal track the operations of its comparison, the tracks that overlay it, or the tracks
/// to overlay or to compare with.
fn kind_links(session: &Session, track: &Track) -> Vec<Section> {
    if track.source.has_variants() {
        return trio_section(track).into_iter().collect();
    }
    if track.source.has_reads() {
        return phase_section(session, track)
            .into_iter()
            .chain(depth_section(session, track))
            .chain(splice_section(session, track))
            .chain(cell_section(track))
            .collect();
    }
    if track.source.has_fragments() {
        return cell_section(track).into_iter().collect();
    }
    if !track.source.has_signal() || track.over.is_some() {
        return Vec::new();
    }
    let id = track.id;
    if let Some(math) = track.plot.math {
        let other = session.tracks.iter().find(|other| other.id == math.other);
        let methyl =
            track.source.is_bedmethyl() && other.is_some_and(|other| other.source.is_bedmethyl());
        let mut items: Vec<(String, Message)> = signal::Operation::ALL
            .into_iter()
            .filter(|operation| methyl || *operation != signal::Operation::Methylation)
            .map(|operation| {
                let change = Message::TrackChanged(id, Change::Operation(operation));
                (operation.title().to_owned(), change)
            })
            .collect();
        let stop = Message::TrackChanged(id, Change::Compare(None));
        items.push(("Stop comparing".to_owned(), stop));
        return vec![Section::new("Comparison", items)];
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
        return vec![Section::new("Separate", items)];
    }
    let others: Vec<&Track> = session
        .tracks
        .iter()
        .filter(|other| other.id != id && other.over.is_none() && other.source.has_signal())
        .collect();
    let overlay = others
        .iter()
        .filter(|other| other.plot.math.is_none())
        .map(|other| {
            let change = Message::TrackChanged(id, Change::Overlay(other.id));
            (other.name.clone(), change)
        })
        .collect();
    let compare = others
        .iter()
        .map(|other| {
            let change = Message::TrackChanged(id, Change::Compare(Some(other.id)));
            (other.name.clone(), change)
        })
        .collect();
    vec![
        Section::new("Overlay on", overlay),
        Section::new("Compare with", compare),
    ]
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

const fn edge_padding() -> Padding {
    Padding {
        top: 1.0,
        right: scene::CHIP_EDGE,
        bottom: 1.0,
        left: 4.0,
    }
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

/// The ruler of a panel of an image. For the whole genome, the name of the chromosome.
pub(super) fn ruler_view<'a>(session: &Session, panel: usize) -> Element<'a, Message> {
    let (contig, viewport) = session.view(panel);
    let ruler = if session.genome {
        let contig = &session.reference.assembly.contigs()[contig].name;
        let name = whole::name(contig, viewport.width() as f32).unwrap_or_default();
        container(text(name).size(11).style(style::muted))
            .center_x(Length::Fill)
            .center_y(RULER_HEIGHT)
    } else {
        container(scene::view(
            ruler::scene(viewport, RULER_HEIGHT),
            RULER_HEIGHT,
        ))
    };
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

fn preparing_lane(item: &Preparing) -> Element<'_, Message> {
    let percent = (item.share * 100.0) as u8;
    let words = format!("{} {}… {percent} %", item.step.verb(), item.name);
    let stop = button(icon::view(Icon::Close, 10.0))
        .width(16)
        .height(16)
        .padding(0)
        .style(style::ghost)
        .on_press(Message::Preparing(item.id, Preparation::Stop));
    let stop = chrome::tip(stop, "Stop and remove the track", tooltip::Position::Left);
    stack![
        hint(words),
        row![label(item.name.as_str()), space().width(Length::Fill), stop]
            .padding(Padding {
                right: scene::CHIP_EDGE,
                ..Padding::ZERO
            })
            .align_y(Alignment::Start),
    ]
    .into()
}

fn toasts<'a>(progress: Option<String>, notice: Option<&'a Notice>) -> Element<'a, Message> {
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
        let action = notice.action.as_ref().map(|(label, message)| {
            button(text(label.as_str()).size(12))
                .padding([2, 8])
                .style(style::primary)
                .on_press(message.clone())
        });
        card(
            row![text(notice.text.as_str()).size(12).width(Length::Fill)]
                .push(action)
                .push(close)
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

fn label<'a>(name: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(name).size(LABEL_SIZE).style(style::muted))
        .padding([0, 4])
        .style(chip)
        .into()
}

fn name_label<'a>(session: &'a Session, track: &'a Track) -> Element<'a, Message> {
    let members = members(session, track);
    if members.is_empty() {
        return label(track.name.as_str());
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
