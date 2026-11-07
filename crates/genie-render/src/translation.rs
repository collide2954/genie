use std::borrow::Cow;

use genie_core::Viewport;
use genie_core::codon::{complement, translate};

use crate::scene::{Align, Base, Font, Paint, Primitive};

pub const ROW_HEIGHT: f32 = 14.0;

const LETTERS_FROM: f64 = 3.0;
const GAPS_FROM: f64 = 6.0;
const LETTER_SIZE: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub offset: u8,
    pub reverse: bool,
}

pub fn scene<'a>(
    viewport: Viewport,
    chunks: impl IntoIterator<Item = (u64, &'a [u8])>,
    frames: &[Frame],
) -> Vec<Primitive> {
    let pixels = 1.0 / viewport.scale();
    if pixels < 1.0 {
        return Vec::new();
    }
    let visible = viewport.visible();
    let start = visible.start().saturating_sub(2);
    let end = visible.end() + 2;
    let mut bases = vec![None; (end - start) as usize];
    for (from, chunk) in chunks {
        let first = start.max(from);
        let last = end.min(from + chunk.len() as u64);
        for position in first..last {
            bases[(position - start) as usize] = Some(chunk[(position - from) as usize]);
        }
    }
    let codon_width = 3.0 * pixels;
    let width = if codon_width >= GAPS_FROM {
        codon_width - 1.0
    } else {
        codon_width
    };
    let letters = pixels >= LETTERS_FROM;
    let mut primitives = Vec::new();
    let mut marks = Vec::new();
    for (row, frame) in frames.iter().enumerate() {
        let top = row as f32 * ROW_HEIGHT;
        let mut position = start + (3 + u64::from(frame.offset) - start % 3) % 3;
        while position + 3 <= end {
            let at = (position - start) as usize;
            let codon = (bases[at], bases[at + 1], bases[at + 2]);
            let visible = position < visible.end() && position + 3 > visible.start();
            if let (Some(first), Some(second), Some(third)) = codon
                && visible
            {
                let codon = if frame.reverse {
                    [complement(third), complement(second), complement(first)]
                } else {
                    [first, second, third]
                };
                let amino = translate(codon);
                let (paint, ink) = match amino {
                    b'M' => (Paint::Base(Base::A), Paint::OnBase(Base::A)),
                    b'*' => (Paint::Base(Base::T), Paint::OnBase(Base::T)),
                    _ => (Paint::Codon((position / 3).is_multiple_of(2)), Paint::Text),
                };
                let x = viewport.to_px(position as f64);
                primitives.push(Primitive::Rect {
                    x: x as f32,
                    y: top,
                    width: width as f32,
                    height: ROW_HEIGHT - 1.0,
                    paint,
                });
                if letters {
                    marks.push(Primitive::Text {
                        x: (x + width / 2.0) as f32,
                        y: top + (ROW_HEIGHT - 1.0) / 2.0,
                        content: Cow::Owned(char::from(amino).to_string()),
                        size: LETTER_SIZE,
                        paint: ink,
                        font: Font::MonoStrong,
                        align: Align::Center,
                    });
                }
            }
            position += 3;
        }
    }
    primitives.extend(marks);
    primitives
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

    use super::{Frame, scene};
    use crate::scene::{Base, Paint, Primitive};

    fn viewport(start: u64, end: u64, width: f64) -> Viewport {
        Viewport::new(1_000_000, width).show(Region::new(start, end).unwrap())
    }

    fn rects(primitives: &[Primitive]) -> Vec<([f32; 4], Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    paint,
                } => Some(([*x, *y, *width, *height], *paint)),
                _ => None,
            })
            .collect()
    }

    fn texts(primitives: &[Primitive]) -> String {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect()
    }

    const FORWARD: Frame = Frame {
        offset: 0,
        reverse: false,
    };

    #[test]
    fn frames_split_the_sequence_into_codons() {
        let primitives = scene(
            viewport(0, 10, 100.0),
            [(0, &b"ATGAAATAGC"[..])],
            &[FORWARD],
        );
        assert_eq!(
            rects(&primitives),
            [
                ([0.0, 0.0, 29.0, 13.0], Paint::Base(Base::A)),
                ([30.0, 0.0, 29.0, 13.0], Paint::Codon(false)),
                ([60.0, 0.0, 29.0, 13.0], Paint::Base(Base::T)),
            ]
        );
        assert_eq!(texts(&primitives), "MK*");
    }

    #[test]
    fn reverse_frames_read_the_other_strand() {
        let frame = Frame {
            offset: 0,
            reverse: true,
        };
        let primitives = scene(viewport(0, 9, 90.0), [(0, &b"CATTTTCTA"[..])], &[frame]);
        assert_eq!(texts(&primitives), "MK*");
    }

    #[test]
    fn frames_shift_and_stack() {
        let shifted = Frame {
            offset: 1,
            reverse: false,
        };
        let primitives = scene(
            viewport(0, 7, 70.0),
            [(0, &b"AATGAAA"[..])],
            &[FORWARD, shifted],
        );
        let second: Vec<f32> = rects(&primitives)
            .iter()
            .filter(|(rect, _)| rect[1] > 0.0)
            .map(|(rect, _)| rect[0])
            .collect();
        assert_eq!(second, [10.0, 40.0]);
        assert_eq!(texts(&primitives), "NEMK");
    }

    #[test]
    fn codons_cross_chunks_and_skip_missing_bases() {
        let chunks = [(0, &b"AT"[..]), (2, &b"GAA"[..])];
        let primitives = scene(viewport(0, 10, 100.0), chunks, &[FORWARD]);
        assert_eq!(texts(&primitives), "M");
    }

    #[test]
    fn letters_need_three_pixels_and_codons_one() {
        let bases = [b'A'; 300];
        let narrow = scene(viewport(0, 300, 600.0), [(0, &bases[..])], &[FORWARD]);
        assert_eq!(texts(&narrow), "");
        assert_eq!(rects(&narrow).len(), 100);
        let tiny = scene(viewport(0, 300, 200.0), [(0, &bases[..])], &[FORWARD]);
        assert_eq!(rects(&tiny).len(), 0);
    }
}
