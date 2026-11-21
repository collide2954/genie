//! The splicing table of a read track under the tracks: the splice junctions in view with the
//! reads and the PSI of each of the two groups or tracks of its splicing difference, the
//! difference and p.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::path::PathBuf;

use genie_core::splicing::Difference;
use genie_core::{Region, Span, Strand, Target, grouped};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Task};

use super::view::{self, hairline};
use super::{Genie, Message, Session};
use crate::icon::{self, Icon};
use crate::paths::{self, file_name};
use crate::theme::tokens;
use crate::{font, style};

const NONE_IN_VIEW: &str = "No splice junctions in view.";
const NO_DIFFERENCE: &str = "The track shows no splicing difference in view.";
const NONE_DIFFER: &str = "No junction in view differs.";
/// The widths of the columns: place, the reads and the PSI of each group, difference and p.
const WIDTHS: [f32; 7] = [230.0, 140.0, 110.0, 140.0, 110.0, 90.0, 72.0];
const ROW_HEIGHT: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Splicing {
    /// Shows the splicing table of a read track, or closes it when it shows.
    Open(u64),
    Close,
    /// Shows only the junctions that differ, or all junctions.
    Differ,
    /// Sorts the rows by a column: up, then down, then in the order of the places again.
    Sort(usize),
    /// Goes to the junction of a row.
    Go(usize),
    Export,
    ExportPicked(Option<PathBuf>),
}

/// The splicing table of a read track.
#[derive(Debug)]
pub(super) struct SpliceTable {
    pub track: u64,
    only: bool,
    sort: Option<(usize, bool)>,
    /// The junction of the row that a click went to last.
    chosen: Option<Region>,
    /// The junctions in view with what they come from, or why the table has no rows.
    found: Option<(Key, Result<Found, &'static str>)>,
}

/// What the rows of a table come from: the view, the switches of the table and the data of the
/// tracks in the first panel.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    contig: usize,
    visible: Region,
    only: bool,
    sort: Option<(usize, bool)>,
    inputs: Vec<usize>,
}

/// The junctions in view: the contig, the names of the two groups, the rows in their order, and
/// the number of junctions in view that differ.
#[derive(Debug, Clone)]
struct Found {
    contig: usize,
    groups: (String, String),
    rows: Vec<Line>,
    differ: usize,
}

/// A row of the table: a junction in view, its strand and its difference.
#[derive(Debug, Clone, Copy)]
struct Line {
    strand: Strand,
    difference: Difference,
}

impl SpliceTable {
    fn new(track: u64) -> Self {
        Self {
            track,
            only: false,
            sort: None,
            chosen: None,
            found: None,
        }
    }

    fn found(&self) -> Option<&Found> {
        self.found.as_ref()?.1.as_ref().ok()
    }
}

/// A PSI as the table shows it, with one decimal, or a dash without reads at the sites.
fn psi(difference: &Difference, group: usize) -> String {
    difference
        .psi(group)
        .map_or_else(|| "–".to_owned(), |value| format!("{value:.1} %"))
}

/// A difference in points with its sign, as "+60.0" or "−25.5", or a dash.
fn points(difference: &Difference) -> String {
    difference.points().map_or_else(
        || "–".to_owned(),
        |points| format!("{points:+.1}").replace('-', "\u{2212}"),
    )
}

/// The order of two rows by a column. Junctions without a difference come last for the
/// difference.
fn compare(a: &Line, b: &Line, column: usize) -> Ordering {
    let (a, b) = (&a.difference, &b.difference);
    let place = |difference: &Difference| (difference.span.start(), difference.span.end());
    let share = |difference: &Difference, group| difference.psi(group).unwrap_or(-1.0);
    match column {
        1 => a.reads[0].cmp(&b.reads[0]),
        2 => share(a, 0).total_cmp(&share(b, 0)),
        3 => a.reads[1].cmp(&b.reads[1]),
        4 => share(a, 1).total_cmp(&share(b, 1)),
        5 => {
            let value = |difference: &Difference| difference.points().unwrap_or(f64::NEG_INFINITY);
            value(a).total_cmp(&value(b))
        }
        6 => a.p.total_cmp(&b.p),
        _ => place(a).cmp(&place(b)),
    }
}

/// The data of the tracks in the first panel, which give the reads of the two groups or tracks.
fn inputs(session: &Session, contig: usize) -> Vec<usize> {
    session
        .tracks
        .iter()
        .map(|track| {
            track
                .slot(0)
                .data(contig)
                .map_or(0, |data| std::ptr::from_ref(data) as usize)
        })
        .collect()
}

/// The kinds of files that the rows of the table export to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Bed,
    Tsv,
}

/// The format of a file from its extension: TSV or BED. A file with another extension gets
/// .bed.
fn with_format(path: PathBuf) -> (PathBuf, Format) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("tsv") => (path, Format::Tsv),
        Some("bed") => (path, Format::Bed),
        _ => {
            let mut text = path.into_os_string();
            text.push(".bed");
            (PathBuf::from(text), Format::Bed)
        }
    }
}

fn strand_text(strand: Strand) -> &'static str {
    match strand {
        Strand::Forward => "+",
        Strand::Reverse => "-",
        Strand::Unknown => ".",
    }
}

/// The difference of a row as a file gives it: with an ASCII sign, or NA.
fn file_points(difference: &Difference) -> String {
    difference
        .points()
        .map_or_else(|| "NA".to_owned(), |points| format!("{points:+.1}"))
}

/// The rows as BED lines: the intron, the difference in points as the name, the reads of the
/// two groups up to 1,000 as the score, and the strand.
fn bed(contig: &str, rows: &[Line]) -> String {
    let mut text = String::new();
    for line in rows {
        let difference = &line.difference;
        let reads = difference.reads[0].saturating_add(difference.reads[1]);
        let _ = writeln!(
            text,
            "{contig}\t{}\t{}\t{}\t{}\t{}",
            difference.span.start(),
            difference.span.end(),
            file_points(difference),
            reads.min(1_000),
            strand_text(line.strand)
        );
    }
    text
}

/// The rows as tab-separated values: the place in four columns, the reads, the reads at the
/// sites and the PSI of each group, the difference and p.
fn tsv(contig: &str, found: &Found) -> String {
    let (first, second) = &found.groups;
    let mut text = String::from("Chrom\tStart\tEnd\tStrand");
    for group in [first, second] {
        let group = group.replace('\t', " ");
        let _ = write!(text, "\t{group} reads\t{group} site reads\t{group} PSI");
    }
    text.push_str("\tDifference\tp\n");
    for line in &found.rows {
        let difference = &line.difference;
        let _ = write!(
            text,
            "{contig}\t{}\t{}\t{}",
            difference.span.start() + 1,
            difference.span.end(),
            strand_text(line.strand)
        );
        for group in 0..2 {
            let share = difference
                .psi(group)
                .map_or_else(|| "NA".to_owned(), |value| format!("{value:.2}"));
            let _ = write!(
                text,
                "\t{}\t{}\t{share}",
                difference.reads[group], difference.totals[group]
            );
        }
        let _ = writeln!(text, "\t{}\t{:.3e}", file_points(difference), difference.p);
    }
    text
}

async fn save_splices(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export the splice junctions")
        .set_file_name(name)
        .add_filter("BED", &["bed"])
        .add_filter("Tab-separated values", &["tsv"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}

impl Genie {
    pub(super) fn splicing_table(&mut self, splicing: Splicing) -> Task<Message> {
        match splicing {
            Splicing::Open(id) => {
                self.close_menus();
                let open = self.splices.as_ref().is_some_and(|table| table.track == id);
                self.splices = (!open).then(|| SpliceTable::new(id));
                if self.splices.is_some() {
                    (self.table, self.dmrs, self.isoforms) = (None, None, None);
                }
            }
            Splicing::Close => self.splices = None,
            Splicing::Differ => {
                if let Some(table) = &mut self.splices {
                    table.only = !table.only;
                }
            }
            Splicing::Sort(column) => {
                if let Some(table) = &mut self.splices {
                    table.sort = match table.sort {
                        Some((known, false)) if known == column => Some((column, true)),
                        Some((known, true)) if known == column => None,
                        _ => Some((column, false)),
                    };
                }
            }
            Splicing::Go(row) => return self.go_to_splice(row),
            Splicing::Export => {
                let name = self
                    .splices
                    .as_ref()
                    .and_then(|table| self.track_named(table.track))
                    .map_or_else(|| "splicing".to_owned(), |name| format!("{name} splicing"));
                return Task::perform(save_splices(format!("{name}.bed")), |path| {
                    Message::Splicing(Splicing::ExportPicked(path))
                });
            }
            Splicing::ExportPicked(Some(path)) => return self.export_splices(path),
            Splicing::ExportPicked(None) => {}
        }
        Task::none()
    }

    fn go_to_splice(&mut self, row: usize) -> Task<Message> {
        let Some(table) = &mut self.splices else {
            return Task::none();
        };
        let Some((contig, span)) = table
            .found()
            .and_then(|found| Some((found.contig, found.rows.get(row)?.difference.span)))
        else {
            return Task::none();
        };
        table.chosen = Some(span);
        self.go_to(Target {
            contig,
            span: Span::Region(span.expand(span.len() / 10 + 20, u64::MAX)),
        })
    }

    fn export_splices(&mut self, path: PathBuf) -> Task<Message> {
        let (path, format) = with_format(path);
        let text = {
            let (Some(found), Some(session)) = (
                self.splices.as_ref().and_then(SpliceTable::found),
                self.session.as_ref(),
            ) else {
                return Task::none();
            };
            let contig = &session.reference.assembly.contigs()[found.contig].name;
            match format {
                Format::Bed => bed(contig, &found.rows),
                Format::Tsv => tsv(contig, found),
            }
        };
        match paths::replace(&path, &text) {
            Ok(()) => self.inform(format!("Saved {}.", file_name(&path))),
            Err(error) => {
                self.warn(format!("The splice junctions could not be saved. {error}"));
                Task::none()
            }
        }
    }

    /// Finds the rows again when the view, the switches of the table or the data of the tracks
    /// changed. The table closes when its track closes.
    pub(super) fn refresh_splices(&mut self) {
        let (Some(table), Some(session)) = (&mut self.splices, &self.session) else {
            self.splices = None;
            return;
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == table.track) else {
            self.splices = None;
            return;
        };
        let (contig, viewport) = session.frame(0);
        let visible = viewport.visible();
        let key = Key {
            contig,
            visible,
            only: table.only,
            sort: table.sort,
            inputs: inputs(session, contig),
        };
        if table.found.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        let found = match view::splicing_piles(session, track, 0) {
            None => Err(NONE_IN_VIEW),
            Some(piles) => match view::shown_splicing(session, track, 0) {
                None => Err(NO_DIFFERENCE),
                Some(shown) => {
                    let (groups, differences) = &*shown;
                    let strand = |span: Region| {
                        let mut piles = [Some(piles.0), piles.1.map(|(_, pile)| pile)]
                            .into_iter()
                            .flatten();
                        piles
                            .find_map(|pile| {
                                let at = pile
                                    .junctions
                                    .binary_search_by_key(&(span.start(), span.end()), |junction| {
                                        (junction.span.start(), junction.span.end())
                                    })
                                    .ok()?;
                                Some(pile.junctions[at].strand)
                            })
                            .unwrap_or(Strand::Unknown)
                    };
                    let shown: Vec<Line> = differences
                        .iter()
                        .filter(|difference| difference.span.intersection(visible).is_some())
                        .map(|&difference| Line {
                            strand: strand(difference.span),
                            difference,
                        })
                        .collect();
                    let differ = shown
                        .iter()
                        .filter(|line| line.difference.differs())
                        .count();
                    let mut rows: Vec<Line> = shown
                        .into_iter()
                        .filter(|line| !table.only || line.difference.differs())
                        .collect();
                    if let Some((column, descending)) = table.sort {
                        rows.sort_by(|a, b| {
                            let order = compare(a, b, column);
                            if descending { order.reverse() } else { order }
                        });
                    }
                    Ok(Found {
                        contig,
                        groups: groups.clone(),
                        rows,
                        differ,
                    })
                }
            },
        };
        table.found = Some((key, found));
    }

    pub(super) fn splice_view<'a>(&'a self, session: &'a Session) -> Option<Element<'a, Message>> {
        let table = self.splices.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == table.track)?;
        let found = table.found();
        let header = bar(table, &track.name, found);
        let body: Element<'a, Message> = match found {
            Some(found) if !found.rows.is_empty() => grid(table, session, found, self.width),
            _ => {
                let hint = match table.found.as_ref().map(|(_, found)| found) {
                    Some(Err(hint)) => *hint,
                    Some(Ok(found)) if table.only && found.differ == 0 => NONE_DIFFER,
                    _ => NONE_IN_VIEW,
                };
                container(text(hint).size(11).style(style::muted))
                    .padding([4, 8])
                    .height(Length::Fill)
                    .into()
            }
        };
        Some(
            container(column![hairline(), header, body])
                .width(Length::Fill)
                .height(super::table::HEIGHT)
                .style(|theme| container::background(tokens(theme).bar))
                .into(),
        )
    }
}

/// The bar of the table: its name, the number of junctions and of differences, the two groups,
/// Only differences, Export and close.
fn bar<'a>(table: &SpliceTable, name: &str, found: Option<&Found>) -> Element<'a, Message> {
    let words = found.map_or_else(String::new, |found| {
        let count = found.rows.len();
        let mut words = match count {
            1 => "1 junction".to_owned(),
            count => format!("{} junctions", grouped(count as u64)),
        };
        let _ = write!(words, " · {} differ", grouped(found.differ as u64));
        words
    });
    let groups = found.map(|found| format!("{} − {}", found.groups.0, found.groups.1));
    let count = found.map_or(0, |found| found.rows.len());
    row![
        text(format!("Splicing · {name}"))
            .font(font::UI_STRONG)
            .size(12),
        text(words).size(11).style(style::muted),
    ]
    .push(groups.map(|groups| text(groups).size(11).style(style::muted)))
    .push(space::horizontal())
    .push(
        button(text("Only differences").size(12))
            .padding([3, 8])
            .style(if table.only {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::Splicing(Splicing::Differ)),
    )
    .push(
        button(text("Export…").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe((count > 0).then_some(Message::Splicing(Splicing::Export))),
    )
    .push(
        button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Splicing(Splicing::Close)),
    )
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([4, 8])
    .into()
}

/// The cells of a row: the place of the intron, the reads and the PSI of each group, the
/// difference and p.
fn cell_texts(contig: &str, line: &Line) -> [String; 7] {
    let difference = &line.difference;
    let reads = |group: usize| {
        format!(
            "{} of {}",
            grouped(u64::from(difference.reads[group])),
            grouped(u64::from(difference.totals[group]))
        )
    };
    [
        format!(
            "{contig}:{}-{}",
            grouped(difference.span.start() + 1),
            grouped(difference.span.end())
        ),
        reads(0),
        psi(difference, 0),
        reads(1),
        psi(difference, 1),
        points(difference),
        crate::card::p_value(difference.p),
    ]
}

/// The rows of the table under a row of column names that sort them. The rows fill the width of
/// the panel, and a scroll bar moves them to the side when the columns are wider. The rows of
/// the junctions that do not differ are grey.
fn grid<'a>(
    table: &SpliceTable,
    session: &Session,
    found: &'a Found,
    panel: f32,
) -> Element<'a, Message> {
    let contig = session.reference.assembly.contigs()[found.contig]
        .name
        .as_str();
    let (first, second) = &found.groups;
    let names = [
        "Place".to_owned(),
        format!("{first} reads"),
        format!("{first} PSI"),
        format!("{second} reads"),
        format!("{second} PSI"),
        "Difference".to_owned(),
        "p".to_owned(),
    ];
    let total = WIDTHS.iter().sum::<f32>().max(panel);
    let head = row(names
        .into_iter()
        .zip(WIDTHS)
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
            .width(width)
            .height(ROW_HEIGHT)
            .padding([2, 6])
            .style(style::ghost)
            .on_press(Message::Splicing(Splicing::Sort(at)))
            .into()
        }));
    let lines = found.rows.iter().enumerate().map(|(index, line)| {
        let differs = line.difference.differs();
        let cells = row(cell_texts(contig, line)
            .into_iter()
            .zip(WIDTHS)
            .map(|(cell, width)| {
                let words = text(cell).size(11).wrapping(Wrapping::None);
                let words = if differs {
                    words
                } else {
                    words.style(style::muted)
                };
                container(words)
                    .width(width)
                    .height(ROW_HEIGHT)
                    .padding([2, 6])
                    .clip(true)
                    .into()
            }));
        let chosen = table.chosen == Some(line.difference.span);
        button(cells)
            .width(Length::Fill)
            .padding(0)
            .style(if chosen { style::chosen } else { style::ghost })
            .on_press(Message::Splicing(Splicing::Go(index)))
            .into()
    });
    let bar = || Scrollbar::new().width(6).scroller_width(6).margin(0);
    let body = scrollable(Column::with_children(lines).width(total))
        .direction(Direction::Vertical(bar()))
        .style(style::scrollbar)
        .width(total)
        .height(Length::Fill);
    scrollable(column![head, body].width(total).height(Length::Fill))
        .direction(Direction::Horizontal(bar()))
        .style(style::scrollbar)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
