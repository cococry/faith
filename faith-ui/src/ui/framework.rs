use super::theme::{CornerRadii, Theme};
use super::{
    TextRenderer, UIRenderer,
    text::TextLayoutOptions,
    widgets::{
        button, checkbox, dropdown,
        input::{Bounds, InputField, InputOptions},
    },
};

use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    platform::{
        Platform, WindowEvent,
        mouse::{CursorPosition, MouseButton},
    },
};

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub font_size: u32,
    pub color: Color,
    pub background: Option<Color>,
    /// Button background while hovered; defaults to a 5% darker background.
    pub hover_color: Option<Color>,
    /// Background while held inside; defaults to 5% darker than hover.
    pub click_color: Option<Color>,
    pub border_color: Option<Color>,
    pub padding: f32,
    /// Centers an icon button's icon and label together within its bounds.
    pub center_button_content: bool,
    pub margin: f32,
    pub radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font_size: 16,
            color: Theme::LIGHT.text,
            background: None,
            hover_color: None,
            click_color: None,
            border_color: None,
            padding: 0.0,
            center_button_content: false,
            margin: 0.0,
            radius: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Font {
    #[default]
    Regular,
    Bold,
    #[allow(dead_code)]
    Italic,
    Icons,
}

pub struct Fonts {
    pub regular: PathBuf,
    pub bold: PathBuf,
    pub italic: Option<PathBuf>,
    pub fallbacks: Vec<PathBuf>,
    pub icons: PathBuf,
}
/// Omitted dimensions fit contents. Explicit dimensions include padding.
/// Scrolling is reserved for future support and defaults to false.
#[derive(Clone, Copy, Default)]
pub struct DivOptions {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub scrollable: bool,
}

struct OpenDiv {
    bounds: Bounds,
    width: Option<f32>,
    height: Option<f32>,
    style: Style,
    decorations: Vec<super::renderer::BoxDecoration>,
}

#[derive(Clone, Copy)]
struct Layout {
    bounds: Bounds,
    x: f32,
    y: f32,
    row_y: f32,
    bottom: f32,
    right: f32,
    extent_right: f32,
    inline: bool,
    scrollable: bool,
}

impl Layout {
    fn new(bounds: Bounds, scrollable: bool) -> Self {
        Self {
            bounds,
            x: bounds.x,
            y: bounds.y,
            row_y: bounds.y,
            bottom: bounds.y,
            right: bounds.x,
            extent_right: bounds.x,
            inline: false,
            scrollable,
        }
    }

    fn fitted_size(&self, width: Option<f32>, height: Option<f32>, padding: f32) -> (f32, f32) {
        let inset = 2.0 * padding.max(0.0);
        (
            width.unwrap_or((self.extent_right - self.bounds.x).max(0.0) + inset),
            height.unwrap_or((self.bottom - self.bounds.y).max(0.0) + inset),
        )
    }

    fn place(&mut self, w: f32, h: f32, margin: f32) -> Bounds {
        let b = Bounds {
            x: self.x + margin,
            y: self.y + margin,
            width: w,
            height: h,
        };

        self.right = b.x + w + margin;
        self.extent_right = self.extent_right.max(self.right);
        self.bottom = self.bottom.max(b.y + h + margin);

        if self.inline {
            self.x = self.right;
        } else {
            self.x = self.bounds.x;
            self.y = self.bottom;
        }

        b
    }
}

pub struct Framework {
    theme: Theme,
    rounded_corners: bool,
    dropdowns: HashMap<String, dropdown::State>,
    dropdown_seen: HashSet<String>,
    active_dropdown: Option<String>,
    dropdown_events: Vec<WindowEvent>,
    pending_popup: Option<dropdown::Popup>,
    text_renderer: TextRenderer,
    ui_renderer: UIRenderer,
    fonts: Fonts,
    font_cache: HashMap<(Font, u32), FontHandle>,
    layouts: Vec<Layout>,
    divs: Vec<OpenDiv>,
    styles: Vec<Style>,
    sizing: Vec<(Option<f32>, Option<f32>)>,
    inputs: HashMap<String, InputField>,
    focused: Option<String>,
    events: Vec<WindowEvent>,
    pointer: Option<CursorPosition>,
    batch_pointer: Option<CursorPosition>,
    buttons: HashMap<(String, usize), button::State>,
    button_occurrences: HashMap<String, usize>,
}

impl Framework {
    pub fn new<G: GraphicsDevice>(
        gpu: &mut G,
        platform: &Platform,
        fonts: Fonts,
    ) -> anyhow::Result<Self> {
        let (w, h) = platform.size();
        let mut framework = Self {
            theme: Theme::LIGHT,
            rounded_corners: false,
            dropdowns: HashMap::new(),
            dropdown_seen: HashSet::new(),
            active_dropdown: None,
            dropdown_events: Vec::new(),
            pending_popup: None,
            text_renderer: TextRenderer::new()?,
            ui_renderer: UIRenderer::new(gpu, w, h)?,
            fonts,
            font_cache: HashMap::new(),
            layouts: vec![],
            divs: vec![],
            styles: vec![Style::default()],
            sizing: vec![],
            inputs: HashMap::new(),
            focused: None,
            events: vec![],
            pointer: None,
            batch_pointer: None,
            buttons: HashMap::new(),
            button_occurrences: HashMap::new(),
        };

        framework
            .text_renderer
            .set_fallback_fonts(framework.fonts.fallbacks.clone());
        Ok(framework)
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn set_rounded_corners(&mut self, enabled: bool) {
        self.rounded_corners = enabled;
    }

    pub fn corner_radii(&self) -> CornerRadii {
        if self.rounded_corners {
            self.theme.radii
        } else {
            CornerRadii::SQUARE
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    fn base_style(&self) -> Style {
        Style {
            color: self.theme.text,
            ..Style::default()
        }
    }

    pub fn begin(&mut self, platform: &Platform, events: &[WindowEvent]) {
        let (w, h) = platform.size();
        self.ui_renderer.begin(w, h);
        self.layouts = vec![Layout::new(
            Bounds {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: h as f32,
            },
            false,
        )];
        self.divs.clear();
        self.styles = vec![self.base_style()];
        self.sizing = vec![(None, None)];
        self.events = events.to_vec();
        self.dropdown_events.clear();
        self.dropdown_seen.clear();
        self.pending_popup = None;

        if self.active_dropdown.is_some() {
            self.dropdown_events.extend_from_slice(events);
            self.events.retain(|event| {
                !matches!(
                    event,
                    WindowEvent::MouseButtonPressed { .. }
                        | WindowEvent::MouseButtonReleased { .. }
                        | WindowEvent::MouseScrolled { .. }
                        | WindowEvent::TextInput(_)
                        | WindowEvent::KeyPressed { .. }
                )
            });

            self.events.push(WindowEvent::MouseCancelled);
        }

        self.button_occurrences.clear();
        self.batch_pointer = self.pointer;

        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    self.pointer = Some(*position)
                }

                WindowEvent::MouseLeft | WindowEvent::MouseCancelled => self.pointer = None,
                _ => {}
            }
        }
    }

    pub fn end<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        anyhow::ensure!(self.layouts.len() == 1, "unclosed div");
        self.buttons.retain(|(label, occurrence), _| {
            self.button_occurrences
                .get(label)
                .is_some_and(|count| occurrence < count)
        });

        self.dropdowns
            .retain(|id, _| self.dropdown_seen.contains(id));

        if self
            .active_dropdown
            .as_ref()
            .is_some_and(|id| !self.dropdown_seen.contains(id))
        {
            self.active_dropdown = None;
        }

        if let Some(popup) = self.pending_popup.take() {
            let radii = self.corner_radii();
            popup.render(
                &mut self.text_renderer,
                gpu,
                &mut self.ui_renderer,
                self.theme,
                radii,
            )?;
        }

        self.ui_renderer.end(gpu)
    }

    pub fn style(&self) -> Style {
        *self.styles.last().unwrap()
    }

    pub fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    pub fn pop_style(&mut self) {
        assert!(self.styles.len() > 1);
        self.styles.pop();
    }

    #[allow(dead_code)]
    pub fn push_sizing(&mut self, width: Option<f32>, height: Option<f32>) {
        self.sizing.push((width, height));
    }

    #[allow(dead_code)]
    pub fn pop_sizing(&mut self) {
        assert!(self.sizing.len() > 1);
        self.sizing.pop();
    }

    pub fn div_width(&self) -> f32 {
        self.layouts.last().unwrap().bounds.width
    }

    pub fn div_height(&self) -> f32 {
        self.layouts.last().unwrap().bounds.height
    }

    pub fn position(&self) -> (f32, f32) {
        let layout = self.layouts.last().unwrap();
        (layout.x - layout.bounds.x, layout.y - layout.bounds.y)
    }

    pub fn set_position(&mut self, x: f32, y: f32) {
        let l = self.layouts.last_mut().unwrap();
        l.x = l.bounds.x + x;
        l.y = l.bounds.y + y;
        l.row_y = l.y;
    }

    pub fn space(&mut self, height: f32) {
        let layout = self.layouts.last_mut().unwrap();
        layout.inline = false;
        layout.x = layout.bounds.x;
        layout.y = layout.bottom + height.max(0.0);
        layout.bottom = layout.y;
        layout.row_y = layout.y;
    }

    /// Keeps subsequent widgets on this row until next_line(), without wrapping.
    #[allow(dead_code)]
    pub fn same_line(&mut self) {
        let l = self.layouts.last_mut().unwrap();
        l.inline = true;
        l.x = l.right;
        l.y = l.row_y;
    }

    #[allow(dead_code)]
    pub fn next_line(&mut self) {
        let l = self.layouts.last_mut().unwrap();
        l.inline = false;
        l.x = l.bounds.x;
        l.y = l.bottom;
        l.row_y = l.y;
    }

    fn place(&mut self, w: f32, h: f32) -> Bounds {
        let s = self.style();
        let (sw, sh) = *self.sizing.last().unwrap();
        let l = self.layouts.last_mut().unwrap();

        if !l.inline {
            l.row_y = l.y;
        }

        l.place(
            sw.unwrap_or(w).max(0.0),
            sh.unwrap_or(h).max(0.0),
            s.margin.max(0.0),
        )
    }

    fn paint(&mut self, b: Bounds) -> anyhow::Result<()> {
        self.paint_style(b, self.style())
    }

    fn paint_style(&mut self, b: Bounds, s: Style) -> anyhow::Result<()> {
        self.ui_renderer.rounded_box(
            [b.x, b.y, b.width, b.height],
            if self.rounded_corners {
                s.radius
            } else {
                0.0
            },
            s.background,
            s.border_color,
        )?;
        Ok(())
    }

    pub fn div_begin(&mut self, options: DivOptions) -> anyhow::Result<()> {
        let s = self.style();
        let sizing = *self.sizing.last().unwrap();
        let width = options.width.or(sizing.0).map(|v| v.max(0.0));
        let height = options.height.or(sizing.1).map(|v| v.max(0.0));
        let parent = *self.layouts.last().unwrap();
        let margin = s.margin.max(0.0);
        let b = Bounds {
            x: parent.x + margin,
            y: parent.y + margin,
            width: width.unwrap_or(
                (parent.bounds.x + parent.bounds.width - parent.x - 2.0 * margin).max(0.0),
            ),
            height: height.unwrap_or(
                (parent.bounds.y + parent.bounds.height - parent.y - 2.0 * margin).max(0.0),
            ),
        };

        let mut decorations = Vec::new();

        if s.background.is_some() || s.border_color.is_some() {
            decorations.push(self.ui_renderer.reserve_box(
                s.background.unwrap_or(Color::rgba(0.0, 0.0, 0.0, 0.0)),
                s.border_color,
                if self.rounded_corners {
                    s.radius
                } else {
                    0.0
                },
            )?);
        }

        self.divs.push(OpenDiv {
            bounds: b,
            width,
            height,
            style: s,
            decorations,
        });

        let p = s.padding.max(0.0);
        let content = Bounds {
            x: b.x + p,
            y: b.y + p,
            width: (b.width - 2.0 * p).max(0.0),
            height: (b.height - 2.0 * p).max(0.0),
        };

        self.layouts.push(Layout::new(content, options.scrollable));
        // Auto axes are unconstrained until their contents have been measured.
        self.ui_renderer.push_clip([
            content.x,
            content.y,
            if width.is_some() {
                content.width
            } else {
                f32::MAX
            },
            if height.is_some() {
                content.height
            } else {
                f32::MAX
            },
        ]);
        self.styles.push(Style {
            font_size: s.font_size,
            ..self.base_style()
        });

        self.sizing.push((None, None));
        Ok(())
    }

    pub fn div_end(&mut self) {
        assert!(self.layouts.len() > 1);
        self.ui_renderer.pop_clip();
        let layout = self.layouts.pop().unwrap();
        let _scrollable = layout.scrollable;
        let div = self.divs.pop().unwrap();
        let (w, h) = layout.fitted_size(div.width, div.height, div.style.padding);
        let b = Bounds {
            width: w,
            height: h,
            ..div.bounds
        };

        for decoration in div.decorations {
            self.ui_renderer.resolve_box(decoration, [b.x, b.y, w, h]);
        }

        let parent = self.layouts.last_mut().unwrap();

        if !parent.inline {
            parent.row_y = parent.y;
        }

        parent.place(w, h, div.style.margin.max(0.0));
        self.styles.pop();
        self.sizing.pop();
    }

    fn font(&mut self, font: Font) -> anyhow::Result<FontHandle> {
        self.font_at(font, self.style().font_size.max(1))
    }

    fn font_at(&mut self, font: Font, size: u32) -> anyhow::Result<FontHandle> {
        if let Some(f) = self.font_cache.get(&(font, size)) {
            return Ok(*f);
        }

        let path = match font {
            Font::Regular => &self.fonts.regular,
            Font::Bold => &self.fonts.bold,
            Font::Italic => self.fonts.italic.as_ref().unwrap_or(&self.fonts.regular),
            Font::Icons => &self.fonts.icons,
        };

        let handle = self.text_renderer.load_primary_font(path, size)?;
        self.font_cache.insert((font, size), handle);
        Ok(handle)
    }

    pub fn text_size(&mut self, value: &str, font: Option<Font>) -> anyhow::Result<(f32, f32)> {
        let font = self.font(font.unwrap_or_default())?;
        let layout = self.text_renderer.measure(value, font)?;
        Ok((layout.width, layout.height))
    }

    pub fn text<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        value: &str,
        font: Option<Font>,
    ) -> anyhow::Result<()> {
        self.text_aligned(gpu, value, font, false)
    }

    pub fn text_centered<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        value: &str,
        font: Option<Font>,
    ) -> anyhow::Result<()> {
        self.text_aligned(gpu, value, font, true)
    }

    fn text_aligned<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        value: &str,
        font: Option<Font>,
        centered: bool,
    ) -> anyhow::Result<()> {
        let font = self.font(font.unwrap_or_default())?;
        let s = self.style();
        let padding = s.padding.max(0.0);
        let margin = s.margin.max(0.0);
        let fixed_width = self.sizing.last().unwrap().0;
        let parent = self.layouts.last().unwrap();
        let available = (parent.bounds.x + parent.bounds.width - parent.x - 2.0 * margin).max(0.0);
        let outer_width = fixed_width.unwrap_or(available).max(0.0);
        let content_width = (outer_width - 2.0 * padding).max(1.0);
        let layout = self.text_renderer.measure_with_options(
            value,
            font,
            TextLayoutOptions {
                max_width: content_width,
            },
        )?;
        let natural =
            fixed_width.is_none() && self.divs.last().is_some_and(|div| div.width.is_none());

        let text_width = if natural {
            layout.width.min(content_width) + 2.0 * padding
        } else {
            outer_width
        };

        let b = self.place(text_width, layout.height + 2.0 * padding);
        self.paint(b)?;
        self.text_renderer.color = s.color;
        self.ui_renderer.push_clip(content_clip(b, s));
        let result = self.text_renderer.render_layout(
            [b.x + padding, b.y + padding],
            &layout,
            (b.width - 2.0 * padding).max(1.0),
            centered,
            gpu,
            &mut self.ui_renderer,
        );
        self.ui_renderer.pop_clip();
        result?;
        Ok(())
    }

    pub fn checkbox<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        checked: bool,
    ) -> anyhow::Result<Option<bool>> {
        let font = self.font(Font::Regular)?;
        let icons = self.font(Font::Icons)?;
        let width = self.sizing.last().unwrap().0.unwrap_or(self.div_width());
        let layout = self.text_renderer.measure_with_options(
            label,
            font,
            TextLayoutOptions {
                max_width: (width - 34.0).max(1.0),
            },
        )?;
        let bounds = self.place(width, layout.height.max(20.0) + 12.0);
        let occurrence = self.button_occurrences.entry(id.to_owned()).or_default();
        let key = (id.to_owned(), *occurrence);
        *occurrence += 1;
        let response =
            self.buttons
                .entry(key)
                .or_default()
                .update(bounds, self.batch_pointer, &self.events);
        let widget = checkbox::Checkbox {
            bounds,
            layout,
            icons,
            checked,
            hovered: response.hovered,
            pressed: response.pressed,
            radius: self.corner_radii().checkbox,
        };

        widget.render(
            &mut self.text_renderer,
            gpu,
            &mut self.ui_renderer,
            self.theme,
        )?;
        Ok(response.clicked.then_some(!checked))
    }

    pub fn dropdown<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        options: &[&str],
        selected: usize,
    ) -> anyhow::Result<Option<usize>> {
        anyhow::ensure!(
            !options.is_empty() && selected < options.len(),
            "invalid dropdown selection"
        );
        self.dropdown_seen.insert(id.to_owned());
        let font = self.font(Font::Regular)?;
        let icons = self.font(Font::Icons)?;
        let style = self.style();
        let metrics = self.text_renderer.measure(options[selected], font)?;
        let bounds = self.place(
            self.div_width() - 2.0 * style.margin,
            metrics.height + 2.0 * style.padding,
        );
        let window = self.layouts.first().unwrap().bounds;
        let popup = dropdown::popup_bounds(bounds, options.len(), [window.width, window.height]);

        let events = if self.active_dropdown.as_deref() == Some(id) {
            &self.dropdown_events
        } else {
            &self.events
        };

        let state = self.dropdowns.entry(id.to_owned()).or_default();
        let response = state.update(bounds, popup, options.len(), self.batch_pointer, events);
        let open = state.open;

        if open {
            self.active_dropdown = Some(id.to_owned());
            self.pending_popup = Some(dropdown::Popup {
                bounds: popup,
                options: options.iter().map(|value| (*value).to_owned()).collect(),
                selected,
                hovered: response.hovered_row,
                font,
                icons,
            });
        } else if self.active_dropdown.as_deref() == Some(id) {
            self.active_dropdown = None;
        }

        let style = button::appearance(style, response.hovered, response.pressed);
        self.paint_style(bounds, style)?;
        self.ui_renderer.push_clip(content_clip(bounds, style));
        self.text_renderer.color = style.color;
        let result = (|| -> anyhow::Result<()> {
            let glyph = if open {
                dropdown::UP
            } else {
                dropdown::DOWN
            };

            let gm = self.text_renderer.measure(glyph, icons)?;
            let right = bounds.x + bounds.width - style.padding - gm.width;
            self.ui_renderer.push_clip([
                bounds.x,
                bounds.y,
                (right - bounds.x - 12.0).max(0.0),
                bounds.height,
            ]);
            let result = self.text_renderer.render(
                bounds.x + style.padding,
                bounds.y + (bounds.height - metrics.height) * 0.5,
                options[selected],
                font,
                gpu,
                &mut self.ui_renderer,
            );
            self.ui_renderer.pop_clip();
            result?;
            self.text_renderer.render(
                right,
                bounds.y + (bounds.height - gm.height) * 0.5,
                glyph,
                icons,
                gpu,
                &mut self.ui_renderer,
            )
        })();
        self.ui_renderer.pop_clip();
        result?;
        Ok(response.selected)
    }

    pub fn load_image<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        path: &std::path::Path,
    ) -> anyhow::Result<super::renderer::Image> {
        self.ui_renderer.load_image(gpu, path)
    }

    pub fn image(
        &mut self,
        image: super::renderer::Image,
        width: f32,
        height: f32,
    ) -> anyhow::Result<()> {
        let b = self.place(width, height);
        self.ui_renderer.image(b.x, b.y, b.width, b.height, image)
    }

    pub fn icon_button<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        icon: &str,
    ) -> anyhow::Result<bool> {
        self.icon_label_button(gpu, id, icon, "")
    }

    pub fn icon_label_button<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        icon: &str,
        label: &str,
    ) -> anyhow::Result<bool> {
        self.icon_label_badge_button(gpu, id, icon, label, None)
    }

    pub fn icon_label_badge_button<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        icon: &str,
        label: &str,
        badge: Option<&str>,
    ) -> anyhow::Result<bool> {
        let font = self.font(Font::Regular)?;
        let icons = self.font(Font::Icons)?;
        let s = self.style();
        let im = self.text_renderer.measure(icon, icons)?;
        let lm = self.text_renderer.measure(label, font)?;

        let badge_layout = if let Some(value) = badge {
            let font = self.font_at(Font::Regular, 11)?;
            Some((font, self.text_renderer.measure(value, font)?))
        } else {
            None
        };

        let badge_width = badge_layout
            .as_ref()
            .map_or(0.0, |(_, layout)| layout.width + 16.0);

        let gap = if label.is_empty() {
            0.0
        } else {
            10.0
        };

        let b = self.place(
            im.width
                + gap
                + lm.width
                + s.padding * 2.0
                + badge_width
                + if badge.is_some() {
                    10.0
                } else {
                    0.0
                },
            im.height.max(lm.height) + s.padding * 2.0,
        );
        let occurrence = self.button_occurrences.entry(id.to_owned()).or_default();
        let key = (id.to_owned(), *occurrence);
        *occurrence += 1;
        let response =
            self.buttons
                .entry(key)
                .or_default()
                .update(b, self.batch_pointer, &self.events);
        let s = button::appearance(s, response.hovered, response.pressed);
        self.paint_style(b, s)?;
        self.ui_renderer.push_clip(content_clip(b, s));
        self.text_renderer.color = s.color;
        let result = (|| -> anyhow::Result<()> {
            let x = if label.is_empty() {
                b.x + (b.width - im.width) * 0.5
            } else if s.center_button_content && badge.is_none() {
                b.x + ((b.width - im.width - gap - lm.width) * 0.5).max(s.padding)
            } else {
                b.x + s.padding
            };

            self.text_renderer.render(
                x,
                b.y + (b.height - im.height) * 0.5,
                icon,
                icons,
                gpu,
                &mut self.ui_renderer,
            )?;
            let badge_x = b.x + b.width - s.padding - badge_width;

            if !label.is_empty() {
                let label_x = x + im.width + gap;

                let right = if badge.is_some() {
                    badge_x - 10.0
                } else {
                    b.x + b.width - s.padding
                };

                self.ui_renderer
                    .push_clip([label_x, b.y, (right - label_x).max(0.0), b.height]);
                let result = self.text_renderer.render(
                    label_x,
                    b.y + (b.height - lm.height) * 0.5,
                    label,
                    font,
                    gpu,
                    &mut self.ui_renderer,
                );
                self.ui_renderer.pop_clip();
                result?;
            }

            if let (Some(value), Some((font, layout))) = (badge, badge_layout) {
                let height = (layout.height + 6.0).min((b.height - 2.0 * s.padding).max(0.0));
                let top = b.y + (b.height - height) * 0.5;
                self.ui_renderer.rounded_box(
                    [badge_x, top, badge_width, height],
                    self.corner_radii().badge,
                    Some(self.theme.primary),
                    None,
                )?;
                self.text_renderer.color = self.theme.primary_text;
                self.text_renderer.render(
                    badge_x + 8.0,
                    b.y + (b.height - layout.height) * 0.5,
                    value,
                    font,
                    gpu,
                    &mut self.ui_renderer,
                )?;
            }

            Ok(())
        })();
        self.ui_renderer.pop_clip();
        result?;
        Ok(response.clicked)
    }

    #[allow(dead_code)]
    pub fn button<G: GraphicsDevice>(&mut self, gpu: &mut G, label: &str) -> anyhow::Result<bool> {
        let font = self.font(Font::Regular)?;
        let s = self.style();
        let m = self.text_renderer.measure(label, font)?;
        let b = self.place(m.width + 2.0 * s.padding, m.height + 2.0 * s.padding);
        let occurrence = self.button_occurrences.entry(label.into()).or_default();
        let key = (label.to_owned(), *occurrence);
        *occurrence += 1;
        let state = self.buttons.entry(key).or_default();
        let response = state.update(b, self.batch_pointer, &self.events);
        let s = button::appearance(s, response.hovered, response.pressed);
        self.paint_style(b, s)?;
        self.ui_renderer.push_clip(content_clip(b, s));
        let result = button::render(
            label,
            b,
            s,
            font,
            &mut self.text_renderer,
            gpu,
            &mut self.ui_renderer,
        );
        self.ui_renderer.pop_clip();
        result?;
        Ok(response.clicked)
    }

    #[allow(dead_code)]
    pub fn input<G: GraphicsDevice>(&mut self, gpu: &mut G, id: &str) -> anyhow::Result<String> {
        self.input_with_options(gpu, id, InputOptions::default())
    }

    pub fn clear_input(&mut self, id: &str) {
        self.inputs.remove(id);

        if self.focused.as_deref() == Some(id) {
            self.focused = None;
        }
    }

    pub fn input_with_options<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        options: InputOptions,
    ) -> anyhow::Result<String> {
        let font = self.font(Font::Regular)?;
        let s = self.style();
        let b = self.place(
            if self.divs.last().is_some_and(|div| div.width.is_none()) {
                240.0
            } else {
                self.div_width() - 2.0 * s.margin
            },
            s.font_size as f32 * 1.5 + 2.0 * s.padding,
        );
        self.paint(b)?;
        let field = self
            .inputs
            .entry(id.into())
            .or_insert_with(|| InputField::new(4096));
        field.set_options(options);
        dispatch_input(
            field,
            &mut self.focused,
            id,
            b,
            &self.events,
            self.batch_pointer,
        );
        self.text_renderer.color = s.color;
        self.ui_renderer.push_clip(content_clip(b, s));
        let result = field.render(
            b,
            s,
            font,
            &mut self.text_renderer,
            gpu,
            &mut self.ui_renderer,
        );
        self.ui_renderer.pop_clip();
        result?;
        Ok(field.text().to_owned())
    }
}

fn dispatch_input(
    field: &mut InputField,
    focused: &mut Option<String>,
    id: &str,
    bounds: Bounds,
    events: &[WindowEvent],
    mut pointer: Option<CursorPosition>,
) {
    field.set_focused(focused.as_deref() == Some(id));

    for event in events {
        match event {
            WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                pointer = Some(*position)
            }

            WindowEvent::MouseLeft | WindowEvent::MouseCancelled => pointer = None,
            WindowEvent::MouseButtonPressed {
                button: MouseButton::Left,
            } => {
                if pointer.is_some_and(|p| contains(bounds, p)) {
                    *focused = Some(id.into());
                } else if focused.as_deref() == Some(id) {
                    *focused = None;
                }

                field.set_focused(focused.as_deref() == Some(id));
            }

            _ => {}
        }

        if !matches!(event, WindowEvent::FocusChanged(true)) {
            field.handle(event);
        }
    }
}

fn content_clip(b: Bounds, style: Style) -> [f32; 4] {
    let p = style.padding.max(0.0);
    [
        b.x + p,
        b.y + p,
        (b.width - 2.0 * p).max(0.0),
        (b.height - 2.0 * p).max(0.0),
    ]
}

fn contains(b: Bounds, p: CursorPosition) -> bool {
    p.x >= b.x as f64
        && p.y >= b.y as f64
        && p.x < (b.x + b.width) as f64
        && p.y < (b.y + b.height) as f64
}
