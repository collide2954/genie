use genie_core::{Band, Stain, Viewport};

use crate::scene::{Paint, Primitive};

const BAR_HEIGHT: f32 = 4.0;
const BAND_HEIGHT: f32 = 8.0;
const WAIST: f32 = 4.0;
const EDGE: f32 = 1.0;
const MIN_WINDOW: f32 = 4.0;
const LINE: f32 = 2.0;

pub fn scene(viewport: Viewport, height: f32, bands: &[Band]) -> Vec<Primitive> {
    let width = viewport.width();
    let bar = Primitive::Rect {
        x: 0.0,
        y: (height - BAR_HEIGHT) / 2.0,
        width: width as f32,
        height: BAR_HEIGHT,
        paint: Paint::Line,
    };
    if viewport.length() == 0 {
        return vec![bar];
    }
    let length = viewport.length() as f64;
    let place = |position: u64| (position as f64 / length * width) as f32;
    let mut primitives: Vec<Primitive> = if bands.len() > 1 {
        bands
            .iter()
            .map(|band| {
                let (ink, size) = ink(band.stain);
                let x = place(band.span.start());
                Primitive::Rect {
                    x,
                    y: (height - size) / 2.0,
                    width: place(band.span.end()) - x,
                    height: size,
                    paint: Paint::Stain(ink),
                }
            })
            .collect()
    } else {
        vec![bar]
    };
    let start = (viewport.start().clamp(0.0, length) / length * width) as f32;
    let end = (viewport.end().clamp(0.0, length) / length * width) as f32;
    let rect = |x: f32, width: f32, paint: Paint| Primitive::Rect {
        x,
        y: 0.0,
        width,
        height,
        paint,
    };
    let window = end - start;
    if window < MIN_WINDOW {
        primitives.push(rect(start, window.max(LINE), Paint::Accent));
    } else {
        primitives.extend([
            rect(start, window, Paint::Selection),
            rect(start, EDGE, Paint::Accent),
            rect(end - EDGE, EDGE, Paint::Accent),
        ]);
    }
    primitives
}

fn ink(stain: Stain) -> (u8, f32) {
    match stain {
        Stain::Gneg => (0, BAND_HEIGHT),
        Stain::Gpos(depth) => (depth, BAND_HEIGHT),
        Stain::Gvar => (60, BAND_HEIGHT),
        Stain::Acen => (40, WAIST),
        Stain::Stalk => (25, WAIST),
    }
}

#[cfg(test)]
mod tests {
    use genie_core::{Band, Region, Stain, Viewport};

    use super::scene;
    use crate::scene::{Paint, Primitive};

    fn rects(primitives: &[Primitive], wanted: Paint) -> Vec<(f32, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x, width, paint, ..
                } if *paint == wanted => Some((*x, *width)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn whole_contig_fills_the_strip() {
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0, &[]);
        assert_eq!(rects(&primitives, Paint::Selection), [(0.0, 100.0)]);
    }

    #[test]
    fn window_is_a_tint_with_edges() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::new(400, 500).unwrap());
        let primitives = scene(viewport, 12.0, &[]);
        assert_eq!(rects(&primitives, Paint::Selection), [(40.0, 10.0)]);
        assert_eq!(
            rects(&primitives, Paint::Accent),
            [(40.0, 1.0), (49.0, 1.0)]
        );
    }

    #[test]
    fn small_windows_become_a_line() {
        let viewport = Viewport::new(1_000_000, 100.0).show(Region::new(400, 401).unwrap());
        let primitives = scene(viewport, 12.0, &[]);
        assert_eq!(rects(&primitives, Paint::Selection), []);
        assert_eq!(rects(&primitives, Paint::Accent).len(), 1);
        assert_eq!(rects(&primitives, Paint::Accent)[0].1, 2.0);
    }

    #[test]
    fn empty_contigs_show_only_the_bar() {
        assert_eq!(scene(Viewport::new(0, 100.0), 12.0, &[]).len(), 1);
    }

    fn band(start: u64, end: u64, stain: Stain) -> Band {
        Band {
            span: Region::new(start, end).unwrap(),
            name: String::new(),
            stain,
        }
    }

    fn stains(primitives: &[Primitive]) -> Vec<(f32, f32, f32, u8)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    width,
                    height,
                    paint: Paint::Stain(ink),
                    ..
                } => Some((*x, *width, *height, *ink)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bands_are_grey_blocks_with_a_narrow_centromere() {
        let bands = [
            band(0, 400, Stain::Gneg),
            band(400, 500, Stain::Acen),
            band(500, 600, Stain::Acen),
            band(600, 900, Stain::Gpos(50)),
            band(900, 1_000, Stain::Gvar),
        ];
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0, &bands);
        assert_eq!(
            stains(&primitives),
            [
                (0.0, 40.0, 8.0, 0),
                (40.0, 10.0, 4.0, 40),
                (50.0, 10.0, 4.0, 40),
                (60.0, 30.0, 8.0, 50),
                (90.0, 10.0, 8.0, 60),
            ]
        );
        assert_eq!(rects(&primitives, Paint::Line), []);
        assert_eq!(rects(&primitives, Paint::Selection), [(0.0, 100.0)]);
    }

    #[test]
    fn one_band_is_only_an_outline_and_draws_the_plain_bar() {
        let bands = [band(0, 1_000, Stain::Gneg)];
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0, &bands);
        assert_eq!(stains(&primitives), []);
        assert_eq!(rects(&primitives, Paint::Line), [(0.0, 100.0)]);
    }

    #[test]
    fn strip_shows_the_contig_bar() {
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0, &[]);
        assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            Primitive::Rect {
                paint: Paint::Line,
                ..
            }
        )));
    }
}
