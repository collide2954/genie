use genie_core::Viewport;
use iced::Animation;
use iced::animation::Easing;
use iced::time::{Duration, Instant};

pub const DURATION: Duration = Duration::from_millis(150);

pub fn slide(open: bool) -> Animation<bool> {
    Animation::new(open)
        .duration(DURATION)
        .easing(Easing::EaseOutCubic)
}

#[derive(Debug, Clone)]
pub struct Transition {
    from: Viewport,
    to: Viewport,
    progress: Animation<bool>,
}

impl Transition {
    pub fn new(from: Viewport, to: Viewport, now: Instant) -> Self {
        Self {
            from,
            to,
            progress: Animation::new(false)
                .duration(DURATION)
                .easing(Easing::EaseOutCubic)
                .go(true, now),
        }
    }

    pub fn at(&self, now: Instant) -> Viewport {
        let progress = self.progress.interpolate(0.0_f32, 1.0, now);
        self.from.lerp(self.to, f64::from(progress))
    }

    pub fn is_done(&self, now: Instant) -> bool {
        !self.progress.is_animating(now)
    }

    pub fn target(&self) -> Viewport {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use genie_core::{Region, Viewport};
    use iced::time::{Duration, Instant};

    use super::{DURATION, Transition};

    fn views() -> (Viewport, Viewport) {
        let from = Viewport::new(1_000, 100.0).show(Region::new(400, 500).unwrap());
        (from, from.pan(-200.0))
    }

    #[test]
    fn starts_at_the_origin_and_ends_at_the_target() {
        let (from, to) = views();
        let now = Instant::now();
        let transition = Transition::new(from, to, now);
        assert_eq!(transition.at(now), from);
        assert_eq!(transition.at(now + DURATION * 2), to);
        assert_eq!(transition.target(), to);
    }

    #[test]
    fn finishes_after_its_duration() {
        let (from, to) = views();
        let now = Instant::now();
        let transition = Transition::new(from, to, now);
        assert!(!transition.is_done(now + Duration::from_millis(1)));
        assert!(transition.is_done(now + DURATION * 2));
    }

    #[test]
    fn eases_out() {
        let (from, to) = views();
        let now = Instant::now();
        let middle = Transition::new(from, to, now).at(now + DURATION / 2);
        let halfway = f64::midpoint(from.center(), to.center());
        assert!(middle.center() > halfway);
    }
}
