use crate::{platform::mouse::CursorPosition, ui::widgets::input::Bounds};

pub const ROW_HEIGHT: f32 = 38.0;

/* we keep the icon and label together for each menu command */
pub struct Item<'a> {
    pub icon: &'a str,
    pub label: &'a str,
}

#[derive(Default)]
pub struct State {
    open: bool,
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
        self.cursor = None;
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
