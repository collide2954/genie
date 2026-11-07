use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    A,
    C,
    G,
    T,
    N,
}

impl Base {
    pub fn from_byte(byte: u8) -> Self {
        match byte.to_ascii_uppercase() {
            b'A' => Self::A,
            b'C' => Self::C,
            b'G' => Self::G,
            b'T' => Self::T,
            _ => Self::N,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Base(Base),
    OnBase(Base),
    Muted,
    Line,
    Accent,
    Selection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Ui,
    Mono,
    MonoStrong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        paint: Paint,
    },
    Text {
        x: f32,
        y: f32,
        content: Cow<'static, str>,
        size: f32,
        paint: Paint,
        font: Font,
        align: Align,
    },
}
