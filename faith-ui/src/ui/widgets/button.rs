use super::input::Bounds;
use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    ui::{TextRenderer, UIRenderer, framework::Style},
};

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
                WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                    self.held = false;
                    pointer = None;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(x: f32) -> Bounds {
        Bounds {
            x,
            y: 0.0,
            width: 40.0,
            height: 30.0,
        }
    }
    fn position(x: f64) -> Option<CursorPosition> {
        Some(CursorPosition { x, y: 10.0 })
    }
    fn press() -> WindowEvent {
        WindowEvent::MouseButtonPressed {
            button: MouseButton::Left,
        }
    }
    fn release() -> WindowEvent {
        WindowEvent::MouseButtonReleased {
            button: MouseButton::Left,
        }
    }
    #[test]
    fn only_release_inside_originating_button_fires() {
        let mut a = State::default();
        let mut b = State::default();
        let r = a.update(bounds(0.0), position(10.0), &[press()]);
        assert!(r.pressed && !r.clicked);
        b.update(bounds(60.0), position(10.0), &[press()]);
        let movement = WindowEvent::MouseMoved {
            position: position(70.0).unwrap(),
        };
        assert!(
            !a.update(bounds(0.0), position(10.0), std::slice::from_ref(&movement))
                .pressed
        );
        b.update(bounds(60.0), position(10.0), &[movement]);
        assert!(!a.update(bounds(0.0), position(70.0), &[release()]).clicked);
        assert!(!b.update(bounds(60.0), position(70.0), &[release()]).clicked);
        let r = b.update(bounds(60.0), position(70.0), &[press(), release()]);
        assert!(r.clicked && !r.pressed);
        assert!(!b.update(bounds(60.0), position(70.0), &[release()]).clicked);
    }
    #[test]
    fn dragging_back_inside_and_cancellation() {
        let mut state = State::default();
        state.update(bounds(0.0), position(10.0), &[press()]);
        assert!(!state.update(bounds(0.0), None, &[]).pressed);
        assert!(state.update(bounds(0.0), position(10.0), &[]).pressed);
        state.update(bounds(0.0), position(10.0), &[WindowEvent::MouseCancelled]);
        assert!(
            !state
                .update(bounds(0.0), position(10.0), &[release()])
                .clicked
        );
    }
    #[test]
    fn custom_background_changes_label_by_opposite_relative_brightness() {
        let base = Style {
            background: Some(Color::rgba(0.5, 0.5, 0.5, 0.6)),
            color: Color::rgba(0.4, 0.4, 0.4, 0.8),
            ..Style::default()
        };
        let darker = appearance(
            Style {
                hover_color: Some(Color::rgba(0.25, 0.25, 0.25, 0.6)),
                ..base
            },
            true,
            false,
        );
        assert!((darker.color.r - 0.7).abs() < 0.00001);
        let lighter = appearance(
            Style {
                click_color: Some(Color::rgba(0.75, 0.75, 0.75, 0.6)),
                ..base
            },
            true,
            true,
        );
        assert!((lighter.color.r - 0.2).abs() < 0.00001);
        assert_eq!(darker.color.a, 0.8);
        assert_eq!(lighter.color.a, 0.8);
    }
    #[test]
    fn colors_are_subtle_and_overrides_preserve_alpha() {
        let base = Style {
            background: Some(Color::rgba(0.5, 0.5, 0.5, 0.7)),
            ..Style::default()
        };
        let hover = appearance(base, true, false);
        let held = appearance(base, true, true);
        assert_eq!(
            hover.background.unwrap(),
            Color::rgba(0.475, 0.475, 0.475, 0.7)
        );
        assert!(held.background.unwrap().r < hover.background.unwrap().r);
        assert!((held.background.unwrap().r - 0.45125).abs() < 0.00001);
        assert!(held.color.r > hover.color.r);
        assert!(hover.color.r > base.color.r);
        let custom = Color::rgba(0.2, 0.3, 0.4, 1.0);
        assert_eq!(
            appearance(
                Style {
                    click_color: Some(custom),
                    ..base
                },
                true,
                true
            )
            .background,
            Some(custom)
        );
        assert_eq!(
            appearance(
                Style {
                    hover_color: Some(custom),
                    ..base
                },
                true,
                false
            )
            .background,
            Some(custom)
        );
    }
}
