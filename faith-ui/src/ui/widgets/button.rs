use super::input::Bounds;
use crate::{
    graphics::{FontHandle, GraphicsDevice},
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
