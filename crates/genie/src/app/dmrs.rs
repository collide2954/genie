//! The DMRs of the methylation difference of a track in a table under the tracks: the runs of
//! sites in view that differ in the same direction.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use genie_core::methylation::{Dmr, DmrRule, dmrs};
use genie_core::{Region, Span, Target, grouped};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Task};

use super::view::{difference_inputs, hairline, shown_difference};
use super::{Genie, Message, Session};
use crate::icon::{self, Icon};
use crate::paths::{self, file_name};
use crate::theme::tokens;
use crate::{font, style};

/// The least numbers of sites of a DMR that the table offers.
const CHOICES: [usize; 3] = [3, 5, 10];
const NONE_IN_VIEW: &str = "No DMRs in view.";
const NO_DIFFERENCE: &str = "The track shows no methylation difference in view.";
/// The widths of the columns: place, sites, length, difference and p.
const WIDTHS: [f32; 5] = [240.0, 60.0, 90.0, 110.0, 80.0];
const NAMES: [&str; 5] = ["Place", "CpGs", "Length", "Difference", "Smallest p"];

#[derive(Debug, Clone, PartialEq)]
pub enum Dmring {
    /// Shows the DMR table of a track, or closes it when it shows.
    Open(u64),
    Close,
    /// Keeps only the DMRs with at least this number of sites.
    Least(usize),
    /// Goes to the DMR of a row.
    Go(usize),
    Export,
    ExportPicked(Option<PathBuf>),
}

/// The DMR table of a track.
#[derive(Debug)]
pub(super) struct DmrTable {
    pub track: u64,
    least: usize,
    /// The DMRs in view, with what they come from.
    found: Option<(Key, Found)>,
}

/// What the DMRs of a table come from: the view, the least sites and the data of the difference.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    contig: usize,
    visible: Region,
    least: usize,
    inputs: Vec<usize>,
}

/// The DMRs in view: the contig, the names of the two groups, and the DMRs in their order.
#[derive(Debug, Clone)]
struct Found {
    contig: usize,
    groups: Option<(String, String)>,
    dmrs: Vec<Dmr>,
}

impl DmrTable {
    fn new(track: u64) -> Self {
        Self {
            track,
            least: DmrRule::default().sites,
            found: None,
        }
    }

    fn found(&self) -> Option<&Found> {
        self.found.as_ref().map(|(_, found)| found)
    }
}

/// The mean difference of a DMR in percentage points, with its sign: "+40.0" or "-25.0".
fn points(dmr: &Dmr) -> String {
    format!("{:+.1}", dmr.points)
}

/// The DMRs as BED lines: the place, the mean difference as the name, and the number of sites
/// as the score.
fn bed(contig: &str, dmrs: &[Dmr]) -> String {
    let mut text = String::new();
    for dmr in dmrs {
        let _ = writeln!(
            text,
            "{contig}\t{}\t{}\t{}\t{}\t.",
            dmr.span.start(),
            dmr.span.end(),
            points(dmr),
            dmr.sites.min(1_000)
        );
    }
    text
}

fn with_bed(path: PathBuf) -> PathBuf {
    let is_bed = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bed"));
    if is_bed {
        return path;
    }
    let mut text = path.into_os_string();
    text.push(".bed");
    PathBuf::from(text)
}

async fn save_bed(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export the DMRs")
        .set_file_name(name)
        .add_filter("BED", &["bed"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}

impl Genie {
    pub(super) fn dmring(&mut self, dmring: Dmring) -> Task<Message> {
        match dmring {
            Dmring::Open(id) => {
                self.close_menus();
                let open = self.dmrs.as_ref().is_some_and(|table| table.track == id);
                self.dmrs = (!open).then(|| DmrTable::new(id));
                if self.dmrs.is_some() {
                    (self.table, self.isoforms, self.splices) = (None, None, None);
                }
            }
            Dmring::Close => self.dmrs = None,
            Dmring::Least(least) => {
                if let Some(table) = &mut self.dmrs {
                    table.least = least;
                }
            }
            Dmring::Go(row) => return self.go_to_dmr(row),
            Dmring::Export => {
                let name = self
                    .dmrs
                    .as_ref()
                    .and_then(|table| self.track_named(table.track))
                    .map_or_else(|| "DMRs".to_owned(), |name| format!("{name} DMRs"));
                return Task::perform(save_bed(format!("{name}.bed")), |path| {
                    Message::Dmrs(Dmring::ExportPicked(path))
                });
            }
            Dmring::ExportPicked(Some(path)) => return self.export_dmrs(&with_bed(path)),
            Dmring::ExportPicked(None) => {}
        }
        Task::none()
    }

    fn go_to_dmr(&mut self, row: usize) -> Task<Message> {
        let Some(found) = self.dmrs.as_ref().and_then(DmrTable::found) else {
            return Task::none();
        };
        let Some(dmr) = found.dmrs.get(row) else {
            return Task::none();
        };
        let target = Target {
            contig: found.contig,
            span: Span::Region(dmr.span.expand(dmr.span.len() / 2, u64::MAX)),
        };
        self.go_to(target)
    }

    fn export_dmrs(&mut self, path: &Path) -> Task<Message> {
        let (Some(found), Some(session)) = (
            self.dmrs.as_ref().and_then(DmrTable::found),
            self.session.as_ref(),
        ) else {
            return Task::none();
        };
        let contig = &session.reference.assembly.contigs()[found.contig].name;
        match paths::replace(path, &bed(contig, &found.dmrs)) {
            Ok(()) => self.inform(format!("Saved {}.", file_name(path))),
            Err(error) => {
                self.warn(format!("The DMRs could not be saved. {error}"));
                Task::none()
            }
        }
    }

    /// Finds the DMRs again when the view, the least sites or the data of the difference
    /// changed. The table closes when its track closes.
    pub(super) fn refresh_dmrs(&mut self) {
        let (Some(table), Some(session)) = (&mut self.dmrs, &self.session) else {
            self.dmrs = None;
            return;
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == table.track) else {
            self.dmrs = None;
            return;
        };
        let (contig, viewport) = session.frame(0);
        let visible = viewport.visible();
        let key = Key {
            contig,
            visible,
            least: table.least,
            inputs: difference_inputs(session, track, 0),
        };
        if table.found.as_ref().is_some_and(|(known, _)| *known == key) {
            return;
        }
        let rule = DmrRule {
            sites: table.least,
            ..DmrRule::default()
        };
        let found = match shown_difference(session, track, 0) {
            Some((groups, differences)) => {
                let shown: Vec<_> = differences
                    .into_iter()
                    .filter(|site| visible.contains(site.position))
                    .collect();
                Found {
                    contig,
                    groups: Some(groups),
                    dmrs: dmrs(&shown, rule),
                }
            }
            None => Found {
                contig,
                groups: None,
                dmrs: Vec::new(),
            },
        };
        table.found = Some((key, found));
    }

    pub(super) fn dmr_view<'a>(&'a self, session: &'a Session) -> Option<Element<'a, Message>> {
        let table = self.dmrs.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == table.track)?;
        let found = table.found();
        let dmrs = found.map_or(&[][..], |found| found.dmrs.as_slice());
        let header = bar(table, &track.name, found);
        let contig = found.map_or("", |found| {
            session.reference.assembly.contigs()[found.contig]
                .name
                .as_str()
        });
        let body: Element<'a, Message> = if dmrs.is_empty() {
            let hint = if found.is_some_and(|found| found.groups.is_none()) {
                NO_DIFFERENCE
            } else {
                NONE_IN_VIEW
            };
            container(text(hint).size(11).style(style::muted))
                .padding([4, 8])
                .height(Length::Fill)
                .into()
        } else {
            let rows = dmrs.iter().enumerate().map(|(index, dmr)| {
                let place = format!(
                    "{contig}:{}-{}",
                    grouped(dmr.span.start() + 1),
                    grouped(dmr.span.end())
                );
                let cells = [
                    place,
                    dmr.sites.to_string(),
                    format!("{} bp", grouped(dmr.span.len())),
                    points(dmr),
                    crate::card::p_value(dmr.p),
                ];
                button(cells_row(cells, false))
                    .padding([1, 8])
                    .width(Length::Fill)
                    .style(style::ghost)
                    .on_press(Message::Dmrs(Dmring::Go(index)))
                    .into()
            });
            scrollable(Column::with_children(rows))
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0),
                ))
                .style(style::scrollbar)
                .height(Length::Fill)
                .into()
        };
        let names = container(cells_row(NAMES.map(str::to_owned), true)).padding([2, 8]);
        Some(
            container(column![hairline(), header, names, body])
                .width(Length::Fill)
                .height(super::table::HEIGHT)
                .style(|theme| container::background(tokens(theme).bar))
                .into(),
        )
    }
}

/// The bar of the table: its name, the number of DMRs, the names of the two groups, the least
/// number of sites, Export BED and close.
fn bar<'a>(table: &DmrTable, name: &str, found: Option<&Found>) -> Element<'a, Message> {
    let count = found.map_or(0, |found| found.dmrs.len());
    let words = match count {
        1 => "1 DMR in view".to_owned(),
        count => format!("{} DMRs in view", grouped(count as u64)),
    };
    let groups = found
        .and_then(|found| found.groups.as_ref())
        .map(|(a, b)| format!("{a} − {b}"));
    let least = CHOICES.map(|count| {
        button(text(count.to_string()).size(12))
            .padding([3, 8])
            .style(if count == table.least {
                style::chosen
            } else {
                style::ghost
            })
            .on_press(Message::Dmrs(Dmring::Least(count)))
            .into()
    });
    row![
        text(format!("DMRs · {name}"))
            .font(font::UI_STRONG)
            .size(12),
        text(words).size(11).style(style::muted),
    ]
    .push(groups.map(|groups| text(groups).size(11).style(style::muted)))
    .push(space::horizontal())
    .push(text("CpGs at least").size(11).style(style::muted))
    .push(row(least).spacing(2))
    .push(
        button(text("Export BED").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe((count > 0).then_some(Message::Dmrs(Dmring::Export))),
    )
    .push(
        button(icon::view(Icon::Close, 12.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Dmrs(Dmring::Close)),
    )
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([4, 8])
    .into()
}

/// A row of the five cells of a DMR, or of the names of the columns.
fn cells_row<'a>(cells: [String; 5], names: bool) -> Element<'a, Message> {
    let cells = cells.into_iter().zip(WIDTHS).map(|(cell, width)| {
        let words = text(cell).size(11).width(width);
        if names {
            words.font(font::UI_STRONG).into()
        } else {
            words.into()
        }
    });
    row(cells).into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::Region;
    use genie_core::methylation::Site;
    use genie_render::signal::Operation;
    use iced::Point;
    use iced_test::simulator;

    use super::Dmring;
    use crate::app::{Change, Genie, Message};
    use crate::reference::Reference;
    use crate::scratch::Scratch;
    use crate::track::{Format, Loaded, SignalFile, Source};

    fn source(name: &str) -> Arc<Source> {
        Arc::new(Source {
            location: PathBuf::from(format!("/data/{name}")).into(),
            name: name.to_owned(),
            data: None,
            format: Format::Signal(SignalFile::BedMethylTable {
                table: genie_formats::table::Table::new(Vec::new()),
                codes: vec!["5mC".to_owned()],
            }),
        })
    }

    /// Two bedMethyl tracks on chr1: the first has 18 of 20 reads modified at five sites and the
    /// second 2 of 20, so the first track compares with the second as a methylation difference.
    /// The sites at 100, 150 and 200 make a DMR. The two at 800 and 850 are more than 500 bp
    /// away, and too few for a DMR.
    fn genie() -> (Genie, u64) {
        let records = vec![genie_formats::fai::FaiRecord {
            name: "chr1".to_owned(),
            length: 1_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        }];
        let reference = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap());
        let mut genie = Genie::default();
        genie.settings.motion = false;
        let _ = genie.update(Message::Opened(Ok(reference)));
        for name in ["tumor.bedMethyl", "normal.bedMethyl"] {
            let _ = genie.update(Message::TrackOpened(Ok(source(name))));
        }
        let region = Region::between(0, 1_000);
        let session = genie.session.as_mut().unwrap();
        let ids: Vec<u64> = session.tracks.iter().map(|track| track.id).collect();
        for (track, modified) in session.tracks.iter_mut().zip([18, 2]) {
            track.slot.wanted = Some((0, region));
            let sites = [100, 150, 200, 800, 850]
                .map(|position| Site {
                    position,
                    modified,
                    total: 20,
                })
                .to_vec();
            let loaded = Loaded::methylation(region, sites, 1);
            track.accept_at(0, 0, region, Ok(Arc::new(loaded)));
        }
        let _ = genie.update(Message::TrackChanged(ids[0], Change::Compare(Some(ids[1]))));
        let methylation = Change::Operation(Operation::Methylation);
        let _ = genie.update(Message::TrackChanged(ids[0], methylation));
        (genie, ids[0])
    }

    fn shows(genie: &Genie, words: &str) -> bool {
        simulator(genie.view()).find(words).is_ok()
    }

    #[test]
    fn the_table_lists_the_dmrs_in_view_and_goes_to_them() {
        let (mut genie, id) = genie();
        let _ = genie.update(Message::TrackMenu(id, Point::new(100.0, 120.0)));
        let mut ui = simulator(genie.view());
        ui.click("DMR table").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Dmrs(Dmring::Open(id))));
        let _ = genie.update(Message::Dmrs(Dmring::Open(id)));
        for words in [
            "DMRs · tumor.bedMethyl",
            "1 DMR in view",
            "tumor.bedMethyl − normal.bedMethyl",
            "chr1:101-201",
            "101 bp",
            "+80.0",
        ] {
            assert!(shows(&genie, words), "{words}");
        }
        let _ = genie.update(Message::Dmrs(Dmring::Least(5)));
        assert!(shows(&genie, super::NONE_IN_VIEW));
        let _ = genie.update(Message::Dmrs(Dmring::Least(3)));
        let _ = genie.update(Message::Dmrs(Dmring::Go(0)));
        assert!(
            shows(&genie, "chr1:51-251"),
            "the DMR with half its length at each side"
        );
    }

    #[test]
    fn dmrs_export_as_bed_and_the_table_closes_with_its_track() {
        let (mut genie, id) = genie();
        let _ = genie.update(Message::Dmrs(Dmring::Open(id)));
        let scratch = Scratch::new("dmrs");
        let path = scratch.file("dmrs", "");
        let _ = genie.update(Message::Dmrs(Dmring::ExportPicked(Some(path.clone()))));
        let text = std::fs::read_to_string(path.with_extension("bed")).unwrap();
        assert_eq!(text, "chr1\t100\t201\t+80.0\t3\t.\n");
        let _ = genie.update(Message::TrackChanged(id, Change::Remove));
        assert!(genie.dmrs.is_none());
    }
}
