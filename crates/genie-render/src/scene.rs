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
    Duplication,
    Inversion,
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
    Tint(Hue),
    Pair(Hue),
    Rgb([u8; 3]),
    Strand(Strand),
    Faint(Strand),
    Plain(Shade),
    Modified(u8),
    /// A change of copy number, from -100 for a full loss to 100 for a full gain.
    Change(i8),
    Coverage,
    Insertion,
    /// Text on the color of an insertion.
    OnInsertion,
    Codon(bool),
    Text,
    Muted,
    Line,
    Accent,
    Selection,
    Stain(u8),
    /// One of the colors of the most common values of a sample field, from 0.
    Category(u8),
    /// A number of a sample field: from 0 for the smallest to 100 for the largest.
    Level(u8),
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
    Label {
        x: f32,
        y: f32,
        content: Cow<'static, str>,
        size: f32,
        align: Align,
    },
    Arc {
        from: f32,
        to: f32,
        base: f32,
        rise: f32,
        width: f32,
        paint: Paint,
    },
    /// A straight line from one point to another, with round ends: a line that is shorter than
    /// its width shows as a dot.
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        paint: Paint,
    },
    /// A filled band with four corners in the order of its sides. A band whose sides cross
    /// fills the two halves.
    Ribbon {
        corners: [(f32, f32); 4],
        paint: Paint,
    },
}

impl Primitive {
    pub fn arc_points(from: f32, to: f32, base: f32, rise: f32) -> Vec<(f32, f32)> {
        (0..=ARC_STEPS)
            .map(|step| {
                let t = step as f32 / ARC_STEPS as f32;
                (from + (to - from) * t, base - 4.0 * rise * t * (1.0 - t))
            })
            .collect()
    }

    #[must_use]
    pub fn lowered(mut self, by: f32) -> Self {
        match &mut self {
            Self::Rect { y, .. }
            | Self::Text { y, .. }
            | Self::Chevron { y, .. }
            | Self::Label { y, .. } => *y += by,
            Self::Arc { base, .. } => *base += by,
            Self::Line { from, to, .. } => {
                from.1 += by;
                to.1 += by;
            }
            Self::Ribbon { corners, .. } => {
                for corner in corners {
                    corner.1 += by;
                }
            }
        }
        self
    }
}

const ARC_STEPS: usize = 24;
pub const LABEL_INSET: f32 = 12.0;

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{Align, Font, Paint, Primitive};

    #[test]
    fn lowered_primitives_move_down() {
        let primitives = [
            Primitive::Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                paint: Paint::Muted,
            },
            Primitive::Text {
                x: 1.0,
                y: 2.0,
                content: Cow::Borrowed("a"),
                size: 9.0,
                paint: Paint::Text,
                font: Font::Ui,
                align: Align::Start,
            },
            Primitive::Chevron {
                x: 1.0,
                y: 2.0,
                size: 2.0,
                forward: true,
                paint: Paint::Muted,
            },
            Primitive::Label {
                x: 1.0,
                y: 2.0,
                content: Cow::Borrowed("b"),
                size: 9.0,
                align: Align::End,
            },
            Primitive::Arc {
                from: 1.0,
                to: 9.0,
                base: 2.0,
                rise: 2.0,
                width: 1.0,
                paint: Paint::Muted,
            },
            Primitive::Line {
                from: (1.0, 2.0),
                to: (5.0, 2.0),
                width: 1.0,
                paint: Paint::Muted,
            },
            Primitive::Ribbon {
                corners: [(1.0, 2.0), (3.0, 2.0), (3.0, 2.0), (1.0, 2.0)],
                paint: Paint::Muted,
            },
        ];
        let tops: Vec<f32> = primitives
            .into_iter()
            .map(|primitive| match primitive.lowered(10.0) {
                Primitive::Rect { y, .. }
                | Primitive::Text { y, .. }
                | Primitive::Chevron { y, .. }
                | Primitive::Label { y, .. } => y,
                Primitive::Arc { base, .. } => base,
                Primitive::Line { from, to, .. } => from.1.min(to.1),
                Primitive::Ribbon { corners, .. } => corners
                    .iter()
                    .map(|corner| corner.1)
                    .fold(f32::INFINITY, f32::min),
            })
            .collect();
        assert_eq!(tops, [12.0; 7]);
    }
}
