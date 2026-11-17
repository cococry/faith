use super::{button, input::Bounds};
use crate::{
    graphics::FontHandle,
    platform::{
        WindowEvent,
        event::Key,
        mouse::{CursorPosition, MouseButton},
    },
    ui::scroll::contains,
};
use std::sync::Arc;

pub const DOWN: &str = "\u{eab4}";
pub const UP: &str = "\u{eab7}";
pub const CHECK: &str = "\u{eab2}";
pub const ROW_HEIGHT: f32 = 38.0;

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub presence: crate::ui::animation::Presence,
    header: button::State,
    cursor: Option<usize>,
    pub reveal: bool,
    options: Arc<[String]>,
}

pub struct Response {
    pub hovered: bool,
    pub pressed: bool,
}

pub fn popup_bounds(header: Bounds, count: usize, window: [f32; 2]) -> Bounds {
    let width = header.width.min((window[0] - 16.0).max(0.0));
    let below = header.y + header.height + 4.0;
    let space_below = (window[1] - below - 8.0).max(0.0);
    let space_above = (header.y - 12.0).max(0.0);
    let desired = count as f32 * ROW_HEIGHT + 8.0;
    let open_below = desired <= space_below || space_below >= space_above;
    let available = if open_below {
        space_below
    } else {
        space_above
    };
    let rows = (((available - 8.0) / ROW_HEIGHT).floor().max(0.0) as usize).min(count);
    let height = rows as f32 * ROW_HEIGHT + 8.0;
    let x = header.x.clamp(8.0, (window[0] - width - 8.0).max(8.0));
    let y = if open_below {
        below
    } else {
        header.y - height - 4.0
    };

    Bounds {
        x,
        y,
        width,
        height,
    }
}

impl State {
    pub fn keyboard(
        &mut self,
        focused: bool,
        selected: usize,
        count: usize,
        events: &[WindowEvent],
    ) -> (Option<usize>, bool) {
        if self.open && self.cursor.is_none() {
            self.cursor = Some(selected);
            self.reveal = true;
        }
        let mut choice = None;
        let mut unfocus = false;
        for event in events {
            if !focused {
                break;
            }
            match event {
                WindowEvent::TextInput(text) if text == " " => {
                    self.open = !self.open;
                    if self.open {
                        self.cursor = Some(selected);
                        self.reveal = true;
                    }
                }
                WindowEvent::KeyPressed {
                    key: Key::Enter, ..
                } => {
                    if self.open {
                        choice = self.cursor;
                        self.open = false;
                    } else {
                        self.open = true;
                        self.cursor = Some(selected);
                        self.reveal = true;
                    }
                }
                WindowEvent::KeyPressed {
                    key: Key::Escape, ..
                } => {
                    if self.open {
                        self.open = false;
                    } else {
                        unfocus = true;
                    }
                }
                WindowEvent::KeyPressed { key, .. } if self.open => {
                    let cursor = self.cursor.unwrap_or(selected).min(count - 1);
                    let next = match key {
                        Key::Down => (cursor + 1) % count,
                        Key::Up => (cursor + count - 1) % count,
                        Key::Home => 0,
                        Key::End => count - 1,
                        Key::PageDown => (cursor + 5).min(count - 1),
                        Key::PageUp => cursor.saturating_sub(5),
                        _ => continue,
                    };
                    self.cursor = Some(next);
                    self.reveal = true;
                }
                _ => {}
            }
        }
        if !self.open {
            self.cursor = None;
        }
        (choice, unfocus)
    }

    pub fn highlighted(&self, selected: usize) -> usize {
        self.cursor.unwrap_or(selected)
    }

    pub fn options(&mut self, options: &[&str]) -> Arc<[String]> {
        if self.options.len() != options.len()
            || self.options.iter().zip(options).any(|(a, b)| a != b)
        {
            self.options = options.iter().map(|value| (*value).to_owned()).collect();
        }

        self.options.clone()
    }

    pub fn update(
        &mut self,
        header: Bounds,
        popup: Bounds,
        mut pointer: Option<CursorPosition>,
        events: &[WindowEvent],
    ) -> Response {
        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    pointer = Some(*position)
                }
                WindowEvent::MouseLeft => pointer = None,
                WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                    /* scrollbars consume pointer events without dismissing the popup */
                    self.header = button::State::default();

                    if matches!(event, WindowEvent::FocusChanged(false)) {
                        self.open = false;
                    }
                }

                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left,
                } if self.open && !contains(header, pointer) && !contains(popup, pointer) => {
                    self.open = false
                }
                _ => {}
            }

            if self
                .header
                .update(header, pointer, std::slice::from_ref(event))
                .clicked
            {
                self.open = !self.open;
            }
        }

        let response = self.header.update(header, pointer, &[]);
        Response {
            hovered: response.hovered,
            pressed: response.pressed,
        }
    }
}

pub struct Popup {
    pub id: String,
    pub bounds: Bounds,
    pub options: Arc<[String]>,
    pub selected: usize,
    pub highlighted: usize,
    pub reveal: bool,
    pub font: FontHandle,
    pub icons: FontHandle,
    pub font_size: u32,
    pub presence: f32,
    pub interactive: bool,
}
