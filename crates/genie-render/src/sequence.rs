use std::borrow::Cow;

use genie_core::Viewport;

use crate::scene::{Align, Base, Font, Paint, Primitive};

pub const HINT: &str = "Zoom in to see the sequence";

const GAPS_FROM: f64 = 6.0;
const LETTERS_FROM: f64 = 9.0;
const LETTER_SIZE: f32 = 12.0;
const HINT_SIZE: f32 = 12.0;

pub fn scene<'a>(
    viewport: Viewport,
    chunks: impl IntoIterator<Item = (u64, &'a [u8])>,
    height: f32,
) -> Vec<Primitive> {
    let pixels = 1.0 / viewport.scale();
    if pixels < 1.0 {
        return vec![Primitive::Text {
            x: (viewport.width() / 2.0) as f32,
            y: height / 2.0,
            content: Cow::Borrowed(HINT),
            size: HINT_SIZE,
            paint: Paint::Muted,
            font: Font::Ui,
            align: Align::Center,
        }];
    }
    let visible = viewport.visible();
    let width = if pixels >= GAPS_FROM {
        pixels - 1.0
    } else {
        pixels
    };
    let letters = pixels >= LETTERS_FROM;
    let mut primitives = Vec::new();
    for (start, bases) in chunks {
        let first = visible.start().max(start);
        let last = visible.end().min(start + bases.len() as u64);
        for position in first..last {
            let byte = bases[(position - start) as usize];
            let base = Base::from_byte(byte);
            let x = viewport.to_px(position as f64);
            primitives.push(Primitive::Rect {
                x: x as f32,
                y: 0.0,
                width: width as f32,
                height,
                paint: Paint::Base(base),
            });
            if letters {
                primitives.push(Primitive::Text {
                    x: (x + width / 2.0) as f32,
                    y: height / 2.0,
                    content: letter(byte),
                    size: LETTER_SIZE,
                    paint: Paint::OnBase(base),
                    font: Font::MonoStrong,
                    align: Align::Center,
                });
            }
        }
    }
    primitives
}

fn letter(byte: u8) -> Cow<'static, str> {
    Cow::Borrowed(match byte {
        b'A' => "A",
        b'C' => "C",
        b'G' => "G",
        b'T' => "T",
        b'N' => "N",
        b'a' => "a",
        b'c' => "c",
        b'g' => "g",
        b't' => "t",
        b'n' => "n",
        other => return Cow::Owned(char::from(other).to_string()),
    })
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

    use super::{HINT, scene};
    use crate::scene::{Base, Paint, Primitive};

    fn viewport(start: u64, end: u64, width: f64) -> Viewport {
        Viewport::new(1_000_000, width).show(Region::new(start, end).unwrap())
    }

    fn rects(primitives: &[Primitive]) -> Vec<(f32, f32, Paint)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x, width, paint, ..
                } => Some((*x, *width, *paint)),
                _ => None,
            })
            .collect()
    }

    fn texts(primitives: &[Primitive]) -> Vec<String> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, .. } => Some(content.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hints_when_bases_are_under_one_pixel() {
        let primitives = scene(Viewport::new(1_000_000, 1_000.0), [(0, &b"ACGT"[..])], 20.0);
        assert_eq!(texts(&primitives), [HINT]);
        assert_eq!(rects(&primitives), []);
    }

    #[test]
    fn draws_plain_bars_below_six_pixels() {
        let primitives = scene(viewport(0, 200, 1_000.0), [(0, &[b'A'; 200][..])], 20.0);
        let rects = rects(&primitives);
        assert_eq!(rects.len(), 200);
        assert_eq!(rects[1], (5.0, 5.0, Paint::Base(Base::A)));
        assert_eq!(texts(&primitives), [] as [String; 0]);
    }

    #[test]
    fn separates_bases_from_six_pixels() {
        let primitives = scene(viewport(0, 100, 700.0), [(0, &[b'C'; 100][..])], 20.0);
        assert_eq!(rects(&primitives)[0], (0.0, 6.0, Paint::Base(Base::C)));
    }

    #[test]
    fn writes_letters_from_nine_pixels_and_keeps_case() {
        let primitives = scene(viewport(0, 10, 100.0), [(0, &b"ACGTNacgtn"[..])], 20.0);
        assert_eq!(rects(&primitives).len(), 10);
        assert_eq!(texts(&primitives).concat(), "ACGTNacgtn");
        assert_eq!(rects(&primitives)[6], (60.0, 9.0, Paint::Base(Base::C)));
    }

    #[test]
    fn draws_only_loaded_chunks() {
        let chunks = [(1, &b"G"[..]), (4, &b"GG"[..])];
        let primitives = scene(viewport(0, 10, 100.0), chunks, 20.0);
        let starts: Vec<f32> = rects(&primitives).iter().map(|rect| rect.0).collect();
        assert_eq!(starts, [10.0, 40.0, 50.0]);
    }
}
