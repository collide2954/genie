use std::f32::consts::TAU;

use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme::tokens;

pub const GRID: f32 = 24.0;
const CORNER: f32 = 5.5;
const TOP: f32 = 4.0;
const BOTTOM: f32 = 20.0;
const CENTER: f32 = 12.0;
const AMPLITUDE: f32 = 5.0;
const TAIL: f32 = 0.15;
const STEPS: u16 = 52;
const STRAND: f32 = 1.75;
const RUNG: f32 = 1.0;
const RUNG_GAP: f32 = 2.25;
const RUNGS: [f32; 4] = [1.0 / 6.0, 1.0 / 3.0, 2.0 / 3.0, 5.0 / 6.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub square: Color,
    pub glyph: Color,
    pub rungs: bool,
}

#[cfg(test)]
pub const ICON: Look = Look {
    square: Color::from_rgb8(0x1F, 0x1F, 0x1F),
    glyph: Color::WHITE,
    rungs: true,
};

fn point(sign: f32, twist: f32) -> [f32; 2] {
    let along = (twist + TAIL) / (1.0 + 2.0 * TAIL);
    [
        CENTER + sign * AMPLITUDE * (TAU * twist).sin(),
        TOP + along * (BOTTOM - TOP),
    ]
}

pub fn strand(sign: f32) -> Vec<[f32; 2]> {
    (0..=STEPS)
        .map(|step| {
            let along = f32::from(step) / f32::from(STEPS);
            point(sign, -TAIL + along * (1.0 + 2.0 * TAIL))
        })
        .collect()
}

pub fn rungs() -> Vec<[[f32; 2]; 2]> {
    RUNGS
        .iter()
        .map(|&twist| {
            let ([a, y], [b, _]) = (point(1.0, twist), point(-1.0, twist));
            [[a.min(b) + RUNG_GAP, y], [a.max(b) - RUNG_GAP, y]]
        })
        .collect()
}

pub fn view<'a, Message: 'a>(size: f32, look: Option<Look>) -> Element<'a, Message> {
    canvas::Canvas::new(Program { size, look })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

struct Program {
    size: f32,
    look: Option<Look>,
}

impl<Message> canvas::Program<Message> for Program {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let look = self.look.unwrap_or_else(|| {
            let tokens = tokens(theme);
            Look {
                square: tokens.accent,
                glyph: tokens.canvas,
                rungs: false,
            }
        });
        let scale = self.size / GRID;
        let at = |[x, y]: [f32; 2]| Point::new(x * scale, y * scale);
        let stroke = |width: f32| {
            Stroke::default()
                .with_color(look.glyph)
                .with_width(width * scale)
                .with_line_cap(LineCap::Round)
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let square = Path::rounded_rectangle(
            Point::ORIGIN,
            Size::new(self.size, self.size),
            (CORNER * scale).into(),
        );
        frame.fill(&square, look.square);
        if look.rungs {
            for [from, to] in rungs() {
                frame.stroke(&Path::line(at(from), at(to)), stroke(RUNG));
            }
        }
        for sign in [1.0, -1.0] {
            let points = strand(sign);
            let path = Path::new(|builder| {
                if let Some((first, rest)) = points.split_first() {
                    builder.move_to(at(*first));
                    for next in rest {
                        builder.line_to(at(*next));
                    }
                }
            });
            frame.stroke(&path, stroke(STRAND));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use iced::Color;

    use super::{GRID, ICON, rungs, strand, view};

    #[test]
    fn strands_twist_with_open_ends_and_rungs_between_them() {
        let (left, right) = (strand(1.0), strand(-1.0));
        assert!(left.len() > 10, "{}", left.len());
        for [x, y] in left.iter().chain(&right) {
            assert!((3.0..=GRID - 3.0).contains(x), "{x}");
            assert!((3.0..=GRID - 3.0).contains(y), "{y}");
        }
        let gap = |at: usize| (left[at][0] - right[at][0]).abs();
        assert!(gap(left.len() / 2) < 0.01);
        assert!(gap(0) > 4.0 && gap(left.len() - 1) > 4.0);
        let crossings = (1..left.len())
            .filter(|&at| {
                (left[at - 1][0] - right[at - 1][0]).signum()
                    != (left[at][0] - right[at][0]).signum()
            })
            .count();
        assert!(crossings >= 2, "{crossings}");
        let widest = (0..left.len()).map(gap).fold(0.0, f32::max);
        assert!(widest > 8.0, "{widest}");
        let rungs = rungs();
        assert_eq!(rungs.len(), 4);
        assert!(rungs[1][0][1] - rungs[0][0][1] >= 1.9);
        for [from, to] in &rungs {
            assert!((from[1] - to[1]).abs() < 0.01);
            let at = left
                .iter()
                .position(|point| (point[1] - from[1]).abs() < 0.2)
                .unwrap();
            let (low, high) = (left[at][0].min(right[at][0]), left[at][0].max(right[at][0]));
            assert!(from[0] > low + 1.0 && to[0] < high - 1.0, "{from:?} {to:?}");
        }
    }

    #[test]
    fn the_icon_is_white_strands_on_a_dark_rounded_square() {
        let image = crate::export::picture(
            view::<()>(GRID, Some(ICON)),
            GRID,
            64.0 / GRID,
            Color::TRANSPARENT,
        )
        .unwrap();
        assert_eq!((image.width, image.height), (64, 64));
        let pixel = |x: usize, y: usize| {
            let at = (y * 64 + x) * 4;
            [
                image.rgba[at],
                image.rgba[at + 1],
                image.rgba[at + 2],
                image.rgba[at + 3],
            ]
        };
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(63, 63)[3], 0);
        let dark = pixel(6, 32);
        assert!(dark[3] == 255 && dark[0] < 0x30, "{dark:?}");
        let crossing = pixel(32, 32);
        assert!(crossing[3] == 255 && crossing[0] > 0xE0, "{crossing:?}");
    }
}
