use std::time::{Duration, Instant};

use super::animation::Curve;

const HOLD: Duration = Duration::from_millis(1750);
const FADE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Indicator {
    last_interaction: Option<Instant>,
    hovered: bool,
    opacity: f32,
    from: f32,
    target: f32,
    transition: Option<Instant>,
}

impl Indicator {
    pub fn show(&mut self, now: Instant) {
        self.last_interaction = Some(now);
        self.opacity = 1.0;
        self.target = 1.0;
        self.transition = None;
    }

    pub fn sample(
        &mut self,
        now: Instant,
        hovered: bool,
        animations: bool,
        curve: Curve,
    ) -> Option<(f32, Option<Instant>)> {
        if let Some(start) = self.transition {
            let t = now.saturating_duration_since(start).as_secs_f32() / FADE.as_secs_f32();
            self.opacity = self.from + (self.target - self.from) * curve.sample(t);
            if t >= 1.0 {
                self.transition = None;
            }
        }

        if hovered != self.hovered {
            self.hovered = hovered;
            self.last_interaction = None;
        }

        let hold = self
            .last_interaction
            .map(|last| last + HOLD)
            .filter(|until| now < *until);
        let target = f32::from(hovered || hold.is_some());
        if target != self.target {
            self.from = self.opacity;
            self.target = target;
            self.transition = Some(now);
        }

        if !animations {
            self.opacity = target;
            self.transition = None;
        }

        if self.opacity <= 0.0 && self.transition.is_none() {
            return None;
        }

        let next = if self.transition.is_some() {
            Some(now)
        } else if hovered {
            None
        } else {
            hold
        };
        Some((self.opacity, next))
    }
}

/// Wheel and keyboard gestures use the configured percentage-point step. Accumulate
/// fractional touchpad motion until it completes a step.
pub fn zoom(current: f32, steps: f64, remainder: &mut f64, step: u32) -> f32 {
    if !steps.is_finite() {
        return current;
    }
    if (current >= 2.0 && steps > 0.0) || (current <= 0.5 && steps < 0.0) {
        *remainder = 0.0;
        return current;
    }
    let accumulated = (*remainder + steps).clamp(-200.0, 200.0);
    let whole = (accumulated + accumulated.signum() * 1e-9).trunc();
    *remainder = accumulated - whole;
    if whole == 0.0 {
        return current;
    }
    let next = ((current as f64 * 100.0).round() + whole * step.clamp(1, 50) as f64)
        .clamp(50.0, 200.0)
        / 100.0;
    if next == 0.5 || next == 2.0 {
        *remainder = 0.0;
    }
    next as f32
}
