use genie_render::translation::Frame;
use iced::widget::column;
use iced::{Element, Point, Size};

use crate::app::Message;
use crate::menu;

pub const MENU_HEIGHT: f32 = 128.0;
const FRAMES: [(&str, Frames); 3] = [
    ("No translation", Frames::Off),
    ("3 frames", Frames::Three),
    ("6 frames", Frames::Six),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frames {
    #[default]
    Off,
    Three,
    Six,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Reverse(bool),
    Frames(Frames),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reading {
    pub reverse: bool,
    pub frames: Frames,
}

impl Reading {
    pub fn change(&mut self, change: Change) {
        match change {
            Change::Reverse(reverse) => self.reverse = reverse,
            Change::Frames(frames) => self.frames = frames,
        }
    }

    pub fn frames(self) -> Vec<Frame> {
        let strand = |reverse| (0..3).map(move |offset| Frame { offset, reverse });
        match self.frames {
            Frames::Off => Vec::new(),
            Frames::Three => strand(self.reverse).collect(),
            Frames::Six => strand(false).chain(strand(true)).collect(),
        }
    }
}

pub fn menu<'a>(reading: Reading, at: Point, window: Size) -> Element<'a, Message> {
    let entry =
        |label, change, chosen| menu::item(label, Message::SequenceChanged(change), chosen, true);
    let content = column![
        entry("Forward strand", Change::Reverse(false), !reading.reverse),
        entry("Reverse strand", Change::Reverse(true), reading.reverse),
        menu::separator(),
    ]
    .extend(
        FRAMES
            .map(|(label, frames)| entry(label, Change::Frames(frames), reading.frames == frames)),
    )
    .spacing(1);
    menu::card(content, at, window, MENU_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::{Change, Frames, Reading};

    fn frames(reading: Reading) -> Vec<(u8, bool)> {
        reading
            .frames()
            .iter()
            .map(|frame| (frame.offset, frame.reverse))
            .collect()
    }

    #[test]
    fn frames_follow_the_strand() {
        let mut reading = Reading::default();
        assert_eq!(frames(reading), []);
        reading.change(Change::Frames(Frames::Three));
        assert_eq!(frames(reading), [(0, false), (1, false), (2, false)]);
        reading.change(Change::Reverse(true));
        assert_eq!(frames(reading), [(0, true), (1, true), (2, true)]);
        reading.change(Change::Frames(Frames::Six));
        assert_eq!(frames(reading).len(), 6);
        assert_eq!(frames(reading)[0], (0, false));
    }
}
