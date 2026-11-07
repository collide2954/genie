use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::{Locus, Region, Span};
use iced::theme::Mode;
use iced::widget::{button, center, column, stack, text};
use iced::{Alignment, Element, Task, Theme, system};

use crate::chrome::{self, Controls};
use crate::icon::Icon;
use crate::reference::{self, Reference};
use crate::theme::{self, Appearance, tokens};
use crate::{font, paths, style};

#[derive(Debug, Default)]
pub struct Genie {
    appearance: Appearance,
    system: Mode,
    status: Status,
    session: Option<Session>,
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
}

impl Session {
    fn locus(&self) -> String {
        let contig = &self.reference.assembly.contigs()[self.contig];
        Locus {
            name: contig.name.clone(),
            span: Span::Region(Region::between(0, contig.length)),
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
                self.session = Some(Session {
                    reference,
                    contig: 0,
                });
            }
            Message::Opened(Err(error)) => self.status = Status::Failed(error),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let appearance = match self.appearance {
            Appearance::System => Icon::Monitor,
            Appearance::Light => Icon::Sun,
            Appearance::Dark => Icon::Moon,
        };
        let locus = self.session.as_ref().map(Session::locus);
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
            let contigs = session.reference.assembly.contigs().len();
            let summary = format!(
                "{} · {contigs} {}",
                session.reference.name,
                if contigs == 1 { "contig" } else { "contigs" }
            );
            return center(text(summary).style(muted)).into();
        }
        let message = match &self.status {
            Status::Idle => text("Open a reference sequence to start.").style(muted),
            Status::Opening(name) => text(format!("Opening {name}…")).style(muted),
            Status::Failed(error) => text(error.as_str()).style(|theme| text::Style {
                color: Some(tokens(theme).danger),
            }),
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

    fn theme(&self) -> Theme {
        if self.appearance.is_dark(self.system) {
            theme::dark()
        } else {
            theme::light()
        }
    }
}

fn muted(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(tokens(theme).muted),
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
        .subscription(|_| system::theme_changes().map(Message::SystemModeChanged))
        .run()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::{Assembly, Contig};
    use genie_formats::fai::FaiRecord;
    use iced::theme::Mode;
    use iced_test::{selector, simulator};

    use super::{Genie, Message};
    use crate::reference::Reference;
    use crate::theme::Appearance;

    fn reference() -> Arc<Reference> {
        let record = FaiRecord {
            name: "chr1".to_owned(),
            length: 1_000,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        };
        Arc::new(Reference {
            path: PathBuf::from("ref.fa"),
            name: "ref.fa".to_owned(),
            assembly: Assembly::new(vec![Contig {
                name: record.name.clone(),
                length: record.length,
            }]),
            records: vec![record],
        })
    }

    #[test]
    fn opened_reference_shows_its_first_contig() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference())));
        let mut ui = simulator(genie.view());
        assert!(ui.find("chr1:1-1,000").is_ok());
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
