//! The dot plot of a PAF track in place of the tracks: the target sequences across, the query
//! sequences down, and a line for each alignment that the filter of the track keeps.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use genie_core::dotplot::{Plot, Shown, Window};
use genie_core::{Feature, Span, Strand, Target, grouped};
use genie_formats::fields::Rule;
use genie_formats::paf;
use genie_render::dotplot::{self as render, Coloring, Marks};
use genie_render::scene::Primitive;
use genie_render::synteny;
use iced::widget::{button, column, container, responsive, row, space, text};
use iced::{Alignment, Element, Length, Point, Task};

use super::view::hairline;
use super::{Genie, Message, Session};
use crate::card::{Card, Changes, Detail};
use crate::icon::{self, Icon};
use crate::reference::Reference;
use crate::scene::{self, Pointer};
use crate::theme::tokens;
use crate::track::{Format, Source};
use crate::{font, style};

/// The least move of the pointer, in pixels, that makes a drag instead of a click.
const SLOP: f32 = 4.0;
const NO_ALIGNMENTS: &str = "The file has no alignments.";
const ALL_HIDDEN: &str = "The filter of the track hides all alignments.";

/// The number of the last reading of the alignments of a plot.
static READINGS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq)]
pub enum Plotting {
    /// Shows the dot plot of a track in place of the tracks.
    Open(u64),
    /// The alignments of a plot, with the number of their reading.
    Read(u64, Result<Arc<Plot>, String>),
    /// Shows the tracks again.
    Close,
    /// The pointer over the plot, with the size of the plot in pixels.
    Hovered(Option<Pointer>, (f32, f32)),
    Pressed,
    Released,
    /// Shows the whole plot.
    Whole,
    Color(Coloring),
    /// Shows the synteny view of the alignments when true, else the dot plot.
    Synteny(bool),
}

/// The dot plot of a track. It stays when the tracks show again, so that Back can show it again
/// without reading the file again.
#[derive(Debug)]
pub(super) struct DotPlot {
    pub track: u64,
    /// True while the plot shows in place of the tracks.
    pub shown: bool,
    /// The part of the plot in view, or None for the whole plot.
    pub window: Option<Window>,
    pub coloring: Coloring,
    /// True for the synteny view: the targets in a row at the top, the queries in a row at the
    /// bottom, and a band for each alignment.
    pub synteny: bool,
    /// The alignments, once genie has read them, or why genie could not read them.
    plot: Option<Result<Arc<Plot>, String>>,
    /// The identities at the two ends of the identity scale of the alignments.
    identities: (f64, f64),
    /// The number of the last reading, and the rules of the filter of the track that it follows.
    reading: u64,
    rules: Vec<Rule>,
    /// The pointer over the plot and the size of the plot.
    pointer: Option<(Pointer, (f32, f32))>,
    /// The place of the pointer when the left button went down, for a box that zooms in.
    pressed: Option<(f32, f32)>,
    /// The alignment under the pointer.
    chosen: Option<usize>,
}

impl DotPlot {
    fn new(shown: Shown) -> Self {
        Self {
            track: shown.track,
            shown: true,
            window: shown.window,
            coloring: Coloring::default(),
            synteny: false,
            plot: None,
            identities: (0.0, 1.0),
            reading: 0,
            rules: Vec::new(),
            pointer: None,
            pressed: None,
            chosen: None,
        }
    }

    /// The plot as the history keeps it, while it shows.
    pub(super) fn shown(&self) -> Option<Shown> {
        self.shown.then_some(Shown {
            track: self.track,
            window: self.window,
        })
    }

    fn data(&self) -> Option<&Arc<Plot>> {
        self.plot.as_ref()?.as_ref().ok()
    }

    fn window_of(&self, plot: &Plot) -> Window {
        self.window.unwrap_or_else(|| plot.whole())
    }

    /// The box that the pointer drags, from the place where the button went down.
    fn dragged(&self) -> Option<((f32, f32), (f32, f32))> {
        let (pointer, _) = self.pointer?;
        Some((self.pressed?, (pointer.local.x, pointer.local.y)))
    }

    fn forget_pointer(&mut self) {
        (self.pointer, self.pressed, self.chosen) = (None, None, None);
    }

    /// The alignment under a place of the view that shows.
    fn hit_at(&self, plot: &Plot, size: (f32, f32), place: (f32, f32)) -> Option<usize> {
        let window = self.window_of(plot);
        if self.synteny {
            synteny::hit(plot, window, size, place)
        } else {
            render::hit(plot, window, size, place)
        }
    }

    /// The part of the plot for a drag from one place to another, in the view that shows: a box
    /// of the dot plot, or a range of a row of the synteny view.
    fn zoomed(
        &self,
        plot: &Plot,
        size: (f32, f32),
        (from, to): ((f32, f32), (f32, f32)),
    ) -> Option<Window> {
        let window = self.window_of(plot);
        if self.synteny {
            synteny::zoomed(plot, window, size, from, to)
        } else {
            render::zoomed(window, size, from, to)
        }
    }

    /// The picture of the view that shows, for a space of `size` pixels.
    fn scene(&self, plot: &Plot, size: (f32, f32)) -> Vec<Primitive> {
        let marks = Marks {
            coloring: self.coloring,
            identities: self.identities,
            chosen: self.chosen,
            dragged: self.dragged(),
        };
        let window = self.window_of(plot);
        if self.synteny {
            synteny::scene(plot, window, size, marks)
        } else {
            render::scene(plot, window, size, marks)
        }
    }
}

impl Session {
    /// The dot plot that shows in place of the tracks.
    pub(super) fn shown_plot(&self) -> Option<&DotPlot> {
        self.plot.as_deref().filter(|plot| plot.shown)
    }

    /// Shows the tracks again in place of the dot plot.
    pub(super) fn hide_plot(&mut self) {
        if let Some(plot) = &mut self.plot {
            plot.shown = false;
            plot.forget_pointer();
        }
    }
}

/// The plot of the alignments of a PAF track that pass the rules of its filter, with the targets
/// in the order of the reference.
fn read(source: &Source, rules: &[Rule], reference: &Reference) -> Result<Plot, String> {
    let Format::Features(file) = &source.format else {
        return Err("The track is not a PAF file.".to_owned());
    };
    let builder = file.alignments(&source.location, rules)?;
    Ok(builder.finish(|name| reference.assembly.find(name)))
}

/// The card of an alignment: the query, the place on the target and the fields of its line.
fn card(track: u64, plot: &Plot, index: usize, at: Point) -> Card {
    let hit = &plot.hits[index];
    let strand = if hit.reverse {
        Strand::Reverse
    } else {
        Strand::Forward
    };
    let name = plot.queries[hit.query].name.as_str();
    Card {
        track,
        contig: plot.targets[hit.target].name.clone(),
        detail: Detail::Feature(Feature::new(name, hit.on_target, strand)),
        at,
        samples: Vec::new(),
        fields: paf::plot_fields(plot, hit),
        effects: Changes::default(),
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        _ => format!("{} {many}", grouped(count as u64)),
    }
}

impl Genie {
    pub(super) fn plotting(&mut self, plotting: Plotting) -> Task<Message> {
        match plotting {
            Plotting::Open(track) => {
                self.close_menus();
                return self.open_plot(
                    Shown {
                        track,
                        window: None,
                    },
                    true,
                );
            }
            Plotting::Read(reading, result) => {
                if let Some(plot) = self.plot_mut().filter(|plot| plot.reading == reading) {
                    let identities = result.as_ref().ok().and_then(|data| data.identities());
                    plot.identities = identities.unwrap_or((0.0, 1.0));
                    plot.plot = Some(result);
                }
            }
            Plotting::Close => self.leave_plot(),
            Plotting::Hovered(pointer, size) => self.plot_hovered(pointer, size),
            Plotting::Pressed => {
                if let Some(plot) = self.plot_mut() {
                    plot.pressed = plot
                        .pointer
                        .map(|(pointer, _)| (pointer.local.x, pointer.local.y));
                    plot.chosen = None;
                }
                self.hover = None;
            }
            Plotting::Released => return self.plot_released(),
            Plotting::Whole => self.zoom_plot(None),
            Plotting::Color(coloring) => {
                if let Some(plot) = self.plot_mut() {
                    plot.coloring = coloring;
                }
            }
            Plotting::Synteny(synteny) => {
                if let Some(plot) = self.plot_mut() {
                    plot.synteny = synteny;
                    plot.forget_pointer();
                }
                self.hover = None;
            }
        }
        Task::none()
    }

    fn plot_mut(&mut self) -> Option<&mut DotPlot> {
        self.session.as_mut()?.plot.as_deref_mut()
    }

    /// Shows a dot plot in place of the tracks, and keeps the view before it in the history when
    /// `record` is true. genie reads the alignments when it has not read them for the filter of
    /// the track.
    pub(super) fn open_plot(&mut self, shown: Shown, record: bool) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let paf = session
            .tracks
            .iter()
            .any(|track| track.id == shown.track && track.source.is_paf());
        if !paf {
            return Task::none();
        }
        let before = session.visit();
        match &mut session.plot {
            Some(plot) if plot.track == shown.track => {
                plot.shown = true;
                plot.window = shown.window;
            }
            _ => session.plot = Some(Box::new(DotPlot::new(shown))),
        }
        if record && session.visit() != before {
            session.history.record(before);
        }
        (self.hover, self.pinned) = (None, None);
        self.refresh_plot()
    }

    /// Reads the alignments of the dot plot that shows when genie has not read them for the
    /// rules of the filter of its track, and forgets the plot when its track is gone.
    pub(super) fn refresh_plot(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let Some(id) = session.plot.as_ref().map(|plot| plot.track) else {
            return Task::none();
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == id) else {
            session.plot = None;
            return Task::none();
        };
        let Some(plot) = session.plot.as_mut() else {
            return Task::none();
        };
        if !plot.shown || (plot.reading > 0 && plot.rules == track.filter.rules) {
            return Task::none();
        }
        let reading = READINGS.fetch_add(1, Ordering::Relaxed) + 1;
        plot.reading = reading;
        plot.rules.clone_from(&track.filter.rules);
        plot.plot = None;
        plot.forget_pointer();
        let source = Arc::clone(&track.source);
        let rules = plot.rules.clone();
        let reference = Arc::clone(&session.reference);
        Task::perform(
            async move { read(&source, &rules, &reference).map(Arc::new) },
            move |result| Message::Plot(Plotting::Read(reading, result)),
        )
    }

    /// Shows the tracks again, and keeps the plot in the history.
    fn leave_plot(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let before = session.visit();
        session.hide_plot();
        if session.visit() != before {
            session.history.record(before);
        }
        self.hover = None;
    }

    fn plot_hovered(&mut self, pointer: Option<Pointer>, size: (f32, f32)) {
        let Some(plot) = self
            .session
            .as_mut()
            .and_then(|session| session.plot.as_mut())
        else {
            return;
        };
        match pointer {
            Some(pointer) => plot.pointer = Some((pointer, size)),
            None if plot.pressed.is_some() => {}
            None => plot.pointer = None,
        }
        plot.chosen = None;
        self.hover = None;
        let (Some(pointer), None) = (pointer, plot.pressed) else {
            return;
        };
        let Some(data) = plot.data().cloned() else {
            return;
        };
        let place = (pointer.local.x, pointer.local.y);
        plot.chosen = plot.hit_at(&data, size, place);
        self.hover = plot
            .chosen
            .map(|index| card(plot.track, &data, index, pointer.at));
    }

    /// Ends a drag: a box or a range zooms in, and a click on an alignment shows it in the
    /// tracks.
    fn plot_released(&mut self) -> Task<Message> {
        let Some(plot) = self.plot_mut() else {
            return Task::none();
        };
        let (Some(from), Some((pointer, size))) = (plot.pressed.take(), plot.pointer) else {
            return Task::none();
        };
        let Some(data) = plot.data().cloned() else {
            return Task::none();
        };
        let to = (pointer.local.x, pointer.local.y);
        if (from.0 - to.0).abs().max((from.1 - to.1).abs()) >= SLOP {
            if let Some(zoomed) = plot.zoomed(&data, size, (from, to)) {
                self.zoom_plot(Some(zoomed));
            }
            return Task::none();
        }
        match plot.hit_at(&data, size, to) {
            Some(index) => self.go_to_alignment(&data, index),
            None => Task::none(),
        }
    }

    /// Shows a part of the plot, and keeps the part before it in the history.
    fn zoom_plot(&mut self, window: Option<Window>) {
        let Some(session) = &mut self.session else {
            return;
        };
        let before = session.visit();
        if let Some(plot) = &mut session.plot {
            plot.window = window;
            plot.chosen = None;
        }
        if session.visit() != before {
            session.history.record(before);
        }
        self.hover = None;
    }

    /// Shows the place of an alignment on its target in the tracks.
    fn go_to_alignment(&mut self, plot: &Plot, index: usize) -> Task<Message> {
        let hit = plot.hits[index];
        let name = &plot.targets[hit.target].name;
        let found = self
            .session
            .as_ref()
            .and_then(|session| session.reference.assembly.find(name));
        let Some(contig) = found else {
            return self.inform(format!("The reference has no sequence {name}."));
        };
        self.hover = None;
        self.go_to(Target {
            contig,
            span: Span::Region(hit.on_target),
        })
    }
}

/// The dot plot with its bar, in place of the tracks.
pub(super) fn plot_view<'a>(session: &'a Session, plot: &'a DotPlot) -> Element<'a, Message> {
    let name = session
        .tracks
        .iter()
        .find(|track| track.id == plot.track)
        .map_or("", |track| track.name.as_str());
    let body: Element<'a, Message> = match &plot.plot {
        None => hint(format!("Reading the alignments of {name}…")),
        Some(Err(error)) => hint(error.clone()),
        Some(Ok(data)) if data.hits.is_empty() && plot.rules.is_empty() => {
            hint(NO_ALIGNMENTS.to_owned())
        }
        Some(Ok(data)) if data.hits.is_empty() => hint(ALL_HIDDEN.to_owned()),
        Some(Ok(data)) => responsive(move |size| {
            let size = (size.width, size.height);
            scene::view(plot.scene(data, size), size.1)
                .on_hover(move |pointer| Message::Plot(Plotting::Hovered(pointer, size)))
                .on_press(|_| Message::Plot(Plotting::Pressed))
                .on_release(Message::Plot(Plotting::Released))
                .crosshair()
                .into()
        })
        .into(),
    };
    column![bar(plot, name), hairline(), body].into()
}

fn hint<'a>(words: String) -> Element<'a, Message> {
    container(text(words).size(11).style(style::muted))
        .padding([8, 12])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The bar over the plot: its name, its counts, the colors, the whole plot and close.
fn bar<'a>(plot: &DotPlot, name: &str) -> Element<'a, Message> {
    let counts = plot.data().map(|data| {
        let words = [
            counted(data.hits.len(), "alignment", "alignments"),
            counted(data.queries.len(), "query", "queries"),
            counted(data.targets.len(), "target", "targets"),
        ];
        text(words.join(" · ")).size(11).style(style::muted)
    });
    let (legend, switch, other) = match plot.coloring {
        Coloring::Strand => (
            "Blue forward · vermillion reverse".to_owned(),
            "Color by identity",
            Coloring::Identity,
        ),
        Coloring::Identity => {
            let (low, high) = plot.identities;
            let legend = format!(
                "Identity {:.1} % or less light · {:.1} % dark",
                low * 100.0,
                high * 100.0
            );
            (legend, "Color by strand", Coloring::Strand)
        }
    };
    let ghost = |words: &'static str, message: Option<Message>| {
        button(text(words).size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press_maybe(message)
    };
    let close = button(icon::view(Icon::Close, 12.0))
        .width(20)
        .height(20)
        .padding(0)
        .style(style::ghost)
        .on_press(Message::Plot(Plotting::Close));
    let whole = plot
        .window
        .is_some()
        .then_some(Message::Plot(Plotting::Whole));
    let (view, other_view) = if plot.synteny {
        ("Synteny", "Dot plot view")
    } else {
        ("Dot plot", "Synteny view")
    };
    let title = text(format!("{view} · {name}"))
        .font(font::UI_STRONG)
        .size(12);
    let views = Message::Plot(Plotting::Synteny(!plot.synteny));
    let row = row![title]
        .push(counts)
        .push(space::horizontal())
        .push(text(legend).size(11).style(style::muted))
        .push(ghost(switch, Some(Message::Plot(Plotting::Color(other)))))
        .push(ghost(other_view, Some(views)))
        .push(ghost("Whole plot", whole))
        .push(close)
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([4, 8]);
    container(row)
        .width(Length::Fill)
        .style(|theme| container::background(tokens(theme).bar))
        .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::Region;
    use genie_formats::fai::FaiRecord;
    use genie_formats::fields::{Rule, Test};
    use iced::Point;
    use iced_test::simulator;

    use super::{Plotting, read};
    use crate::app::{Change, Filtering, Genie, Message};
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::scratch::Scratch;
    use crate::shortcuts::Command;
    use genie_render::dotplot::Coloring;

    /// Two alignments of ctgA with chr1, one on each strand, and one of ctgB with chr2.
    const PAF: &str = "ctgA\t2000\t0\t1000\t+\tchr1\t3000\t0\t1000\t950\t1000\t60\ttp:A:P\n\
                       ctgA\t2000\t1000\t2000\t-\tchr1\t3000\t1500\t2500\t800\t1000\t60\ttp:A:I\n\
                       ctgB\t1000\t0\t1000\t+\tchr2\t1000\t0\t1000\t1000\t1000\t60\ttp:A:P\n";
    /// The size of the plot in the tests.
    const SIZE: (f32, f32) = (1_000.0, 600.0);

    fn genie(scratch: &Scratch) -> Genie {
        let records = [("chr1", 3_000), ("chr2", 1_000)]
            .iter()
            .scan(0, |offset, (name, length)| {
                let record = FaiRecord {
                    name: (*name).to_owned(),
                    length: *length,
                    offset: *offset,
                    line_bases: 60,
                    line_width: 61,
                };
                *offset += length + length / 60 + 8;
                Some(record)
            })
            .collect();
        let reference = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap());
        let mut genie = Genie::default();
        genie.settings.motion = false;
        let _ = genie.update(Message::Opened(Ok(reference)));
        let _ = genie.update(Message::Resized(1_000.0));
        let source = crate::track::open(scratch.file("asm.paf", PAF)).unwrap();
        let _ = genie.update(Message::TrackOpened(Ok(Arc::new(source))));
        genie
    }

    /// Reads the alignments of the plot at once, as the task of the plot would.
    fn read_now(genie: &mut Genie) {
        let session = genie.session.as_ref().unwrap();
        let plot = session.plot.as_ref().unwrap();
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == plot.track)
            .unwrap();
        let result = read(&track.source, &plot.rules, &session.reference).map(Arc::new);
        let reading = plot.reading;
        let _ = genie.update(Message::Plot(Plotting::Read(reading, result)));
    }

    fn opened(scratch: &Scratch) -> Genie {
        let mut genie = genie(scratch);
        let _ = genie.update(Message::Plot(Plotting::Open(0)));
        read_now(&mut genie);
        genie
    }

    fn shows(genie: &Genie, words: &str) -> bool {
        simulator(genie.view()).find(words).is_ok()
    }

    /// Moves the pointer to a place of the plot.
    fn point(genie: &mut Genie, x: f32, y: f32) {
        let pointer = Pointer {
            local: Point::new(x, y),
            at: Point::new(x + 10.0, y + 80.0),
        };
        let _ = genie.update(Message::Plot(Plotting::Hovered(Some(pointer), SIZE)));
    }

    /// The place in the plot of a place in bases, for the plot of [`PAF`]: 4,000 bases across
    /// and 3,000 bases down in an area of 872 by 574 pixels.
    fn place(across: f32, down: f32) -> (f32, f32) {
        (
            120.0 + across / 4_000.0 * 872.0,
            18.0 + down / 3_000.0 * 574.0,
        )
    }

    fn window(genie: &Genie) -> Option<genie_core::dotplot::Window> {
        genie.session.as_ref()?.plot.as_ref()?.window
    }

    #[test]
    fn the_menu_of_a_paf_track_opens_its_dot_plot_in_place_of_the_tracks() {
        let scratch = Scratch::new("dotplot-open");
        let mut genie = genie(&scratch);
        assert!(shows(&genie, "asm.paf"));
        let _ = genie.update(Message::TrackMenu(0, Point::new(100.0, 40.0)));
        let mut ui = simulator(genie.view());
        ui.click("Dot plot").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Plot(Plotting::Open(0))));
        let _ = genie.update(Message::Plot(Plotting::Open(0)));
        assert!(shows(&genie, "Reading the alignments of asm.paf…"));
        read_now(&mut genie);
        assert!(shows(&genie, "Dot plot · asm.paf"));
        assert!(shows(&genie, "3 alignments · 2 queries · 2 targets"));
        assert!(!shows(&genie, "asm.paf"), "the tracks do not show");
        let _ = genie.update(Message::Plot(Plotting::Close));
        assert!(shows(&genie, "asm.paf"));
        let _ = genie.update(Message::Command(Command::Back));
        assert!(
            shows(&genie, "Dot plot · asm.paf"),
            "back shows the plot again"
        );
    }

    #[test]
    fn a_click_on_an_alignment_shows_it_in_the_tracks_and_back_returns() {
        let scratch = Scratch::new("dotplot-click");
        let mut genie = opened(&scratch);
        let (x, y) = place(500.0, 500.0);
        point(&mut genie, x, y);
        assert!(shows(&genie, "ctgA"), "the card of the alignment");
        assert!(shows(&genie, "chr1:1-1,000"));
        let _ = genie.update(Message::Plot(Plotting::Pressed));
        let _ = genie.update(Message::Plot(Plotting::Released));
        assert!(!shows(&genie, "Dot plot · asm.paf"));
        assert!(shows(&genie, "chr1:1-1,000"), "the locus of the alignment");
        let _ = genie.update(Message::Command(Command::Back));
        assert!(shows(&genie, "Dot plot · asm.paf"));
        let (x, y) = place(2_000.0, 1_000.0);
        point(&mut genie, x, y + 40.0);
        assert!(!shows(&genie, "ctgA"), "no card away from the lines");
    }

    #[test]
    fn a_dragged_box_zooms_in_and_back_returns_to_the_whole_plot() {
        let scratch = Scratch::new("dotplot-zoom");
        let mut genie = opened(&scratch);
        assert_eq!(window(&genie), None);
        let (x, y) = place(0.0, 0.0);
        point(&mut genie, x, y);
        let _ = genie.update(Message::Plot(Plotting::Pressed));
        let (x, y) = place(1_000.0, 1_000.0);
        point(&mut genie, x, y);
        let _ = genie.update(Message::Plot(Plotting::Hovered(None, SIZE)));
        let _ = genie.update(Message::Plot(Plotting::Released));
        let zoomed = window(&genie).unwrap();
        assert!(zoomed.across.start() < 5 && zoomed.across.end().abs_diff(1_000) < 5);
        assert!(zoomed.down.start() < 5 && zoomed.down.end().abs_diff(1_000) < 5);
        assert!(
            shows(&genie, "chr1:1-3,000"),
            "the tracks stay where they were"
        );
        let _ = genie.update(Message::Plot(Plotting::Whole));
        assert_eq!(window(&genie), None);
        let _ = genie.update(Message::Command(Command::Back));
        assert_eq!(window(&genie), Some(zoomed));
        let _ = genie.update(Message::Command(Command::Back));
        assert_eq!(window(&genie), None);
    }

    #[test]
    fn the_plot_follows_the_filter_and_the_colors_of_the_menu() {
        let scratch = Scratch::new("dotplot-filter");
        let mut genie = opened(&scratch);
        let rule = |bound| Rule {
            label: "Identity (%)".to_owned(),
            test: Test::AtLeast(bound),
        };
        let _ = genie.update(Message::Filter(0, Filtering::Add(rule(90.0))));
        assert!(shows(&genie, "Reading the alignments of asm.paf…"));
        read_now(&mut genie);
        assert!(shows(&genie, "2 alignments · 2 queries · 2 targets"));
        let _ = genie.update(Message::Filter(0, Filtering::Add(rule(101.0))));
        read_now(&mut genie);
        assert!(shows(&genie, super::ALL_HIDDEN));
        assert!(shows(&genie, "Color by identity"));
        let _ = genie.update(Message::Filter(0, Filtering::Remove(1)));
        read_now(&mut genie);
        let _ = genie.update(Message::Plot(Plotting::Color(Coloring::Identity)));
        assert!(shows(&genie, "Color by strand"));
        assert!(shows(
            &genie,
            "Identity 95.0 % or less light · 100.0 % dark"
        ));
    }

    #[test]
    fn the_synteny_view_shows_the_alignments_as_bands_between_two_rows() {
        let scratch = Scratch::new("dotplot-synteny");
        let mut genie = opened(&scratch);
        let _ = genie.update(Message::Plot(Plotting::Synteny(true)));
        assert!(shows(&genie, "Synteny · asm.paf"));
        assert!(shows(&genie, "Dot plot view"));
        // The rows are 984 pixels wide from 8, with 4,000 bases of targets at the top and 3,000
        // bases of queries at the bottom, so the band of ctgA on chr1:1-1,000 goes from 8-254
        // at the top to 8-336 at the bottom.
        point(&mut genie, 100.0, 300.0);
        assert!(shows(&genie, "ctgA"), "the card of the band");
        assert!(shows(&genie, "chr1:1-1,000"));
        point(&mut genie, 8.0, 10.0);
        let _ = genie.update(Message::Plot(Plotting::Pressed));
        point(&mut genie, 254.0, 10.0);
        let _ = genie.update(Message::Plot(Plotting::Released));
        let zoomed = genie_core::dotplot::Window {
            across: Region::between(0, 1_000),
            down: Region::between(0, 1_020),
        };
        assert_eq!(
            window(&genie),
            Some(zoomed),
            "the queries follow the targets"
        );
        let _ = genie.update(Message::Plot(Plotting::Synteny(false)));
        assert!(shows(&genie, "Dot plot · asm.paf"));
        assert_eq!(window(&genie), Some(zoomed), "the dot plot keeps the zoom");
    }

    #[test]
    fn removing_the_track_forgets_its_plot() {
        let scratch = Scratch::new("dotplot-remove");
        let mut genie = opened(&scratch);
        let _ = genie.update(Message::TrackChanged(0, Change::Remove));
        assert!(genie.session.as_ref().unwrap().plot.is_none());
        assert!(!shows(&genie, "Dot plot · asm.paf"));
        let region = genie.session.as_ref().unwrap().viewport.visible();
        assert_eq!(region, Region::between(0, 3_000));
    }
}
