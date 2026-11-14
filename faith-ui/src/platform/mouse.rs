use std::collections::HashSet;

use super::event::WindowEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Back,
    Forward,
    /// Unmapped native button number; only named buttons are portable.
    Other(u32),
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Discrete wheel steps. Positive x scrolls right, positive y down.
    Lines { x: f64, y: f64 },
    /// Smooth scrolling in window coordinates, with the same signs as Lines.
    Pixels { x: f64, y: f64 },
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MouseScroll {
    pub lines: CursorPosition,
    pub pixels: CursorPosition,
}

/// Mouse state snapshot
#[derive(Debug, Default)]
pub struct Mouse {
    cursor: Option<CursorPosition>,
    pressed: HashSet<MouseButton>,
    went_down: HashSet<MouseButton>,
    released: HashSet<MouseButton>,
    movement: CursorPosition,
    moved: bool,
    scroll: MouseScroll,
}

impl Mouse {
    /// None before pointer input, after leaving, or after cancellation. During
    /// a drag the backend may keep supplying positions beyond window bounds.
    #[allow(dead_code)]
    pub fn get_cursor(&self) -> Option<CursorPosition> {
        self.cursor
    }

    /// Whether the button is currently held.
    #[allow(dead_code)]
    pub fn pressed(&self, button: MouseButton) -> bool {
        self.pressed.contains(&button)
    }

    /// Whether a press occurred in this batch.
    #[allow(dead_code)]
    pub fn went_down(&self, button: MouseButton) -> bool {
        self.went_down.contains(&button)
    }

    /// Whether a held button was released or cancelled in this batch.
    #[allow(dead_code)]
    pub fn released(&self, button: MouseButton) -> bool {
        self.released.contains(&button)
    }

    #[allow(dead_code)]
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// Accumulated displacement; entering the window does not add a delta.
    #[allow(dead_code)]
    pub fn movement(&self) -> CursorPosition {
        self.movement
    }

    #[allow(dead_code)]
    pub fn scroll(&self) -> MouseScroll {
        self.scroll
    }

    pub fn begin_frame(&mut self) {
        self.went_down.clear();
        self.released.clear();
        self.movement = CursorPosition::default();
        self.moved = false;
        self.scroll = MouseScroll::default();
    }

    pub fn handle(&mut self, event: &WindowEvent) {
        match *event {
            WindowEvent::MouseEntered { position } => self.cursor = Some(position),
            WindowEvent::MouseMoved { position } => {
                if let Some(previous) = self.cursor {
                    self.movement.x += position.x - previous.x;
                    self.movement.y += position.y - previous.y;
                }
                self.moved |= self.cursor != Some(position);
                self.cursor = Some(position);
            }
            // X11's implicit grab continues delivering drag events outside the
            // window. Leave invalidates hover but must not cancel that drag.
            WindowEvent::MouseLeft => self.cursor = None,
            WindowEvent::MouseButtonPressed { button } => {
                if self.pressed.insert(button) {
                    self.went_down.insert(button);
                }
            }
            WindowEvent::MouseButtonReleased { button } => {
                if self.pressed.remove(&button) {
                    self.released.insert(button);
                }
            }
            WindowEvent::MouseScrolled { delta } => {
                let (target, x, y) = match delta {
                    ScrollDelta::Lines { x, y } => (&mut self.scroll.lines, x, y),
                    ScrollDelta::Pixels { x, y } => (&mut self.scroll.pixels, x, y),
                };
                target.x += x;
                target.y += y;
            }
            WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                self.released.extend(self.pressed.drain());
                self.cursor = None;
            }
            _ => {}
        }
    }
}
