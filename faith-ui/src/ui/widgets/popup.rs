use std::time::{Duration, Instant};

use crate::{
    platform::mouse::{CursorPosition, MouseButton},
    ui::widgets::input::Bounds,
};

pub const ROW_HEIGHT: f32 = 38.0;
const CONTEXT_REPEAT_GUARD: Duration = Duration::from_millis(200);

/* we keep the icon and label together for each menu command */
pub struct Item<'a> {
    pub icon: &'a str,
    pub label: &'a str,
}

#[derive(Default)]
pub struct State {
    open: bool,
    context_opened_at: Option<Instant>,
    pub(crate) presence: crate::ui::animation::Presence,
    pub(crate) origin: Option<CursorPosition>,
    pub(crate) cursor: Option<usize>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_visible(&self) -> bool {
        self.open || self.presence.visible()
    }

    pub fn open(&mut self, position: CursorPosition) {
        self.origin = Some(position);
        self.open = true;
        self.context_opened_at = None;
        self.cursor = None;
    }

    pub fn open_context(&mut self, position: CursorPosition, now: Instant) {
        self.open(position);
        self.context_opened_at = Some(now);
    }

    pub(crate) fn dismisses_on_press(
        &self,
        button: MouseButton,
        inside: bool,
        now: Instant,
    ) -> bool {
        match (button, self.context_opened_at) {
            (MouseButton::Right, Some(opened)) => now - opened >= CONTEXT_REPEAT_GUARD,
            _ => !inside,
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.cursor = None;
    }

    pub(crate) fn bounds(&self, width: f32, count: usize, window: Bounds) -> Bounds {
        let origin = self.origin.unwrap();
        let width = width.min((window.width - 16.0).max(0.0));
        let height = (count as f32 * ROW_HEIGHT + 8.0).min((window.height - 16.0).max(0.0));
        Bounds {
            x: (origin.x as f32).clamp(8.0, (window.width - width - 8.0).max(8.0)),
            y: (origin.y as f32).clamp(8.0, (window.height - height - 8.0).max(8.0)),
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_click_toggles_context_menus_after_the_repeat_guard() {
        let mut state = State::default();
        let position = CursorPosition { x: 10.0, y: 20.0 };
        let opened = Instant::now();
        state.open_context(position, opened);
        for inside in [false, true] {
            assert!(!state.dismisses_on_press(
                MouseButton::Right,
                inside,
                opened + Duration::from_millis(60)
            ));
            assert!(state.dismisses_on_press(
                MouseButton::Right,
                inside,
                opened + CONTEXT_REPEAT_GUARD
            ));
        }

        assert!(state.dismisses_on_press(MouseButton::Left, false, opened));
        assert!(!state.dismisses_on_press(MouseButton::Left, true, opened));

        state.open(position);
        assert!(state.dismisses_on_press(MouseButton::Right, false, opened));
        assert!(!state.dismisses_on_press(MouseButton::Right, true, opened));
    }

    #[test]
    fn anchors_at_cursor_and_stays_inside_window() {
        let window = Bounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let mut state = State::default();
        state.open(CursorPosition { x: 120.0, y: 80.0 });
        let bounds = state.bounds(220.0, 2, window);
        assert_eq!((bounds.x, bounds.y), (120.0, 80.0));
        state.open(CursorPosition { x: 799.0, y: 599.0 });
        let bounds = state.bounds(220.0, 2, window);
        assert_eq!(bounds.x + bounds.width, 792.0);
        assert_eq!(bounds.y + bounds.height, 592.0);
    }
}
