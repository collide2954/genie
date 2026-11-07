use genie_core::Viewport;

use crate::scene::{Paint, Primitive};

const BAR_HEIGHT: f32 = 4.0;
const EDGE: f32 = 1.0;
const MIN_WINDOW: f32 = 4.0;
const LINE: f32 = 2.0;

pub fn scene(viewport: Viewport, height: f32) -> Vec<Primitive> {
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
        return vec![bar, rect(start, window.max(LINE), Paint::Accent)];
    }
    vec![
        bar,
        rect(start, window, Paint::Selection),
        rect(start, EDGE, Paint::Accent),
        rect(end - EDGE, EDGE, Paint::Accent),
    ]
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

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
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0);
        assert_eq!(rects(&primitives, Paint::Selection), [(0.0, 100.0)]);
    }

    #[test]
    fn window_is_a_tint_with_edges() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::new(400, 500).unwrap());
        let primitives = scene(viewport, 12.0);
        assert_eq!(rects(&primitives, Paint::Selection), [(40.0, 10.0)]);
        assert_eq!(
            rects(&primitives, Paint::Accent),
            [(40.0, 1.0), (49.0, 1.0)]
        );
    }

    #[test]
    fn small_windows_become_a_line() {
        let viewport = Viewport::new(1_000_000, 100.0).show(Region::new(400, 401).unwrap());
        let primitives = scene(viewport, 12.0);
        assert_eq!(rects(&primitives, Paint::Selection), []);
        assert_eq!(rects(&primitives, Paint::Accent).len(), 1);
        assert_eq!(rects(&primitives, Paint::Accent)[0].1, 2.0);
    }

    #[test]
    fn empty_contigs_show_only_the_bar() {
        assert_eq!(scene(Viewport::new(0, 100.0), 12.0).len(), 1);
    }

    #[test]
    fn strip_shows_the_contig_bar() {
        let primitives = scene(Viewport::new(1_000, 100.0), 12.0);
        assert!(primitives.iter().any(|primitive| matches!(
            primitive,
            Primitive::Rect {
                paint: Paint::Line,
                ..
            }
        )));
    }
}
