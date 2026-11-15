use std::{collections::HashMap, time::Instant};

use super::{
    animation::{Curve, DURATION},
    widgets::input::Bounds,
};
use crate::platform::{
    WindowEvent,
    mouse::{CursorPosition, MouseButton, ScrollDelta},
};

pub const BAR: f32 = 10.0;
const LINE: f32 = 38.0;

pub struct PrepareOptions {
    pub axes: [bool; 2],
    pub clip: Bounds,
    pub modal: bool,
    pub now: Instant,
    pub smooth: bool,
    pub animations: bool,
    pub curve: Curve,
}

pub fn contains(b: Bounds, p: Option<CursorPosition>) -> bool {
    p.is_some_and(|p| {
        p.x >= b.x as f64
            && p.y >= b.y as f64
            && p.x < (b.x + b.width) as f64
            && p.y < (b.y + b.height) as f64
    })
}

pub fn intersect(a: Bounds, b: Bounds) -> Bounds {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Bounds {
        x,
        y,
        width: ((a.x + a.width).min(b.x + b.width) - x).max(0.0),
        height: ((a.y + a.height).min(b.y + b.height) - y).max(0.0),
    }
}

#[derive(Clone, Copy)]
pub struct View {
    pub bounds: Bounds,
    pub viewport: Bounds,
    pub offset: [f32; 2],
    pub maximum: [f32; 2],
    pub expansion: [f32; 2],
}

impl View {
    pub fn track(self, axis: usize) -> Option<Bounds> {
        if self.maximum[axis] <= 0.0 {
            return None;
        }

        Some(if axis == 0 {
            Bounds {
                x: self.bounds.x,
                y: self.bounds.y + self.viewport.height,
                width: self.viewport.width,
                height: BAR,
            }
        } else {
            Bounds {
                x: self.bounds.x + self.viewport.width,
                y: self.bounds.y,
                width: BAR,
                height: self.viewport.height,
            }
        })
    }

    pub fn thumb(self, axis: usize) -> Option<Bounds> {
        let track = self.track(axis)?;
        let length = if axis == 0 {
            track.width
        } else {
            track.height
        };
        let size = (length * length / (length + self.maximum[axis]))
            .max(18.0)
            .min(length);
        let position = (length - size) * self.offset[axis] / self.maximum[axis];
        let inset = 2.0 * (1.0 - self.expansion[axis]);

        Some(if axis == 0 {
            Bounds {
                x: track.x + position,
                y: track.y + inset,
                width: size,
                height: BAR - 2.0 * inset,
            }
        } else {
            Bounds {
                x: track.x + inset,
                y: track.y + position,
                width: BAR - 2.0 * inset,
                height: size,
            }
        })
    }
}

struct State {
    view: View,
    extent: [f32; 2],
    target: [f32; 2],
    from: [f32; 2],
    start: Option<Instant>,
    hovered: [bool; 2],
    hover_from: [f32; 2],
    hover_start: [Option<Instant>; 2],
}

impl State {
    fn new(bounds: Bounds) -> Self {
        Self {
            view: View {
                bounds,
                viewport: bounds,
                offset: [0.0; 2],
                maximum: [0.0; 2],
                expansion: [0.0; 2],
            },
            extent: [0.0; 2],
            target: [0.0; 2],
            from: [0.0; 2],
            start: None,
            hovered: [false; 2],
            hover_from: [0.0; 2],
            hover_start: [None; 2],
        }
    }

    fn hover(&mut self, hovered: [bool; 2], now: Instant, enabled: bool, curve: Curve) -> bool {
        let mut active = false;

        for (axis, hovered) in hovered.into_iter().enumerate() {
            if let Some(start) = self.hover_start[axis] {
                let t = now.saturating_duration_since(start).as_secs_f32() / DURATION.as_secs_f32();
                let target = f32::from(self.hovered[axis]);
                self.view.expansion[axis] =
                    self.hover_from[axis] + (target - self.hover_from[axis]) * curve.sample(t);

                if t >= 1.0 {
                    self.view.expansion[axis] = target;
                    self.hover_start[axis] = None;
                }
            }

            if hovered != self.hovered[axis] {
                self.hover_from[axis] = self.view.expansion[axis];
                self.hovered[axis] = hovered;
                self.hover_start[axis] = Some(now);
            }

            if !enabled {
                self.view.expansion[axis] = f32::from(hovered);
                self.hover_start[axis] = None;
            }

            active |= self.hover_start[axis].is_some();
        }

        active
    }

    fn configure(&mut self, bounds: Bounds, axes: [bool; 2]) {
        let mut bars = [false; 2];

        /* one scrollbar can make the other axis overflow too */
        for _ in 0..3 {
            let width = (bounds.width
                - if bars[1] {
                    BAR
                } else {
                    0.0
                })
            .max(0.0);
            let height = (bounds.height
                - if bars[0] {
                    BAR
                } else {
                    0.0
                })
            .max(0.0);
            bars = [
                axes[0] && self.extent[0] > width + 0.5,
                axes[1] && self.extent[1] > height + 0.5,
            ];
        }

        let viewport = Bounds {
            width: (bounds.width
                - if bars[1] {
                    BAR
                } else {
                    0.0
                })
            .max(0.0),
            height: (bounds.height
                - if bars[0] {
                    BAR
                } else {
                    0.0
                })
            .max(0.0),
            ..bounds
        };
        self.view.bounds = bounds;
        self.view.viewport = viewport;
        self.view.maximum = [
            if bars[0] {
                (self.extent[0] - viewport.width).max(0.0)
            } else {
                0.0
            },
            if bars[1] {
                (self.extent[1] - viewport.height).max(0.0)
            } else {
                0.0
            },
        ];

        for axis in 0..2 {
            self.target[axis] = self.target[axis].clamp(0.0, self.view.maximum[axis]);
            self.from[axis] = self.from[axis].clamp(0.0, self.view.maximum[axis]);
            self.view.offset[axis] = self.view.offset[axis].clamp(0.0, self.view.maximum[axis]);
        }
    }

    fn tick(&mut self, now: Instant, smooth: bool, curve: Curve) -> bool {
        let Some(start) = self.start else {
            return false;
        };
        let t = if smooth {
            now.saturating_duration_since(start).as_secs_f32() / DURATION.as_secs_f32()
        } else {
            1.0
        };

        if t >= 1.0 || self.view.offset == self.target {
            self.view.offset = self.target;
            self.start = None;
            return false;
        }

        let t = curve.sample(t);

        for axis in 0..2 {
            self.view.offset[axis] = self.from[axis] + (self.target[axis] - self.from[axis]) * t;
        }

        true
    }

    fn move_to(&mut self, target: [f32; 2], now: Instant, smooth: bool, curve: Curve) -> bool {
        self.tick(now, smooth, curve);
        let target = [
            target[0].clamp(0.0, self.view.maximum[0]),
            target[1].clamp(0.0, self.view.maximum[1]),
        ];

        if target == self.target {
            return false;
        }

        self.from = self.view.offset;
        self.target = target;
        self.start = smooth.then_some(now);

        if !smooth {
            self.view.offset = target;
        }

        true
    }
}

#[derive(Clone, Copy)]
struct Region {
    id: u64,
    clip: Bounds,
    modal: bool,
}

struct Drag {
    id: u64,
    axis: usize,
    grab: f32,
}

#[derive(Default)]
pub struct Areas {
    states: HashMap<u64, State>,
    regions: Vec<Region>,
    previous: Vec<Region>,
    drag: Option<Drag>,
    pointer: Option<CursorPosition>,
    modal: bool,
}

impl Areas {
    pub fn begin(
        &mut self,
        events: &[WindowEvent],
        mut pointer: Option<CursorPosition>,
        now: Instant,
        smooth: bool,
        curve: Curve,
        modal: bool,
    ) -> Vec<WindowEvent> {
        std::mem::swap(&mut self.regions, &mut self.previous);
        self.regions.clear();
        let mut filtered = Vec::with_capacity(events.len());

        if self.drag.as_ref().is_some_and(|drag| {
            !self
                .previous
                .iter()
                .any(|r| r.id == drag.id && r.modal == modal)
        }) {
            self.drag = None;
        }

        for event in events {
            match event {
                WindowEvent::MouseMoved { position } | WindowEvent::MouseEntered { position } => {
                    pointer = Some(*position)
                }
                WindowEvent::MouseLeft | WindowEvent::MouseCancelled => pointer = None,
                _ => {}
            }

            let mut consumed = false;

            if let Some(drag) = &self.drag {
                if let (Some(pointer), Some(state)) = (pointer, self.states.get_mut(&drag.id))
                    && let (Some(track), Some(thumb)) =
                        (state.view.track(drag.axis), state.view.thumb(drag.axis))
                {
                    let (position, origin, length, size) = if drag.axis == 0 {
                        (pointer.x as f32, track.x, track.width, thumb.width)
                    } else {
                        (pointer.y as f32, track.y, track.height, thumb.height)
                    };
                    let mut target = state.target;
                    target[drag.axis] = (position - origin - drag.grab) / (length - size).max(1.0)
                        * state.view.maximum[drag.axis];
                    state.move_to(target, now, false, curve);
                }

                consumed = matches!(
                    event,
                    WindowEvent::MouseMoved { .. }
                        | WindowEvent::MouseButtonPressed { .. }
                        | WindowEvent::MouseButtonReleased { .. }
                        | WindowEvent::MouseScrolled { .. }
                );

                if matches!(
                    event,
                    WindowEvent::MouseButtonReleased {
                        button: MouseButton::Left
                    } | WindowEvent::MouseCancelled
                        | WindowEvent::MouseLeft
                        | WindowEvent::FocusChanged(false)
                ) {
                    self.drag = None;
                }
            }

            if !consumed {
                for region in self
                    .previous
                    .iter()
                    .rev()
                    .filter(|r| r.modal == modal && contains(r.clip, pointer))
                {
                    let Some(state) = self.states.get_mut(&region.id) else {
                        continue;
                    };

                    match event {
                        WindowEvent::MouseScrolled { delta } => {
                            let mut delta = match delta {
                                ScrollDelta::Lines { x, y } => [*x as f32 * LINE, *y as f32 * LINE],
                                ScrollDelta::Pixels { x, y } => [*x as f32, *y as f32],
                            };

                            if state.view.maximum[1] == 0.0 && delta[0] == 0.0 {
                                delta[0] = delta[1];
                            }

                            consumed = state.move_to(
                                [state.target[0] + delta[0], state.target[1] + delta[1]],
                                now,
                                smooth,
                                curve,
                            );
                        }

                        WindowEvent::MouseButtonPressed {
                            button: MouseButton::Left,
                        } => {
                            for axis in 0..2 {
                                let (Some(track), Some(thumb)) =
                                    (state.view.track(axis), state.view.thumb(axis))
                                else {
                                    continue;
                                };

                                if !contains(track, pointer) {
                                    continue;
                                }

                                let p = pointer.unwrap();
                                let (position, origin, length, size, thumb_start) = if axis == 0 {
                                    (p.x as f32, track.x, track.width, thumb.width, thumb.x)
                                } else {
                                    (p.y as f32, track.y, track.height, thumb.height, thumb.y)
                                };
                                let grab =
                                    if position >= thumb_start && position < thumb_start + size {
                                        position - thumb_start
                                    } else {
                                        size * 0.5
                                    };
                                let mut target = state.view.offset;
                                target[axis] = (position - origin - grab)
                                    / (length - size).max(1.0)
                                    * state.view.maximum[axis];
                                state.move_to(target, now, false, curve);
                                self.drag = Some(Drag {
                                    id: region.id,
                                    axis,
                                    grab,
                                });
                                consumed = true;
                                break;
                            }
                        }

                        _ => {}
                    }

                    if consumed {
                        break;
                    }
                }
            }

            filtered.push(if consumed {
                WindowEvent::MouseCancelled
            } else {
                event.clone()
            });
        }

        self.pointer = pointer;
        self.modal = modal;
        filtered
    }

    pub fn prepare(&mut self, id: u64, bounds: Bounds, options: PrepareOptions) -> (View, bool) {
        let PrepareOptions {
            axes,
            clip,
            modal,
            now,
            smooth,
            animations,
            curve,
        } = options;
        let state = self.states.entry(id).or_insert_with(|| State::new(bounds));
        state.configure(bounds, axes);
        let mut active = state.tick(now, smooth, curve);
        let hovered = std::array::from_fn(|axis| {
            let dragging = self
                .drag
                .as_ref()
                .is_some_and(|drag| drag.id == id && drag.axis == axis);
            modal == self.modal
                && (dragging
                    || state
                        .view
                        .track(axis)
                        .is_some_and(|track| contains(intersect(track, clip), self.pointer)))
        });
        active |= state.hover(hovered, now, animations, curve);
        self.regions.push(Region {
            id,
            clip: intersect(bounds, clip),
            modal,
        });
        (state.view, active)
    }

    pub fn finish(
        &mut self,
        id: u64,
        bounds: Bounds,
        extent: [f32; 2],
        axes: [bool; 2],
    ) -> (View, bool) {
        let state = self.states.get_mut(&id).unwrap();
        let old = state.view;
        state.extent = extent;
        state.configure(bounds, axes);
        let bounds = Bounds {
            width: bounds.width
                + if !axes[0] && state.view.maximum[1] > 0.0 {
                    BAR
                } else {
                    0.0
                },
            height: bounds.height
                + if !axes[1] && state.view.maximum[0] > 0.0 {
                    BAR
                } else {
                    0.0
                },
            ..bounds
        };
        state.configure(bounds, axes);
        let changed = (axes[0] && old.viewport.width != state.view.viewport.width)
            || (axes[1] && old.viewport.height != state.view.viewport.height)
            || old.offset != state.view.offset;
        (state.view, changed)
    }
}
