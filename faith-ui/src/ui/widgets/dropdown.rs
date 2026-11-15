use super::{button, input::Bounds};
use crate::{
    graphics::{FontHandle, GraphicsDevice},
    platform::{
        WindowEvent,
        mouse::{CursorPosition, MouseButton, ScrollDelta},
    },
    ui::{
        TextRenderer, UIRenderer,
        theme::{CornerRadii, Theme},
    },
};

pub const DOWN: &str = "\u{eab4}";
pub const UP: &str = "\u{eab7}";
const CHECK: &str = "\u{eab2}";
const ROW_HEIGHT: f32 = 38.0;

#[derive(Default)]
pub struct State {
    pub open: bool,
    first: usize,
    initialized: bool,
    scroll: f64,
    header: button::State,
    rows: Vec<button::State>,
}

pub struct Response {
    pub selected: Option<usize>,
    pub hovered: bool,
    pub pressed: bool,
    pub hovered_row: Option<usize>,
    pub first: usize,
    pub visible: usize,
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

fn row_bounds(popup: Bounds, index: usize) -> Bounds {
    Bounds {
        x: popup.x + 4.0,
        y: popup.y + 4.0 + index as f32 * ROW_HEIGHT,
        width: (popup.width - 14.0).max(0.0),
        height: ROW_HEIGHT,
    }
}

fn contains(bounds: Bounds, pointer: Option<CursorPosition>) -> bool {
    pointer.is_some_and(|p| {
        p.x >= bounds.x as f64
            && p.y >= bounds.y as f64
            && p.x < (bounds.x + bounds.width) as f64
            && p.y < (bounds.y + bounds.height) as f64
    })
}

impl State {
    pub fn update(
        &mut self,
        header: Bounds,
        popup: Bounds,
        count: usize,
        current: usize,
        mut pointer: Option<CursorPosition>,
        events: &[WindowEvent],
    ) -> Response {
        let visible = (((popup.height - 8.0) / ROW_HEIGHT).round() as usize).min(count);
        let max_first = count.saturating_sub(visible);
        self.first = self.first.min(max_first);
        self.rows.resize_with(visible, button::State::default);
        let mut selected = None;

        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    pointer = Some(*position)
                }

                WindowEvent::MouseScrolled { delta } if self.open && contains(popup, pointer) => {
                    self.scroll += match delta {
                        ScrollDelta::Lines { y, .. } => *y,
                        ScrollDelta::Pixels { y, .. } => *y / ROW_HEIGHT as f64,
                    };
                    let steps = self.scroll.trunc() as isize;
                    self.scroll -= steps as f64;
                    let first = self.first.saturating_add_signed(steps).min(max_first);

                    if first != self.first {
                        self.first = first;

                        for row in &mut self.rows {
                            *row = button::State::default();
                        }
                    }
                }

                WindowEvent::MouseLeft => pointer = None,
                WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                    self.open = false;
                    self.header = button::State::default();

                    for row in &mut self.rows {
                        *row = button::State::default();
                    }
                }

                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left,
                } if self.open && !contains(header, pointer) && !contains(popup, pointer) => {
                    self.open = false
                }

                _ => {}
            }

            let response = self
                .header
                .update(header, pointer, std::slice::from_ref(event));

            if response.clicked {
                self.open = !self.open;

                if self.open && !self.initialized {
                    self.first = current.min(max_first);
                    self.initialized = true;
                }

                for row in &mut self.rows {
                    *row = button::State::default();
                }

                continue;
            }

            if self.open {
                for (index, row) in self.rows.iter_mut().enumerate() {
                    if row
                        .update(
                            row_bounds(popup, index),
                            pointer,
                            std::slice::from_ref(event),
                        )
                        .clicked
                    {
                        selected = Some(self.first + index);
                        self.open = false;
                    }
                }
            }
        }

        let response = self.header.update(header, pointer, &[]);
        Response {
            selected,
            hovered: response.hovered,
            pressed: response.pressed,
            hovered_row: (0..visible)
                .find(|i| contains(row_bounds(popup, *i), pointer))
                .map(|index| self.first + index),
            first: self.first,
            visible,
        }
    }
}

pub struct Popup {
    pub bounds: Bounds,
    pub options: Vec<String>,
    pub row_styles: Vec<crate::ui::framework::Style>,
    pub selected: usize,
    pub first: usize,
    pub total: usize,
    pub font: FontHandle,
    pub icons: FontHandle,
}

impl Popup {
    pub fn render<G: GraphicsDevice>(
        &self,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
        theme: Theme,
        radii: CornerRadii,
    ) -> anyhow::Result<()> {
        let b = self.bounds;
        ui.rounded_box(
            [b.x, b.y, b.width, b.height],
            radii.popup,
            Some(theme.surface),
            Some(theme.border),
        )?;

        ui.push_clip([
            b.x + 1.0,
            b.y + 1.0,
            (b.width - 2.0).max(0.0),
            (b.height - 2.0).max(0.0),
        ]);
        let result = (|| -> anyhow::Result<()> {
            for (index, value) in self.options.iter().enumerate() {
                let row = row_bounds(b, index);

                if let Some(background) = self.row_styles[index].background {
                    ui.rounded_box(
                        [row.x, row.y, row.width, row.height],
                        radii.row,
                        Some(background),
                        None,
                    )?;
                }

                let metrics = text.measure(value, self.font)?;
                text.color = theme.text;
                ui.push_clip([row.x, row.y, (row.width - 36.0).max(0.0), row.height]);
                let result = text.render(
                    row.x + 10.0,
                    row.y + (row.height - metrics.height) * 0.5,
                    value,
                    self.font,
                    gpu,
                    ui,
                );
                ui.pop_clip();
                result?;

                if self.first + index == self.selected {
                    let metrics = text.measure(CHECK, self.icons)?;
                    text.color = theme.selected_icon;
                    text.render(
                        row.x + row.width - 26.0,
                        row.y + (row.height - metrics.height) * 0.5,
                        CHECK,
                        self.icons,
                        gpu,
                        ui,
                    )?;
                }
            }

            Ok(())
        })();
        ui.pop_clip();
        result?;

        if self.total > self.options.len() {
            let track = b.height - 8.0;
            let thumb = (track * self.options.len() as f32 / self.total as f32).max(12.0);
            let y = b.y
                + 4.0
                + (track - thumb) * self.first as f32 / (self.total - self.options.len()) as f32;
            ui.rounded_box(
                [b.x + b.width - 5.0, y, 2.0, thumb],
                1.0,
                Some(theme.muted),
                None,
            )?;
        }

        Ok(())
    }
}
