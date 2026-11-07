use std::path::PathBuf;

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Task};

use crate::export;
use crate::paths::file_name;
use crate::{font, style};

use super::view::{hairline, ruler_view};
use super::{CAPTION_HEIGHT, CAPTION_SIZE, DRAW_FAILED, EXPORT_CUT, Genie, Message, Session};

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
        let caption = container(
            row![
                text(&session.reference.name).size(CAPTION_SIZE),
                text("·").size(CAPTION_SIZE).style(style::muted),
                text(session.locus())
                    .font(font::MONO)
                    .size(CAPTION_SIZE)
                    .style(style::muted),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([0, 8])
        .height(CAPTION_HEIGHT)
        .align_y(Alignment::Center);
        column![
            caption,
            hairline(),
            ruler_view(session.viewport),
            hairline(),
            self.tracks(session, true)
        ]
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

async fn save_as(name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export PNG")
        .set_file_name(name)
        .add_filter("PNG image", &["png"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}
