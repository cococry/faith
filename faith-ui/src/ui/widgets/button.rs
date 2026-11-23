use super::input::Bounds;
use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    ui::{TextRenderer, UIRenderer, framework::Style},
};

#[allow(dead_code)]
pub fn render<G: GraphicsDevice>(
    label: &str,
    bounds: Bounds,
    style: Style,
    font: FontHandle,
    text: &mut TextRenderer,
    gpu: &mut G,
    ui: &mut UIRenderer,
) -> anyhow::Result<()> {
    text.color = style.color;
    text.render(
        bounds.x + style.padding,
        bounds.y + style.padding,
        label,
        font,
        gpu,
        ui,
    )
}

pub fn highlight(background: Color, foreground: Color) -> Color {
    crate::ui::animation::color(background, foreground, 0.05)
}

/* we tint toward the foreground so hover remains visible in both light and dark themes */
pub fn appearance(mut style: Style, hovered: bool, pressed: bool) -> Style {
    if !hovered && !pressed {
        return style;
    }

    let tint = |amount| {
        style.background.map_or_else(
            || Color {
                a: style.color.a * amount,
                ..style.color
            },
            |background| crate::ui::animation::color(background, style.color, amount),
        )
    };
    let hover = style.hover_color.unwrap_or_else(|| tint(0.05));
    style.background = Some(if pressed {
        style.click_color.unwrap_or_else(|| {
            crate::ui::animation::color(hover, style.hover_text_color.unwrap_or(style.color), 0.05)
        })
    } else {
        hover
    });

    if let Some(color) = if pressed {
        style.click_text_color.or(style.hover_text_color)
    } else {
        style.hover_text_color
    } {
        style.color = color;
    }
    style
}

use crate::platform::{
    WindowEvent,
    mouse::{CursorPosition, MouseButton},
};

#[derive(Default)]
pub struct State {
    held: bool,
}

pub struct Response {
    pub clicked: bool,
    pub hovered: bool,
    pub pressed: bool,
}

impl State {
    /// Capture only presses originating here. Leaving removes the pressed
    /// appearance; release elsewhere or cancellation never activates a button.
    pub fn update(
        &mut self,
        bounds: Bounds,
        mut pointer: Option<CursorPosition>,
        events: &[WindowEvent],
    ) -> Response {
        let inside = |pointer: Option<CursorPosition>| {
            pointer.is_some_and(|p| {
                p.x >= bounds.x as f64
                    && p.y >= bounds.y as f64
                    && p.x < (bounds.x + bounds.width) as f64
                    && p.y < (bounds.y + bounds.height) as f64
            })
        };

        let mut clicked = false;

        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    pointer = Some(*position)
                }

                WindowEvent::MouseLeft => pointer = None,
                WindowEvent::MouseCancelled => {
                    self.held = false;
                    pointer = None;
                }

                WindowEvent::FocusChanged(false) => self.held = false,
                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left,
                    ..
                } => self.held = inside(pointer),
                WindowEvent::MouseButtonReleased {
                    button: MouseButton::Left,
                } => {
                    clicked |= self.held && inside(pointer);
                    self.held = false;
                }

                _ => {}
            }
        }

        let hovered = inside(pointer);
        Response {
            clicked,
            hovered,
            pressed: self.held && hovered,
        }
    }
}
