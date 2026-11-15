use super::animation::{Curve, Scheduler, Transition};
use super::scroll::{self, Areas, PrepareOptions, View};
use super::theme::{CornerRadii, Theme};
use super::{
    TextRenderer, UIRenderer,
    text::TextLayoutOptions,
    widgets::{
        button, checkbox, dropdown,
        input::{Bounds, InputField, InputOptions},
    },
};
use std::hash::{Hash, Hasher};
use std::time::Instant;

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
            color: Theme::default().text,
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
/// Scrolling applies to constrained axes and defaults to false.
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
    scroll_id: Option<u64>,
    outer_clip: Bounds,
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
    id: u64,
    children: usize,
}

impl Layout {
    fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            x: bounds.x,
            y: bounds.y,
            row_y: bounds.y,
            bottom: bounds.y,
            right: bounds.x,
            extent_right: bounds.x,
            inline: false,
            id: 0,
            children: 0,
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
    zoom: f32,
    zoom_indicator: super::scale::Indicator,
    zoom_indicator_pending: bool,
    scale: f32,
    pointer_factor: f32,
    theme: Theme,
    theme_transition: Option<(Theme, Theme, Instant)>,
    theme_animating: bool,
    animations: bool,
    curve: Curve,
    transitions: HashMap<(String, usize), Transition>,
    scheduler: Scheduler,
    frame_time: Instant,
    has_rendered: bool,
    deadline: Option<Instant>,
    rounded_corners: bool,
    dropdowns: HashMap<String, dropdown::State>,
    dropdown_seen: HashSet<String>,
    active_dropdown: Option<String>,
    dropdown_events: Vec<WindowEvent>,
    pending_popup: Option<dropdown::Popup>,
    dropdown_selection: Option<(String, usize)>,
    scrolls: Areas,
    scrollbars: Vec<(View, Bounds)>,
    smooth_scrolling: bool,
    overlay: bool,
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
            zoom: 1.0,
            zoom_indicator: super::scale::Indicator::default(),
            zoom_indicator_pending: false,
            scale: platform.scale_factor(),
            pointer_factor: platform.pointer_scale() / platform.scale_factor(),
            theme: Theme::default(),
            theme_transition: None,
            theme_animating: false,
            animations: true,
            curve: Curve::default(),
            transitions: HashMap::new(),
            scheduler: Scheduler::new(platform.waker())?,
            frame_time: Instant::now(),
            has_rendered: false,
            deadline: None,
            rounded_corners: false,
            dropdowns: HashMap::new(),
            dropdown_seen: HashSet::new(),
            active_dropdown: None,
            dropdown_events: Vec::new(),
            pending_popup: None,
            dropdown_selection: None,
            scrolls: Areas::default(),
            scrollbars: Vec::new(),
            smooth_scrolling: true,
            overlay: false,
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

    pub fn set_smooth_scrolling(&mut self, enabled: bool) {
        self.smooth_scrolling = enabled;
    }

    fn clip_bounds(&self) -> Bounds {
        let [x, y, right, bottom] = self.ui_renderer.clip();
        Bounds {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }

    fn hit_bounds(&self, bounds: Bounds) -> Bounds {
        scroll::intersect(bounds, self.clip_bounds())
    }

    fn visible(&self, bounds: Bounds) -> bool {
        self.ui_renderer
            .visible([bounds.x, bounds.y, bounds.width, bounds.height])
    }

    pub fn set_animations(&mut self, enabled: bool, curve: Curve) {
        self.animations = enabled;
        self.curve = curve;

        if !enabled && let Some((_, target, _)) = self.theme_transition.take() {
            self.theme = target;
        }
    }

    pub fn animate_until(&mut self, at: Instant) {
        let next = self.scheduler.next_frame(self.frame_time).max(at);
        self.deadline = Some(self.deadline.map_or(next, |old| old.min(next)));
    }

    fn animate_style(&mut self, key: (String, usize), style: Style) -> Style {
        let transition = self
            .transitions
            .entry(key)
            .or_insert_with(|| Transition::new(style, self.frame_time));
        let (style, active) = transition.update(
            style,
            self.frame_time,
            self.curve,
            self.animations && !self.theme_animating,
        );

        if active {
            self.animate_until(self.frame_time);
        }

        style
    }

    pub fn update_animations(&mut self) {
        self.theme_animating = self.theme_transition.is_some();

        if let Some((from, target, start)) = self.theme_transition {
            let t = start.elapsed().as_secs_f32() / super::animation::DURATION.as_secs_f32();
            self.theme = from.interpolate(target, self.curve.sample(t));

            if t >= 1.0 {
                self.theme = target;
                self.theme_transition = None;
            }
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if self.animations && self.has_rendered {
            self.theme_transition = Some((self.theme, theme, Instant::now()));
        } else {
            self.theme = theme;
            self.theme_transition = None;
        }
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
        self.frame_time = Instant::now();
        self.deadline = None;

        if self.theme_transition.is_some() {
            self.animate_until(self.frame_time);
        }

        for transition in self.transitions.values_mut() {
            transition.seen = false;
        }

        let previous_scale = self.scale;
        for event in events {
            if let WindowEvent::Zoom(steps) = event {
                self.zoom = super::scale::zoom(self.zoom, *steps);
                if steps.is_finite() && *steps != 0.0 {
                    self.zoom_indicator_pending = true;
                }
            }
        }
        self.scale = platform.scale_factor() * self.zoom;
        let pointer_factor = platform.pointer_scale() / self.scale;
        if let Some(pointer) = &mut self.pointer {
            pointer.x *= (pointer_factor / self.pointer_factor) as f64;
            pointer.y *= (pointer_factor / self.pointer_factor) as f64;
        }
        self.pointer_factor = pointer_factor;
        if previous_scale != self.scale {
            self.scrolls.invalidate_layout();
        }
        let events: Vec<_> = events
            .iter()
            .filter_map(|event| {
                let mut event = event.clone();
                match &mut event {
                    WindowEvent::Zoom(_) => return None,
                    WindowEvent::MouseEntered { position }
                    | WindowEvent::MouseMoved { position } => {
                        let factor = (platform.pointer_scale() / self.scale) as f64;
                        position.x *= factor;
                        position.y *= factor;
                    }
                    WindowEvent::MouseScrolled {
                        delta: crate::platform::mouse::ScrollDelta::Pixels { x, y },
                    } => {
                        let factor = (platform.pointer_scale() / self.scale) as f64;
                        *x *= factor;
                        *y *= factor;
                    }
                    _ => {}
                }
                Some(event)
            })
            .collect();
        let events = events.as_slice();
        self.text_renderer.set_scale(self.scale);
        let (w, h) = platform.size();
        let (w, h) = (w as f32 / self.scale, h as f32 / self.scale);
        self.ui_renderer.begin(w, h);
        self.layouts = vec![Layout::new(Bounds {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        })];
        self.divs.clear();
        self.styles = vec![self.base_style()];
        self.sizing = vec![(None, None)];
        self.events = self.scrolls.begin(
            events,
            self.pointer,
            self.frame_time,
            self.smooth_scrolling,
            self.curve,
            self.active_dropdown.is_some(),
        );
        self.dropdown_events.clone_from(&self.events);
        self.scrollbars.clear();
        self.dropdown_seen.clear();
        self.pending_popup = None;

        if self.active_dropdown.is_some() {
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

        self.dropdowns
            .retain(|id, _| self.dropdown_seen.contains(id));

        if self
            .active_dropdown
            .as_ref()
            .is_some_and(|id| !self.dropdown_seen.contains(id))
        {
            self.active_dropdown = None;
        }

        self.render_scrollbars()?;

        if let Some(popup) = self.pending_popup.take() {
            self.render_dropdown_popup(gpu, popup)?;
            self.render_scrollbars()?;
        }

        self.buttons.retain(|(label, occurrence), _| {
            self.button_occurrences
                .get(label)
                .is_some_and(|count| occurrence < count)
        });

        self.has_rendered = true;
        self.render_zoom_indicator(gpu)?;
        self.transitions.retain(|_, transition| transition.seen);
        self.scheduler.schedule(self.deadline);
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

    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> [f32; 2] {
        let Some(div) = self.divs.last().filter(|div| div.scroll_id.is_some()) else {
            return [0.0; 2];
        };
        let layout = self.layouts.last().unwrap();
        let p = div.style.padding.max(0.0);
        [
            div.bounds.x + p - layout.bounds.x,
            div.bounds.y + p - layout.bounds.y,
        ]
    }

    /* fixed-height lists can skip hidden rows without measuring their text */
    pub fn visible_rows(&self, row_height: f32, count: usize) -> std::ops::Range<usize> {
        let origin = self.layouts.last().unwrap().bounds.y;
        let clip = self.clip_bounds();
        let height = row_height.max(1.0);
        let first = (((clip.y - origin) / height).floor().max(0.0) as usize).min(count);
        let end =
            ((((clip.y + clip.height) - origin) / height).ceil().max(0.0) as usize).min(count);
        first..end.max(first)
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
        self.div_begin_impl(None, options)
    }

    /* names keep scroll positions stable when surrounding containers change */
    pub fn div_begin_id(&mut self, id: &str, options: DivOptions) -> anyhow::Result<()> {
        self.div_begin_impl(Some(id), options)
    }

    fn div_begin_impl(&mut self, name: Option<&str>, options: DivOptions) -> anyhow::Result<()> {
        let s = self.style();
        let sizing = *self.sizing.last().unwrap();
        let width = options.width.or(sizing.0).map(|v| v.max(0.0));
        let height = options.height.or(sizing.1).map(|v| v.max(0.0));
        let parent = self.layouts.last_mut().unwrap();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        parent.id.hash(&mut hasher);

        if let Some(name) = name {
            name.hash(&mut hasher);
        } else {
            parent.children.hash(&mut hasher);
        }

        parent.children += 1;
        let id = hasher.finish();
        let parent = *parent;
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

        if self.visible(b) && (s.background.is_some() || s.border_color.is_some()) {
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

        let outer_clip = self.clip_bounds();
        self.divs.push(OpenDiv {
            bounds: b,
            width,
            height,
            style: s,
            decorations,
            scroll_id: options.scrollable.then_some(id),
            outer_clip,
        });

        let p = s.padding.max(0.0);
        let content = Bounds {
            x: b.x + p,
            y: b.y + p,
            width: (b.width - 2.0 * p).max(0.0),
            height: (b.height - 2.0 * p).max(0.0),
        };

        let (viewport, offset) = if options.scrollable {
            let (view, active) = self.scrolls.prepare(
                id,
                content,
                PrepareOptions {
                    axes: [width.is_some(), height.is_some()],
                    clip: outer_clip,
                    modal: self.overlay,
                    now: self.frame_time,
                    smooth: self.smooth_scrolling,
                    animations: self.animations,
                    curve: self.curve,
                },
            );

            if active && self.visible(content) {
                self.animate_until(self.frame_time);
            }

            (view.viewport, view.offset)
        } else {
            (content, [0.0; 2])
        };
        let mut layout = Layout::new(Bounds {
            x: viewport.x - offset[0],
            y: viewport.y - offset[1],
            ..viewport
        });
        layout.id = id;
        self.layouts.push(layout);
        // Auto axes are unconstrained until their contents have been measured.
        self.ui_renderer.push_clip([
            viewport.x,
            viewport.y,
            if width.is_some() {
                viewport.width
            } else {
                f32::MAX
            },
            if height.is_some() {
                viewport.height
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
        let div = self.divs.pop().unwrap();
        let (mut w, mut h) = layout.fitted_size(div.width, div.height, div.style.padding);
        let mut b = Bounds {
            width: w,
            height: h,
            ..div.bounds
        };

        if let Some(id) = div.scroll_id {
            let p = div.style.padding.max(0.0);
            let bounds = Bounds {
                x: b.x + p,
                y: b.y + p,
                width: (w - 2.0 * p).max(0.0),
                height: (h - 2.0 * p).max(0.0),
            };
            let extent = [
                (layout.extent_right - layout.bounds.x).max(0.0),
                (layout.bottom - layout.bounds.y).max(0.0),
            ];
            let (view, changed) = self.scrolls.finish(
                id,
                bounds,
                extent,
                [div.width.is_some(), div.height.is_some()],
            );

            if div.width.is_none() {
                w = view.bounds.width + 2.0 * p;
                b.width = w;
            }

            if div.height.is_none() {
                h = view.bounds.height + 2.0 * p;
                b.height = h;
            }

            if changed {
                self.animate_until(self.frame_time);
            }

            if view.maximum.iter().any(|value| *value > 0.0) {
                self.scrollbars.push((view, div.outer_clip));
            }
        }

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

        if !self.visible(b) {
            return Ok(());
        }

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
        let hit = self.hit_bounds(bounds);
        let response = self.buttons.entry(key.clone()).or_default().update(
            hit,
            self.batch_pointer,
            &self.events,
        );
        if !self.visible(bounds) {
            return Ok(None);
        }

        let style = button::appearance(
            Style {
                background: Some(if checked {
                    self.theme.primary
                } else {
                    self.theme.input
                }),
                color: if checked {
                    self.theme.primary_text
                } else {
                    self.theme.text
                },
                border_color: (!checked).then_some(self.theme.border),
                ..Style::default()
            },
            response.hovered,
            response.pressed,
        );
        let style = self.animate_style(key, style);
        let widget = checkbox::Checkbox {
            bounds,
            layout,
            icons,
            checked,
            style,
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

        let hit = self.hit_bounds(bounds);
        let visible = self.visible(bounds);
        let state = self.dropdowns.entry(id.to_owned()).or_default();
        let response = state.update(hit, popup, self.batch_pointer, events);

        if !visible {
            state.open = false;
        }
        let open = state.open;

        if open {
            self.active_dropdown = Some(id.to_owned());
            self.pending_popup = Some(dropdown::Popup {
                id: id.to_owned(),
                bounds: popup,
                options: state.options(options),
                selected,
                font,
                icons,
                font_size: style.font_size,
            });
        } else if self.active_dropdown.as_deref() == Some(id) {
            self.active_dropdown = None;
        }

        let choice = if self
            .dropdown_selection
            .as_ref()
            .is_some_and(|(key, _)| key == id)
        {
            self.dropdown_selection
                .take()
                .map(|(_, index)| index)
                .filter(|index| *index < options.len())
        } else {
            None
        };

        if !visible {
            return Ok(choice);
        }

        let style = button::appearance(style, response.hovered, response.pressed);
        let style = self.animate_style((format!("dropdown:{id}"), 0), style);
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
        Ok(choice)
    }

    fn render_zoom_indicator<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        // Start the hold after layout and glyph uploads so a costly zoom frame
        // does not consume the indicator's visible time before presentation.
        let now = Instant::now();
        if std::mem::take(&mut self.zoom_indicator_pending) {
            self.zoom_indicator.show(now);
        }
        let Some((opacity, next)) = self.zoom_indicator.sample(now, self.animations, self.curve)
        else {
            return Ok(());
        };
        self.animate_until(next);

        let label = format!("{:.0}%", self.zoom * 100.0);
        let font = self.font_at(Font::Regular, 12)?;
        let metrics = self.text_renderer.measure(&label, font)?;
        let window = self.layouts[0].bounds;
        let x = (window.width - metrics.width - 28.0).max(0.0);
        let y = (window.height - metrics.height - 24.0).max(0.0);
        self.text_renderer.color = Color {
            a: self.theme.muted.a * opacity,
            ..self.theme.muted
        };
        self.text_renderer
            .render(x, y, &label, font, gpu, &mut self.ui_renderer)
    }

    fn render_scrollbars(&mut self) -> anyhow::Result<()> {
        for (view, clip) in self.scrollbars.drain(..) {
            self.ui_renderer
                .push_clip([clip.x, clip.y, clip.width, clip.height]);
            let result = (|| -> anyhow::Result<()> {
                for axis in 0..2 {
                    if let (Some(track), Some(thumb)) = (view.track(axis), view.thumb(axis)) {
                        self.ui_renderer.rounded_box(
                            [track.x, track.y, track.width, track.height],
                            0.0,
                            Some(self.theme.surface),
                            None,
                        )?;
                        self.ui_renderer.rounded_box(
                            [thumb.x, thumb.y, thumb.width, thumb.height],
                            if self.rounded_corners {
                                3.0
                            } else {
                                0.0
                            },
                            Some(self.theme.muted),
                            None,
                        )?;
                    }
                }

                Ok(())
            })();
            self.ui_renderer.pop_clip();
            result?;
        }

        Ok(())
    }

    fn render_dropdown_popup<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        popup: dropdown::Popup,
    ) -> anyhow::Result<()> {
        let events = std::mem::replace(&mut self.events, self.dropdown_events.clone());
        let window = self.layouts[0].bounds;
        self.layouts.push(Layout::new(window));
        self.overlay = true;
        self.set_position(popup.bounds.x, popup.bounds.y);
        self.push_style(Style {
            padding: 4.0,
            background: Some(self.theme.surface),
            border_color: Some(self.theme.border),
            radius: self.corner_radii().popup,
            font_size: popup.font_size,
            ..self.base_style()
        });
        self.sizing.push((None, None));
        let result = (|| -> anyhow::Result<()> {
            self.div_begin_id(
                &format!("dropdown:{}", popup.id),
                DivOptions {
                    width: Some(popup.bounds.width),
                    height: Some(popup.bounds.height),
                    scrollable: true,
                },
            )?;

            let rows = self.visible_rows(dropdown::ROW_HEIGHT, popup.options.len());
            let end = rows.end;
            self.space(rows.start as f32 * dropdown::ROW_HEIGHT);

            for index in rows {
                let value = &popup.options[index];
                let b = self.place(self.div_width(), dropdown::ROW_HEIGHT);

                if !self.visible(b) {
                    continue;
                }

                let label = format!("popup:{}:{index}", popup.id);
                self.button_occurrences.insert(label.clone(), 1);
                let key = (label, 0);
                let hit = self.hit_bounds(b);
                let response = self.buttons.entry(key.clone()).or_default().update(
                    hit,
                    self.batch_pointer,
                    &self.events,
                );
                let style = Style {
                    color: self.theme.text,
                    background: (response.hovered || index == popup.selected)
                        .then_some(self.theme.button),
                    radius: self.corner_radii().row,
                    ..self.base_style()
                };
                let style = self.animate_style(key, style);
                self.paint_style(b, style)?;
                self.ui_renderer
                    .push_clip([b.x, b.y, (b.width - 36.0).max(0.0), b.height]);
                let metrics = self.text_renderer.measure(value, popup.font)?;
                self.text_renderer.color = style.color;
                let result = self.text_renderer.render(
                    b.x + 10.0,
                    b.y + (b.height - metrics.height) * 0.5,
                    value,
                    popup.font,
                    gpu,
                    &mut self.ui_renderer,
                );
                self.ui_renderer.pop_clip();
                result?;

                if index == popup.selected {
                    let metrics = self.text_renderer.measure(dropdown::CHECK, popup.icons)?;
                    self.text_renderer.color = self.theme.selected_icon;
                    self.text_renderer.render(
                        b.x + b.width - 26.0,
                        b.y + (b.height - metrics.height) * 0.5,
                        dropdown::CHECK,
                        popup.icons,
                        gpu,
                        &mut self.ui_renderer,
                    )?;
                }

                if response.clicked {
                    self.dropdown_selection = Some((popup.id.clone(), index));
                    self.dropdowns.get_mut(&popup.id).unwrap().open = false;
                    self.active_dropdown = None;
                    self.animate_until(self.frame_time);
                }
            }

            self.space((popup.options.len() - end) as f32 * dropdown::ROW_HEIGHT);
            self.div_end();
            Ok(())
        })();
        self.sizing.pop();
        self.pop_style();
        self.layouts.pop();
        self.overlay = false;
        self.events = events;
        result
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
        let hit = self.hit_bounds(b);
        let response = self.buttons.entry(key.clone()).or_default().update(
            hit,
            self.batch_pointer,
            &self.events,
        );
        if !self.visible(b) {
            return Ok(response.clicked);
        }

        let s = button::appearance(s, response.hovered, response.pressed);
        let s = self.animate_style(key, s);
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
        let hit = self.hit_bounds(b);
        let state = self.buttons.entry(key.clone()).or_default();
        let response = state.update(hit, self.batch_pointer, &self.events);
        if !self.visible(b) {
            return Ok(response.clicked);
        }

        let s = button::appearance(s, response.hovered, response.pressed);
        let s = self.animate_style(key, s);
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
        let s = self.animate_style((format!("input:{id}"), 0), s);
        let hit = self.hit_bounds(b);
        let visible = self.visible(b);
        let field = self
            .inputs
            .entry(id.into())
            .or_insert_with(|| InputField::new(4096));
        field.set_options(options);
        dispatch_input(
            field,
            &mut self.focused,
            id,
            hit,
            &self.events,
            self.batch_pointer,
        );
        if !visible {
            return Ok(field.text().to_owned());
        }

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
