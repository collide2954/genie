use std::path::{Path, PathBuf};

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Task};

use crate::export;
use crate::paths::file_name;
use crate::{font, style};

use super::view::{HEADER_HEIGHT, RULER_HEIGHT, across, hairline, panel_label, ruler_view};
use super::{Genie, Message, Session};

pub(super) const CAPTION_HEIGHT: f32 = 22.0;
pub(super) const CAPTION_SIZE: f32 = 11.0;
const DRAW_FAILED: &str = "The view could not be drawn.";
const EXPORT_CUT: &str = "The view is too tall. The image shows only the top part.";

impl Genie {
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

    pub(super) fn export_view<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let count = session.count();
        let mut title = row![text(session.reference.label()).size(CAPTION_SIZE)]
            .spacing(6)
            .align_y(Alignment::Center);
        if count == 1 {
            title = title
                .push(text("·").size(CAPTION_SIZE).style(style::muted))
                .push(
                    text(session.locus())
                        .font(font::MONO)
                        .size(CAPTION_SIZE)
                        .style(style::muted),
                );
        }
        let caption = container(title)
            .padding([0, 8])
            .height(CAPTION_HEIGHT)
            .align_y(Alignment::Center);
        let mut page = column![caption, hairline()];
        if count > 1 {
            page = page
                .push(across(count, HEADER_HEIGHT, |panel| {
                    panel_label(session, panel)
                }))
                .push(hairline());
        }
        page.push(across(count, RULER_HEIGHT, |panel| {
            ruler_view(session, panel)
        }))
        .push(hairline())
        .push(self.tracks(session, true))
        .into()
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
