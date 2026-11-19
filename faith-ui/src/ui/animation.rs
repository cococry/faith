use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::framework::Style;
use crate::{graphics::Color, platform::window::PlatformWaker};

pub const DURATION: Duration = Duration::from_millis(200);
pub const MORPH_DURATION: Duration = Duration::from_millis(100);
pub const PRESENCE_DURATION: Duration = Duration::from_millis(90);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    Linear,
    #[default]
    Quadratic,
    Cubic,
    Smoothstep,
}

impl Curve {
    pub const ALL: [Self; 4] = [Self::Linear, Self::Quadratic, Self::Cubic, Self::Smoothstep];
    pub const LABELS: [&str; 4] = [
        "Linear",
        "Quadratic ease out",
        "Cubic ease out",
        "Smoothstep",
    ];

    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        match self {
            Self::Linear => t,
            Self::Quadratic => 1.0 - (1.0 - t).powi(2),
            Self::Cubic => 1.0 - (1.0 - t).powi(3),
            Self::Smoothstep => t * t * (3.0 - 2.0 * t),
        }
    }
}

/* sliders and container scrollbars share the same hover transition */
#[derive(Clone, Copy, Default)]
pub struct Hover {
    value: f32,
    hovered: bool,
    from: f32,
    start: Option<Instant>,
}

impl Hover {
    pub fn update(
        &mut self,
        hovered: bool,
        now: Instant,
        enabled: bool,
        curve: Curve,
    ) -> (f32, bool) {
        if let Some(start) = self.start {
            let t = now.saturating_duration_since(start).as_secs_f32() / DURATION.as_secs_f32();
            let target = f32::from(self.hovered);
            self.value = self.from + (target - self.from) * curve.sample(t);
            if t >= 1.0 {
                self.value = target;
                self.start = None;
            }
        }

        if hovered != self.hovered {
            self.from = self.value;
            self.hovered = hovered;
            self.start = Some(now);
        }

        if !enabled {
            self.value = f32::from(hovered);
            self.start = None;
        }

        (self.value, self.start.is_some())
    }
}

/* we sleep until the next requested frame, and stay asleep when nothing moves */
pub struct Scheduler {
    sender: Option<Sender<Option<Instant>>>,
    worker: Option<JoinHandle<()>>,
    waker: PlatformWaker,
    pending: bool,
}

impl Scheduler {
    pub fn new(waker: PlatformWaker) -> anyhow::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Option<Instant>>();
        let wake = waker.clone();
        let worker = thread::Builder::new()
            .name("ui-animation".into())
            .spawn(move || {
                let mut deadline: Option<Instant> = None;

                loop {
                    let command = match deadline {
                        Some(at) => match receiver
                            .recv_timeout(at.saturating_duration_since(Instant::now()))
                        {
                            Ok(command) => command,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                let _ = wake.request_redraw();
                                deadline = None;
                                continue;
                            }

                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                        None => match receiver.recv() {
                            Ok(command) => command,
                            Err(_) => break,
                        },
                    };

                    deadline = command;
                }
            })?;

        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            waker,
            pending: false,
        })
    }

    pub fn schedule(&mut self, deadline: Option<Instant>) {
        if deadline.is_some() || self.pending {
            let _ = self.sender.as_ref().unwrap().send(deadline);
        }

        self.pending = deadline.is_some();
    }

    pub fn next_frame(&self, now: Instant) -> Instant {
        now + self.waker.frame_interval()
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.sender.take();

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn optional_color(a: Option<Color>, b: Option<Color>, t: f32) -> Option<Color> {
    let transparent = |c: Color| Color { a: 0.0, ..c };

    match (a, b) {
        (Some(a), Some(b)) => Some(color(a, b, t)),
        (Some(a), None) => Some(color(a, transparent(a), t)),
        (None, Some(b)) => Some(color(transparent(b), b, t)),
        (None, None) => None,
    }
}

pub struct Transition {
    from: Style,
    target: Style,
    start: Instant,
    pub seen: bool,
}

impl Transition {
    pub fn new(style: Style, now: Instant) -> Self {
        Self {
            from: style,
            target: style,
            start: now - DURATION,
            seen: true,
        }
    }

    fn value(&self, now: Instant, curve: Curve) -> Style {
        let t = curve.sample(now.duration_since(self.start).as_secs_f32() / DURATION.as_secs_f32());

        if t >= 1.0 {
            return self.target;
        }

        Style {
            color: color(self.from.color, self.target.color, t),
            background: optional_color(self.from.background, self.target.background, t),
            border_color: optional_color(self.from.border_color, self.target.border_color, t),
            radius: self.from.radius + (self.target.radius - self.from.radius) * t,
            ..self.target
        }
    }

    pub fn update(
        &mut self,
        target: Style,
        now: Instant,
        curve: Curve,
        enabled: bool,
    ) -> (Style, bool) {
        self.seen = true;

        if self.target.color != target.color
            || self.target.background != target.background
            || self.target.border_color != target.border_color
            || self.target.radius != target.radius
        {
            self.from = self.value(now, curve);
            self.start = now;
        }

        self.target = target;

        if !enabled {
            self.from = target;
            self.target = target;
            self.start = now - DURATION;
        }

        (
            self.value(now, curve),
            now.duration_since(self.start) < DURATION,
        )
    }
}

/* we retarget from the current frame, so reversing a layout never jumps */
pub struct Motion<const N: usize> {
    from: [f32; N],
    target: [f32; N],
    start: Instant,
    pub seen: bool,
}

impl<const N: usize> Motion<N> {
    pub fn new(value: [f32; N], now: Instant) -> Self {
        Self {
            from: value,
            target: value,
            start: now - DURATION,
            seen: true,
        }
    }

    fn value(&self, now: Instant, curve: Curve, duration: Duration) -> [f32; N] {
        let t = curve.sample(
            now.saturating_duration_since(self.start).as_secs_f32() / duration.as_secs_f32(),
        );

        if t >= 1.0 {
            return self.target;
        }

        std::array::from_fn(|i| self.from[i] + (self.target[i] - self.from[i]) * t)
    }

    pub fn update(
        &mut self,
        target: [f32; N],
        now: Instant,
        curve: Curve,
        enabled: bool,
        duration: Duration,
    ) -> ([f32; N], bool) {
        self.seen = true;

        if self.target != target {
            self.from = self.value(now, curve, duration);
            self.target = target;
            self.start = now;
        }

        if !enabled {
            self.from = target;
            self.start = now - duration;
        }

        let active =
            self.from != self.target && now.saturating_duration_since(self.start) < duration;
        (self.value(now, curve, duration), active)
    }
}

/* presence lasts just long enough to soften an overlay without delaying its action */
#[derive(Default)]
pub struct Presence {
    motion: Option<Motion<1>>,
    value: f32,
}

impl Presence {
    pub fn visible(&self) -> bool {
        self.value > 0.0
    }

    pub fn update(&mut self, open: bool, now: Instant, enabled: bool) -> (f32, bool) {
        let motion = self.motion.get_or_insert_with(|| Motion::new([0.0], now));
        let ([value], active) = motion.update(
            [f32::from(open)],
            now,
            Curve::Cubic,
            enabled,
            PRESENCE_DURATION,
        );
        self.value = value;
        (value, active)
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn reversing_layout_preserves_the_current_position() {
        let now = Instant::now();
        let mut motion = Motion::new([0.0, 40.0], now);
        motion.update([100.0, 80.0], now, Curve::Linear, true, DURATION);
        let halfway = now + DURATION / 2;
        let (value, _) = motion.update([100.0, 80.0], halfway, Curve::Linear, true, DURATION);
        assert_eq!(value, [50.0, 60.0]);
        let (reversed, active) = motion.update([0.0, 40.0], halfway, Curve::Linear, true, DURATION);
        assert_eq!(reversed, value);
        assert!(active);
        let (finished, active) = motion.update(
            [0.0, 40.0],
            halfway + DURATION,
            Curve::Linear,
            true,
            DURATION,
        );
        assert_eq!(finished, [0.0, 40.0]);
        assert!(!active);
    }

    #[test]
    fn disabling_animations_settles_immediately() {
        let now = Instant::now();
        let mut motion = Motion::new([0.0], now);
        let (value, active) = motion.update([100.0], now, Curve::Quadratic, false, DURATION);
        assert_eq!(value, [100.0]);
        assert!(!active);
    }

    #[test]
    fn presence_keeps_the_exit_visible_until_complete() {
        let now = Instant::now();
        let mut presence = Presence::default();
        presence.update(true, now, true);
        presence.update(true, now + DURATION, true);
        let (value, active) = presence.update(false, now + DURATION, true);
        assert_eq!(value, 1.0);
        assert!(active);
        assert!(presence.visible());
        presence.update(false, now + DURATION * 2, true);
        assert!(!presence.visible());
    }
}

/* we allow a little squash, but keep an image's aspect ratio within four percent */
pub fn image_bounds(mut rect: [f32; 4], size: [u32; 2]) -> [f32; 4] {
    if size[0] == 0 || size[1] == 0 || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return rect;
    }

    let aspect = size[0] as f32 / size[1] as f32;
    let ratio = (rect[2] / rect[3]).clamp(aspect / 1.04, aspect * 1.04);
    let width = rect[2].min(rect[3] * ratio);
    let height = rect[3].min(rect[2] / ratio);
    rect[0] += (rect[2] - width) * 0.5;
    rect[1] += (rect[3] - height) * 0.5;
    rect[2] = width;
    rect[3] = height;
    rect
}

#[cfg(test)]
mod responsiveness_tests {
    use super::*;

    #[test]
    fn layout_covers_most_distance_in_the_first_fifty_milliseconds() {
        let now = Instant::now();
        let mut motion = Motion::new([0.0], now);
        motion.update([100.0], now, Curve::Cubic, true, MORPH_DURATION);
        let ([value], active) = motion.update(
            [100.0],
            now + Duration::from_millis(50),
            Curve::Cubic,
            true,
            MORPH_DURATION,
        );
        assert_eq!(value, 87.5);
        assert!(active);
        let ([value], active) = motion.update(
            [100.0],
            now + MORPH_DURATION,
            Curve::Cubic,
            true,
            MORPH_DURATION,
        );
        assert_eq!(value, 100.0);
        assert!(!active);
    }

    #[test]
    fn image_distortion_is_limited_in_both_directions() {
        for rect in [[10.0, 20.0, 900.0, 40.0], [10.0, 20.0, 40.0, 900.0]] {
            let fitted = image_bounds(rect, [200, 100]);
            let ratio = fitted[2] / fitted[3];
            assert!((2.0 / 1.04 - 0.0001..=2.0 * 1.04 + 0.0001).contains(&ratio));
            assert_eq!(fitted[0] + fitted[2] * 0.5, rect[0] + rect[2] * 0.5);
            assert_eq!(fitted[1] + fitted[3] * 0.5, rect[1] + rect[3] * 0.5);
        }
    }
}

// Scale visual displacement while preserving the final layout bounds.
pub fn morph_strength(target: [f32; 4], animated: [f32; 4], strength: f32) -> [f32; 4] {
    std::array::from_fn(|i| target[i] + (animated[i] - target[i]) * strength.clamp(0.0, 1.0))
}

#[cfg(test)]
mod strength_tests {
    use super::*;

    #[test]
    fn strength_scales_displacement_and_preserves_final_bounds() {
        let target = [100.0, 200.0, 300.0, 400.0];
        let animated = [20.0, 40.0, 60.0, 80.0];
        assert_eq!(morph_strength(target, animated, 0.0), target);
        assert_eq!(morph_strength(target, animated, 1.0), animated);
        assert_eq!(
            morph_strength(target, animated, 0.5),
            [60.0, 120.0, 180.0, 240.0]
        );
        for strength in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(morph_strength(target, target, strength), target);
        }
    }
}
