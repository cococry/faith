use super::widgets::input::Bounds;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Button,
    Input,
    Checkbox,
    Slider,
    Dropdown,
}

pub struct Control {
    pub id: String,
    pub bounds: Bounds,
    pub kind: Kind,
    pub selected: bool,
}

pub struct Navigation {
    pub controls: Vec<Control>,
    pub pending: bool,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            controls: Vec::new(),
            pending: true,
        }
    }
}

impl Navigation {
    pub fn control(&self, focused: Option<&str>) -> Option<&Control> {
        self.controls
            .iter()
            .find(|control| Some(control.id.as_str()) == focused)
    }

    fn ordered(&self) -> Vec<&Control> {
        let mut controls: Vec<_> = self.controls.iter().collect();
        controls.sort_unstable_by(|a, b| {
            a.bounds
                .x
                .total_cmp(&b.bounds.x)
                .then_with(|| a.bounds.y.total_cmp(&b.bounds.y))
                .then_with(|| a.id.cmp(&b.id))
        });
        controls
    }

    fn initial(&self) -> Option<&Control> {
        let controls = self.ordered();
        controls
            .iter()
            .find(|control| control.selected)
            .or_else(|| controls.first())
            .copied()
    }

    fn select(&mut self, focused: &mut Option<String>, id: String) {
        *focused = Some(id);
        self.pending = false;
    }

    pub fn tab(&mut self, focused: &mut Option<String>, backwards: bool) {
        let controls = self.ordered();
        if controls.is_empty() {
            return;
        }
        let count = controls.len();
        let current = controls
            .iter()
            .position(|control| Some(control.id.as_str()) == focused.as_deref());
        let next = match (current, backwards) {
            (Some(index), true) => (index + count - 1) % count,
            (Some(index), false) => (index + 1) % count,
            (None, true) => count - 1,
            (None, false) => 0,
        };
        let id = controls[next].id.clone();
        self.select(focused, id);
    }

    /* we repair focus when navigation removes the current control */
    pub fn finish(&mut self, focused: &mut Option<String>) -> bool {
        if self.control(focused.as_deref()).is_some() {
            self.pending = false;
            return false;
        }
        if focused.take().is_some() {
            self.pending = true;
        }
        if !self.pending {
            return false;
        }
        let id = self.initial().map(|control| control.id.clone());
        if let Some(id) = id {
            self.select(focused, id);
            return true;
        }
        false
    }
}
