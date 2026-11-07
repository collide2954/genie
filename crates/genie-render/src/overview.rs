use genie_core::Viewport;

use crate::scene::{Paint, Primitive};

const BAR_HEIGHT: f32 = 4.0;
const MIN_MARK: f32 = 2.0;

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
    let start = viewport.start().clamp(0.0, length) / length * width;
    let end = viewport.end().clamp(0.0, length) / length * width;
    let mark = Primitive::Rect {
        x: start as f32,
        y: 0.0,
        width: ((end - start) as f32).max(MIN_MARK),
        height,
        paint: Paint::Accent,
    };
    vec![bar, mark]
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};

    use super::scene;
    use crate::scene::{Paint, Primitive};

    fn mark(primitives: &[Primitive]) -> (f32, f32) {
        primitives
            .iter()
            .find_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    width,
                    paint: Paint::Accent,
                    ..
                } => Some((*x, *width)),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn whole_contig_fills_the_strip() {
        assert_eq!(
            mark(&scene(Viewport::new(1_000, 100.0), 12.0)),
            (0.0, 100.0)
        );
    }

    #[test]
    fn window_sits_at_its_share_of_the_contig() {
        let viewport = Viewport::new(1_000, 100.0).show(Region::new(400, 500).unwrap());
        assert_eq!(mark(&scene(viewport, 12.0)), (40.0, 10.0));
    }

    #[test]
    fn small_windows_stay_visible() {
        let viewport = Viewport::new(1_000_000, 100.0).show(Region::new(400, 401).unwrap());
        assert_eq!(mark(&scene(viewport, 12.0)).1, 2.0);
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
