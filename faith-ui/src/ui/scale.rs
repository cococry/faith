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

/// Wheel and keyboard steps change zoom by ten percentage points. Accumulate
/// fractional touchpad motion until it completes a step.
pub fn zoom(current: f32, steps: f64, remainder: &mut f64) -> f32 {
    if !steps.is_finite() {
        return current;
    }
    if (current >= 2.0 && steps > 0.0) || (current <= 0.5 && steps < 0.0) {
        *remainder = 0.0;
        return current;
    }
    let accumulated = (*remainder + steps).clamp(-32.0, 32.0);
    let whole = (accumulated + accumulated.signum() * 1e-9).trunc();
    *remainder = accumulated - whole;
    if whole == 0.0 {
        return current;
    }
    let next = ((current as f64 * 10.0).round() + whole).clamp(5.0, 20.0) / 10.0;
    if next == 0.5 || next == 2.0 {
        *remainder = 0.0;
    }
    next as f32
}
