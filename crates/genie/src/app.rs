use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::{Locus, Span, Viewport};
use genie_render::{overview, ruler, sequence};
use iced::theme::Mode;
use iced::widget::{button, center, column, container, operation, space, stack, text};
use iced::{Alignment, Border, Element, Length, Subscription, Task, Theme, keyboard, system};

use crate::chrome::{self, Controls, LocusField};
use crate::icon::Icon;
use crate::navigator::Navigator;
use crate::reference::{self, Reference};
use crate::shortcuts::{self, Command};
use crate::theme::{self, Appearance, tokens};
use crate::tiles::{Key, TILING, Tiles};
use crate::{font, paths, scene, style};

const INITIAL_WIDTH: f32 = 1_000.0;
const OVERVIEW_HEIGHT: f32 = 12.0;
const RULER_HEIGHT: f32 = 20.0;
const BASE_HEIGHT: f32 = 16.0;
const TRACK_PADDING: f32 = 2.0;
const LABEL_SIZE: f32 = 10.0;

#[derive(Debug)]
pub struct Genie {
    appearance: Appearance,
    system: Mode,
    status: Status,
    session: Option<Session>,
    width: f32,
    tiles: Tiles,
    generation: u64,
    draft: Option<String>,
    locus_error: Option<String>,
}

impl Default for Genie {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            system: Mode::default(),
            status: Status::default(),
            session: None,
            width: INITIAL_WIDTH,
            tiles: Tiles::default(),
            generation: 0,
            draft: None,
            locus_error: None,
        }
    }
}

#[derive(Debug, Default)]
enum Status {
    #[default]
    Idle,
    Opening(String),
    Failed(String),
}

#[derive(Debug)]
struct Session {
    reference: Arc<Reference>,
    contig: usize,
    viewport: Viewport,
}

impl Session {
    fn new(reference: Arc<Reference>, width: f32) -> Self {
        let length = reference.assembly.contigs()[0].length;
        Self {
            reference,
            contig: 0,
            viewport: Viewport::new(length, f64::from(width)),
        }
    }

    fn locus(&self) -> String {
        Locus {
            name: self.reference.assembly.contigs()[self.contig].name.clone(),
            span: Span::Region(self.viewport.visible()),
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    CycleAppearance,
    SystemModeChanged(Mode),
    Window(chrome::Action),
    Open,
    Picked(Option<PathBuf>),
    Opened(Result<Arc<Reference>, String>),
    Pan(f32),
    Zoom(f32, f32),
    Resized(f32),
    TileLoaded(u64, Key, Result<Arc<[u8]>, String>),
    LocusEdited(String),
    LocusSubmitted,
    Command(Command),
}

impl Genie {
    fn boot() -> (Self, Task<Message>) {
        (
            Self::default(),
            system::theme().map(Message::SystemModeChanged),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CycleAppearance => self.appearance = self.appearance.next(),
            Message::SystemModeChanged(mode) => self.system = mode,
            Message::Window(action) => return chrome::perform(action),
            Message::Open => return Task::perform(pick(), Message::Picked),
            Message::Picked(Some(path)) => {
                self.status = Status::Opening(file_name(&path));
                return Task::perform(
                    async move { reference::open(path, paths::cache_dir().as_deref()).map(Arc::new) },
                    Message::Opened,
                );
            }
            Message::Picked(None) => {}
            Message::Opened(Ok(reference)) => {
                self.status = Status::Idle;
                self.generation += 1;
                self.tiles.clear();
                self.session = Some(Session::new(reference, self.width));
                return self.fetch();
            }
            Message::Opened(Err(error)) => self.status = Status::Failed(error),
            Message::Pan(dx) => {
                self.drop_draft();
                return self.navigate(|viewport| viewport.pan(f64::from(dx)));
            }
            Message::Zoom(factor, anchor) => {
                self.drop_draft();
                return self
                    .navigate(|viewport| viewport.zoom(f64::from(factor), f64::from(anchor)));
            }
            Message::Resized(width) => {
                self.width = width;
                return self.navigate(|viewport| viewport.resize(f64::from(width)));
            }
            Message::TileLoaded(generation, key, result) => {
                if generation == self.generation {
                    match result {
                        Ok(bases) => self.tiles.insert(key, bases),
                        Err(_) => self.tiles.cancel(key),
                    }
                }
            }
            Message::LocusEdited(text) => {
                self.draft = Some(text);
                self.locus_error = None;
            }
            Message::LocusSubmitted => return self.submit(),
            Message::Command(command) => return self.command(command),
        }
        Task::none()
    }

    fn command(&mut self, command: Command) -> Task<Message> {
        match command {
            Command::Pan(share) => {
                self.drop_draft();
                self.navigate(|viewport| viewport.pan(f64::from(share) * viewport.width()))
            }
            Command::Zoom(factor) => {
                self.drop_draft();
                self.navigate(|viewport| viewport.zoom(f64::from(factor), viewport.width() / 2.0))
            }
            Command::FocusLocus => Task::batch([
                operation::focus(chrome::LOCUS),
                operation::select_all(chrome::LOCUS),
            ]),
        }
    }

    fn drop_draft(&mut self) {
        self.draft = None;
        self.locus_error = None;
    }

    fn submit(&mut self) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let text = self.draft.take().unwrap_or_else(|| session.locus());
        match session.reference.assembly.resolve(&text) {
            Ok(target) => {
                if target.contig != session.contig {
                    let length = session.reference.assembly.contigs()[target.contig].length;
                    session.viewport = session.viewport.retarget(length);
                    session.contig = target.contig;
                }
                session.viewport = session.viewport.jump(target.span);
                self.locus_error = None;
                self.fetch()
            }
            Err(error) => {
                self.locus_error = Some(error.to_string());
                self.draft = Some(text);
                Task::none()
            }
        }
    }

    fn navigate(&mut self, change: impl FnOnce(Viewport) -> Viewport) -> Task<Message> {
        if let Some(session) = &mut self.session {
            session.viewport = change(session.viewport);
        }
        self.fetch()
    }

    fn fetch(&mut self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let viewport = session.viewport;
        if viewport.scale() > 1.0 {
            return Task::none();
        }
        let visible = viewport.visible();
        let wanted = visible.expand(visible.len(), viewport.length());
        let generation = self.generation;
        Task::batch(
            self.tiles
                .request(session.contig, wanted)
                .into_iter()
                .map(|key| {
                    let reference = Arc::clone(&session.reference);
                    Task::perform(
                        async move {
                            reference
                                .fetch(key.contig, TILING.span(key.index))
                                .map(Arc::from)
                        },
                        move |result| Message::TileLoaded(generation, key, result),
                    )
                }),
        )
    }

    fn view(&self) -> Element<'_, Message> {
        let appearance = match self.appearance {
            Appearance::System => Icon::Monitor,
            Appearance::Light => Icon::Sun,
            Appearance::Dark => Icon::Moon,
        };
        let locus = self.session.as_ref().map(|session| LocusField {
            value: self.draft.clone().unwrap_or_else(|| session.locus()),
            error: self.locus_error.clone(),
        });
        let content = column![
            chrome::title_bar(Controls::CURRENT, appearance, locus),
            self.body()
        ];
        match Controls::CURRENT {
            Controls::Native => content.into(),
            Controls::Custom => stack![content, chrome::resize_handles()].into(),
        }
    }

    fn body(&self) -> Element<'_, Message> {
        if let Some(session) = &self.session {
            return self.browser(session);
        }
        let message = match &self.status {
            Status::Idle => text("Open a reference sequence to start.").style(style::muted),
            Status::Opening(name) => text(format!("Opening {name}â€¦")).style(style::muted),
            Status::Failed(error) => text(error.as_str()),
        };
        let open = button(text("Open FASTA").font(font::UI_STRONG).size(13))
            .padding([8, 16])
            .style(style::primary)
            .on_press(Message::Open);
        center(
            column![message, open]
                .spacing(16)
                .align_x(Alignment::Center),
        )
        .into()
    }

    fn browser(&self, session: &Session) -> Element<'_, Message> {
        let viewport = session.viewport;
        let bases = sequence::scene(
            viewport,
            self.tiles.chunks(session.contig, viewport.visible()),
            BASE_HEIGHT,
        );
        let sequence = stack![
            container(scene::view(bases, BASE_HEIGHT)).padding([TRACK_PADDING, 0.0]),
            label("Sequence"),
        ];
        let tracks = column![
            container(scene::view(
                overview::scene(viewport, OVERVIEW_HEIGHT),
                OVERVIEW_HEIGHT
            ))
            .id("overview"),
            container(scene::view(
                ruler::scene(viewport, RULER_HEIGHT),
                RULER_HEIGHT
            ))
            .id("ruler"),
            hairline(),
            container(sequence).id("sequence"),
            hairline(),
        ]
        .height(Length::Fill);
        Navigator::new(tracks, self.width)
            .on_pan(Message::Pan)
            .on_zoom(Message::Zoom)
            .on_resize(Message::Resized)
            .into()
    }

    fn theme(&self) -> Theme {
        if self.appearance.is_dark(self.system) {
            theme::dark()
        } else {
            theme::light()
        }
    }
}

fn label(name: &str) -> Element<'_, Message> {
    container(text(name).size(LABEL_SIZE).style(style::muted))
        .padding([0, 4])
        .style(|theme| container::Style {
            background: Some(tokens(theme).canvas.scale_alpha(0.85).into()),
            border: Border::default().rounded(3),
            ..container::Style::default()
        })
        .into()
}

fn hairline<'a>() -> Element<'a, Message> {
    container(space())
        .width(Length::Fill)
        .height(1)
        .style(|theme| container::background(tokens(theme).line))
        .into()
}

fn shortcut(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => {
            shortcuts::command(&key, modifiers).map(Message::Command)
        }
        _ => None,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

async fn pick() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Open a reference sequence")
        .add_filter("FASTA", &["fa", "fasta", "fna", "fas"])
        .pick_file()
        .await
        .map(|file| file.path().to_owned())
}

pub fn run() -> iced::Result {
    font::FILES
        .into_iter()
        .fold(
            iced::application(Genie::boot, Genie::update, Genie::view),
            |application, (bytes, _)| application.font(bytes),
        )
        .default_font(font::UI)
        .title("genie")
        .window(chrome::settings())
        .theme(Genie::theme)
        .subscription(|_| {
            Subscription::batch([
                system::theme_changes().map(Message::SystemModeChanged),
                keyboard::listen().filter_map(shortcut),
            ])
        })
        .run()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::{Assembly, Contig, Region};
    use genie_formats::fai::FaiRecord;
    use iced::keyboard::{self, key::Named};
    use iced::theme::Mode;
    use iced_test::{selector, simulator};

    use super::{Genie, Message, shortcut};
    use crate::reference::Reference;
    use crate::shortcuts::Command;
    use crate::theme::Appearance;
    use crate::tiles::Key;

    fn reference() -> Arc<Reference> {
        let records = vec![
            FaiRecord {
                name: "chr1".to_owned(),
                length: 1_000,
                offset: 6,
                line_bases: 60,
                line_width: 61,
            },
            FaiRecord {
                name: "chr2".to_owned(),
                length: 500,
                offset: 1_029,
                line_bases: 60,
                line_width: 61,
            },
        ];
        Arc::new(Reference {
            path: PathBuf::from("ref.fa"),
            name: "ref.fa".to_owned(),
            assembly: Assembly::new(
                records
                    .iter()
                    .map(|record| Contig {
                        name: record.name.clone(),
                        length: record.length,
                    })
                    .collect(),
            ),
            records,
        })
    }

    fn go(genie: &mut Genie, locus: &str) {
        let _ = genie.update(Message::LocusEdited(locus.to_owned()));
        let _ = genie.update(Message::LocusSubmitted);
    }

    #[test]
    fn submitting_a_locus_jumps_there() {
        let mut genie = opened();
        go(&mut genie, "chr1:101-200");
        assert!(shows(&genie, "chr1:101-200"));
    }

    #[test]
    fn submitting_another_contig_switches_to_it() {
        let mut genie = opened();
        go(&mut genie, "2");
        assert!(shows(&genie, "chr2:1-500"));
    }

    #[test]
    fn bad_loci_keep_the_draft_and_explain() {
        let mut genie = opened();
        go(&mut genie, "chr9:1-10");
        assert!(shows(&genie, "chr9:1-10"));
        assert!(shows(&genie, "The assembly has no contig \"chr9\"."));
    }

    #[test]
    fn arrow_keys_pan_a_quarter_view() {
        let mut genie = opened();
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        let _ = genie.update(Message::Command(Command::Pan(-0.25)));
        assert!(shows(&genie, "chr1:126-625"));
    }

    #[test]
    fn plus_zooms_in_around_the_center() {
        let mut genie = opened();
        let _ = genie.update(Message::Command(Command::Zoom(2.0)));
        assert!(shows(&genie, "chr1:251-750"));
    }

    #[test]
    fn key_presses_become_commands() {
        let press = keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::ArrowLeft),
            modified_key: keyboard::Key::Named(Named::ArrowLeft),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ArrowLeft),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        };
        assert_eq!(shortcut(press), Some(Message::Command(Command::Pan(0.25))));
        assert_eq!(
            shortcut(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::SHIFT
            )),
            None
        );
    }

    #[test]
    fn navigating_drops_an_unsubmitted_draft() {
        let mut genie = opened();
        let _ = genie.update(Message::LocusEdited("chr1:5".to_owned()));
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        assert!(shows(&genie, "chr1:1-500"));
    }

    #[test]
    fn opened_reference_shows_its_first_contig() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let mut ui = simulator(genie.view());
        assert!(ui.find("chr1:1-1,000").is_ok());
    }

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    #[test]
    fn zoom_and_pan_move_the_locus() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let _ = genie.update(Message::Zoom(2.0, 0.0));
        assert!(shows(&genie, "chr1:1-500"));
        let _ = genie.update(Message::Pan(-100.0));
        assert!(shows(&genie, "chr1:51-550"));
    }

    #[test]
    fn resizing_keeps_the_scale() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let _ = genie.update(Message::Resized(500.0));
        assert!(shows(&genie, "chr1:1-500"));
    }

    fn opened() -> Genie {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        genie
    }

    #[test]
    fn opening_requests_the_visible_tiles() {
        let mut genie = opened();
        assert_eq!(genie.tiles.request(0, Region::between(0, 1_000)), []);
    }

    #[test]
    fn loaded_tiles_feed_the_sequence() {
        let mut genie = opened();
        let tile = Key {
            contig: 0,
            index: 0,
        };
        let bases: Arc<[u8]> = Arc::from(vec![b'A'; 1_000]);
        let _ = genie.update(Message::TileLoaded(genie.generation, tile, Ok(bases)));
        assert_eq!(genie.tiles.chunks(0, Region::between(0, 10)).count(), 1);
    }

    #[test]
    fn tiles_from_an_older_reference_are_ignored() {
        let mut genie = opened();
        let tile = Key {
            contig: 0,
            index: 0,
        };
        let bases: Arc<[u8]> = Arc::from(vec![b'A'; 1_000]);
        let _ = genie.update(Message::TileLoaded(genie.generation - 1, tile, Ok(bases)));
        assert_eq!(genie.tiles.chunks(0, Region::between(0, 10)).count(), 0);
    }

    #[test]
    fn browser_shows_a_labelled_sequence_track() {
        let genie = opened();
        let mut ui = simulator(genie.view());
        let bounds = ui.find(selector::id("sequence")).unwrap().bounds();
        assert!((bounds.height - 20.0).abs() < f32::EPSILON);
        assert!(ui.find("Sequence").is_ok());
    }

    #[test]
    fn browser_stacks_a_thin_overview_and_ruler() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let mut ui = simulator(genie.view());
        for (id, height) in [("overview", 12.0), ("ruler", 20.0)] {
            let bounds = ui.find(selector::id(id)).unwrap().bounds();
            assert!((bounds.height - height).abs() < f32::EPSILON, "{id}");
        }
    }

    #[test]
    fn failed_open_shows_the_error() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Err(
            "Line 1: a field is missing.".to_owned()
        )));
        let mut ui = simulator(genie.view());
        assert!(ui.find("Line 1: a field is missing.").is_ok());
    }

    #[test]
    fn picking_no_file_keeps_the_welcome_view() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Picked(None));
        let mut ui = simulator(genie.view());
        assert!(ui.find("Open a reference sequence to start.").is_ok());
    }

    #[test]
    fn welcome_open_button_asks_for_a_file() {
        let genie = Genie::default();
        let mut ui = simulator(genie.view());
        ui.click("Open FASTA").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), [Message::Open]);
    }

    #[test]
    fn welcome_view_asks_for_a_reference() {
        let genie = Genie::default();
        let mut ui = simulator(genie.view());
        assert!(ui.find("Open a reference sequence to start.").is_ok());
    }

    #[test]
    fn appearance_button_cycles_appearance() {
        let mut genie = Genie::default();
        let mut ui = simulator(genie.view());
        ui.click(selector::id("appearance")).unwrap();
        for message in ui.into_messages() {
            let _ = genie.update(message);
        }
        assert_eq!(genie.appearance, Appearance::Light);
    }

    #[test]
    fn system_mode_changes_the_theme() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::SystemModeChanged(Mode::Dark));
        assert!(genie.theme().extended_palette().is_dark);
    }
}
