//! The variant table under the tracks: a row for each variant in view of a track, and a column
//! for each of its fields.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::{Region, Span, Target, grouped};
use iced::widget::scrollable::{Direction, RelativeOffset, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, button, column, container, operation, row, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use super::view::hairline;
use super::{Genie, Message, Session};
use crate::icon::{self, Icon};
use crate::paths::{self, file_name};
use crate::table::Table;
use crate::theme::tokens;
use crate::track::{self, Content};
use crate::{font, style};

/// The height of the table, with its header.
pub(super) const HEIGHT: f32 = 220.0;
const ROW_HEIGHT: f32 = 20.0;
/// The rows that genie draws over and under the rows in sight, so that a scroll shows no gap.
const SPARE: usize = 8;
/// The rows in sight of a table of full height.
const IN_SIGHT: usize = 9;
const ROWS: &str = "variant-table-rows";
pub(super) const SEARCH: &str = "variant-table-search";
const CHARACTER: f32 = 6.7;
const PADDING: f32 = 12.0;
const MIN_WIDTH: f32 = 36.0;
const MAX_WIDTH: f32 = 280.0;
const ZOOM_IN: &str = "Zoom in to see the variants.";
const NONE_IN_VIEW: &str = "No variants in view.";

#[derive(Debug, Clone, PartialEq)]
pub enum Tabling {
    /// Shows the table of a track, or closes it when it shows already.
    Open(u64),
    Close,
    Sort(usize),
    Search(String),
    /// Goes to the variant of a row.
    Go(usize),
    Scrolled(f32),
    Export,
    ExportPicked(Option<PathBuf>),
}

/// The variant table of a track, with the rows of the data in view.
#[derive(Debug, Default)]
pub(super) struct VariantTable {
    pub track: u64,
    sort: Option<(usize, bool)>,
    search: String,
    chosen: Option<Region>,
    scroll: f32,
    /// The table of the loaded variants, with the key of the data that it comes from.
    built: Option<(Built, Arc<Table>)>,
    /// The rows in view in their order and the widths of the columns, with the key of the view.
    shown: Option<(Shown, Vec<usize>, Vec<f32>)>,
    /// Why the table has no rows.
    hint: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Built {
    data: usize,
    region: Region,
    count: usize,
    contig: usize,
    /// The trios of the track, which give the Inheritance column.
    trios: usize,
    /// The review marks of the track, which give the Review columns.
    reviews: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct Shown {
    visible: Region,
    sort: Option<(usize, bool)>,
    search: String,
}

impl VariantTable {
    fn rows(&self) -> Option<(&Table, &[usize], &[f32])> {
        let (_, table) = self.built.as_ref()?;
        let (_, order, widths) = self.shown.as_ref()?;
        Some((table, order, widths))
    }
}

/// The width of each column for its name and the cells of the rows in an order.
fn widths(table: &Table, order: &[usize]) -> Vec<f32> {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(at, name)| {
            let longest = order
                .iter()
                .map(|&row| table.rows[row].cells[at].chars().count())
                .chain([name.chars().count() + 2])
                .max()
                .unwrap_or(0);
            (longest as f32 * CHARACTER + PADDING).clamp(MIN_WIDTH, MAX_WIDTH)
        })
        .collect()
}

async fn save_tsv(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export the variant table")
        .set_file_name(name)
        .add_filter("Tab-separated values", &["tsv"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}

fn with_tsv(path: PathBuf) -> PathBuf {
    let is_tsv = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"));
    if is_tsv {
        return path;
    }
    let mut text = path.into_os_string();
    text.push(".tsv");
    PathBuf::from(text)
}

impl Genie {
    pub(super) fn tabling(&mut self, tabling: Tabling) -> Task<Message> {
        match tabling {
            Tabling::Open(id) => {
                self.close_menus();
                let open = self.table.as_ref().is_some_and(|table| table.track == id);
                self.table = (!open).then(|| VariantTable {
                    track: id,
                    ..VariantTable::default()
                });
                if self.table.is_some() {
                    self.dmrs = None;
                }
            }
            Tabling::Close => self.table = None,
            Tabling::Sort(column) => {
                if let Some(table) = &mut self.table {
                    table.sort = match table.sort {
                        Some((known, false)) if known == column => Some((column, true)),
                        Some((known, true)) if known == column => None,
                        _ => Some((column, false)),
                    };
                    table.scroll = 0.0;
                    return operation::snap_to(ROWS, RelativeOffset::START);
                }
            }
            Tabling::Search(words) => {
                if let Some(table) = &mut self.table {
                    table.search = words;
                    table.scroll = 0.0;
                    return operation::snap_to(ROWS, RelativeOffset::START);
                }
            }
            Tabling::Scrolled(offset) => {
                if let Some(table) = &mut self.table {
                    table.scroll = offset;
                }
            }
            Tabling::Go(row) => return self.go_to_row(row),
            Tabling::Export => {
                let name = self
                    .table
                    .as_ref()
                    .and_then(|table| self.track_named(table.track))
                    .map_or_else(|| "variants".to_owned(), |name| format!("{name} variants"));
                return Task::perform(save_tsv(format!("{name}.tsv")), |path| {
                    Message::Table(Tabling::ExportPicked(path))
                });
            }
            Tabling::ExportPicked(Some(path)) => return self.export_table(&with_tsv(path)),
            Tabling::ExportPicked(None) => {}
        }
        Task::none()
    }

    pub(super) fn track_named(&self, id: u64) -> Option<String> {
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        Some(track.name.clone())
    }

    fn go_to_row(&mut self, row: usize) -> Task<Message> {
        let Some(table) = &mut self.table else {
            return Task::none();
        };
        let Some((built, rows)) = &table.built else {
            return Task::none();
        };
        let Some(span) = rows.rows.get(row).map(|row| row.span) else {
            return Task::none();
        };
        let contig = built.contig;
        table.chosen = Some(span);
        let wide = self
            .session
            .as_ref()
            .is_some_and(|session| span.len() as f64 > session.destination().span());
        let span = if wide {
            Span::Region(span)
        } else {
            Span::Position(span.start() + span.len() / 2)
        };
        self.go_to(Target { contig, span })
    }

    fn export_table(&mut self, path: &Path) -> Task<Message> {
        let Some((table, order, _)) = self.table.as_ref().and_then(VariantTable::rows) else {
            return Task::none();
        };
        match paths::replace(path, &table.tsv(order)) {
            Ok(()) => self.inform(format!("Saved {}.", file_name(path))),
            Err(error) => {
                self.warn(format!("The table could not be saved. {error}"));
                Task::none()
            }
        }
    }

    /// Builds the rows of the table again when the data or the view of its track changed. The
    /// table closes when its track closes.
    pub(super) fn refresh_table(&mut self) {
        let (Some(table), Some(session)) = (&mut self.table, &self.session) else {
            self.table = None;
            return;
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == table.track) else {
            self.table = None;
            return;
        };
        let (contig, viewport) = session.frame(0);
        let visible = viewport.visible();
        let calls = match track.data(contig).map(|data| (data, &data.content)) {
            Some((data, Content::Variants(calls))) => Some((data, calls)),
            Some(_) => {
                (table.built, table.shown, table.hint) = (None, None, Some(ZOOM_IN));
                return;
            }
            None => None,
        };
        let Some((data, calls)) = calls else {
            table.hint = table.built.is_none().then_some(NONE_IN_VIEW);
            return;
        };
        let key = Built {
            data: std::ptr::from_ref(data) as usize,
            region: data.region,
            count: calls.variants.len(),
            contig,
            trios: Arc::as_ptr(&track.trios).cast::<()>() as usize,
            reviews: Arc::as_ptr(&track.reviews) as usize,
        };
        if table.built.as_ref().is_none_or(|(known, _)| *known != key) {
            let name = &session.reference.assembly.contigs()[contig].name;
            let samples = !track.source.samples().is_empty();
            let (source, trios) = (&track.source, &track.trios_on(contig));
            let built = Table::new(name, &calls.variants, samples, |variant| {
                let review = track.review_of(contig, variant);
                track::variant_fields(source, variant, trios, review)
            });
            table.built = Some((key, Arc::new(built)));
            table.shown = None;
        }
        let shown = Shown {
            visible,
            sort: table.sort,
            search: table.search.clone(),
        };
        if table
            .shown
            .as_ref()
            .is_none_or(|(known, ..)| *known != shown)
        {
            let Some((_, built)) = &table.built else {
                return;
            };
            let order = built.order(visible, table.sort, &table.search);
            let widths = widths(built, &order);
            table.shown = Some((shown, order, widths));
        }
        table.hint = None;
    }

    pub(super) fn table_view<'a>(&'a self, session: &'a Session) -> Option<Element<'a, Message>> {
        let table = self.table.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == table.track)?;
        let rows = table.rows();
        let count = rows.map_or(0, |(_, order, _)| order.len());
        let close = button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Table(Tabling::Close));
        let search = text_input("Find in the table", &table.search)
            .id(SEARCH)
            .on_input(|words| Message::Table(Tabling::Search(words)))
            .size(12)
            .padding([3, 6])
            .width(180);
        let export = button(text("Export TSV").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe((count > 0).then_some(Message::Table(Tabling::Export)));
        let words = match count {
            1 => "1 variant in view".to_owned(),
            _ => format!("{} variants in view", grouped(count as u64)),
        };
        let header = row![
            text(format!("Variants · {}", track.name))
                .font(font::UI_STRONG)
                .size(12),
            text(words).size(11).style(style::muted),
            space::horizontal(),
            search,
            export,
            close,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([4, 8]);
        let body = match rows {
            Some((built, order, widths)) if !order.is_empty() => {
                grid(table, built, (order, widths), self.width)
            }
            _ => {
                let hint = table.hint.unwrap_or(NONE_IN_VIEW);
                container(text(hint).size(11).style(style::muted))
                    .padding([4, 8])
                    .height(Length::Fill)
                    .into()
            }
        };
        Some(
            container(column![hairline(), header, body])
                .width(Length::Fill)
                .height(HEIGHT)
                .style(|theme| container::background(tokens(theme).bar))
                .into(),
        )
    }
}

/// The rows of a table under a row of column names. The rows fill the width of the panel, so
/// that the scroll bar is at its right edge when the columns are narrower.
fn grid<'a>(
    table: &VariantTable,
    built: &'a Table,
    (order, widths): (&[usize], &[f32]),
    panel: f32,
) -> Element<'a, Message> {
    let total = widths.iter().sum::<f32>().max(panel);
    let head = row(built
        .columns
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(at, (name, width))| {
            let arrow = match table.sort {
                Some((column, false)) if column == at => " ▲",
                Some((column, true)) if column == at => " ▼",
                _ => "",
            };
            button(
                text(format!("{name}{arrow}"))
                    .font(font::UI_STRONG)
                    .size(11)
                    .wrapping(Wrapping::None),
            )
            .width(*width)
            .height(ROW_HEIGHT)
            .padding([2, 6])
            .style(style::ghost)
            .on_press(Message::Table(Tabling::Sort(at)))
            .into()
        }));
    let first = ((table.scroll / ROW_HEIGHT) as usize).saturating_sub(SPARE);
    let last = (first + IN_SIGHT + 2 * SPARE).min(order.len());
    let first = first.min(last);
    let lines = order[first..last].iter().map(|&at| {
        let line = &built.rows[at];
        let cells = row(line.cells.iter().zip(widths).map(|(cell, width)| {
            container(
                text(cell.as_str())
                    .font(font::MONO)
                    .size(11)
                    .wrapping(Wrapping::None),
            )
            .width(*width)
            .height(ROW_HEIGHT)
            .padding([2, 6])
            .clip(true)
            .into()
        }));
        let chosen = table.chosen == Some(line.span);
        button(cells)
            .width(Length::Fill)
            .padding(0)
            .style(if chosen { style::chosen } else { style::ghost })
            .on_press(Message::Table(Tabling::Go(at)))
            .into()
    });
    let above = first as f32 * ROW_HEIGHT;
    let below = (order.len() - last) as f32 * ROW_HEIGHT;
    let lines = Column::new()
        .push(space().height(above))
        .extend(lines)
        .push(space().height(below))
        .width(total);
    let bar = || Scrollbar::new().width(6).scroller_width(6).margin(0);
    let body = scrollable(lines)
        .id(ROWS)
        .direction(Direction::Vertical(bar()))
        .style(style::scrollbar)
        .on_scroll(|viewport| Message::Table(Tabling::Scrolled(viewport.absolute_offset().y)))
        .width(total)
        .height(Length::Fill);
    scrollable(column![head, body].width(total).height(Length::Fill))
        .direction(Direction::Horizontal(bar()))
        .style(style::scrollbar)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::with_tsv;

    #[test]
    fn exports_end_with_tsv() {
        assert_eq!(with_tsv(PathBuf::from("a")), PathBuf::from("a.tsv"));
        assert_eq!(with_tsv(PathBuf::from("b.TSV")), PathBuf::from("b.TSV"));
    }
}
