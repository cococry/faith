use super::input::Bounds;
use crate::{
    graphics::{FontHandle, GraphicsDevice},
    ui::{TextRenderer, UIRenderer, framework::Style, text::TextLayout, theme::Theme},
};

use std::sync::Arc;

pub const SUFFIX_GAP: f32 = 4.0;

pub struct Checkbox {
    pub bounds: Bounds,
    pub layout: Arc<TextLayout>,
    pub suffix: Option<Arc<TextLayout>>,
    pub suffix_inline: bool,
    pub label_height: f32,
    pub size: f32,
    pub text_offset: f32,
    pub icons: FontHandle,
    pub checked: bool,
    pub style: Style,
    pub radius: f32,
    pub focused: bool,
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
        let center_y = b.y + b.height * 0.5;
        let size = self.size
            + if self.focused {
                2.0
            } else {
                0.0
            };
        let box_rect = [b.x, center_y - size * 0.5, size, size];
        let style = self.style;
        ui.rounded_box(
            box_rect,
            self.radius,
            style.background,
            if self.focused {
                Some(theme.primary)
            } else {
                style.border_color
            },
        )?;

        if self.checked {
            let metrics = text.measure("\u{eab2}", self.icons)?;
            text.color = style.color;
            text.render(
                box_rect[0] + (box_rect[2] - metrics.width) * 0.5,
                box_rect[1] + (box_rect[3] - metrics.height) * 0.5,
                "\u{eab2}",
                self.icons,
                gpu,
                ui,
            )?;
        }

        let x = b.x + self.text_offset;
        let y = center_y - self.label_height * 0.5;
        let width = (b.width - self.text_offset).max(1.0);
        text.color = theme.text;
        text.render_layout([x, y], &self.layout, width, false, gpu, ui)?;

        if let Some(suffix) = &self.suffix {
            let offset = if self.suffix_inline {
                [self.layout.width + SUFFIX_GAP, 0.0]
            } else {
                [0.0, self.layout.height]
            };
            text.color = theme.muted;
            text.render_layout(
                [x + offset[0], y + offset[1]],
                suffix,
                (width - offset[0]).max(1.0),
                false,
                gpu,
                ui,
            )?;
        }

        Ok(())
    }
}
