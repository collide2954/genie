use crate::{Region, Span};

pub const MAX_PIXELS_PER_BASE: f64 = 32.0;

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    length: u64,
    width: f64,
    start: f64,
    scale: f64,
}

impl Viewport {
    pub fn new(length: u64, width: f64) -> Self {
        Self {
            length,
            width: width.max(1.0),
            start: 0.0,
            scale: f64::INFINITY,
        }
        .clamped()
    }

    pub fn length(self) -> u64 {
        self.length
    }

    pub fn width(self) -> f64 {
        self.width
    }

    pub fn start(self) -> f64 {
        self.start
    }

    pub fn end(self) -> f64 {
        self.start + self.span()
    }

    pub fn scale(self) -> f64 {
        self.scale
    }

    pub fn span(self) -> f64 {
        self.width * self.scale
    }

    pub fn to_px(self, bp: f64) -> f64 {
        (bp - self.start) / self.scale
    }

    pub fn to_bp(self, px: f64) -> f64 {
        self.start + px * self.scale
    }

    pub fn visible(self) -> Region {
        let start = self.start.floor().max(0.0) as u64;
        let end = self.end().ceil().max(0.0) as u64;
        Region::between(start.min(self.length), end.min(self.length))
    }

    pub fn show(self, region: Region) -> Self {
        let length = region.len() as f64;
        let center = region.start() as f64 + length / 2.0;
        Self {
            scale: self.limit(length / self.width),
            ..self
        }
        .center(center)
    }

    pub fn center(self, position: f64) -> Self {
        Self {
            start: position - self.span() / 2.0,
            ..self
        }
        .clamped()
    }

    pub fn pan(self, dx: f64) -> Self {
        Self {
            start: self.start - dx * self.scale,
            ..self
        }
        .clamped()
    }

    pub fn zoom(self, factor: f64, anchor: f64) -> Self {
        let position = self.to_bp(anchor);
        let scale = self.limit(self.scale / factor);
        Self {
            start: position - anchor * scale,
            scale,
            ..self
        }
        .clamped()
    }

    pub fn jump(self, span: Span) -> Self {
        match span {
            Span::Whole => Self::new(self.length, self.width),
            Span::Position(position) => self.center(position as f64 + 0.5),
            Span::Region(region) => self.show(region),
        }
    }

    pub fn retarget(self, length: u64) -> Self {
        Self {
            length,
            start: 0.0,
            ..self
        }
        .clamped()
    }

    pub fn resize(self, width: f64) -> Self {
        Self {
            width: width.max(1.0),
            ..self
        }
        .clamped()
    }

    fn limit(self, scale: f64) -> f64 {
        let min = 1.0 / MAX_PIXELS_PER_BASE;
        scale.clamp(min, (self.length as f64 / self.width).max(min))
    }

    fn clamped(self) -> Self {
        let scale = self.limit(self.scale);
        let span = self.width * scale;
        let length = self.length as f64;
        let start = if span >= length {
            (length - span) / 2.0
        } else {
            self.start.clamp(0.0, length - span)
        };
        Self {
            start,
            scale,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PIXELS_PER_BASE, Viewport};
    use crate::{Region, Span};

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} is not {expected}"
        );
    }

    fn zoomed_in() -> Viewport {
        Viewport::new(1_000, 100.0).show(region(400, 500))
    }

    #[test]
    fn starts_with_whole_contig() {
        let viewport = Viewport::new(1_000, 100.0);
        assert_close(viewport.start(), 0.0);
        assert_close(viewport.scale(), 10.0);
        assert_eq!(viewport.visible(), region(0, 1_000));
    }

    #[test]
    fn converts_between_bases_and_pixels() {
        let viewport = Viewport::new(1_000, 100.0);
        assert_close(viewport.to_px(500.0), 50.0);
        assert_close(viewport.to_bp(50.0), 500.0);
    }

    #[test]
    fn show_fits_region() {
        let viewport = zoomed_in();
        assert_close(viewport.start(), 400.0);
        assert_close(viewport.scale(), 1.0);
    }

    #[test]
    fn show_centers_regions_smaller_than_max_zoom() {
        let viewport = Viewport::new(1_000, 100.0).show(region(400, 401));
        assert_close(viewport.scale(), 1.0 / MAX_PIXELS_PER_BASE);
        assert_close(viewport.to_px(400.5), 50.0);
    }

    #[test]
    fn pan_moves_content_with_the_pointer() {
        assert_close(zoomed_in().pan(10.0).start(), 390.0);
        assert_close(zoomed_in().pan(-10.0).start(), 410.0);
    }

    #[test]
    fn pan_stops_at_contig_ends() {
        assert_close(zoomed_in().pan(1e9).start(), 0.0);
        assert_close(zoomed_in().pan(-1e9).start(), 900.0);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let viewport = zoomed_in().zoom(2.0, 50.0);
        assert_close(viewport.scale(), 0.5);
        assert_close(viewport.to_bp(50.0), 450.0);
    }

    #[test]
    fn zoom_stops_at_limits() {
        assert_close(
            zoomed_in().zoom(1e9, 50.0).scale(),
            1.0 / MAX_PIXELS_PER_BASE,
        );
        let out = zoomed_in().zoom(1e-9, 50.0);
        assert_close(out.scale(), 10.0);
        assert_close(out.start(), 0.0);
    }

    #[test]
    fn center_keeps_scale() {
        assert_close(zoomed_in().center(800.0).start(), 750.0);
        assert_close(zoomed_in().center(990.0).start(), 900.0);
    }

    #[test]
    fn resize_keeps_start_and_scale() {
        let wider = zoomed_in().resize(200.0);
        assert_close(wider.start(), 400.0);
        assert_eq!(wider.visible(), region(400, 600));
        let widest = zoomed_in().resize(2_000.0);
        assert_close(widest.scale(), 0.5);
        assert_eq!(widest.visible(), region(0, 1_000));
    }

    #[test]
    fn visible_rounds_outward() {
        let viewport = Viewport::new(1_000, 100.0).show(region(400, 500)).pan(-0.5);
        assert_eq!(viewport.visible(), region(400, 501));
    }

    #[test]
    fn jump_to_whole_contig_shows_everything() {
        let viewport = zoomed_in().jump(Span::Whole);
        assert_close(viewport.start(), 0.0);
        assert_close(viewport.scale(), 10.0);
    }

    #[test]
    fn jump_to_position_centers_its_base_at_current_zoom() {
        let viewport = zoomed_in().jump(Span::Position(800));
        assert_close(viewport.scale(), 1.0);
        assert_close(viewport.to_bp(50.0), 800.5);
    }

    #[test]
    fn jump_to_region_shows_it() {
        let viewport = Viewport::new(1_000, 100.0).jump(Span::Region(region(400, 500)));
        assert_close(viewport.start(), 400.0);
        assert_close(viewport.scale(), 1.0);
    }

    #[test]
    fn retarget_keeps_width_and_zoom() {
        let viewport = zoomed_in().retarget(2_000);
        assert_close(viewport.scale(), 1.0);
        assert_close(viewport.start(), 0.0);
        assert_eq!(viewport.length(), 2_000);
    }

    #[test]
    fn retarget_limits_zoom_to_the_new_contig() {
        let viewport = Viewport::new(1_000, 100.0).retarget(500);
        assert_close(viewport.scale(), 5.0);
    }

    #[test]
    fn tiny_contigs_are_centered() {
        let viewport = Viewport::new(2, 100.0);
        assert_close(viewport.scale(), 1.0 / MAX_PIXELS_PER_BASE);
        assert_close(viewport.to_px(1.0), 50.0);
        assert_eq!(viewport.visible(), region(0, 2));
    }
}
