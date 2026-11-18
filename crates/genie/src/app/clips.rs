//! Records the clips of the tutorial on the website: genie runs without a window, a script moves
//! the pointer and clicks, and each frame goes to a PNG file. The tests are ignored, because they
//! fetch real data from the web. `site/clips/record.ps1` runs them and makes the MP4 files.
//!
//! `GENIE_CLIP_FRAMES` is the folder for the frames: each clip gets a folder of its own, such as
//! `cards-dark`. `GENIE_CLIP_THEME` set to `light` or `dark` records only that look.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use iced::futures::StreamExt;
use iced::futures::executor::block_on;
use iced::widget::{canvas, container, stack};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme, mouse};
use iced_test::runtime::Action;
use iced_test::runtime::task::into_stream;
use iced_test::{Simulator, selector};

use crate::app::{Genie, Message};
use crate::theme::Appearance;

const FPS: f32 = 30.0;
const SIZE: Size = Size::new(1024.0, 576.0);
/// The exome reads of NA12878 on hg38, from the public data of `JBrowse`.
const CRAM: &str = "https://s3.amazonaws.com/jbrowse.org/genomes/GRCh38/alignments/NA12878/NA12878.alt_bwamem_GRCh38DH.20150826.CEU.exome.cram";
/// The exon of BRCA1 where the clips show reads.
const EXON: &str = "chr17:43,091,601-43,093,600";

/// The mouse pointer of a clip: an arrow, with a ring that grows for a click.
struct Pointer {
    at: Point,
    ring: Option<f32>,
    dark: bool,
}

impl<M> canvas::Program<M> for Pointer {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some(progress) = self.ring {
            let shade = if self.dark { 0.85 } else { 0.25 };
            let ring = canvas::Path::circle(self.at, 5.0 + 15.0 * progress);
            let color = Color::from_rgba(shade, shade, shade, 0.7 * (1.0 - progress));
            frame.stroke(
                &ring,
                canvas::Stroke::default().with_width(2.0).with_color(color),
            );
        }
        let corners = [
            (0.0, 0.0),
            (0.0, 16.0),
            (3.9, 12.4),
            (6.6, 18.4),
            (9.0, 17.3),
            (6.4, 11.5),
            (11.4, 11.5),
        ];
        let arrow = canvas::Path::new(|builder| {
            builder.move_to(Point::new(self.at.x, self.at.y));
            for (x, y) in &corners[1..] {
                builder.line_to(Point::new(self.at.x + x, self.at.y + y));
            }
            builder.close();
        });
        frame.fill(&arrow, Color::WHITE);
        frame.stroke(
            &arrow,
            canvas::Stroke::default()
                .with_width(1.1)
                .with_color(Color::from_rgb(0.08, 0.08, 0.08)),
        );
        vec![frame.into_geometry()]
    }
}

fn settings() -> iced::Settings {
    iced::Settings {
        default_font: crate::font::UI,
        ..iced::Settings::default()
    }
}

/// True when the work of a message may run: it loads data or moves the view, and it opens no
/// dialog of the system.
fn runs(message: &Message) -> bool {
    use crate::shortcuts::Command;
    match message {
        Message::Command(command) => !matches!(
            command,
            Command::Open | Command::SaveSession | Command::Copy
        ),
        Message::Genome(_)
        | Message::Opened(_)
        | Message::TrackOpened(_)
        | Message::TrackLoaded(..)
        | Message::TileLoaded(..)
        | Message::NamesLoaded(..)
        | Message::Catalog(_)
        | Message::CatalogOpened(..)
        | Message::CatalogMenu
        | Message::GenomeMenu
        | Message::Frame(_)
        | Message::LocusEdited(_)
        | Message::LocusSubmitted
        | Message::SuggestionChosen(_)
        | Message::Looked(..)
        | Message::Zoom(..)
        | Message::Pan(..)
        | Message::Ruler(..)
        | Message::Overview(..)
        | Message::Resized(_)
        | Message::WindowResized(_)
        | Message::TrackHovered(..)
        | Message::TrackClicked(..)
        | Message::Effects(..)
        | Message::Jump(..)
        | Message::Jumped(..)
        | Message::Find(_)
        | Message::Address(_)
        | Message::TrackMenu(..)
        | Message::TrackChanged(..)
        | Message::MenuDismissed
        | Message::CardClosed
        | Message::NoticeDismissed
        | Message::Panel(..)
        | Message::Filter(..)
        | Message::Field(_)
        | Message::Prepare(_)
        | Message::Preparing(..)
        | Message::SessionPicked(_)
        | Message::ExportPicked(_)
        | Message::Exported(..) => true,
        _ => false,
    }
}

fn ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The folder for the frames of all clips.
fn frames() -> PathBuf {
    PathBuf::from(std::env::var("GENIE_CLIP_FRAMES").expect("GENIE_CLIP_FRAMES names a folder"))
}

/// A genie without a window, its work on threads, and frames on disk.
struct Recorder {
    genie: Genie,
    pointer: Point,
    ring: Option<f32>,
    clock: Instant,
    folder: PathBuf,
    count: usize,
    sender: mpsc::Sender<Message>,
    receiver: mpsc::Receiver<Message>,
    running: Arc<AtomicUsize>,
}

impl Recorder {
    fn new(appearance: Appearance) -> Self {
        crate::font::load();
        let mut genie = Genie::default();
        genie.settings.motion = true;
        genie.settings.tips = false;
        genie.settings.appearance = appearance;
        genie.window = SIZE;
        let (sender, receiver) = mpsc::channel();
        let mut recorder = Self {
            genie,
            pointer: Point::new(SIZE.width * 0.62, SIZE.height * 0.58),
            ring: None,
            clock: Instant::now(),
            folder: PathBuf::new(),
            count: 0,
            sender,
            receiver,
            running: Arc::new(AtomicUsize::new(0)),
        };
        recorder.apply(Message::WindowResized(SIZE));
        recorder.measure();
        recorder
    }

    /// The width of the browser area, as the app measures it on a cursor move.
    fn measure(&mut self) {
        let messages: Vec<Message> = {
            let mut ui = Simulator::with_size(settings(), SIZE, self.genie.view());
            let _ = ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(2.0, 2.0),
            })]);
            ui.into_messages().collect()
        };
        for message in messages {
            if matches!(message, Message::Resized(_)) {
                self.apply(message);
            }
        }
    }

    /// Starts a new clip: its frames go to a folder of their own.
    fn clip(&mut self, name: &str) {
        self.folder = frames().join(name);
        let _ = std::fs::remove_dir_all(&self.folder);
        std::fs::create_dir_all(&self.folder).unwrap();
        self.count = 0;
    }

    /// Applies a message of the script. Only the work of messages that load data or move the
    /// view runs: the work of others could open a file dialog of the system, so it is dropped.
    fn apply(&mut self, message: Message) {
        // A glide starts at the time of the system, so the clock of the frames catches up with it
        // before an action of the script, which can start one.
        self.clock = self.clock.max(Instant::now());
        self.deliver(message);
    }

    /// Applies a message without a change of the clock: a frame, or the result of work.
    fn deliver(&mut self, message: Message) {
        if matches!(message, Message::NoticeExpired(_)) {
            return;
        }
        let runs = runs(&message);
        let label: String = format!("{message:?}").chars().take(70).collect();
        let task = self.genie.update(message);
        if runs {
            self.spawn(task);
        } else if into_stream(task).is_some() {
            println!("[dropped] the work of {label}");
        }
    }

    fn spawn(&self, task: Task<Message>) {
        let Some(stream) = into_stream(task) else {
            return;
        };
        let sender = self.sender.clone();
        let running = Arc::clone(&self.running);
        running.fetch_add(1, Ordering::SeqCst);
        thread::spawn(move || {
            block_on(stream.for_each(|action| {
                if let Action::Output(message) = action {
                    let _ = sender.send(message);
                }
                iced::futures::future::ready(())
            }));
            running.fetch_sub(1, Ordering::SeqCst);
        });
    }

    /// Applies the messages that the work on threads sent so far.
    fn pump(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            self.deliver(message);
        }
    }

    fn settled(&self) -> bool {
        let genie = &self.genie;
        genie.busy == 0
            && genie.looking.is_none()
            && genie.tiles.waiting().is_empty()
            && genie.session.as_ref().is_none_or(|session| {
                !session.is_moving()
                    && session.preparing.is_empty()
                    && session.tracks.iter().all(|track| {
                        track.slot.wanted.is_none()
                            && track.extra.iter().all(|slot| slot.wanted.is_none())
                            && (track.names.is_some() || track.source.has_reads())
                    })
            })
    }

    /// Waits, without frames, until the work for the view is done. Motion goes on with the clock,
    /// so that a glide ends.
    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            self.pump();
            if self.settled() {
                break;
            }
            if Instant::now() > deadline {
                println!("[wait] the view did not settle in 90 s");
                break;
            }
            if self
                .genie
                .session
                .as_ref()
                .is_some_and(crate::app::Session::is_moving)
            {
                self.skip(1.0 / FPS);
            } else {
                thread::sleep(Duration::from_millis(15));
            }
        }
        self.pump();
    }

    /// Moves the clock on without frames.
    fn skip(&mut self, seconds: f32) {
        for _ in 0..(seconds * FPS).round().max(1.0) as usize {
            self.clock += Duration::from_secs_f32(1.0 / FPS);
            self.deliver(Message::Frame(self.clock));
            self.pump();
        }
    }

    /// Goes to a locus at once, as a setup step without frames.
    fn go(&mut self, locus: &str) {
        self.apply(Message::LocusEdited(locus.to_owned()));
        self.apply(Message::LocusSubmitted);
        self.wait();
    }

    /// Adds a track of the UCSC list of the genome, as a setup step without frames.
    fn add(&mut self, name: &str) {
        let index = crate::catalog::entries("hg38")
            .iter()
            .position(|entry| entry.name == name)
            .unwrap();
        self.apply(Message::Catalog(index));
        self.wait();
    }

    /// Opens hg38, as a setup step without frames.
    fn open_hg38(&mut self) {
        self.apply(Message::Genome("hg38".to_owned()));
        self.wait();
    }

    fn view(&self) -> Element<'_, Message> {
        let pointer = canvas(Pointer {
            at: self.pointer,
            ring: self.ring,
            dark: self.genie.settings.appearance == Appearance::Dark,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let browser = container(self.genie.view())
            .width(SIZE.width)
            .height(SIZE.height);
        stack![browser, pointer].into()
    }

    /// Draws one frame, a 30th of a second after the last one.
    fn frame(&mut self) {
        self.clock += Duration::from_secs_f32(1.0 / FPS);
        self.deliver(Message::Frame(self.clock));
        self.pump();
        let snapshot = {
            let theme = self.genie.theme();
            let mut ui = Simulator::with_size(settings(), SIZE, self.view());
            ui.point_at(self.pointer);
            ui.snapshot(&theme).unwrap()
        };
        self.count += 1;
        let path = self.folder.join(format!("f{:05}", self.count));
        snapshot.matches_image(path).unwrap();
    }

    fn hold(&mut self, seconds: f32) {
        for _ in 0..(seconds * FPS).round() as usize {
            self.frame();
        }
    }

    fn glide(&mut self, to: Point, seconds: f32) {
        let from = self.pointer;
        let steps = (seconds * FPS).round().max(1.0) as usize;
        for step in 1..=steps {
            let t = ease(step as f32 / steps as f32);
            self.pointer = Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            self.frame();
        }
    }

    /// The bounds of the widget with an id, or of a text.
    fn bounds_of(&self, id: &str) -> Rectangle {
        let mut ui = Simulator::with_size(settings(), SIZE, self.genie.view());
        ui.find(selector::id(id.to_owned()))
            .map(|target| target.bounds())
            .unwrap_or_else(|_| ui.find(id).unwrap().bounds())
    }

    /// Shows the ring of a click.
    fn ring(&mut self) {
        for step in 0..9 {
            self.ring = Some(step as f32 / 9.0);
            self.frame();
        }
        self.ring = None;
    }

    /// Moves the pointer to a point with the simulator, then sends mouse events there, and
    /// applies what they do.
    fn mouse_at(&mut self, point: Point, events: &[mouse::Event]) {
        let messages: Vec<Message> = {
            let mut ui = Simulator::with_size(settings(), SIZE, self.genie.view());
            ui.point_at(point);
            let _ = ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved {
                position: point,
            })]);
            if !events.is_empty() {
                let _ = ui.simulate(events.iter().copied().map(iced::Event::Mouse));
            }
            ui.into_messages().collect()
        };
        for message in messages {
            self.apply(message);
        }
    }

    /// Clicks at a point, and shows a ring.
    fn click_at(&mut self, point: Point) {
        let button = mouse::Button::Left;
        let events = [
            mouse::Event::ButtonPressed(button),
            mouse::Event::ButtonReleased(button),
        ];
        self.mouse_at(point, &events);
        self.ring();
    }

    /// Moves the pointer to a widget (an id or a text) and clicks it.
    fn click(&mut self, target: &str, seconds: f32) {
        let center = self.bounds_of(target).center();
        self.glide(center, seconds);
        self.click_at(center);
    }

    /// Right-clicks at a point, for the menu of a track.
    fn right_click_at(&mut self, point: Point) {
        let button = mouse::Button::Right;
        let events = [
            mouse::Event::ButtonPressed(button),
            mouse::Event::ButtonReleased(button),
        ];
        self.mouse_at(point, &events);
        self.ring();
    }

    /// Moves the pointer to a point, so that cards and hovers show.
    fn hover(&mut self, point: Point) {
        self.mouse_at(point, &[]);
    }

    /// Types text, one letter every few frames, as the messages of each prefix.
    fn type_text(&mut self, text: &str, message: impl Fn(String) -> Message) {
        let mut typed = String::new();
        for letter in text.chars() {
            typed.push(letter);
            self.apply(message(typed.clone()));
            self.hold(0.12);
        }
    }

    /// Drags the view sideways: the pointer moves and the view pans with it, frame by frame.
    fn drag(&mut self, dx: f32, seconds: f32) {
        let from = self.pointer;
        let steps = (seconds * FPS).round().max(1.0) as usize;
        let mut done = 0.0;
        for step in 1..=steps {
            let t = ease(step as f32 / steps as f32);
            let now = dx * t;
            self.apply(Message::Pan(0, now - done));
            done = now;
            self.pointer = Point::new(from.x + now, from.y);
            self.frame();
        }
    }

    /// Zooms at the pointer as a mouse wheel does: a step every few frames.
    fn wheel(&mut self, factor: f32, steps: usize) {
        for _ in 0..steps {
            self.apply(Message::Zoom(0, factor, self.pointer.x));
            self.hold(0.1);
        }
    }

    /// Drags on the ruler from the pointer to x, then lets go: genie zooms to that part. The
    /// ruler takes places as parts of the width of the panel.
    fn sweep(&mut self, to: f32, seconds: f32) {
        use crate::app::Sweep;
        let width = self.genie.width.max(1.0);
        let from = self.pointer.x;
        self.apply(Message::Ruler(0, Sweep::Pressed(from / width)));
        let steps = (seconds * FPS).round().max(1.0) as usize;
        for step in 1..=steps {
            let x = from + (to - from) * ease(step as f32 / steps as f32);
            self.pointer = Point::new(x, self.pointer.y);
            self.apply(Message::Ruler(0, Sweep::Dragged(x / width)));
            self.frame();
        }
        self.apply(Message::Ruler(0, Sweep::Released));
    }

    fn note(&self, label: &str) {
        let budget = genie_formats::budget::budget();
        println!(
            "[clip] {label}: {} frames, {:.3} MB fetched in all runs",
            self.count,
            budget.total() as f64 / 1e6
        );
    }
}

/// Records a clip in the light and the dark look: the setup runs without frames, then the
/// script runs with frames.
fn record(name: &str, setup: impl Fn(&mut Recorder), script: impl Fn(&mut Recorder)) {
    let only = std::env::var("GENIE_CLIP_THEME").ok();
    for (theme, appearance) in [("light", Appearance::Light), ("dark", Appearance::Dark)] {
        if only.as_deref().is_some_and(|only| only != theme) {
            continue;
        }
        let mut recorder = Recorder::new(appearance);
        setup(&mut recorder);
        recorder.clip(&format!("{name}-{theme}"));
        script(&mut recorder);
        recorder.note(&format!("{name}-{theme}"));
    }
}

fn at_brca1(recorder: &mut Recorder) {
    recorder.open_hg38();
    recorder.go("BRCA1");
}

fn with_tracks(recorder: &mut Recorder) {
    at_brca1(recorder);
    recorder.add("ClinVar");
    recorder.add("phyloP 100-way");
}

/// BRCA1 with `ClinVar` and `phyloP` at the exon, and the exome reads.
fn with_reads(recorder: &mut Recorder) {
    with_tracks(recorder);
    recorder.go(EXON);
    let source = crate::track::open(crate::location::Location::parse(CRAM).unwrap());
    recorder.apply(Message::TrackOpened(source.map(Arc::new)));
    recorder.wait();
}

#[test]
#[ignore = "fetches real data from the web"]
fn open_genome() {
    record(
        "open-genome",
        |_| {},
        |recorder| {
            recorder.hold(0.8);
            recorder.click("hg38", 0.8);
            recorder.hold(0.4);
            recorder.wait();
            recorder.hold(1.8);
        },
    );
}

#[test]
#[ignore = "fetches real data from the web"]
fn go_to_gene() {
    record("go-to-gene", Recorder::open_hg38, |recorder| {
        recorder.hold(0.4);
        recorder.click(crate::chrome::LOCUS, 0.6);
        recorder.type_text("BRCA1", Message::LocusEdited);
        recorder.hold(0.5);
        recorder.apply(Message::LocusSubmitted);
        recorder.hold(1.0);
        recorder.wait();
        recorder.hold(1.8);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn several_loci() {
    record("several-loci", at_brca1, |recorder| {
        recorder.hold(0.4);
        recorder.click(crate::chrome::LOCUS, 0.6);
        recorder.type_text("BRCA1 BRCA2", Message::LocusEdited);
        recorder.hold(0.5);
        recorder.apply(Message::LocusSubmitted);
        recorder.hold(1.0);
        recorder.wait();
        recorder.hold(1.8);
        recorder.click("chr13:32,315,508-32,400,268", 0.8);
        recorder.hold(0.6);
        recorder.wait();
        recorder.hold(1.6);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn add_tracks() {
    record("add-tracks", at_brca1, |recorder| {
        recorder.hold(0.4);
        recorder.click("catalog", 0.7);
        recorder.hold(0.7);
        recorder.click("ClinVar", 0.6);
        recorder.hold(0.3);
        recorder.wait();
        recorder.hold(0.7);
        recorder.click("catalog", 0.6);
        recorder.hold(0.6);
        recorder.click("phyloP 100-way", 0.6);
        recorder.hold(0.3);
        recorder.wait();
        recorder.hold(1.8);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn move_zoom() {
    record("move-zoom", with_tracks, |recorder| {
        recorder.hold(0.3);
        recorder.glide(Point::new(640.0, 330.0), 0.6);
        recorder.drag(-220.0, 0.9);
        recorder.wait();
        recorder.hold(0.3);
        recorder.drag(260.0, 0.9);
        recorder.wait();
        recorder.hold(0.4);
        recorder.glide(Point::new(505.0, 330.0), 0.5);
        recorder.wheel(1.35, 6);
        recorder.wait();
        recorder.hold(0.6);
        let ruler = recorder.bounds_of("ruler");
        recorder.glide(Point::new(300.0, ruler.center_y()), 0.6);
        recorder.sweep(700.0, 0.8);
        recorder.hold(0.8);
        recorder.wait();
        recorder.hold(0.6);
        recorder.click("zoom-out", 0.7);
        recorder.hold(0.6);
        recorder.wait();
        recorder.click("zoom-out", 0.3);
        recorder.hold(0.6);
        recorder.wait();
        recorder.hold(1.2);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn open_url() {
    use crate::app::Address;
    let setup = |recorder: &mut Recorder| {
        with_tracks(recorder);
        recorder.go(EXON);
    };
    record("open-url", setup, |recorder| {
        recorder.hold(0.4);
        recorder.click("open-url", 0.7);
        recorder.hold(0.6);
        recorder.apply(Message::Address(Address::Edit(CRAM.to_owned())));
        recorder.hold(1.2);
        recorder.apply(Message::Address(Address::Submit));
        recorder.hold(0.3);
        recorder.wait();
        recorder.hold(2.2);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn cards() {
    record("cards", with_reads, |recorder| {
        let (variant, read) = (Point::new(420.0, 117.0), Point::new(560.0, 330.0));
        recorder.hold(0.4);
        recorder.glide(variant, 0.8);
        recorder.hover(variant);
        recorder.hold(2.0);
        recorder.glide(read, 0.8);
        recorder.hover(read);
        recorder.hold(1.2);
        recorder.click_at(read);
        recorder.hold(1.6);
        recorder.apply(Message::CardClosed);
        recorder.hold(0.6);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn track_menu() {
    record("track-menu", with_reads, |recorder| {
        let reads = Point::new(560.0, 330.0);
        recorder.hold(0.4);
        recorder.glide(reads, 0.8);
        recorder.right_click_at(reads);
        recorder.hold(0.8);
        recorder.click("Squished", 0.6);
        recorder.hold(0.4);
        recorder.wait();
        recorder.hold(1.0);
        recorder.glide(reads, 0.6);
        recorder.right_click_at(reads);
        recorder.hold(0.8);
        recorder.click("Group by strand", 0.6);
        recorder.hold(0.4);
        recorder.wait();
        recorder.hold(0.6);
        let away = Point::new(860.0, 50.0);
        recorder.glide(away, 0.6);
        recorder.hover(away);
        recorder.hold(1.6);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn find_sequence() {
    use crate::app::find::Finding;
    use crate::shortcuts::Command;
    record("find-sequence", with_reads, |recorder| {
        recorder.hold(0.5);
        recorder.apply(Message::Command(Command::Find));
        recorder.hold(0.5);
        recorder.type_text("GAAA", |text| Message::Find(Finding::Edit(text)));
        recorder.hold(0.3);
        recorder.wait();
        recorder.hold(1.2);
        for _ in 0..2 {
            recorder.apply(Message::Find(Finding::Submit));
            recorder.hold(0.9);
            recorder.wait();
            recorder.hold(0.6);
        }
        recorder.hold(0.8);
    });
}

#[test]
#[ignore = "fetches real data from the web"]
fn save_export() {
    record("save-export", with_reads, |recorder| {
        let folder = frames().join("saved");
        std::fs::create_dir_all(&folder).unwrap();
        recorder.hold(0.5);
        recorder.click("save", 0.8);
        recorder.hold(0.6);
        recorder.apply(Message::SessionPicked(Some(folder.join("brca1.genie"))));
        recorder.wait();
        recorder.hold(1.6);
        recorder.click("export", 0.7);
        recorder.hold(0.5);
        recorder.type_text("BRCA1", |text| {
            Message::Figure(crate::app::Figuring::Title(text))
        });
        recorder.hold(0.5);
        recorder.click("figure-export", 0.6);
        recorder.hold(0.4);
        recorder.apply(Message::ExportPicked(Some(folder.join("brca1.png"))));
        recorder.wait();
        recorder.hold(2.0);
    });
}

/// The picture at the top of the home page: BRCA1 with `ClinVar` and `phyloP`, without the
/// pointer.
#[test]
#[ignore = "fetches real data from the web"]
fn hero() {
    record("hero", with_tracks, |recorder| {
        recorder.pointer = Point::new(-60.0, -60.0);
        recorder.hold(0.1);
    });
}
