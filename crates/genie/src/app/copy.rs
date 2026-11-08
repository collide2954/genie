use iced::Task;

use crate::card;

use super::{Genie, Message};

impl Genie {
    pub(super) fn copy_text(&self) -> Option<String> {
        if let Some(card) = &self.pinned {
            return Some(card::plain(card));
        }
        self.session.as_ref().map(super::Session::locus)
    }

    pub(super) fn copy(&mut self) -> Task<Message> {
        let Some(text) = self.copy_text() else {
            return Task::none();
        };
        let notice = if self.pinned.is_some() {
            "Copied the card text.".to_owned()
        } else {
            format!("Copied {text}.")
        };
        Task::batch([iced::clipboard::write(text), self.inform(notice)])
    }
}
