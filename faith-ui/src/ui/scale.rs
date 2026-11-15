use std::time::{Duration, Instant};

use super::animation::Curve;

const HOLD: Duration = Duration::from_millis(1750);
const FADE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Indicator {
    last_interaction: Option<Instant>,
}

impl Indicator {
    pub fn show(&mut self, now: Instant) {
        self.last_interaction = Some(now);
    }

    /// Returns opacity and the next redraw deadline, sleeping during the hold.
    pub fn sample(
        &mut self,
        now: Instant,
        animations: bool,
        curve: Curve,
    ) -> Option<(f32, Instant)> {
        let fade_start = self.last_interaction? + HOLD;
        if now < fade_start {
            return Some((1.0, fade_start));
        }
        let elapsed = now.saturating_duration_since(fade_start);
        if !animations || elapsed >= FADE {
            self.last_interaction = None;
            return None;
        }
        let t = elapsed.as_secs_f32() / FADE.as_secs_f32();
        Some((1.0 - curve.sample(t), now))
    }
}

/// User zoom is relative to the desktop scale. Wheel and keyboard steps use
/// the same ratio, with fractional steps retained for continuous scrolling.
pub fn zoom(current: f32, steps: f64) -> f32 {
    if !steps.is_finite() {
        return current;
    }
    (current as f64 * 1.1_f64.powf(steps)).clamp(0.5, 2.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_holds_then_fades_and_stops() {
        let now = Instant::now();
        let mut indicator = Indicator::default();
        assert!(indicator.sample(now, true, Curve::Linear).is_none());
        indicator.show(now);
        assert_eq!(
            indicator.sample(now, true, Curve::Linear),
            Some((1.0, now + HOLD))
        );
        let middle = now + HOLD + FADE / 2;
        assert_eq!(
            indicator.sample(middle, true, Curve::Linear),
            Some((0.5, middle))
        );
        assert!(
            indicator
                .sample(now + HOLD + FADE, true, Curve::Linear)
                .is_none()
        );
        assert!(
            indicator
                .sample(now + HOLD + FADE, true, Curve::Linear)
                .is_none()
        );
    }

    #[test]
    fn interaction_during_fade_restores_visibility() {
        let now = Instant::now();
        let mut indicator = Indicator::default();
        indicator.show(now);
        let next = now + HOLD + FADE / 2;
        indicator.show(next);
        assert_eq!(
            indicator.sample(next, true, Curve::Linear),
            Some((1.0, next + HOLD))
        );
    }

    #[test]
    fn disabled_animations_hide_after_hold() {
        let now = Instant::now();
        let mut indicator = Indicator::default();
        indicator.show(now);
        assert!(indicator.sample(now, false, Curve::Linear).is_some());
        assert!(indicator.sample(now + HOLD, false, Curve::Linear).is_none());
    }

    #[test]
    fn zoom_steps_are_proportional_and_reversible() {
        assert!((zoom(1.0, 1.0) - 1.1).abs() < 0.00001);
        assert!((zoom(zoom(1.0, 1.0), -1.0) - 1.0).abs() < 0.00001);
        assert!((zoom(zoom(1.0, 0.5), 0.5) - zoom(1.0, 1.0)).abs() < 0.00001);
    }

    #[test]
    fn limits_remain_reachable_without_overshoot() {
        assert_eq!(zoom(1.0, 1_000_000.0), 2.0);
        assert_eq!(zoom(1.0, -1_000_000.0), 0.5);
        assert!(zoom(2.0, -1.0) < 2.0);
        assert!(zoom(0.5, 1.0) > 0.5);
    }

    #[test]
    fn invalid_gestures_do_not_corrupt_scale() {
        for steps in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(zoom(1.0, steps), 1.0);
        }
    }
}
