use super::input::Bounds;
use crate::{
    graphics::{FontHandle, GraphicsDevice},
    ui::{TextRenderer, UIRenderer, framework::Style, text::TextLayout, theme::Theme},
};

use std::sync::Arc;

pub struct Checkbox {
    pub bounds: Bounds,
    pub layout: Arc<TextLayout>,
    pub icons: FontHandle,
    pub checked: bool,
    pub style: Style,
    pub radius: f32,
}

impl Checkbox {
    pub fn render<G: GraphicsDevice>(
        &self,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
        theme: Theme,
    ) -> anyhow::Result<()> {
        let b = self.bounds;
        let box_rect = [b.x, b.y + 6.0, 20.0, 20.0];
        let style = self.style;
        ui.rounded_box(box_rect, self.radius, style.background, style.border_color)?;

        if self.checked {
            let metrics = text.measure("\u{eab2}", self.icons)?;
            text.color = style.color;
            text.render(
                b.x + (20.0 - metrics.width) * 0.5,
                box_rect[1] + (20.0 - metrics.height) * 0.5,
                "\u{eab2}",
                self.icons,
                gpu,
                ui,
            )?;
        }

        text.color = theme.text;
        text.render_layout(
            [b.x + 34.0, b.y + 6.0],
            &self.layout,
            (b.width - 34.0).max(1.0),
            false,
            gpu,
            ui,
        )
    }
}
