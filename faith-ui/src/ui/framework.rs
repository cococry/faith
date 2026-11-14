use super::{
    TextRenderer, UIRenderer,
    text::TextLayoutOptions,
    widgets::{
        button,
        input::{Bounds, InputField},
    },
};
use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    platform::{
        Platform, WindowEvent,
        mouse::{CursorPosition, MouseButton},
    },
};
use std::{collections::HashMap, path::PathBuf};

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
    pub margin: f32,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            font_size: 16,
            color: Color::rgba(0.12, 0.14, 0.18, 1.0),
            background: None,
            hover_color: None,
            click_color: None,
            border_color: None,
            padding: 0.0,
            margin: 0.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Font {
    #[default]
    Regular,
    Bold,
    Italic,
}
pub struct Fonts {
    pub regular: PathBuf,
    pub bold: PathBuf,
    pub italic: Option<PathBuf>,
    pub fallbacks: Vec<PathBuf>,
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
    decorations: Vec<usize>,
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
        for font in [Font::Regular, Font::Bold, Font::Italic] {
            framework.font(font)?;
        }
        Ok(framework)
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
        self.styles = vec![Style::default()];
        self.sizing = vec![(None, None)];
        self.events = events.to_vec();
        self.button_occurrences.clear();
        self.batch_pointer = self.pointer;
        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    self.pointer = Some(*position)
                }
                WindowEvent::MouseLeft
                | WindowEvent::MouseCancelled
                | WindowEvent::FocusChanged(false) => self.pointer = None,
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
    pub fn set_position(&mut self, x: f32, y: f32) {
        let l = self.layouts.last_mut().unwrap();
        l.x = l.bounds.x + x;
        l.y = l.bounds.y + y;
        l.row_y = l.y;
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
        if let Some(c) = s.background {
            self.ui_renderer.quad([b.x, b.y, b.width, b.height], c)?;
        }
        if let Some(c) = s.border_color {
            for r in [
                [b.x, b.y, b.width, 1.0],
                [b.x, b.y + b.height - 1.0, b.width, 1.0],
                [b.x, b.y, 1.0, b.height],
                [b.x + b.width - 1.0, b.y, 1.0, b.height],
            ] {
                self.ui_renderer.quad(r, c)?;
            }
        }
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
        if let Some(color) = s.background {
            decorations.push(self.ui_renderer.reserve_box(color)?);
        }
        if let Some(color) = s.border_color {
            for _ in 0..4 {
                decorations.push(self.ui_renderer.reserve_box(color)?);
            }
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
            ..Style::default()
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
        let mut indices = div.decorations.into_iter();
        if div.style.background.is_some() {
            self.ui_renderer
                .resolve_box(indices.next().unwrap(), [b.x, b.y, w, h]);
        }
        if div.style.border_color.is_some() {
            for rect in [
                [b.x, b.y, w, 1.0_f32.min(h)],
                [b.x, b.y + (h - 1.0).max(0.0), w, 1.0_f32.min(h)],
                [b.x, b.y, 1.0_f32.min(w), h],
                [b.x + (w - 1.0).max(0.0), b.y, 1.0_f32.min(w), h],
            ] {
                self.ui_renderer.resolve_box(indices.next().unwrap(), rect);
            }
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
        let size = self.style().font_size.max(1);
        if let Some(f) = self.font_cache.get(&(font, size)) {
            return Ok(*f);
        }
        let path = match font {
            Font::Regular => &self.fonts.regular,
            Font::Bold => &self.fonts.bold,
            Font::Italic => self.fonts.italic.as_ref().unwrap_or(&self.fonts.regular),
        };
        let handle = self.text_renderer.load_font(path, size)?;
        for path in &self.fonts.fallbacks {
            self.text_renderer.load_font(path, size)?;
        }
        self.font_cache.insert((font, size), handle);
        Ok(handle)
    }
    pub fn text<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        value: &str,
        font: Option<Font>,
    ) -> anyhow::Result<()> {
        let font = self.font(font.unwrap_or_default())?;
        let s = self.style();
        let width = self.sizing.last().unwrap().0.unwrap_or(self.div_width())
            - 2.0 * (s.padding + s.margin);
        let layout = self.text_renderer.measure_with_options(
            value,
            font,
            TextLayoutOptions {
                max_width: width.max(1.0),
            },
        )?;
        let natural = self.divs.last().is_some_and(|div| div.width.is_none());
        let text_width = if natural {
            layout.width.min(width)
        } else {
            width
        };
        let b = self.place(
            text_width + 2.0 * s.padding,
            layout.height + 2.0 * s.padding,
        );
        self.paint(b)?;
        self.text_renderer.color = s.color;
        self.ui_renderer.push_clip(content_clip(b, s));
        let result = self.text_renderer.render_wrapped(
            [b.x + s.padding, b.y + s.padding],
            value,
            font,
            (b.width - 2.0 * s.padding).max(1.0),
            gpu,
            &mut self.ui_renderer,
        );
        self.ui_renderer.pop_clip();
        result?;
        Ok(())
    }
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
    pub fn input<G: GraphicsDevice>(&mut self, gpu: &mut G, id: &str) -> anyhow::Result<String> {
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
