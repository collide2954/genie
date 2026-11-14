use std::f32::consts::TAU;

use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme::{DARK_DATA, LIGHT_DATA, tokens};

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
const BASES: f32 = 6.0;

pub const GREEN_ON_DARK: Color = DARK_DATA.bases[0];
pub const PURPLE_ON_DARK: Color = DARK_DATA.insertion;
pub const GREEN_ON_LIGHT: Color = LIGHT_DATA.bases[0];
pub const PURPLE_ON_LIGHT: Color = LIGHT_DATA.insertion;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub square: Color,
    pub glyph: Color,
    pub bases: [Color; 2],
    pub rungs: bool,
}

#[cfg(test)]
pub const ICON: Look = Look {
    square: Color::from_rgb8(0x1F, 0x1F, 0x1F),
    glyph: Color::WHITE,
    bases: [GREEN_ON_DARK, PURPLE_ON_DARK],
    rungs: true,
};

pub fn themed(theme: &Theme) -> Look {
    let tokens = tokens(theme);
    let bases = if theme.extended_palette().is_dark {
        [GREEN_ON_LIGHT, PURPLE_ON_LIGHT]
    } else {
        [GREEN_ON_DARK, PURPLE_ON_DARK]
    };
    Look {
        square: tokens.accent,
        glyph: tokens.canvas,
        bases,
        rungs: false,
    }
}

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

pub fn bases(sign: f32) -> Vec<(usize, Vec<[f32; 2]>)> {
    let mut parts: Vec<(usize, Vec<[f32; 2]>)> = Vec::new();
    for step in 0..=STEPS {
        let along = f32::from(step) / f32::from(STEPS);
        let twist = -TAIL + along * (1.0 + 2.0 * TAIL);
        let base = ((twist * BASES).floor() as i32).rem_euclid(2) as usize;
        let at = point(sign, twist);
        match parts.last_mut() {
            Some((current, points)) if *current == base => points.push(at),
            last => {
                if let Some((_, points)) = last {
                    points.push(at);
                }
                parts.push((base, vec![at]));
            }
        }
    }
    parts
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
        let look = self.look.unwrap_or_else(|| themed(theme));
        let scale = self.size / GRID;
        let at = |[x, y]: [f32; 2]| Point::new(x * scale, y * scale);
        let stroke = |width: f32| {
            Stroke::default()
                .with_color(look.glyph)
                .with_width(width * scale)
                .with_line_cap(LineCap::Round)
        };
        let line = |points: &[[f32; 2]]| {
            Path::new(|builder| {
                if let Some((first, rest)) = points.split_first() {
                    builder.move_to(at(*first));
                    for next in rest {
                        builder.line_to(at(*next));
                    }
                }
            })
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
        let parts = bases(-1.0);
        let ends = [parts.first(), parts.last()];
        for (base, points) in ends.into_iter().flatten() {
            frame.stroke(&line(points), stroke(STRAND).with_color(look.bases[*base]));
        }
        for (base, points) in &parts {
            let flat = stroke(STRAND)
                .with_color(look.bases[*base])
                .with_line_cap(LineCap::Butt);
            frame.stroke(&line(points), flat);
        }
        frame.stroke(&line(&strand(1.0)), stroke(STRAND));
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use genie_formats::{icon, png};
    use iced::Color;

    use super::{
        GREEN_ON_DARK, GREEN_ON_LIGHT, GRID, ICON, Look, PURPLE_ON_DARK, PURPLE_ON_LIGHT, rungs,
        strand, themed, view,
    };

    const PNGS: [u32; 4] = [32, 128, 256, 512];
    const ICO: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    const ICNS: [(&[u8; 4], u32); 11] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic13", 256),
        (b"ic14", 512),
    ];
    const WINDOW: u32 = 64;
    const MAC_MARGIN: f32 = 100.0 / 1024.0;

    fn folder() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icon")
    }

    fn rgba(size: u32, margin: f32) -> Vec<u8> {
        let look = if size >= 32 {
            ICON
        } else {
            Look {
                rungs: false,
                ..ICON
            }
        };
        let scale = size as f32 / GRID;
        let inset = GRID * margin;
        let mark =
            iced::widget::container(view::<()>(GRID - 2.0 * inset, Some(look))).padding(inset);
        let image = crate::export::picture(mark.into(), GRID, scale, Color::TRANSPARENT).unwrap();
        assert_eq!((image.width, image.height), (size, size));
        image.rgba
    }

    fn encoded(size: u32, margin: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        png::write(&mut bytes, size, size, &rgba(size, margin)).unwrap();
        bytes
    }

    #[test]
    #[ignore = "run to write the icon files"]
    fn writes_the_icon_files() {
        let folder = folder();
        fs::create_dir_all(&folder).unwrap();
        for size in PNGS {
            fs::write(
                folder.join(format!("{size}x{size}.png")),
                encoded(size, 0.0),
            )
            .unwrap();
        }
        let images: Vec<(u32, Vec<u8>)> =
            ICO.iter().map(|&size| (size, encoded(size, 0.0))).collect();
        let entries: Vec<(u32, &[u8])> = images
            .iter()
            .map(|(size, bytes)| (*size, bytes.as_slice()))
            .collect();
        let ico = icon::ico(&entries);
        fs::write(folder.join("icon.res"), icon::res(&ico).unwrap()).unwrap();
        fs::write(folder.join("icon.ico"), ico).unwrap();
        let images: Vec<([u8; 4], Vec<u8>)> = ICNS
            .iter()
            .map(|(kind, size)| (**kind, encoded(*size, MAC_MARGIN)))
            .collect();
        let entries: Vec<([u8; 4], &[u8])> = images
            .iter()
            .map(|(kind, bytes)| (*kind, bytes.as_slice()))
            .collect();
        fs::write(folder.join("icon.icns"), icon::icns(&entries)).unwrap();
        fs::write(folder.join("window.rgba"), rgba(WINDOW, 0.0)).unwrap();
    }

    #[test]
    fn icon_files_match_the_mark() {
        let folder = folder();
        for size in PNGS {
            let bytes = fs::read(folder.join(format!("{size}x{size}.png"))).unwrap();
            let side = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
            assert_eq!((side(16), side(20)), (size, size));
        }
        let ico = fs::read(folder.join("icon.ico")).unwrap();
        assert_eq!(ico[..6], [0, 0, 1, 0, ICO.len() as u8, 0]);
        let res = fs::read(folder.join("icon.res")).unwrap();
        assert_eq!(Some(res), icon::res(&ico));
        let icns = fs::read(folder.join("icon.icns")).unwrap();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
            icns.len()
        );
        let stored = fs::read(folder.join("window.rgba")).unwrap();
        let fresh = rgba(WINDOW, 0.0);
        assert_eq!(stored.len(), fresh.len());
        let worst = stored
            .iter()
            .zip(&fresh)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 16, "{worst}");
    }

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
    fn the_icon_has_a_white_strand_and_a_green_and_purple_strand() {
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
        let near = |found: [u8; 4], want: Color| {
            let want = want.into_rgba8();
            (0..3).all(|channel| found[channel].abs_diff(want[channel]) < 40)
        };
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(63, 63)[3], 0);
        let dark = pixel(6, 32);
        assert!(dark[3] == 255 && dark[0] < 0x30, "{dark:?}");
        let crossing = pixel(32, 32);
        assert!(crossing[3] == 255 && crossing[0] > 0xE0, "{crossing:?}");
        let green = pixel(25, 18);
        assert!(near(green, ICON.bases[0]), "{green:?}");
        let purple = pixel(18, 23);
        assert!(near(purple, ICON.bases[1]), "{purple:?}");
    }

    #[test]
    fn the_logo_bases_stand_out_on_the_square_of_each_theme() {
        let light = themed(&crate::theme::light());
        let dark = themed(&crate::theme::dark());
        assert_eq!(light.square, crate::theme::LIGHT.accent);
        assert_eq!(dark.square, crate::theme::DARK.accent);
        assert_eq!(light.bases, [GREEN_ON_DARK, PURPLE_ON_DARK]);
        assert_eq!(dark.bases, [GREEN_ON_LIGHT, PURPLE_ON_LIGHT]);
    }
}
