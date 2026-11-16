use std::time::{Duration, Instant};

use super::input::Bounds;
use crate::platform::{
    WindowEvent,
    event::Key,
    mouse::{CursorPosition, MouseButton},
};
use crate::ui::animation::{Curve, Hover};

#[derive(Clone, Copy)]
pub struct Options {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub suffix: &'static str,
}

#[derive(Default)]
pub struct State {
    dragging: bool,
    draft: Option<u32>,
    value_motion: Motion,
    hover: Hover,
    pub wheel: f64,
    pub hit: Option<Bounds>,
}

pub struct Interaction<'a> {
    pub track: Bounds,
    pub hit: Bounds,
    pub pointer: Option<CursorPosition>,
    pub events: &'a [WindowEvent],
    pub focused: &'a mut Option<String>,
}

pub struct Response {
    pub value: u32,
    pub committed: bool,
    pub hovered: bool,
    pub dragging: bool,
}

impl Options {
    pub fn snap(self, value: f64) -> u32 {
        let step = self.step.max(1) as f64;
        let value = value.clamp(self.min as f64, self.max as f64);
        if value == self.max as f64 {
            return self.max;
        }

        (self.min as f64 + ((value - self.min as f64) / step).round() * step)
            .clamp(self.min as f64, self.max as f64) as u32
    }

    fn advance(self, value: u32, steps: f64) -> u32 {
        let step = self.step.max(1);
        if steps < 0.0 && value == self.max && !(self.max - self.min).is_multiple_of(step) {
            let last = self.min + (self.max - self.min) / step * step;
            return self.snap(last as f64 + (steps + 1.0) * step as f64);
        }

        self.snap(value as f64 + steps * step as f64)
    }
}

impl State {
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    pub fn position(
        &mut self,
        target: f32,
        now: Instant,
        enabled: bool,
        curve: Curve,
    ) -> (f32, bool) {
        self.value_motion
            .sample(target, now, enabled, Duration::from_millis(70), curve)
    }

    pub fn expansion(
        &mut self,
        hovered: bool,
        now: Instant,
        enabled: bool,
        curve: Curve,
    ) -> (f32, bool) {
        self.hover.update(hovered, now, enabled, curve)
    }

    pub fn update(
        &mut self,
        id: &str,
        previous: u32,
        options: Options,
        input: Interaction<'_>,
    ) -> Response {
        let mut current = self.draft.unwrap_or_else(|| options.snap(previous as f64));
        let mut cancelled = false;
        let mut pointer = input.pointer;
        let inset = 2.0_f32.min(input.track.width * 0.5);
        let left = input.track.x + inset;
        let length = (input.track.width - 2.0 * inset).max(1.0);
        let value_at = |x: f32| {
            options.snap(
                options.min as f64
                    + ((x - left) / length).clamp(0.0, 1.0) as f64
                        * (options.max - options.min) as f64,
            )
        };
        let inside = |pointer: Option<CursorPosition>| {
            pointer.is_some_and(|p| {
                p.x >= input.hit.x as f64
                    && p.x < (input.hit.x + input.hit.width) as f64
                    && p.y >= input.hit.y as f64
                    && p.y < (input.hit.y + input.hit.height) as f64
            })
        };

        for event in input.events {
            match event {
                WindowEvent::MouseMoved { position } | WindowEvent::MouseEntered { position } => {
                    pointer = Some(*position);
                    if self.dragging {
                        current = value_at(position.x as f32);
                    }
                }
                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left,
                } => {
                    if inside(pointer) {
                        *input.focused = Some(id.into());
                        let x = pointer.unwrap().x as f32;
                        self.dragging = true;
                        current = value_at(x);
                    } else if input.focused.as_deref() == Some(id) {
                        *input.focused = None;
                    }
                }
                WindowEvent::MouseButtonReleased {
                    button: MouseButton::Left,
                } => {
                    if self.dragging
                        && let Some(p) = pointer
                    {
                        current = value_at(p.x as f32);
                    }

                    self.dragging = false;
                }
                WindowEvent::KeyPressed { key, .. } if input.focused.as_deref() == Some(id) => {
                    match key {
                        Key::Left | Key::Down => current = options.advance(current, -1.0),
                        Key::Right | Key::Up => current = options.advance(current, 1.0),
                        Key::PageDown => current = options.advance(current, -10.0),
                        Key::PageUp => current = options.advance(current, 10.0),
                        Key::Home => current = options.min,
                        Key::End => current = options.max,
                        Key::Escape => {
                            self.dragging = false;
                            cancelled = true;
                            *input.focused = None;
                        }
                        _ => {}
                    }
                }
                WindowEvent::MouseLeft => pointer = None,
                WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                    self.dragging = false;
                    cancelled = true;
                    self.wheel = 0.0;
                    if input.focused.as_deref() == Some(id) {
                        *input.focused = None;
                    }
                }
                _ => {}
            }
        }

        let whole = self.wheel.trunc();
        self.wheel -= whole;
        if whole != 0.0 && input.focused.as_deref() == Some(id) {
            current = options.advance(current, whole);
        }

        if cancelled {
            current = options.snap(previous as f64);
        }

        self.draft = self.dragging.then_some(current);
        Response {
            value: current,
            committed: !self.dragging && !cancelled && current != previous,
            hovered: inside(pointer),
            dragging: self.dragging,
        }
    }
}

/* we only keep waking frames while the fill or hover size is still changing */
#[derive(Default)]
struct Motion {
    shown: Option<f32>,
    target: Option<f32>,
    transition: Option<(f32, f32, Instant)>,
}

impl Motion {
    fn sample(
        &mut self,
        target: f32,
        now: Instant,
        enabled: bool,
        duration: Duration,
        curve: Curve,
    ) -> (f32, bool) {
        let mut shown = self.shown.unwrap_or(target);
        if let Some((from, to, start)) = self.transition {
            let t = now.saturating_duration_since(start).as_secs_f32() / duration.as_secs_f32();
            shown = from + (to - from) * curve.sample(t);
            if t >= 1.0 {
                self.transition = None;
            }
        }

        if self.target != Some(target) {
            self.transition = (enabled && self.shown.is_some() && (shown - target).abs() > 0.0001)
                .then_some((shown, target, now));
            self.target = Some(target);
        }

        if !enabled || self.transition.is_none() {
            shown = target;
            self.transition = None;
        }

        self.shown = Some(shown);
        (shown, self.transition.is_some())
    }
}
