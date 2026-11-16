use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use super::framework::Style;
use crate::{graphics::Color, platform::window::PlatformWaker};

pub const DURATION: Duration = Duration::from_millis(200);

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
