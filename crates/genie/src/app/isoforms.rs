//! The isoform table of a read track under the tracks: the isoforms in view with their reads,
//! also for each group of the reads, and how their junctions compare with the open
//! transcripts.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::path::PathBuf;

use genie_core::isoform::{Kind, kind};
use genie_core::methylation::fisher;
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
use crate::track::{Content, GroupKey, Grouping, Pile};
use crate::{font, style};

/// The most groups of the reads with a column of their own. The reads of the other groups
/// share the column "Other".
const GROUPS: usize = 6;
const NONE_IN_VIEW: &str = "No spliced reads in view.";
const FEWER: &str =
    "The isoforms in view have fewer reads than a row of the lane. Show all lists them.";
const ZOOM_IN: &str = "Zoom in to see the isoforms.";
/// The names and the widths of the columns before the columns of the groups.
const COLUMNS: [(&str, f32); 7] = [
    ("Name", 250.0),
    ("Kind", 116.0),
    ("Place", 230.0),
    ("Reads", 64.0),
    ("Share", 64.0),
    ("Exons", 52.0),
    ("Length", 84.0),
];
const GROUP_WIDTH: f32 = 116.0;
const P_WIDTH: f32 = 72.0;
const ROW_HEIGHT: f32 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Isoforming {
    /// Shows the isoform table of a read track, or closes it when it shows.
    Open(u64),
    Close,
    /// Shows the isoforms that have too few reads for a row of the lane too, or hides them.
    All,
    /// Sorts the rows by a column: up, then down, then the most reads first again.
    Sort(usize),
    /// Goes to the isoform of a row.
    Go(usize),
    Export,
    ExportPicked(Option<PathBuf>),
}

/// The isoform table of a read track.
#[derive(Debug)]
pub(super) struct IsoformTable {
    pub track: u64,
    all: bool,
    sort: Option<(usize, bool)>,
    /// The span of the isoform of the row that a click went to last.
    chosen: Option<Region>,
    /// The isoforms in view with what they come from, or why the table has no rows.
    found: Option<(Key, Result<Found, &'static str>)>,
}

/// What the rows of a table come from: the view, the switches of the table and the data of the
/// tracks in the first panel.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    contig: usize,
    visible: Region,
    all: bool,
    sort: Option<(usize, bool)>,
    inputs: Vec<usize>,
}

/// The isoforms in view: the contig, the names of the columns of the groups, whether a p
/// column compares two groups, the rows in their order, and the number of isoforms that the
/// table does not show because they have too few reads for a row of the lane.
#[derive(Debug, Clone)]
struct Found {
    contig: usize,
    groups: Vec<String>,
    compared: bool,
    rows: Vec<Line>,
    hidden: usize,
}

/// A row of the table: an isoform in view.
#[derive(Debug, Clone)]
struct Line {
    name: String,
    /// The ID of the isoform in BED and GTF files, without spaces.
    id: String,
    gene: String,
    kind: Option<Kind>,
    span: Region,
    strand: Strand,
    exons: Vec<Region>,
    reads: usize,
    /// The part of the spliced reads in view, from 0 to 1.
    share: f64,
    /// The reads of each column of the groups, and their part of the reads of the column.
    groups: Vec<(usize, f64)>,
    p: Option<f64>,
    /// Whether the isoform has too few reads for a row of the lane.
    few: bool,
}

impl Line {
    fn length(&self) -> u64 {
        self.exons.iter().map(|exon| exon.len()).sum()
    }
}

impl IsoformTable {
    fn new(track: u64) -> Self {
        Self {
            track,
            all: false,
            sort: None,
            chosen: None,
            found: None,
        }
    }

    fn found(&self) -> Option<&Found> {
        self.found.as_ref()?.1.as_ref().ok()
    }
}

/// The part of a total, from 0 to 1.
fn part(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The reads of each group in a list of counts.
fn add_counts(totals: &mut Vec<(GroupKey, usize)>, counts: &[(GroupKey, usize)]) {
    for &(key, reads) in counts {
        match totals.iter_mut().find(|(known, _)| *known == key) {
            Some((_, total)) => *total += reads,
            None => totals.push((key, reads)),
        }
    }
}

/// The rows of the isoforms in view of a pile, with the isoforms that have too few reads for a
/// row of the lane when `all` is on, in the order of a column.
fn table_rows(session: &Session, pile: &Pile, (all, sort): (bool, Option<(usize, bool)>)) -> Found {
    let (contig, viewport) = session.frame(0);
    let shown = view::isoforms_in_view(session, 0, pile, (contig, viewport.visible()));
    let introns = view::transcript_introns(session, 0, contig);
    let spliced: usize = shown.seen.iter().map(|seen| seen.reads).sum();
    let mut totals = Vec::new();
    if pile.grouping != Grouping::None {
        for seen in &shown.seen {
            add_counts(&mut totals, &seen.groups);
        }
    }
    let mut largest = totals.clone();
    largest.sort_by(|(a, first), (b, second)| second.cmp(first).then(a.cmp(b)));
    let mut kept: Vec<GroupKey> = largest.iter().take(GROUPS).map(|(key, _)| *key).collect();
    kept.sort_unstable();
    let mut groups: Vec<String> = kept.iter().map(|&key| pile.group_name(key)).collect();
    if largest.len() > GROUPS {
        groups.push("Other".to_owned());
    }
    let column = |key: GroupKey| {
        kept.iter()
            .position(|known| *known == key)
            .unwrap_or(kept.len())
    };
    let mut column_totals = vec![0; groups.len()];
    for &(key, total) in &totals {
        column_totals[column(key)] += total;
    }
    let mut valued: Vec<(GroupKey, usize)> = totals
        .iter()
        .copied()
        .filter(|&(key, _)| pile.group_has_value(key))
        .collect();
    valued.sort_unstable();
    let compared = (valued.len() == 2).then(|| (valued[0], valued[1]));
    let mut hidden = 0;
    let mut rows = Vec::new();
    for seen in &shown.seen {
        let few = seen.reads < shown.least;
        if few && !all {
            hidden += 1;
            continue;
        }
        let isoform = &shown.all[seen.index];
        let mut counts = vec![0; groups.len()];
        for &(key, reads) in &seen.groups {
            if let Some(slot) = counts.get_mut(column(key)) {
                *slot += reads;
            }
        }
        let reads_of = |key: GroupKey| {
            seen.groups
                .iter()
                .find(|(known, _)| *known == key)
                .map_or(0, |(_, reads)| *reads)
        };
        let p = compared.map(|((first, first_total), (second, second_total))| {
            let (a, b) = (reads_of(first), reads_of(second));
            fisher([
                [count(a), count(first_total - a)],
                [count(b), count(second_total - b)],
            ])
        });
        rows.push(Line {
            name: seen.name.clone(),
            id: seen.id.clone(),
            gene: seen.gene.clone(),
            kind: (!introns.is_empty()).then(|| kind(&isoform.junctions, &introns, view::SLACK)),
            span: isoform.span,
            strand: seen.strand,
            exons: isoform.exons(),
            reads: seen.reads,
            share: part(seen.reads, spliced),
            groups: counts
                .iter()
                .zip(&column_totals)
                .map(|(&reads, &total)| (reads, part(reads, total)))
                .collect(),
            p,
            few,
        });
    }
    if let Some((column, descending)) = sort {
        rows.sort_by(|a, b| {
            let order = compare(a, b, column);
            if descending { order.reverse() } else { order }
        });
    }
    Found {
        contig,
        groups,
        compared: compared.is_some(),
        rows,
        hidden,
    }
}

/// The order of two rows by a column: a column of the table, of a group, or p.
fn compare(a: &Line, b: &Line, column: usize) -> Ordering {
    match column {
        0 => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        1 => a.kind.cmp(&b.kind),
        2 => (a.span.start(), a.span.end()).cmp(&(b.span.start(), b.span.end())),
        3 => a.reads.cmp(&b.reads),
        4 => a.share.total_cmp(&b.share),
        5 => a.exons.len().cmp(&b.exons.len()),
        6 => a.length().cmp(&b.length()),
        column => {
            let at = column - COLUMNS.len();
            match (a.groups.get(at), b.groups.get(at)) {
                (Some(first), Some(second)) => first.0.cmp(&second.0),
                _ => a.p.unwrap_or(1.0).total_cmp(&b.p.unwrap_or(1.0)),
            }
        }
    }
}

/// The data of the tracks in the first panel, which give the reads and the transcripts.
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
    Gtf,
    Tsv,
}

/// The format of a file from its extension: GTF, TSV or BED. A file with another extension
/// gets .bed.
fn with_format(path: PathBuf) -> (PathBuf, Format) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("gtf") => (path, Format::Gtf),
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

/// The rows as BED12 lines: the ID as the name, the reads up to 1,000 as the score, and the
/// exons as the blocks.
fn bed(contig: &str, rows: &[Line]) -> String {
    let mut text = String::new();
    for line in rows {
        let start = line.span.start();
        let (mut sizes, mut starts) = (String::new(), String::new());
        for exon in &line.exons {
            let _ = write!(sizes, "{},", exon.len());
            let _ = write!(starts, "{},", exon.start() - start);
        }
        let _ = writeln!(
            text,
            "{contig}\t{start}\t{}\t{}\t{}\t{}\t{start}\t{start}\t0\t{}\t{sizes}\t{starts}",
            line.span.end(),
            line.id,
            line.reads.min(1_000),
            strand_text(line.strand),
            line.exons.len()
        );
    }
    text
}

/// A value of a GTF attribute, without the double quotes that end it.
fn quoted(value: &str) -> String {
    value.replace('"', "'")
}

/// The rows as GTF lines: a transcript line and the exon lines of each isoform, with its gene,
/// its ID, its reads and its kind. The exons count from the 5' end.
fn gtf(contig: &str, rows: &[Line]) -> String {
    let mut text = String::new();
    for line in rows {
        let strand = strand_text(line.strand);
        let ids = format!(
            "gene_id \"{}\"; transcript_id \"{}\";",
            quoted(&line.gene),
            quoted(&line.id)
        );
        let mut more = format!(" reads \"{}\";", line.reads);
        if let Some(kind) = line.kind {
            let _ = write!(more, " kind \"{}\";", kind.label());
        }
        let _ = writeln!(
            text,
            "{contig}\tgenie\ttranscript\t{}\t{}\t.\t{strand}\t.\t{ids}{more}",
            line.span.start() + 1,
            line.span.end()
        );
        let exons = line.exons.len();
        for (at, exon) in line.exons.iter().enumerate() {
            let number = if line.strand == Strand::Reverse {
                exons - at
            } else {
                at + 1
            };
            let _ = writeln!(
                text,
                "{contig}\tgenie\texon\t{}\t{}\t.\t{strand}\t.\t{ids} exon_number \"{number}\";",
                exon.start() + 1,
                exon.end()
            );
        }
    }
    text
}

/// The rows as text with a tab between the cells and a header line: the columns of the table
/// with the place in four columns, the shares from 0 to 1, and the reads and the share of each
/// group in two columns.
fn tsv(contig: &str, found: &Found) -> String {
    let mut text =
        String::from("Name\tKind\tChrom\tStart\tEnd\tStrand\tReads\tShare\tExons\tLength");
    for group in &found.groups {
        let _ = write!(text, "\t{group} reads\t{group} share");
    }
    if found.compared {
        text.push_str("\tp");
    }
    text.push('\n');
    for line in &found.rows {
        let kind = line.kind.map_or("", Kind::label);
        let _ = write!(
            text,
            "{}\t{kind}\t{contig}\t{}\t{}\t{}\t{}\t{:.4}\t{}\t{}",
            line.name.replace('\t', " "),
            line.span.start() + 1,
            line.span.end(),
            strand_text(line.strand),
            line.reads,
            line.share,
            line.exons.len(),
            line.length()
        );
        for (reads, share) in &line.groups {
            let _ = write!(text, "\t{reads}\t{share:.4}");
        }
        if let Some(p) = line.p {
            let _ = write!(text, "\t{p:.4e}");
        }
        text.push('\n');
    }
    text
}

async fn save_isoforms(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export the isoforms")
        .set_file_name(name)
        .add_filter("BED", &["bed"])
        .add_filter("GTF", &["gtf"])
        .add_filter("Tab-separated values", &["tsv"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}

impl Genie {
    pub(super) fn isoforming(&mut self, isoforming: Isoforming) -> Task<Message> {
        match isoforming {
            Isoforming::Open(id) => {
                self.close_menus();
                let open = self
                    .isoforms
                    .as_ref()
                    .is_some_and(|table| table.track == id);
                self.isoforms = (!open).then(|| IsoformTable::new(id));
                if self.isoforms.is_some() {
                    (self.table, self.dmrs, self.splices) = (None, None, None);
                }
            }
            Isoforming::Close => self.isoforms = None,
            Isoforming::All => {
                if let Some(table) = &mut self.isoforms {
                    table.all = !table.all;
                }
            }
            Isoforming::Sort(column) => {
                if let Some(table) = &mut self.isoforms {
                    table.sort = match table.sort {
                        Some((known, false)) if known == column => Some((column, true)),
                        Some((known, true)) if known == column => None,
                        _ => Some((column, false)),
                    };
                }
            }
            Isoforming::Go(row) => return self.go_to_isoform(row),
            Isoforming::Export => {
                let name = self
                    .isoforms
                    .as_ref()
                    .and_then(|table| self.track_named(table.track))
                    .map_or_else(|| "isoforms".to_owned(), |name| format!("{name} isoforms"));
                return Task::perform(save_isoforms(format!("{name}.bed")), |path| {
                    Message::Isoforms(Isoforming::ExportPicked(path))
                });
            }
            Isoforming::ExportPicked(Some(path)) => return self.export_isoforms(path),
            Isoforming::ExportPicked(None) => {}
        }
        Task::none()
    }

    fn go_to_isoform(&mut self, row: usize) -> Task<Message> {
        let Some(table) = &mut self.isoforms else {
            return Task::none();
        };
        let Some((contig, span)) = table
            .found()
            .and_then(|found| Some((found.contig, found.rows.get(row)?.span)))
        else {
            return Task::none();
        };
        table.chosen = Some(span);
        self.go_to(Target {
            contig,
            span: Span::Region(span.expand(span.len() / 10, u64::MAX)),
        })
    }

    fn export_isoforms(&mut self, path: PathBuf) -> Task<Message> {
        let (path, format) = with_format(path);
        let text = {
            let (Some(found), Some(session)) = (
                self.isoforms.as_ref().and_then(IsoformTable::found),
                self.session.as_ref(),
            ) else {
                return Task::none();
            };
            let contig = &session.reference.assembly.contigs()[found.contig].name;
            match format {
                Format::Bed => bed(contig, &found.rows),
                Format::Gtf => gtf(contig, &found.rows),
                Format::Tsv => tsv(contig, found),
            }
        };
        match paths::replace(&path, &text) {
            Ok(()) => self.inform(format!("Saved {}.", file_name(&path))),
            Err(error) => {
                self.warn(format!("The isoforms could not be saved. {error}"));
                Task::none()
            }
        }
    }

    /// Finds the rows again when the view, the switches of the table or the data of the tracks
    /// changed. The table closes when its track closes.
    pub(super) fn refresh_isoforms(&mut self) {
        let (Some(table), Some(session)) = (&mut self.isoforms, &self.session) else {
            self.isoforms = None;
            return;
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == table.track) else {
            self.isoforms = None;
            return;
        };
        let (contig, viewport) = session.frame(0);
        let key = Key {
            contig,
            visible: viewport.visible(),
            all: table.all,
            sort: table.sort,
            inputs: inputs(session, contig),
        };
        if table.found.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        let found = match track.slot(0).data(contig).map(|data| &data.content) {
            Some(Content::Reads(pile)) => Ok(table_rows(session, pile, (table.all, table.sort))),
            Some(_) => Err(ZOOM_IN),
            None => Err(NONE_IN_VIEW),
        };
        table.found = Some((key, found));
    }

    pub(super) fn isoform_view<'a>(&'a self, session: &'a Session) -> Option<Element<'a, Message>> {
        let table = self.isoforms.as_ref()?;
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
                    Some(Ok(found)) if found.hidden > 0 => FEWER,
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

/// The bar of the table: its name, the number of isoforms, Show all, Export and close.
fn bar<'a>(table: &IsoformTable, name: &str, found: Option<&Found>) -> Element<'a, Message> {
    let count = found.map_or(0, |found| found.rows.len());
    let mut words = match count {
        1 => "1 isoform in view".to_owned(),
        count => format!("{} isoforms in view", grouped(count as u64)),
    };
    if let Some(found) = found.filter(|found| found.hidden > 0) {
        let _ = write!(
            words,
            " · {} with fewer reads",
            grouped(found.hidden as u64)
        );
    }
    row![
        text(format!("Isoforms · {name}"))
            .font(font::UI_STRONG)
            .size(12),
        text(words).size(11).style(style::muted),
        space::horizontal(),
        button(text("Show all").size(12))
            .padding([3, 8])
            .style(if table.all {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::Isoforms(Isoforming::All)),
        button(text("Export…").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe((count > 0).then_some(Message::Isoforms(Isoforming::Export))),
        button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Isoforms(Isoforming::Close)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([4, 8])
    .into()
}

fn percent(share: f64) -> String {
    format!("{:.1} %", share * 100.0)
}

/// The cells of a row: the columns of the table, of the groups, and p.
fn cell_texts(contig: &str, line: &Line) -> Vec<String> {
    let mut cells = vec![
        line.name.clone(),
        line.kind
            .map_or_else(String::new, |kind| kind.label().to_owned()),
        format!(
            "{contig}:{}-{}",
            grouped(line.span.start() + 1),
            grouped(line.span.end())
        ),
        grouped(line.reads as u64),
        percent(line.share),
        line.exons.len().to_string(),
        format!("{} bp", grouped(line.length())),
    ];
    cells.extend(
        line.groups
            .iter()
            .map(|&(reads, share)| format!("{} · {}", grouped(reads as u64), percent(share))),
    );
    cells.extend(line.p.map(crate::card::p_value));
    cells
}

/// The rows of the table under a row of column names that sort them. The rows fill the width of
/// the panel, and a scroll bar moves them to the side when the columns are wider.
fn grid<'a>(
    table: &IsoformTable,
    session: &Session,
    found: &'a Found,
    panel: f32,
) -> Element<'a, Message> {
    let contig = session.reference.assembly.contigs()[found.contig]
        .name
        .as_str();
    let mut names: Vec<String> = COLUMNS.iter().map(|(name, _)| (*name).to_owned()).collect();
    let mut widths: Vec<f32> = COLUMNS.iter().map(|(_, width)| *width).collect();
    for group in &found.groups {
        names.push(group.clone());
        widths.push(GROUP_WIDTH);
    }
    if found.compared {
        names.push("p".to_owned());
        widths.push(P_WIDTH);
    }
    let total = widths.iter().sum::<f32>().max(panel);
    let head = row(names
        .into_iter()
        .zip(&widths)
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
            .on_press(Message::Isoforms(Isoforming::Sort(at)))
            .into()
        }));
    let lines = found.rows.iter().enumerate().map(|(index, line)| {
        let few = line.few;
        let cells = row(cell_texts(contig, line)
            .into_iter()
            .zip(&widths)
            .map(|(cell, width)| {
                let words = text(cell).size(11).wrapping(Wrapping::None);
                let words = if few {
                    words.style(style::muted)
                } else {
                    words
                };
                container(words)
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
            .on_press(Message::Isoforms(Isoforming::Go(index)))
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
