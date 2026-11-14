use genie_render::signal;
use iced::Element;

use crate::track_dialog::{self, Draft};

use super::tracks::MIN_HEIGHT;
use super::{Drafting, Genie, Message, Popup};

impl Genie {
    pub(super) fn open_track_settings(&mut self, id: u64) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == id) else {
            return;
        };
        let draft = Draft::of(track);
        self.close_menus();
        self.popup = Some(Popup::Track(id));
        self.track_draft = Some(draft);
        self.range_draft = self
            .shown_range(id)
            .map(|scale| (signal::number(scale.min), signal::number(scale.max)));
    }

    pub(super) fn draft_track(&mut self, drafting: Drafting) {
        let Some(draft) = &mut self.track_draft else {
            return;
        };
        let id = draft.id;
        match &drafting {
            Drafting::Name(text) => draft.name.clone_from(text),
            Drafting::Height(text) => draft.height.clone_from(text),
        }
        self.on_session(|session| {
            let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) else {
                return;
            };
            match drafting {
                Drafting::Name(text) if text.trim().is_empty() => {
                    track.name.clone_from(&track.source.name);
                }
                Drafting::Name(text) => track.name = text,
                Drafting::Height(text) if text.trim().is_empty() => track.height = None,
                Drafting::Height(text) => {
                    if let Ok(height) = text.trim().parse::<f32>()
                        && height.is_finite()
                    {
                        track.height = Some(height.max(MIN_HEIGHT));
                    }
                }
            }
        });
    }

    pub(super) fn track_dialog(&self) -> Option<Element<'_, Message>> {
        let Some(Popup::Track(id)) = self.popup else {
            return None;
        };
        let draft = self.track_draft.as_ref()?;
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        let mode = track.mode_at(0, session.contig);
        Some(track_dialog::view(
            track,
            mode,
            draft,
            self.range_draft.as_ref(),
        ))
    }
}
