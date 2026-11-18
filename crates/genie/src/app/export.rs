use std::path::{Path, PathBuf};

use genie_core::Region;
use genie_render::scene::Primitive;
use genie_render::{bands, ruler};
use iced::widget::{
    button, center, column, container, opaque, operation, row, space, stack, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::icon::{self, Icon};
use crate::paths::file_name;
use crate::{export, font, menu, scene, style, track_dialog};

use super::view::{HEADER_HEIGHT, RULER_HEIGHT, across, hairline, panel_label, ruler_view};
use super::{Genie, Message, Popup, Session};

pub(super) const CAPTION_HEIGHT: f32 = 22.0;
pub(super) const CAPTION_SIZE: f32 = 11.0;
pub(super) const TITLE_SIZE: f32 = 12.0;
/// The space between the end of a scale bar and the end of its row.
pub(super) const BAR_INSET: f32 = 8.0;
const DIALOG_WIDTH: f32 = 360.0;
const TITLE_FIELD: &str = "figure-title";
const DRAW_FAILED: &str = "The view could not be drawn.";
const EXPORT_CUT: &str = "The view is too tall. The image shows only the top part.";

/// The changes of the export dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum Figuring {
    /// A new title of the image.
    Title(String),
    /// Shows or hides the scale bar.
    Scale,
    /// Shows or hides the region bands.
    Bands,
    /// Closes the dialog and asks where to save the image.
    Export,
}

/// The bands of the regions in view of a panel, and the number of their rows.
pub(super) fn panel_bands(session: &Session, panel: usize) -> (Vec<Primitive>, usize) {
    let (contig, viewport) = session.view(panel);
    let regions: Vec<(Region, &str)> = session
        .marks
        .on(contig)
        .map(|mark| (mark.span, mark.name.as_str()))
        .collect();
    bands::scene(viewport, &regions)
}

/// The rows of region bands of an image: as many as the panel that needs the most. None when the
/// bands are off or no region is in view.
pub(super) fn band_rows(session: &Session) -> Option<usize> {
    if !session.figure.bands {
        return None;
    }
    let rows = (0..session.count())
        .map(|panel| panel_bands(session, panel).1)
        .max()
        .unwrap_or(0);
    (rows > 0).then_some(rows)
}

/// True when the caption of an image holds the locus and the scale bar: for one panel or for the
/// whole genome. Else each panel has a header with them.
pub(super) fn one_caption(session: &Session) -> bool {
    session.count() == 1 || session.genome
}

/// The header of a panel of an image: its locus and its scale bar.
fn panel_head(session: &Session, panel: usize) -> Element<'_, Message> {
    let label = panel_label(session, panel);
    if !session.figure.scale {
        return label;
    }
    let (_, viewport) = session.view(panel);
    let right = viewport.width() as f32 - BAR_INSET;
    let bar = ruler::scale_bar(viewport, right, HEADER_HEIGHT / 2.0);
    let bar = container(scene::view(bar, HEADER_HEIGHT)).id(format!("scale-bar-{panel}"));
    stack![container(label).width(Length::Fill), bar].into()
}

impl Genie {
    pub(super) fn open_export(&mut self) -> Task<Message> {
        if self.session.is_none() {
            return Task::none();
        }
        self.close_menus();
        self.popup = Some(Popup::Export);
        operation::focus(TITLE_FIELD)
    }

    pub(super) fn figuring(&mut self, figuring: Figuring) -> Task<Message> {
        if figuring == Figuring::Export {
            self.close_menus();
            return self.export();
        }
        self.on_session(|session| {
            let figure = &mut session.figure;
            match figuring {
                Figuring::Title(title) => figure.title = title,
                Figuring::Scale => figure.scale = !figure.scale,
                Figuring::Bands => figure.bands = !figure.bands,
                Figuring::Export => {}
            }
        });
        Task::none()
    }

    pub(super) fn export_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Export) {
            return None;
        }
        let session = self.session.as_ref()?;
        let figure = &session.figure;
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let heading = row![
            text("Export image").font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let title = text_input("No title", &figure.title)
            .id(TITLE_FIELD)
            .size(12)
            .padding([2, 6])
            .style(style::locus)
            .on_input(|title| Message::Figure(Figuring::Title(title)))
            .on_submit(Message::Figure(Figuring::Export));
        let regions = !session.marks.is_empty();
        let parts = row![
            menu::item(
                "Scale bar",
                Message::Figure(Figuring::Scale),
                figure.scale,
                true
            ),
            menu::item(
                "Region bands",
                Message::Figure(Figuring::Bands),
                figure.bands,
                regions
            ),
        ]
        .spacing(2);
        let export = button(text("Export…").size(12))
            .padding([4, 12])
            .style(style::primary)
            .on_press(Message::Figure(Figuring::Export));
        let body = column![
            track_dialog::line("Title", title),
            track_dialog::line("Show", parts),
            row![space::horizontal(), container(export).id("figure-export")],
        ]
        .spacing(8);
        let card = container(column![heading, body].spacing(12))
            .id("export-dialog")
            .width(DIALOG_WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }

    pub(super) fn exported(&mut self, result: Result<PathBuf, String>, cut: bool) -> Task<Message> {
        match result {
            Ok(path) if cut => {
                self.warn(format!("Saved {}. {EXPORT_CUT}", file_name(&path)));
                Task::none()
            }
            Ok(path) => self.inform(format!("Saved {}.", file_name(&path))),
            Err(error) => {
                self.warn(error);
                Task::none()
            }
        }
    }

    /// The first line of an image: the title, the reference and the locus, and the scale bar.
    fn caption<'a>(&self, session: &'a Session) -> Element<'a, Message> {
        let figure = &session.figure;
        let mut line = row![].spacing(6).align_y(Alignment::Center);
        if let Some(title) = figure.title() {
            line = line
                .push(
                    text(title.to_owned())
                        .font(font::UI_STRONG)
                        .size(TITLE_SIZE),
                )
                .push(text("·").size(CAPTION_SIZE).style(style::muted));
        }
        line = line.push(text(session.reference.label()).size(CAPTION_SIZE));
        let single = one_caption(session);
        if single {
            line = line
                .push(text("·").size(CAPTION_SIZE).style(style::muted))
                .push(
                    text(session.locus())
                        .font(font::MONO)
                        .size(CAPTION_SIZE)
                        .style(style::muted),
                );
        }
        let caption = container(line)
            .padding([0, 8])
            .width(Length::Fill)
            .height(CAPTION_HEIGHT)
            .align_y(Alignment::Center);
        if !(figure.scale && single) {
            return caption.into();
        }
        let (_, viewport) = session.view(0);
        let bar = ruler::scale_bar(viewport, self.width - BAR_INSET, CAPTION_HEIGHT / 2.0);
        let bar = container(scene::view(bar, CAPTION_HEIGHT)).id("scale-bar");
        stack![caption, bar].into()
    }

    pub(super) fn export_view<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let mut page = column![self.caption(session), hairline()];
        let portions = session.portions();
        if !one_caption(session) {
            page = page
                .push(across(&portions, HEADER_HEIGHT, |panel| {
                    panel_head(session, panel)
                }))
                .push(hairline());
        }
        page = page
            .push(across(&portions, RULER_HEIGHT, |panel| {
                ruler_view(session, panel)
            }))
            .push(hairline());
        if let Some(rows) = band_rows(session) {
            let height = rows as f32 * bands::ROW_HEIGHT;
            let lane = across(&portions, height, |panel| {
                scene::view(panel_bands(session, panel).0, height).into()
            });
            page = page
                .push(container(lane).id("figure-bands"))
                .push(hairline());
        }
        page.push(self.tracks(session, true)).into()
    }

    pub(super) fn export(&self) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let contig = &session.reference.assembly.contigs()[session.contig].name;
        let name = export::file_name(contig, session.viewport.visible());
        Task::perform(save_as(name), Message::ExportPicked)
    }

    pub(super) fn export_to(&mut self, path: Option<PathBuf>) -> Task<Message> {
        let (Some(path), Some(session)) = (path, &self.session) else {
            return Task::none();
        };
        if is_svg(&path) {
            let result = std::fs::write(&path, self.figure(session))
                .map(|()| path)
                .map_err(|error| format!("The image could not be saved. {error}"));
            return self.exported(result, false);
        }
        if let Some(image) = export::render(self.export_view(session), self.width, &self.theme()) {
            let cut = image.cut;
            Task::perform(
                async move {
                    let result = export::save(path, &image);
                    if let Ok(path) = &result {
                        log::info!(
                            "Saved {} ({}x{})",
                            path.display(),
                            image.width,
                            image.height
                        );
                    }
                    result
                },
                move |result| Message::Exported(result, cut),
            )
        } else {
            self.warn(DRAW_FAILED.to_owned());
            Task::none()
        }
    }
}

pub(super) fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

async fn save_as(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export image")
        .set_file_name(name)
        .add_filter("PNG image", &["png"])
        .add_filter("SVG image", &["svg"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}
