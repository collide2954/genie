use std::borrow::Cow;

use genie_core::Strand;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Hue {
    #[default]
    Blue,
    Green,
    Orange,
    Vermillion,
    Purple,
}

impl Hue {
    pub const ALL: [Self; 5] = [
        Self::Blue,
        Self::Green,
        Self::Orange,
        Self::Vermillion,
        Self::Purple,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allele {
    Base(Base),
    Insertion,
    Deletion,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    Full,
    Half,
    Faded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Allele(Allele, Shade),
    HomRef,
    Base(Base),
    OnBase(Base),
    Faded(Base),
    Hue(Hue),
    Strand(Strand),
    Coverage,
    Insertion,
    Codon(bool),
    Text,
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
    End,
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
    Chevron {
        x: f32,
        y: f32,
        size: f32,
        forward: bool,
        paint: Paint,
    },
}
