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

/// Background changes adjust label brightness in the opposite direction by
/// the same relative amount. Alpha is preserved independently for both colors.
pub fn appearance(mut style: Style, hovered: bool, pressed: bool) -> Style {
    if !hovered && !pressed {
        return style;
    }

    let original = style.background;
    let hover = style
        .hover_color
        .or_else(|| original.map(|c| darken(c, 0.05)));

    let target = if pressed {
        style.click_color.or_else(|| hover.map(|c| darken(c, 0.05)))
    } else {
        hover
    };

    let change = match (original, target) {
        (Some(base), Some(target)) => {
            let base = brightness(base);
            let delta = brightness(target) - base;
            // Black has no relative brightness; use the absolute change.
            if base > f32::EPSILON {
                delta / base
            } else {
                delta
            }
        }

        _ => {
            if pressed {
                -0.0975
            } else {
                -0.05
            }
        }
    };

    style.background = target;
    style.color = if change < 0.0 {
        brighten(style.color, (-change).min(1.0))
    } else {
        darken(style.color, change.min(1.0))
    };

    style
}

fn brightness(color: Color) -> f32 {
    color.r * 0.2126 + color.g * 0.7152 + color.b * 0.0722
}

fn darken(color: Color, amount: f32) -> Color {
    Color::rgba(
        color.r * (1.0 - amount),
        color.g * (1.0 - amount),
        color.b * (1.0 - amount),
        color.a,
    )
}

fn brighten(color: Color, amount: f32) -> Color {
    Color::rgba(
        color.r + (1.0 - color.r) * amount,
        color.g + (1.0 - color.g) * amount,
        color.b + (1.0 - color.b) * amount,
        color.a,
    )
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
