//! The whole-genome view: the main chromosomes side by side, each as wide as its share of the
//! bases.

use genie_core::{Viewport, layout};
use iced::Task;

use super::{Genie, Message, PANEL_GAP, Panel, Session, panel_width};

/// The least width of a chromosome in the whole-genome view, so that a click can reach it.
const LEAST: f32 = 3.0;
/// The width of a letter of a chromosome name.
const CHARACTER: f32 = 6.0;

/// The name of a chromosome on the ruler of the whole-genome view, without "chr", if it fits
/// in the width of the chromosome.
pub(super) fn name(contig: &str, width: f32) -> Option<String> {
    let short = contig
        .strip_prefix("chr")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(contig);
    let fits = short.chars().count() as f32 * CHARACTER + 2.0 <= width;
    fits.then(|| short.to_owned())
}

/// Whether the words of the locus box ask for the whole-genome view.
pub(super) fn is_genome(text: &str) -> bool {
    let text = text.trim();
    text.eq_ignore_ascii_case("all") || text.eq_ignore_ascii_case("genome")
}

impl Session {
    /// The width of each panel: the same for all panels, or the share of the bases of each
    /// chromosome in the whole-genome view.
    pub(super) fn widths(&self, width: f32) -> Vec<f32> {
        let count = self.count();
        if !self.genome {
            return vec![panel_width(width, count); count];
        }
        let lengths: Vec<u64> = (0..count)
            .map(|panel| self.view(panel).1.length())
            .collect();
        layout::widths(&lengths, width, PANEL_GAP, LEAST)
    }

    /// The share of the width of each panel, for a row of panels.
    pub(super) fn portions(&self) -> Vec<u16> {
        (0..self.count())
            .map(|panel| {
                let tenths = (self.view(panel).1.width() * 10.0).round();
                tenths.clamp(1.0, f64::from(u16::MAX)) as u16
            })
            .collect()
    }

    /// Shows the main chromosomes side by side. False if the reference has no contigs.
    pub(super) fn enter_genome(&mut self, width: f32) -> bool {
        let chosen = self.reference.assembly.chromosomes();
        let Some((first, rest)) = chosen.split_first() else {
            return false;
        };
        let contigs = self.reference.assembly.contigs();
        let lengths: Vec<u64> = chosen
            .iter()
            .map(|contig| contigs[*contig].length)
            .collect();
        let widths = layout::widths(&lengths, width, PANEL_GAP, LEAST);
        self.transition = None;
        self.genome = true;
        self.contig = *first;
        self.viewport = Viewport::new(lengths[0], f64::from(widths[0]));
        self.panels = rest
            .iter()
            .zip(&lengths[1..])
            .zip(&widths[1..])
            .map(|((contig, length), width)| Panel {
                contig: *contig,
                viewport: Viewport::new(*length, f64::from(*width)),
                transition: None,
            })
            .collect();
        for track in &mut self.tracks {
            track.extra.clear();
        }
        true
    }

    /// Goes back to one panel that shows the whole first chromosome.
    pub(super) fn leave_genome(&mut self, width: f32) {
        if !self.genome {
            return;
        }
        self.genome = false;
        self.panels.clear();
        for track in &mut self.tracks {
            track.extra.clear();
        }
        self.transition = None;
        self.viewport = Viewport::new(self.viewport.length(), f64::from(width));
    }

    /// Fits each chromosome to its width again, for example when the window changes.
    pub(super) fn fit_genome(&mut self, width: f32) {
        let widths = self.widths(width);
        self.transition = None;
        self.viewport = Viewport::new(self.viewport.length(), f64::from(widths[0]));
        for (panel, width) in self.panels.iter_mut().zip(&widths[1..]) {
            panel.transition = None;
            panel.viewport = Viewport::new(panel.viewport.length(), f64::from(*width));
        }
    }
}

impl Genie {
    /// Opens the whole-genome view, and keeps the view before it in the history.
    pub(super) fn show_genome(&mut self) -> Task<Message> {
        let width = self.width;
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let before = session.visit();
        if !session.enter_genome(width) {
            return Task::none();
        }
        if session.visit() != before {
            session.history.record(before);
        }
        (self.hover, self.pinned) = (None, None);
        self.drop_draft();
        self.fetch()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::Region;
    use genie_formats::fai::FaiRecord;
    use genie_formats::features::Kind;
    use genie_formats::tabix::Index;

    use iced::Point;

    use crate::app::{Genie, Message, Sweep};
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::shortcuts::Command;
    use crate::track::{FeatureFile, Format, Source};

    /// A genie with four contigs: two chromosomes, an unplaced contig and chrM.
    fn genie() -> Genie {
        let contigs = [
            ("chr1", 3_000),
            ("chr2", 1_000),
            ("chrUn_x", 500),
            ("chrM", 5),
        ];
        let mut offset = 0;
        let records = contigs
            .iter()
            .map(|(name, length)| {
                let record = FaiRecord {
                    name: (*name).to_owned(),
                    length: *length,
                    offset,
                    line_bases: 60,
                    line_width: 61,
                };
                offset += length + length / 60 + 8;
                record
            })
            .collect();
        let reference = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap());
        let mut genie = Genie::default();
        genie.settings.motion = false;
        let _ = genie.update(Message::Opened(Ok(reference)));
        let _ = genie.update(Message::Resized(1_000.0));
        genie
    }

    fn go(genie: &mut Genie, locus: &str) {
        let _ = genie.update(Message::LocusEdited(locus.to_owned()));
        let _ = genie.update(Message::LocusSubmitted);
    }

    /// The contig and the visible region of each panel.
    fn panels(genie: &Genie) -> Vec<(usize, Region)> {
        let session = genie.session.as_ref().unwrap();
        (0..session.count())
            .map(|panel| {
                let (contig, viewport) = session.view(panel);
                (contig, viewport.visible())
            })
            .collect()
    }

    #[test]
    fn all_shows_the_main_chromosomes_side_by_side() {
        let mut genie = genie();
        go(&mut genie, "chr2:100-200");
        go(&mut genie, "All");
        let session = genie.session.as_ref().unwrap();
        assert_eq!(session.locus(), "All");
        let whole = |contig, length| (contig, Region::between(0, length));
        assert_eq!(
            panels(&genie),
            [whole(0, 3_000), whole(1, 1_000), whole(2, 5)]
        );
        let widths = session.widths(1_000.0);
        assert!((widths[0] - 3.0 * widths[1]).abs() < 0.01, "{widths:?}");
        assert!((widths[2] - 3.0).abs() < 0.01, "{widths:?}");
        let _ = genie.update(Message::Zoom(1, 2.0, 0.0));
        let _ = genie.update(Message::Pan(0, 50.0));
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        assert_eq!(panels(&genie)[..2], [whole(0, 3_000), whole(1, 1_000)]);
        go(&mut genie, "chr1:101-300");
        assert_eq!(panels(&genie), [(0, Region::between(100, 300))]);
        let _ = genie.update(Message::Command(Command::Back));
        assert_eq!(genie.session.as_ref().unwrap().locus(), "All");
        let _ = genie.update(Message::Command(Command::Back));
        assert_eq!(panels(&genie), [(1, Region::between(99, 200))]);
        let _ = genie.update(Message::Command(Command::Forward));
        assert_eq!(panels(&genie).len(), 3);
        go(&mut genie, "genome");
        assert_eq!(panels(&genie).len(), 3);
    }

    #[test]
    fn each_chromosome_loads_its_own_track_data() {
        let mut genie = genie();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/genes.bed.gz").into(),
            name: "genes.bed.gz".to_owned(),
            data: None,
            format: Format::Features(FeatureFile::Indexed {
                kind: Kind::Bed,
                index: Index::default(),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        genie.session.as_mut().unwrap().tracks[0].contigs =
            vec![Some("chr1".to_owned()), Some("chr2".to_owned()), None, None];
        go(&mut genie, "All");
        let track = &genie.session.as_ref().unwrap().tracks[0];
        assert_eq!(track.slot(0).wanted, Some((0, Region::between(0, 3_000))));
        assert_eq!(track.slot(1).wanted, Some((1, Region::between(0, 1_000))));
        assert_eq!(track.slot(2).wanted, None);
    }

    #[test]
    fn a_click_opens_a_chromosome_and_a_drag_opens_a_region() {
        let mut genie = genie();
        go(&mut genie, "All");
        let _ = genie.update(Message::Ruler(1, Sweep::Pressed(0.5)));
        let _ = genie.update(Message::Ruler(1, Sweep::Released));
        assert_eq!(panels(&genie), [(1, Region::between(0, 1_000))]);
        go(&mut genie, "All");
        let _ = genie.update(Message::Ruler(0, Sweep::Pressed(0.25)));
        let _ = genie.update(Message::Ruler(0, Sweep::Dragged(0.5)));
        let _ = genie.update(Message::Ruler(0, Sweep::Released));
        assert_eq!(panels(&genie), [(0, Region::between(750, 1_500))]);
        go(&mut genie, "All");
        let pointer = Pointer {
            local: Point::new(10.0, 5.0),
            at: Point::new(10.0, 100.0),
        };
        let _ = genie.update(Message::TrackClicked(7, 2, pointer));
        assert_eq!(panels(&genie), [(2, Region::between(0, 5))]);
        assert!(genie.pinned.is_none());
    }

    #[test]
    fn chromosome_names_show_where_they_fit() {
        assert_eq!(super::name("chr21", 18.0).as_deref(), Some("21"));
        assert_eq!(super::name("chr1", 100.0).as_deref(), Some("1"));
        assert_eq!(super::name("chrM", 3.0), None);
        assert_eq!(super::name("2L", 30.0).as_deref(), Some("2L"));
        assert_eq!(super::name("chrUn_scaffold", 30.0), None);
    }

    #[test]
    fn signal_tracks_share_one_scale_across_chromosomes() {
        use genie_core::signal::Summary;
        use genie_formats::table::Table;

        use crate::app::view::{signal_columns, signal_scale};
        use crate::track::{Content, Loaded, SignalFile};

        let mut genie = genie();
        let source = Arc::new(Source {
            location: PathBuf::from("/data/coverage.bedGraph").into(),
            name: "coverage.bedGraph".to_owned(),
            data: None,
            format: Format::Signal(SignalFile::BedGraphTable {
                table: Table::new(Vec::new()),
            }),
        });
        let _ = genie.update(Message::TrackOpened(Ok(source)));
        let names = ["chr1", "chr2", "chrM", "chrUn_x"];
        genie.session.as_mut().unwrap().tracks[0].contigs =
            names.iter().map(|name| Some((*name).to_owned())).collect();
        go(&mut genie, "All");
        for (panel, value) in [(0, 10.0), (1, 40.0)] {
            let (contig, region) = genie.session.as_ref().unwrap().tracks[0]
                .slot(panel)
                .wanted
                .unwrap();
            let summaries = vec![Summary::single(region, value)];
            let loaded = Arc::new(Loaded::signal(region, summaries, 1));
            let _ = genie.update(Message::TrackLoaded(0, panel, contig, region, Ok(loaded)));
        }
        let session = genie.session.as_ref().unwrap();
        let track = &session.tracks[0];
        let scale = |panel| {
            let (contig, viewport) = session.view(panel);
            let Content::Signal(data) = &track.slot(panel).data(contig).unwrap().content else {
                panic!("expected a signal");
            };
            let columns = signal_columns(track, viewport, &data.summaries);
            signal_scale(session, track, panel, &columns).unwrap()
        };
        assert_eq!(scale(0), scale(1));
        assert!((scale(0).max - 40.0).abs() < 1e-3, "{:?}", scale(0));
    }
}
