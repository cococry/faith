use super::animation::{Curve, MORPH_DURATION, Motion, Presence, Scheduler, Transition};
use super::focus::{Control, Kind as FocusKind, Navigation};
use super::scroll::{self, Areas, PrepareOptions, View};
use super::theme::{CornerRadii, Theme};
use super::{
    TextRenderer, UIRenderer,
    text::TextLayoutOptions,
    widgets::{
        button, checkbox, dropdown,
        input::{Bounds, InputField, InputOptions},
        slider,
    },
};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

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
    /// Independent icon size for icon-and-label buttons; defaults to font_size.
    pub icon_font_size: Option<u32>,
    pub color: Color,
    pub background: Option<Color>,
    /// Button background while hovered; defaults to a 5% foreground tint.
    pub hover_color: Option<Color>,
    pub hover_text_color: Option<Color>,
    /// Background while held inside; defaults to an additional foreground tint.
    pub click_color: Option<Color>,
    pub click_text_color: Option<Color>,
    pub border_color: Option<Color>,
    pub padding: f32,
    /// Centers an icon button's icon and label together within its bounds.
    pub center_button_content: bool,
    pub margin: f32,
    /// Extra clickable space outside the visual bounds, in addition to margin.
    /// Use half the gap between adjacent rows so their hit areas meet.
    pub hit_padding: f32,
    pub radius: f32,
    /// Rounded corners clockwise from top-left; false keeps a corner square.
    pub rounded_corners: [bool; 4],
    pub enabled: bool,
    pub focus_when_disabled: bool,
    pub focus_outline: bool,
    /// Prefer this selected control when restoring keyboard focus.
    pub selected: bool,
    pub tooltip: Option<&'static str>,
    /* siblings in this group share one moving selection background */
    pub selection_group: Option<&'static str>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font_size: 16,
            icon_font_size: None,
            color: Theme::default().text,
            background: None,
            hover_color: None,
            hover_text_color: None,
            click_color: None,
            click_text_color: None,
            border_color: None,
            padding: 0.0,
            center_button_content: false,
            margin: 0.0,
            hit_padding: 0.0,
            radius: 0.0,
            rounded_corners: [true; 4],
            enabled: true,
            focus_when_disabled: false,
            focus_outline: false,
            selected: false,
            tooltip: None,
            selection_group: None,
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
    follow_scroll_end: bool,
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
    widgets: u64,
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
            widgets: 0,
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

pub struct ActionRow<'a> {
    pub title: &'a str,
    pub detail: &'a str,
    pub icon: &'a str,
    pub selected: bool,
    pub badge: Option<&'a str>,
}

pub enum ClipboardAction {
    Copy(String),
    Paste(String),
}

#[derive(Clone, Copy)]
struct Tooltip {
    id: u64,
    anchor: Bounds,
    text: &'static str,
}

pub struct Framework {
    hovered_tooltip: Option<Tooltip>,
    tooltip_hover: Option<(u64, Instant)>,
    tooltip: Option<Tooltip>,
    tooltip_presence: Presence,
    clipboard_actions: Vec<ClipboardAction>,
    submitted_input: Option<String>,
    zoom: f32,
    zoom_step: u32,
    zoom_remainder: f64,
    zoom_indicator: super::scale::Indicator,
    zoom_indicator_pending: bool,
    zoom_hit: Option<Bounds>,
    zoom_reset: button::State,
    zoom_reset_pressed: bool,
    scale: f32,
    pointer_factor: f32,
    theme: Theme,
    theme_transition: Option<(Theme, Theme, Instant)>,
    theme_animating: bool,
    animations: bool,
    layout_morphing: bool,
    morph_duration: std::time::Duration,
    morph_curve: Curve,
    morph_strength: f32,
    layout_resized: bool,
    curve: Curve,
    transitions: HashMap<(String, usize), Transition>,
    motions: HashMap<(u64, u64), Motion<4>>,
    selection_slots: HashMap<(u64, &'static str), Option<super::renderer::BoxDecoration>>,
    overlay_mark: (usize, usize),
    overlay_opacity: f32,
    scheduler: Scheduler,
    frame_time: Instant,
    has_rendered: bool,
    window_focused: bool,
    deadline: Option<Instant>,
    rounded_corners: bool,
    dropdowns: HashMap<String, dropdown::State>,
    dropdown_seen: HashSet<String>,
    active_dropdown: Option<String>,
    dropdown_events: Vec<WindowEvent>,
    pending_popups: Vec<dropdown::Popup>,
    dropdown_selection: Option<(String, usize)>,
    scrolls: Areas,
    scrollbars: Vec<(View, Bounds)>,
    smooth_scrolling: bool,
    overlay: bool,
    modal_input: bool,
    modal_events: Vec<WindowEvent>,
    modal_focus: Option<String>,
    text_renderer: TextRenderer,
    ui_renderer: UIRenderer,
    fonts: Fonts,
    font_cache: HashMap<(Font, u32), FontHandle>,
    layouts: Vec<Layout>,
    divs: Vec<OpenDiv>,
    styles: Vec<Style>,
    sizing: Vec<(Option<f32>, Option<f32>)>,
    inputs: HashMap<String, InputField>,
    sliders: HashMap<String, slider::State>,
    focus_order: Vec<String>,
    navigation: Navigation,
    keyboard_clicks: HashSet<String>,
    keyboard_presses: HashMap<String, Instant>,
    checkbox_ids: HashSet<String>,
    focus_reveals: Vec<(u64, Bounds)>,
    reveal_focus: bool,
    focused: Option<String>,
    keyboard_focus_visible: bool,
    events: Vec<WindowEvent>,
    pointer: Option<CursorPosition>,
    batch_pointer: Option<CursorPosition>,
    text_cursor: bool,
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
            hovered_tooltip: None,
            tooltip_hover: None,
            tooltip: None,
            tooltip_presence: Presence::default(),
            clipboard_actions: Vec::new(),
            submitted_input: None,
            zoom: 1.0,
            zoom_step: 10,
            zoom_remainder: 0.0,
            zoom_indicator: super::scale::Indicator::default(),
            zoom_indicator_pending: false,
            zoom_hit: None,
            zoom_reset: button::State::default(),
            zoom_reset_pressed: false,
            scale: platform.scale_factor(),
            pointer_factor: platform.pointer_scale() / platform.scale_factor(),
            theme: Theme::default(),
            theme_transition: None,
            theme_animating: false,
            animations: true,
            layout_morphing: true,
            morph_duration: MORPH_DURATION,
            morph_curve: Curve::Cubic,
            morph_strength: 0.5,
            layout_resized: false,
            curve: Curve::default(),
            transitions: HashMap::new(),
            motions: HashMap::new(),
            selection_slots: HashMap::new(),
            overlay_mark: (0, 0),
            overlay_opacity: 1.0,
            scheduler: Scheduler::new(platform.waker())?,
            frame_time: Instant::now(),
            has_rendered: false,
            window_focused: true,
            deadline: None,
            rounded_corners: false,
            dropdowns: HashMap::new(),
            dropdown_seen: HashSet::new(),
            active_dropdown: None,
            dropdown_events: Vec::new(),
            pending_popups: Vec::new(),
            dropdown_selection: None,
            scrolls: Areas::default(),
            scrollbars: Vec::new(),
            smooth_scrolling: true,
            overlay: false,
            modal_input: false,
            modal_events: Vec::new(),
            modal_focus: None,
            text_renderer: TextRenderer::new()?,
            ui_renderer: UIRenderer::new(gpu, w, h)?,
            fonts,
            font_cache: HashMap::new(),
            layouts: vec![],
            divs: vec![],
            styles: vec![Style::default()],
            sizing: vec![],
            inputs: HashMap::new(),
            sliders: HashMap::new(),
            focus_order: Vec::new(),
            navigation: Navigation::default(),
            keyboard_clicks: HashSet::new(),
            keyboard_presses: HashMap::new(),
            checkbox_ids: HashSet::new(),
            focus_reveals: Vec::new(),
            reveal_focus: false,
            focused: None,
            keyboard_focus_visible: false,
            events: vec![],
            pointer: None,
            batch_pointer: None,
            text_cursor: false,
            buttons: HashMap::new(),
            button_occurrences: HashMap::new(),
        };

        framework
            .text_renderer
            .set_fallback_fonts(framework.fonts.fallbacks.clone());
        Ok(framework)
    }

    pub fn input_focused(&self) -> bool {
        self.focused
            .as_ref()
            .is_some_and(|id| self.inputs.contains_key(id))
    }

    pub fn space_reserved(&self) -> bool {
        self.input_focused()
            || self
                .focused
                .as_ref()
                .is_some_and(|id| self.checkbox_ids.contains(id) || self.dropdowns.contains_key(id))
    }

    pub fn navigation_changed(&mut self) {
        self.navigation.pending = true;
        self.clear_focus();
    }

    fn register_control(&mut self, id: &str, bounds: Bounds, kind: FocusKind, selected: bool) {
        self.focus_order.push(id.to_owned());
        if !self.modal_input && !self.overlay {
            self.navigation.controls.push(Control {
                id: id.to_owned(),
                bounds,
                kind,
                selected,
            });
        }
        if self.reveal_focus && self.focused.as_deref() == Some(id) {
            self.focus_reveals
                .push((self.layouts.last().unwrap().id, bounds));
        }
    }

    fn press_keyboard_button(&mut self, id: String) {
        self.keyboard_clicks.insert(id.clone());
        self.keyboard_presses
            .insert(id, self.frame_time + Duration::from_millis(120));
    }

    fn button_response(
        &mut self,
        key: (String, usize),
        bounds: Bounds,
        style: Style,
    ) -> button::Response {
        let id = format!("button:{}:{}", key.0, key.1);
        if !style.enabled {
            self.buttons.insert(key, button::State::default());
            if style.focus_when_disabled {
                self.register_control(&id, bounds, FocusKind::DisabledButton, false);
                return button::Response {
                    clicked: false,
                    hovered: self.keyboard_focus_visible && self.focused.as_deref() == Some(&id),
                    pressed: false,
                };
            }

            let pressed = if let Some(&until) = self.keyboard_presses.get(&id) {
                self.animate_until(until);
                true
            } else {
                false
            };
            return button::Response {
                clicked: false,
                hovered: false,
                pressed,
            };
        }
        self.register_control(&id, bounds, FocusKind::Button, style.selected);
        let hit = self.hit_bounds(bounds);
        let mut response =
            self.buttons
                .entry(key)
                .or_default()
                .update(hit, self.batch_pointer, &self.events);
        if response.hovered
            && self.events.iter().any(|event| {
                matches!(
                    event,
                    WindowEvent::MouseButtonPressed {
                        button: MouseButton::Left
                    }
                )
            })
        {
            self.focused = Some(id.clone());
        }
        response.clicked |= self.keyboard_clicks.contains(&id);
        response.hovered |= self.keyboard_focus_visible && self.focused.as_deref() == Some(&id);
        if let Some(&until) = self.keyboard_presses.get(&id) {
            response.pressed = true;
            self.animate_until(until);
        }
        response
    }

    pub fn clear_focus(&mut self) {
        self.focused = None;
    }

    pub fn focus_input(&mut self, id: &str) {
        self.inputs
            .entry(id.to_owned())
            .or_insert_with(|| InputField::new(4096));
        self.focus_control(id, false);
    }

    pub fn focus_control(&mut self, id: &str, reveal: bool) {
        self.focused = Some(id.to_owned());
        self.reveal_focus = reveal;
    }

    pub fn zoom_percent(&self) -> u32 {
        (self.zoom * 100.0).round() as u32
    }

    pub fn reset_zoom(&mut self) {
        self.zoom = 1.0;
        self.zoom_remainder = 0.0;
        self.zoom_indicator_pending = true;
    }

    pub fn set_modal_input(&mut self, enabled: bool) {
        if enabled && !self.modal_input {
            self.modal_focus = self.focused.take();
            self.active_dropdown = None;
            for dropdown in self.dropdowns.values_mut() {
                dropdown.open = false;
            }
        } else if !enabled && self.modal_input {
            self.focused = self.modal_focus.take();
        }

        self.modal_input = enabled;
    }

    pub fn events(&self) -> &[WindowEvent] {
        &self.events
    }

    pub fn cursor_icon(&self) -> crate::platform::window::CursorIcon {
        if self.text_cursor && self.active_dropdown.is_none() {
            crate::platform::window::CursorIcon::Text
        } else {
            crate::platform::window::CursorIcon::Default
        }
    }

    pub fn div_hovered(&self) -> bool {
        self.region_hovered([0.0, 0.0, self.div_width(), self.div_height()])
    }

    pub fn region_hovered(&self, [x, y, width, height]: [f32; 4]) -> bool {
        !self.modal_input
            && self.pointer.is_some_and(|pointer| {
                let origin = self.layouts.last().unwrap().bounds;
                let bounds = Bounds {
                    x: origin.x + x,
                    y: origin.y + y,
                    width,
                    height,
                };
                contains(scroll::intersect(bounds, self.clip_bounds()), pointer)
            })
    }

    pub fn pointer_position(&self) -> Option<CursorPosition> {
        self.pointer
    }

    pub fn begin_overlay(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.layouts.len() == 1,
            "overlay requires a closed page layout"
        );
        let window = self.layouts[0].bounds;
        self.render_scrollbars()?;
        self.events = std::mem::take(&mut self.modal_events);
        self.layouts.push(Layout::new(window));
        self.styles.push(self.base_style());
        self.sizing.push((None, None));
        self.overlay = true;
        self.text_cursor = false;
        self.overlay_mark = self.ui_renderer.mark();
        self.overlay_opacity = 1.0;
        Ok(())
    }

    pub fn begin_passive_overlay(&mut self) -> anyhow::Result<()> {
        let events = std::mem::take(&mut self.modal_events);
        let result = self.begin_overlay();
        self.modal_events = events;
        result
    }

    pub fn ignore_overlay_input(&mut self) {
        self.events.clear();
        self.events.push(WindowEvent::MouseCancelled);
    }

    pub fn end_overlay(&mut self) -> anyhow::Result<()> {
        self.render_scrollbars()?;
        self.ui_renderer
            .fade_since(self.overlay_mark, self.overlay_opacity);
        self.layouts.pop();
        self.styles.pop();
        self.sizing.pop();
        self.overlay = false;
        Ok(())
    }

    pub fn overlay_opacity(&mut self, opacity: f32) {
        self.overlay_opacity = opacity.clamp(0.0, 1.0);
    }

    pub fn presence(&mut self, presence: &mut Presence, open: bool) -> f32 {
        let (value, active) = presence.update(
            open,
            self.frame_time,
            self.animations && !self.layout_resized,
        );

        if active {
            self.animate_until(self.frame_time);
        }

        value
    }

    /* we draw above the page with modal input reserved while the menu is open;
     * choosing an item returns its index and closes the menu */
    pub fn popup<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        state: &mut super::widgets::popup::State,
        items: &[super::widgets::popup::Item<'_>],
    ) -> anyhow::Result<Option<usize>> {
        use super::widgets::popup::ROW_HEIGHT;
        use crate::platform::event::Key;
        if !state.is_visible() {
            return Ok(None);
        }
        if items.is_empty() {
            state.close();
            return Ok(None);
        }
        if state.is_open() {
            self.begin_overlay()?;
        } else {
            self.begin_passive_overlay()?;
        }
        let result = (|| -> anyhow::Result<Option<usize>> {
            self.push_style(Style {
                font_size: 14,
                ..self.style()
            });
            let mut width = 200.0f32;
            for item in items {
                width = width.max(self.text_size(item.label, None)?.0 + 64.0);
            }
            self.pop_style();
            let bounds = state.bounds(width, items.len(), self.layouts[0].bounds);
            let mut choice = None;
            let mut pointer = self.batch_pointer;
            for event in &self.events {
                match event {
                    WindowEvent::MouseEntered { position }
                    | WindowEvent::MouseMoved { position } => {
                        pointer = Some(*position);
                        state.cursor = None;
                    }
                    WindowEvent::MouseLeft => pointer = None,
                    WindowEvent::MouseButtonPressed { .. }
                        if !pointer.is_some_and(|p| contains(bounds, p)) =>
                    {
                        state.close()
                    }
                    WindowEvent::FocusChanged(false) => state.close(),
                    WindowEvent::KeyPressed {
                        key: Key::Escape, ..
                    } => state.close(),
                    WindowEvent::KeyPressed { key: Key::Down, .. }
                    | WindowEvent::FocusNext { backwards: false } => {
                        state.cursor = Some(state.cursor.map_or(0, |i| (i + 1) % items.len()));
                    }
                    WindowEvent::KeyPressed { key: Key::Up, .. }
                    | WindowEvent::FocusNext { backwards: true } => {
                        state.cursor = Some(
                            state
                                .cursor
                                .map_or(items.len() - 1, |i| (i + items.len() - 1) % items.len()),
                        );
                    }
                    WindowEvent::KeyPressed { key: Key::Home, .. } => state.cursor = Some(0),
                    WindowEvent::KeyPressed { key: Key::End, .. } => {
                        state.cursor = Some(items.len() - 1)
                    }
                    WindowEvent::KeyPressed {
                        key: Key::Enter, ..
                    } => choice = state.cursor,
                    _ => {}
                }
            }
            if !state.is_open() {
                self.ignore_overlay_input();
            }

            let open = state.is_open();
            let presence = self.presence(&mut state.presence, open);
            self.overlay_opacity(presence);
            self.set_position(bounds.x, bounds.y);
            self.push_style(Style {
                padding: 4.0,
                background: Some(self.theme.surface),
                border_color: Some(self.theme.border),
                radius: self.corner_radii().popup,
                ..self.base_style()
            });
            self.div_begin_reveal(
                id,
                DivOptions {
                    width: Some(bounds.width),
                    height: Some(bounds.height),
                    ..Default::default()
                },
                presence,
            )?;
            for (index, item) in items.iter().enumerate() {
                self.push_style(Style {
                    font_size: 14,
                    icon_font_size: Some(18),
                    padding: 10.0,
                    color: self.theme.text,
                    background: (state.cursor == Some(index))
                        .then_some(button::highlight(self.theme.button, self.theme.text)),
                    border_color: None,
                    radius: self.corner_radii().row,
                    ..self.base_style()
                });
                self.push_sizing(Some(self.div_width()), Some(ROW_HEIGHT));
                let clicked = self.icon_label_button(
                    gpu,
                    &format!("popup:{id}:{index}"),
                    item.icon,
                    item.label,
                )?;
                self.pop_sizing();
                self.pop_style();
                if clicked {
                    choice = Some(index);
                }
            }
            self.div_end();
            self.pop_style();
            if choice.is_some() {
                state.close();
                self.animate_until(self.frame_time);
            }
            Ok(choice)
        })();
        self.end_overlay()?;
        result
    }

    pub fn reveal_row(&mut self, index: usize, height: f32) {
        if index == 0 {
            self.reset_scroll();
            return;
        }

        let layout = self.layouts.last().unwrap();
        let bounds = Bounds {
            x: layout.bounds.x,
            y: layout.bounds.y + index as f32 * height,
            width: layout.bounds.width,
            height,
        };
        self.focus_reveals.push((layout.id, bounds));
    }

    /* a fresh result list starts at zero, including any scroll still in flight */
    pub fn reset_scroll(&mut self) {
        let Some(id) = self.divs.last().and_then(|div| div.scroll_id) else {
            return;
        };

        let offset = self.scrolls.reset(id);
        let layout = self.layouts.last_mut().unwrap();
        layout.bounds.x += offset[0];
        layout.bounds.y += offset[1];
        layout.x += offset[0];
        layout.y += offset[1];
        layout.row_y += offset[1];
        layout.bottom += offset[1];
        layout.right += offset[0];
        layout.extent_right += offset[0];
    }

    pub fn set_zoom_step(&mut self, step: u32) {
        self.zoom_step = step.clamp(1, 50);
        self.zoom_remainder = 0.0;
    }

    pub fn set_startup_zoom(&mut self, percent: u32) {
        self.zoom = percent.clamp(50, 200) as f32 / 100.0;
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
        let style = self.style();
        let padding = style.margin.max(0.0) + style.hit_padding.max(0.0);
        scroll::intersect(
            Bounds {
                x: bounds.x - padding,
                y: bounds.y - padding,
                width: bounds.width + 2.0 * padding,
                height: bounds.height + 2.0 * padding,
            },
            self.clip_bounds(),
        )
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

    pub fn set_layout_morphing(&mut self, enabled: bool) {
        self.layout_morphing = enabled;
    }

    pub fn set_morph_parameters(&mut self, duration_ms: u32, curve: Curve, strength: u32) {
        self.morph_duration = std::time::Duration::from_millis(duration_ms.clamp(20, 1000) as u64);
        self.morph_curve = curve;
        self.morph_strength = strength.min(100) as f32 / 100.0;
    }

    fn morphing(&self) -> bool {
        self.animations && self.layout_morphing && self.morph_strength > 0.0 && !self.layout_resized
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

    fn animate_bounds(&mut self, key: (u64, u64), bounds: Bounds, origin: Bounds) -> Bounds {
        let enabled = self.morphing();
        let target = [
            bounds.x - origin.x,
            bounds.y - origin.y,
            bounds.width,
            bounds.height,
        ];
        let initial = if enabled && self.has_rendered {
            [
                target[0] + target[2] * 0.03,
                target[1] + target[3] * 0.03,
                target[2] * 0.94,
                target[3] * 0.94,
            ]
        } else {
            target
        };
        let motion = self
            .motions
            .entry(key)
            .or_insert_with(|| Motion::new(initial, self.frame_time));
        let (value, active) = motion.update(
            target,
            self.frame_time,
            self.morph_curve,
            enabled,
            self.morph_duration,
        );

        if active {
            self.animate_until(self.frame_time);
        }

        let value = super::animation::morph_strength(target, value, self.morph_strength);
        Bounds {
            x: origin.x + value[0],
            y: origin.y + value[1],
            width: value[2],
            height: value[3],
        }
    }

    /* one reserved background travels between the selected siblings, below their labels */
    fn selection_style(&mut self, bounds: Bounds, mut style: Style) -> anyhow::Result<Style> {
        let Some(group) = style.selection_group else {
            return Ok(style);
        };

        let layout = *self.layouts.last().unwrap();
        let key = (layout.id, group);

        if !self.selection_slots.contains_key(&key) {
            let slot = self.ui_renderer.reserve_box(
                if style.selected {
                    style.background.unwrap_or(self.theme.button)
                } else {
                    self.theme.button
                },
                None,
                if self.rounded_corners {
                    style.radius
                } else {
                    0.0
                },
                style.rounded_corners,
            )?;
            self.selection_slots.insert(key, Some(slot));
        }

        if style.selected {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            group.hash(&mut hasher);
            let bounds = self.animate_bounds((layout.id, hasher.finish()), bounds, layout.bounds);

            if let Some(slot) = self.selection_slots.get_mut(&key).unwrap().take() {
                self.ui_renderer
                    .resolve_box(slot, [bounds.x, bounds.y, bounds.width, bounds.height]);
            }

            style.background = None;
        } else if style.background == Some(self.theme.surface) {
            style.background = None;
        }

        Ok(style)
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

    pub fn rounded_corners_enabled(&self) -> bool {
        self.rounded_corners
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
        self.submitted_input = None;
        self.hovered_tooltip = None;
        self.text_cursor = false;
        self.frame_time = Instant::now();
        self.layout_resized = events
            .iter()
            .any(|event| matches!(event, WindowEvent::Resized { .. }));
        self.deadline = None;

        for event in events {
            if let WindowEvent::FocusChanged(focused) = event {
                self.window_focused = *focused;
            }
        }

        if self.theme_transition.is_some() {
            self.animate_until(self.frame_time);
        }

        for transition in self.transitions.values_mut() {
            transition.seen = false;
        }

        for motion in self.motions.values_mut() {
            motion.seen = false;
        }

        self.selection_slots.clear();

        let previous_scale = self.scale;
        let mut corner_consumed = false;
        let mut corner_pointer = self.pointer;
        let mut reset = false;
        if let Some(bounds) = self
            .zoom_hit
            .filter(|_| self.zoom != 1.0 && self.active_dropdown.is_none() && !self.modal_input)
        {
            let factor = (platform.pointer_scale() / previous_scale) as f64;
            let corner_events: Vec<_> = events
                .iter()
                .map(|event| {
                    let mut event = event.clone();
                    if let WindowEvent::MouseEntered { position }
                    | WindowEvent::MouseMoved { position } = &mut event
                    {
                        position.x *= factor;
                        position.y *= factor;
                    }

                    match &event {
                        WindowEvent::MouseEntered { position }
                        | WindowEvent::MouseMoved { position } => corner_pointer = Some(*position),
                        WindowEvent::MouseLeft | WindowEvent::MouseCancelled => {
                            corner_pointer = None
                        }
                        _ => {}
                    }

                    event
                })
                .collect();
            let response = self.zoom_reset.update(bounds, self.pointer, &corner_events);
            corner_consumed = response.hovered || response.pressed || response.clicked;
            self.zoom_reset_pressed = response.pressed;
            reset = response.clicked;
        } else {
            self.zoom_reset = button::State::default();
            self.zoom_reset_pressed = false;
        }

        for event in events {
            if let WindowEvent::Zoom(steps) = event {
                self.zoom =
                    super::scale::zoom(self.zoom, *steps, &mut self.zoom_remainder, self.zoom_step);
                if steps.is_finite() && *steps != 0.0 {
                    self.zoom_indicator_pending = true;
                }
            }
        }
        if reset {
            self.zoom = 1.0;
            self.zoom_remainder = 0.0;
            self.zoom_indicator_pending = true;
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
        let mut events: Vec<_> = events
            .iter()
            .filter_map(|event| {
                let mut event = event.clone();
                match &mut event {
                    WindowEvent::Zoom(_) => return None,
                    WindowEvent::MouseButtonPressed {
                        button: MouseButton::Left,
                    }
                    | WindowEvent::MouseButtonReleased {
                        button: MouseButton::Left,
                    } if corner_consumed => {
                        event = WindowEvent::MouseCancelled;
                    }

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
        self.keyboard_clicks.clear();
        self.keyboard_presses
            .retain(|_, until| self.frame_time < *until);
        let mut pointer = self.pointer;
        for event in &mut events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    pointer = Some(*position)
                }
                WindowEvent::MouseLeft | WindowEvent::MouseCancelled => pointer = None,
                WindowEvent::MouseButtonPressed { .. } => self.keyboard_focus_visible = false,
                WindowEvent::FocusNext { backwards } if !self.modal_input => {
                    self.keyboard_focus_visible = true;
                    if let Some(id) = self.active_dropdown.take()
                        && let Some(state) = self.dropdowns.get_mut(&id)
                    {
                        state.open = false;
                    }
                    self.navigation.tab(&mut self.focused, *backwards);
                    self.reveal_focus = true;
                    *event = WindowEvent::RedrawRequested;
                }
                WindowEvent::KeyPressed {
                    key:
                        key @ (crate::platform::event::Key::Left | crate::platform::event::Key::Right),
                    control: true,
                } if !self.modal_input
                    && self.active_dropdown.is_none()
                    && !self.input_focused() =>
                {
                    let right = *key == crate::platform::event::Key::Right;
                    let id = if right {
                        "button:history-forward:0"
                    } else {
                        "button:history-back:0"
                    };
                    if self.navigation.control(Some(id)).is_some() {
                        self.press_keyboard_button(id.to_owned());
                    }
                    *event = WindowEvent::RedrawRequested;
                }
                WindowEvent::KeyPressed {
                    key: crate::platform::event::Key::Enter,
                    ..
                } if !self.modal_input && self.active_dropdown.is_none() => {
                    if let Some(control) = self.navigation.control(self.focused.as_deref())
                        && control.kind == FocusKind::Button
                    {
                        self.press_keyboard_button(control.id.clone());
                        *event = WindowEvent::RedrawRequested;
                    }
                }
                WindowEvent::MouseScrolled { delta }
                    if !self.modal_input && self.active_dropdown.is_none() =>
                {
                    if let Some(state) = self
                        .focused
                        .as_ref()
                        .and_then(|id| self.sliders.get_mut(id))
                        && state
                            .hit
                            .is_some_and(|bounds| pointer.is_some_and(|p| contains(bounds, p)))
                    {
                        state.wheel += match delta {
                            crate::platform::mouse::ScrollDelta::Lines { y, .. } => -*y,
                            crate::platform::mouse::ScrollDelta::Pixels { y, .. } => -*y / 40.0,
                        };
                        *event = WindowEvent::RedrawRequested;
                    }
                }
                _ => {}
            }
        }

        if !self.modal_input {
            self.navigation.controls.clear();
        }
        self.focus_order.clear();
        self.checkbox_ids.clear();
        self.focus_reveals.clear();
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
            self.modal_input || self.active_dropdown.is_some(),
        );
        if self.modal_input {
            self.modal_events.clone_from(&self.events);
            self.events.retain(|event| {
                !matches!(
                    event,
                    WindowEvent::TextInput(_)
                        | WindowEvent::ClipboardText { .. }
                        | WindowEvent::KeyPressed { .. }
                        | WindowEvent::FocusNext { .. }
                        | WindowEvent::MouseButtonPressed { .. }
                        | WindowEvent::MouseButtonReleased { .. }
                        | WindowEvent::MouseMoved { .. }
                        | WindowEvent::MouseEntered { .. }
                        | WindowEvent::MouseScrolled { .. }
                )
            });
            self.events.push(WindowEvent::MouseCancelled);
        }

        self.dropdown_events.clone_from(&self.events);
        self.scrollbars.clear();
        self.dropdown_seen.clear();
        self.pending_popups.clear();

        if self.active_dropdown.is_some() {
            self.events.retain(|event| {
                !matches!(
                    event,
                    WindowEvent::MouseButtonPressed { .. }
                        | WindowEvent::MouseButtonReleased { .. }
                        | WindowEvent::MouseScrolled { .. }
                        | WindowEvent::TextInput(_)
                        | WindowEvent::ClipboardText { .. }
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
        if corner_consumed {
            self.pointer = corner_pointer.map(|mut position| {
                position.x *= (previous_scale / self.scale) as f64;
                position.y *= (previous_scale / self.scale) as f64;
                position
            });
        }
        if self.modal_input {
            self.pointer = pointer;
        }
    }

    fn render_tooltip<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        let mut show = false;
        if let Some(hovered) = self.hovered_tooltip.take() {
            let (_, since) = self
                .tooltip_hover
                .filter(|(id, _)| *id == hovered.id)
                .unwrap_or((hovered.id, self.frame_time));
            self.tooltip_hover = Some((hovered.id, since));
            let deadline = since + Duration::from_millis(600);
            show = self.frame_time >= deadline;
            if show {
                self.tooltip = Some(hovered);
            } else {
                self.animate_until(deadline);
            }
        } else {
            self.tooltip_hover = None;
        }

        let Some(Tooltip { anchor, text, .. }) = self.tooltip else {
            return Ok(());
        };
        let mut presence = std::mem::take(&mut self.tooltip_presence);
        let reveal = self.presence(&mut presence, show);
        self.tooltip_presence = presence;

        if reveal <= 0.0 {
            if !show {
                self.tooltip = None;
            }

            return Ok(());
        }

        let font = self.font_at(Font::Regular, 12)?;
        let metrics = self.text_renderer.measure(text, font)?;
        let window = self.layouts[0].bounds;
        let width = metrics.width + 18.0;
        let height = metrics.height + 12.0;
        let [x, y] = tooltip_position(anchor, window, width, height);
        let mark = self.ui_renderer.mark();
        self.set_position(x, y);
        self.push_style(Style {
            font_size: 12,
            radius: self.corner_radii().control,
            background: Some(self.theme.surface),
            border_color: Some(self.theme.border),
            ..self.base_style()
        });
        self.div_begin_reveal(
            "tooltip",
            DivOptions {
                width: Some(width),
                height: Some(height),
                ..Default::default()
            },
            reveal,
        )?;
        self.set_position(9.0, 6.0);
        let result = self.text(gpu, text, None);
        self.div_end();
        self.pop_style();
        self.ui_renderer.fade_since(mark, reveal);
        result
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

        let mut popups = std::mem::take(&mut self.pending_popups);

        for popup in popups.drain(..) {
            self.render_dropdown_popup(gpu, popup)?;
        }

        self.pending_popups = popups;
        self.render_tooltip(gpu)?;

        self.buttons.retain(|(label, occurrence), _| {
            self.button_occurrences
                .get(label)
                .is_some_and(|count| occurrence < count)
        });

        self.reveal_focus = false;
        self.sliders.retain(|id, _| self.focus_order.contains(id));
        if self.modal_input
            && self
                .focused
                .as_ref()
                .is_some_and(|id| !self.focus_order.contains(id))
        {
            self.focused = None;
        }

        self.has_rendered = true;
        self.render_zoom_indicator(gpu)?;
        if !self.modal_input && self.navigation.finish(&mut self.focused) {
            self.reveal_focus = true;
            self.animate_until(self.frame_time);
        }
        self.transitions.retain(|_, transition| transition.seen);
        self.motions.retain(|_, motion| motion.seen);
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

    pub fn layout_scale(&self) -> f32 {
        self.scale
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
        self.visible_rows_from(0.0, row_height, count)
    }

    pub fn visible_rows_from(
        &self,
        offset: f32,
        row_height: f32,
        count: usize,
    ) -> std::ops::Range<usize> {
        let origin = self.layouts.last().unwrap().bounds.y + offset;
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
        self.place_named(w, h, None)
    }

    fn place_named(&mut self, w: f32, h: f32, name: Option<&str>) -> Bounds {
        let s = self.style();
        let (sw, sh) = *self.sizing.last().unwrap();
        let l = self.layouts.last_mut().unwrap();

        if !l.inline {
            l.row_y = l.y;
        }

        let b = l.place(
            sw.unwrap_or(w).max(0.0),
            sh.unwrap_or(h).max(0.0),
            s.margin.max(0.0),
        );
        let widget = if let Some(name) = name {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut hasher);
            hasher.finish()
        } else {
            let index = l.widgets;
            l.widgets += 1;
            index
        };
        let key = (l.id, widget);
        let origin = l.bounds;
        self.animate_bounds(key, b, origin)
    }

    fn paint(&mut self, b: Bounds) -> anyhow::Result<()> {
        self.paint_style(b, self.style())
    }

    fn paint_style(&mut self, b: Bounds, s: Style) -> anyhow::Result<()> {
        self.ui_renderer.corner_box(
            [b.x, b.y, b.width, b.height],
            if self.rounded_corners {
                s.radius
            } else {
                0.0
            },
            s.rounded_corners,
            s.background,
            s.border_color,
        )?;
        Ok(())
    }

    pub fn div_begin(&mut self, options: DivOptions) -> anyhow::Result<()> {
        self.div_begin_impl(None, options, 1.0, 0.5)
    }

    /* names keep scroll positions stable when surrounding containers change */
    pub fn div_begin_id(&mut self, id: &str, options: DivOptions) -> anyhow::Result<()> {
        self.div_begin_impl(Some(id), options, 1.0, 0.5)
    }

    /* reveal grows from the center on both axes while the target layout stays stable */
    pub fn div_begin_reveal(
        &mut self,
        id: &str,
        options: DivOptions,
        reveal: f32,
    ) -> anyhow::Result<()> {
        self.div_begin_impl(Some(id), options, reveal, 0.5)
    }

    pub fn div_begin_reveal_from_left(
        &mut self,
        id: &str,
        options: DivOptions,
        reveal: f32,
    ) -> anyhow::Result<()> {
        self.div_begin_impl(Some(id), options, reveal, 0.0)
    }

    fn div_begin_impl(
        &mut self,
        name: Option<&str>,
        options: DivOptions,
        reveal: f32,
        anchor_x: f32,
    ) -> anyhow::Result<()> {
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
        let target = b;
        let mut b = self.animate_bounds((id, u64::MAX), b, parent.bounds);

        if self.morphing() {
            let reveal = 1.0 - (1.0 - reveal.clamp(0.0, 1.0)) * self.morph_strength;
            let revealed_width =
                b.width.min(1.0) + (b.width - 1.0).max(0.0) * reveal.clamp(0.0, 1.0);
            let revealed_height =
                b.height.min(1.0) + (b.height - 1.0).max(0.0) * reveal.clamp(0.0, 1.0);
            b.x += (b.width - revealed_width) * anchor_x;
            b.y += (b.height - revealed_height) * 0.5;
            b.width = revealed_width;
            b.height = revealed_height;
        }

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
                s.rounded_corners,
            )?);
        }

        let outer_clip = self.clip_bounds();
        let p = s.padding.max(0.0);
        let visual_content = Bounds {
            x: b.x + p,
            y: b.y + p,
            width: (b.width - 2.0 * p).max(0.0),
            height: (b.height - 2.0 * p).max(0.0),
        };
        let scroll_clip = scroll::intersect(outer_clip, visual_content);
        self.divs.push(OpenDiv {
            bounds: b,
            width,
            height,
            style: s,
            decorations,
            scroll_id: options.scrollable.then_some(id),
            follow_scroll_end: false,
            outer_clip: scroll_clip,
        });

        let content = Bounds {
            x: b.x + p,
            y: b.y + p,
            width: (target.width - 2.0 * p).max(0.0),
            height: (target.height - 2.0 * p).max(0.0),
        };

        let (viewport, offset) = if options.scrollable {
            let (view, active) = self.scrolls.prepare(
                id,
                content,
                PrepareOptions {
                    axes: [width.is_some(), height.is_some()],
                    clip: scroll_clip,
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
        /* we lay out at the final size so children don't chase their parent's animation */
        let mut layout = Layout::new(Bounds {
            x: viewport.x - offset[0],
            y: viewport.y - offset[1],
            width: viewport.width,
            height: viewport.height,
        });
        layout.id = id;
        self.layouts.push(layout);
        let clip_viewport = scroll::intersect(viewport, visual_content);
        // Auto axes are unconstrained until their contents have been measured.
        self.ui_renderer.push_clip([
            clip_viewport.x,
            clip_viewport.y,
            if width.is_some() {
                clip_viewport.width
            } else {
                f32::MAX
            },
            if height.is_some() {
                clip_viewport.height
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

    pub fn follow_scroll_end(&mut self) {
        if let Some(div) = self.divs.last_mut() {
            div.follow_scroll_end = true;
        }
    }

    pub fn div_end(&mut self) {
        assert!(self.layouts.len() > 1);
        self.ui_renderer.pop_clip();
        let layout = self.layouts.pop().unwrap();
        let div = self.divs.pop().unwrap();
        let (mut w, mut h) = layout.fitted_size(div.width, div.height, div.style.padding);
        let mut b = Bounds {
            width: if div.width.is_some() {
                div.bounds.width
            } else {
                w
            },
            height: if div.height.is_some() {
                div.bounds.height
            } else {
                h
            },
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
                div.follow_scroll_end,
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

            /* we reveal after measuring, including the first frame of a new settings page */
            let reveals: Vec<_> = self
                .focus_reveals
                .iter()
                .filter(|(target, _)| *target == id)
                .map(|(_, bounds)| *bounds)
                .collect();
            for bounds in reveals {
                if self.scrolls.reveal(id, bounds, self.frame_time) {
                    self.animate_until(self.frame_time);
                }
            }

            if view.maximum.iter().any(|value| *value > 0.0) {
                self.scrollbars.push((view, div.outer_clip));
            }
        }

        if div.width.is_none() || div.height.is_none() {
            let origin = self.layouts.last().unwrap().bounds;
            let animated = self.animate_bounds((layout.id, u64::MAX - 1), b, origin);

            if div.width.is_none() {
                b.width = animated.width;
            }

            if div.height.is_none() {
                b.height = animated.height;
            }
        }

        for decoration in div.decorations {
            self.ui_renderer
                .resolve_box(decoration, [b.x, b.y, b.width, b.height]);
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
        self.wrapped_text_size(value, font, f32::MAX)
    }

    pub fn wrapped_text_size(
        &mut self,
        value: &str,
        font: Option<Font>,
        max_width: f32,
    ) -> anyhow::Result<(f32, f32)> {
        let font = self.font(font.unwrap_or_default())?;
        let layout = self.text_renderer.measure_with_options(
            value,
            font,
            TextLayoutOptions { max_width },
        )?;
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

    pub fn slider_dragging(&self) -> bool {
        self.sliders.values().any(slider::State::dragging)
    }

    pub fn slider<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        value: u32,
        options: slider::Options,
    ) -> anyhow::Result<Option<u32>> {
        anyhow::ensure!(
            options.min < options.max && options.step > 0,
            "invalid slider range"
        );
        let font = self.font_at(Font::Regular, 14)?;
        let label_metrics = self.text_renderer.measure(label, font)?;
        let width = self
            .sizing
            .last()
            .unwrap()
            .0
            .unwrap_or(self.div_width())
            .max(1.0);
        let bounds = self.place(width, label_metrics.height + 50.0);
        self.register_control(id, bounds, FocusKind::Slider, false);

        let visible = self.visible(bounds);
        let track = Bounds {
            x: bounds.x,
            y: bounds.y + label_metrics.height + 6.0,
            width: bounds.width,
            height: 44.0,
        };
        let hit = self.hit_bounds(track);
        let state = self.sliders.entry(id.to_owned()).or_default();
        state.hit = Some(hit);
        let response = state.update(
            id,
            value,
            options,
            slider::Interaction {
                track,
                hit,
                pointer: self.batch_pointer,
                events: &self.events,
                focused: &mut self.focused,
            },
        );
        let value = response.value;
        if !visible {
            return Ok(response.committed.then_some(value));
        }

        let target =
            (value.saturating_sub(options.min)) as f32 / (options.max - options.min) as f32;
        let (fraction, moving) =
            state.position(target, self.frame_time, self.animations, self.curve);
        let focused = self.focused.as_deref() == Some(id);
        let (expansion, resizing) = state.expansion(
            response.hovered || response.dragging || focused,
            self.frame_time,
            self.animations,
            self.curve,
        );
        if moving || resizing {
            self.animate_until(self.frame_time);
        }

        let display = format!("{}{}", value, options.suffix);
        let value_metrics = self.text_renderer.measure(&display, font)?;
        self.ui_renderer
            .push_clip([bounds.x, bounds.y, bounds.width, bounds.height]);
        let result = (|| -> anyhow::Result<()> {
            self.text_renderer.color = self.theme.text;
            self.text_renderer.render(
                bounds.x,
                bounds.y,
                label,
                font,
                gpu,
                &mut self.ui_renderer,
            )?;
            self.text_renderer.color = self.theme.muted;
            self.text_renderer.render(
                bounds.x + (bounds.width - value_metrics.width).max(0.0),
                bounds.y,
                &display,
                font,
                gpu,
                &mut self.ui_renderer,
            )?;
            let inset = 2.0_f32.min(bounds.width * 0.5);
            let length = (bounds.width - 2.0 * inset).max(0.0);
            let height = scroll::thickness(expansion);
            let radius = if self.rounded_corners {
                3.0
            } else {
                0.0
            };
            let track_color =
                super::animation::color(self.theme.border, self.theme.text, 0.12 * expansion);
            let y = track.y + track.height * 0.5;
            if focused {
                self.ui_renderer.rounded_box(
                    [
                        bounds.x + inset - 1.0,
                        y - height * 0.5 - 1.0,
                        length + 2.0,
                        height + 2.0,
                    ],
                    if self.rounded_corners {
                        radius + 1.0
                    } else {
                        0.0
                    },
                    Some(self.theme.primary),
                    None,
                )?;
            }
            self.ui_renderer.rounded_box(
                [bounds.x + inset, y - height * 0.5, length, height],
                radius,
                Some(track_color),
                None,
            )?;
            self.ui_renderer.rounded_box(
                [
                    bounds.x + inset,
                    y - height * 0.5,
                    fraction * length,
                    height,
                ],
                radius,
                Some(self.theme.text),
                None,
            )
        })();
        self.ui_renderer.pop_clip();
        result?;
        Ok(response.committed.then_some(value))
    }

    pub fn checkbox<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        checked: bool,
    ) -> anyhow::Result<Option<bool>> {
        self.checkbox_with_suffix(gpu, id, label, None, checked)
    }

    pub fn checkbox_with_suffix<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        suffix: Option<&str>,
        checked: bool,
    ) -> anyhow::Result<Option<bool>> {
        self.checkbox_sized(gpu, id, label, suffix, checked, 20.0)
    }

    pub fn compact_checkbox<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        checked: bool,
    ) -> anyhow::Result<Option<bool>> {
        self.checkbox_sized(gpu, id, label, None, checked, 16.0)
    }

    fn checkbox_sized<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        label: &str,
        suffix: Option<&str>,
        checked: bool,
        size: f32,
    ) -> anyhow::Result<Option<bool>> {
        let enabled = self.style().enabled;
        let font = self.font(Font::Regular)?;
        let icons = self.font(Font::Icons)?;
        let width = self.sizing.last().unwrap().0.unwrap_or(self.div_width());
        let text_offset = size + 14.0;
        let max_width = (width - text_offset).max(1.0);
        let options = TextLayoutOptions { max_width };
        let layout = self
            .text_renderer
            .measure_with_options(label, font, options)?;
        let suffix = suffix
            .map(|suffix| {
                self.text_renderer
                    .measure_with_options(suffix, font, options)
            })
            .transpose()?;
        let suffix_inline = suffix
            .as_ref()
            .is_some_and(|suffix| layout.width + checkbox::SUFFIX_GAP + suffix.width <= max_width);
        let label_height = layout.height
            + suffix
                .as_ref()
                .filter(|_| !suffix_inline)
                .map_or(0.0, |suffix| suffix.height);
        let bounds = self.place_named(width, label_height.max(size) + size * 0.6, Some(id));
        if enabled {
            self.register_control(id, bounds, FocusKind::Checkbox, false);
        }

        self.checkbox_ids.insert(id.to_owned());
        let occurrence = self.button_occurrences.entry(id.to_owned()).or_default();
        let key = (id.to_owned(), *occurrence);
        *occurrence += 1;
        let hit = self.hit_bounds(bounds);
        let response = self.buttons.entry(key.clone()).or_default().update(
            hit,
            self.batch_pointer,
            &self.events,
        );
        for event in self.events.iter().filter(|_| enabled) {
            if matches!(
                event,
                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left
                }
            ) {
                if response.hovered {
                    self.focused = Some(id.to_owned());
                } else if self.focused.as_deref() == Some(id) {
                    self.focused = None;
                }
            }
        }
        if self.focused.as_deref() == Some(id)
            && self.events.iter().any(|event| {
                matches!(
                    event,
                    WindowEvent::KeyPressed {
                        key: crate::platform::event::Key::Escape,
                        ..
                    }
                )
            })
        {
            self.focused = None;
        }
        let focused = enabled && self.focused.as_deref() == Some(id);
        let toggled = focused
            && self.events.iter().any(|event| {
                matches!(event, WindowEvent::TextInput(text) if text == " ")
                    || matches!(
                        event,
                        WindowEvent::KeyPressed {
                            key: crate::platform::event::Key::Enter,
                            ..
                        }
                    )
            });
        if !self.visible(bounds) {
            return Ok(toggled.then_some(!checked));
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
            enabled && response.hovered,
            enabled && response.pressed,
        );
        let style = self.animate_style(key, style);
        let widget = checkbox::Checkbox {
            bounds,
            layout,
            suffix,
            suffix_inline,
            label_height,
            size,
            text_offset,
            icons,
            checked,
            style,
            radius: self.corner_radii().checkbox,
            focused,
        };

        widget.render(
            &mut self.text_renderer,
            gpu,
            &mut self.ui_renderer,
            self.theme,
        )?;
        Ok((enabled && (response.clicked || toggled)).then_some(!checked))
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
        self.register_control(id, bounds, FocusKind::Dropdown, false);
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
        for event in events {
            if matches!(
                event,
                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left
                }
            ) {
                if response.hovered {
                    self.focused = Some(id.to_owned());
                } else if !state.open && self.focused.as_deref() == Some(id) {
                    self.focused = None;
                }
            }
        }
        let (keyboard_choice, unfocus) = state.keyboard(
            self.focused.as_deref() == Some(id),
            selected,
            options.len(),
            events,
        );
        if unfocus {
            self.focused = None;
        }
        if let Some(index) = keyboard_choice {
            self.dropdown_selection = Some((id.to_owned(), index));
        }

        if !visible {
            state.open = false;
        }
        let open = state.open;
        let (presence, active) = state.presence.update(
            open,
            self.frame_time,
            self.animations && !self.layout_resized,
        );

        if open {
            self.active_dropdown = Some(id.to_owned());
        } else if self.active_dropdown.as_deref() == Some(id) {
            self.active_dropdown = None;
        }

        if open || presence > 0.0 {
            self.pending_popups.push(dropdown::Popup {
                id: id.to_owned(),
                bounds: popup,
                options: state.options(options),
                selected,
                highlighted: state.highlighted(selected),
                reveal: if presence >= 1.0 {
                    std::mem::take(&mut state.reveal)
                } else {
                    false
                },
                font,
                icons,
                font_size: style.font_size,
                presence,
                interactive: open,
            });
        }

        if active {
            self.animate_until(self.frame_time);
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
        if self.focused.as_deref() == Some(id) {
            let radius = if self.rounded_corners {
                style.radius
            } else {
                0.0
            };
            for inset in [0.0, 1.0] {
                self.ui_renderer.corner_box(
                    [
                        bounds.x + inset,
                        bounds.y + inset,
                        (bounds.width - 2.0 * inset).max(0.0),
                        (bounds.height - 2.0 * inset).max(0.0),
                    ],
                    (radius - inset).max(0.0),
                    style.rounded_corners,
                    None,
                    Some(self.theme.primary),
                )?;
            }
        }
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
        if self.modal_input {
            return Ok(());
        }

        let now = Instant::now();
        if std::mem::take(&mut self.zoom_indicator_pending) {
            self.zoom_indicator.show(now);
        }

        if self.keyboard_clicks.contains("zoom-reset") {
            self.reset_zoom();
            self.animate_until(self.frame_time);
        }
        let label = format!("{:.0}%", self.zoom * 100.0);
        let font = self.font_at(Font::Regular, 12)?;
        let metrics = self.text_renderer.measure(&label, font)?;
        let window = self.layouts[0].bounds;
        let x = (window.width - metrics.width - 28.0).max(0.0);
        let y = (window.height - metrics.height - 24.0).max(0.0);
        let hit = Bounds {
            x: (x - 12.0).max(0.0),
            y: (y - 12.0).max(0.0),
            width: window.width - (x - 12.0).max(0.0),
            height: window.height - (y - 12.0).max(0.0),
        };
        self.zoom_hit = (self.zoom != 1.0).then_some(hit);
        if self.zoom != 1.0 {
            self.register_control("zoom-reset", hit, FocusKind::Button, false);
        }
        let hovered = self.zoom_hit.is_some_and(|bounds| {
            !self.modal_input
                && self.active_dropdown.is_none()
                && self
                    .pointer
                    .is_some_and(|pointer| contains(bounds, pointer))
        });
        let hovered = hovered || self.focused.as_deref() == Some("zoom-reset");
        let Some((opacity, next)) =
            self.zoom_indicator
                .sample(now, hovered, self.animations, self.curve)
        else {
            return Ok(());
        };

        if let Some(next) = next {
            self.animate_until(next);
        }

        let color = if self.zoom_reset_pressed {
            self.theme.selected_icon
        } else if hovered {
            self.theme.text
        } else {
            self.theme.muted
        };
        self.text_renderer.color = Color {
            a: color.a * opacity,
            ..color
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
        if !popup.interactive {
            self.ignore_overlay_input();
        }

        let mark = self.ui_renderer.mark();
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
            self.div_begin_reveal(
                &format!("dropdown:{}", popup.id),
                DivOptions {
                    width: Some(popup.bounds.width),
                    height: Some(popup.bounds.height),
                    scrollable: true,
                },
                popup.presence,
            )?;

            if popup.reveal {
                self.reveal_row(popup.highlighted, dropdown::ROW_HEIGHT);
            }
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
                    background: (response.hovered || index == popup.highlighted)
                        .then_some(button::highlight(self.theme.button, self.theme.text)),
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
        self.render_scrollbars()?;
        self.ui_renderer.fade_since(mark, popup.presence);
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
        let rect = super::animation::image_bounds([b.x, b.y, b.width, b.height], image.size);
        self.ui_renderer
            .image(rect[0], rect[1], rect[2], rect[3], image)
    }

    fn paint_button(
        &mut self,
        key: (String, usize),
        bounds: Bounds,
        mut style: Style,
        hovered: bool,
        pressed: bool,
    ) -> anyhow::Result<Style> {
        let grouped = style.selection_group.is_some();
        let morph = grouped && self.morphing();
        if morph {
            style = self.selection_style(bounds, style)?;
        } else if grouped && !style.selected && style.background == Some(self.theme.surface) {
            style.background = None;
        }

        let style = button::appearance(style, hovered, pressed);
        let layout = *self.layouts.last().unwrap();
        let geometry_key = morph.then(|| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            "button-hover".hash(&mut hasher);
            key.hash(&mut hasher);
            (layout.id, hasher.finish())
        });
        let style = self.animate_style(key, style);
        let mut background = bounds;
        if let Some(geometry_key) = geometry_key
            && style.background.is_some()
        {
            if !hovered && !pressed {
                let inset = [bounds.width * 0.03, bounds.height * 0.03];
                background.x += inset[0];
                background.y += inset[1];
                background.width -= inset[0] * 2.0;
                background.height -= inset[1] * 2.0;
            }

            background = self.animate_bounds(geometry_key, background, layout.bounds);
        }

        self.paint_style(background, style)?;
        Ok(style)
    }

    pub fn action_row<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        row: ActionRow<'_>,
    ) -> anyhow::Result<button::Response> {
        self.action_row_with_checkbox(gpu, id, row, None)
    }

    pub fn action_row_with_checkbox<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        row: ActionRow<'_>,
        checked: Option<bool>,
    ) -> anyhow::Result<button::Response> {
        let bounds = self.place_named(self.div_width(), 56.0, Some(id));
        let key = (id.to_owned(), 0);
        self.button_occurrences.insert(id.to_owned(), 1);
        let response = if self.overlay {
            let hit = self.hit_bounds(bounds);
            self.buttons.entry(key.clone()).or_default().update(
                hit,
                self.batch_pointer,
                &self.events,
            )
        } else {
            self.button_response(
                key.clone(),
                bounds,
                Style {
                    selected: row.selected,
                    ..self.style()
                },
            )
        };
        let hovered = !self.overlay && response.hovered;
        let pressed = !self.overlay && response.pressed;
        let style = Style {
            background: row.selected.then_some(self.theme.button),
            color: if row.selected {
                self.theme.button_text
            } else {
                self.theme.text
            },
            hover_text_color: Some(self.theme.button_text),
            radius: self.corner_radii().row,
            selected: row.selected,
            selection_group: Some("action-rows"),
            ..self.base_style()
        };
        let style = self.paint_button(key, bounds, style, hovered, pressed)?;
        let icons = self.font_at(Font::Icons, 20)?;
        let title = self.font_at(Font::Regular, 14)?;
        let detail = self.font_at(Font::Regular, 12)?;
        let badge_font = if row.badge.is_some() {
            Some(self.font_at(Font::Regular, 11)?)
        } else {
            None
        };
        let badge = match (row.badge, badge_font) {
            (Some(value), Some(font)) => Some(self.text_renderer.measure(value, font)?),
            _ => None,
        };
        self.ui_renderer
            .push_clip([bounds.x, bounds.y, bounds.width, bounds.height]);
        let result = (|| -> anyhow::Result<()> {
            let metrics = self.text_renderer.measure(row.icon, icons)?;
            self.text_renderer.color = style.color;
            self.text_renderer.render(
                bounds.x + 12.0,
                bounds.y + (bounds.height - metrics.height) * 0.5,
                row.icon,
                icons,
                gpu,
                &mut self.ui_renderer,
            )?;
            let badge_width = badge.as_ref().map_or(0.0, |layout| layout.width + 16.0);
            let reserved = if checked.is_some() {
                26.0
            } else {
                badge_width
            };
            self.ui_renderer.push_clip([
                bounds.x + 44.0,
                bounds.y,
                (bounds.width - 56.0 - reserved - 8.0).max(0.0),
                bounds.height,
            ]);
            let result = (|| -> anyhow::Result<()> {
                self.text_renderer.render(
                    bounds.x + 44.0,
                    bounds.y + 7.0,
                    row.title,
                    title,
                    gpu,
                    &mut self.ui_renderer,
                )?;
                self.text_renderer.color = self.theme.muted;
                self.text_renderer.render(
                    bounds.x + 44.0,
                    bounds.y + 29.0,
                    row.detail,
                    detail,
                    gpu,
                    &mut self.ui_renderer,
                )
            })();
            self.ui_renderer.pop_clip();
            if let Some(checked) = checked {
                let x = bounds.x + bounds.width - 30.0;
                let y = bounds.y + (bounds.height - 18.0) * 0.5;
                self.ui_renderer.rounded_box(
                    [x, y, 18.0, 18.0],
                    if self.rounded_corners {
                        4.0
                    } else {
                        0.0
                    },
                    checked.then_some(self.theme.primary),
                    Some(if checked {
                        self.theme.primary
                    } else {
                        self.theme.muted
                    }),
                )?;
                if checked {
                    let glyph = "\u{eab2}";
                    let font = self.font_at(Font::Icons, 14)?;
                    let metrics = self.text_renderer.measure(glyph, font)?;
                    self.text_renderer.color = self.theme.primary_text;
                    self.text_renderer.render(
                        x + (18.0 - metrics.width) * 0.5,
                        y + (18.0 - metrics.height) * 0.5,
                        glyph,
                        font,
                        gpu,
                        &mut self.ui_renderer,
                    )?;
                }
            } else if let (Some(value), Some(layout)) = (row.badge, badge) {
                let height = (layout.height + 6.0).min(bounds.height - 12.0);
                let badge_x = bounds.x + bounds.width - badge_width - 10.0;
                let badge_y = bounds.y + (bounds.height - height) * 0.5;
                let badge_font = self.font_at(Font::Regular, 11)?;
                self.ui_renderer.rounded_box(
                    [badge_x, badge_y, badge_width, height],
                    if self.rounded_corners {
                        height * 0.5
                    } else {
                        0.0
                    },
                    Some(self.theme.primary),
                    None,
                )?;
                self.text_renderer.color = self.theme.primary_text;
                self.text_renderer.render(
                    badge_x + 8.0,
                    bounds.y + (bounds.height - layout.height) * 0.5,
                    value,
                    badge_font,
                    gpu,
                    &mut self.ui_renderer,
                )?;
            }
            result
        })();
        self.ui_renderer.pop_clip();
        result?;
        Ok(response)
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
        let s = self.style();
        let icons = self.font_at(Font::Icons, s.icon_font_size.unwrap_or(s.font_size).max(1))?;
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

        let gap = if label.is_empty() || icon.is_empty() {
            0.0
        } else {
            10.0
        };

        let b = self.place_named(
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
            Some(id),
        );
        let occurrence = self.button_occurrences.entry(id.to_owned()).or_default();
        let key = (id.to_owned(), *occurrence);
        *occurrence += 1;
        let response = self.button_response(key.clone(), b, s);
        if !self.visible(b) {
            return Ok(response.clicked);
        }

        if let Some(tooltip) = s.tooltip
            && s.enabled
            && (!self.modal_input || self.overlay)
            && self.active_dropdown.is_none()
            && self
                .pointer
                .is_some_and(|pointer| contains(self.hit_bounds(b), pointer))
        {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            self.layouts.last().unwrap().id.hash(&mut hasher);
            key.hash(&mut hasher);
            self.hovered_tooltip = Some(Tooltip {
                id: hasher.finish(),
                anchor: b,
                text: tooltip,
            });
        }

        let s = if s.focus_outline
            && self.keyboard_focus_visible
            && self.focused.as_deref() == Some(format!("button:{id}:{}", key.1).as_str())
        {
            Style {
                border_color: Some(self.theme.selected_icon),
                ..s
            }
        } else {
            s
        };
        let s = self.paint_button(key, b, s, response.hovered, response.pressed)?;
        // Keep horizontal padding while allowing the independently sized icon
        // and centered label to use the row's full height.
        self.ui_renderer.push_clip([
            b.x + s.padding,
            b.y,
            (b.width - 2.0 * s.padding).max(0.0),
            b.height,
        ]);
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
                    if self.rounded_corners {
                        height * 0.5
                    } else {
                        0.0
                    },
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
        let b = self.place_named(
            m.width + 2.0 * s.padding,
            m.height + 2.0 * s.padding,
            Some(label),
        );
        let occurrence = self.button_occurrences.entry(label.into()).or_default();
        let key = (label.to_owned(), *occurrence);
        *occurrence += 1;
        let response = self.button_response(key.clone(), b, s);
        if !self.visible(b) {
            return Ok(response.clicked);
        }

        let s = self.selection_style(b, s)?;
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

    pub fn take_clipboard_actions(&mut self) -> Vec<ClipboardAction> {
        std::mem::take(&mut self.clipboard_actions)
    }

    pub fn input_submitted(&self, id: &str) -> bool {
        self.submitted_input.as_deref() == Some(id)
    }

    pub fn input_value(&self, id: &str) -> Option<&str> {
        self.inputs.get(id).map(InputField::text)
    }

    pub fn set_input(&mut self, id: &str, text: &str) {
        let field = self
            .inputs
            .entry(id.to_owned())
            .or_insert_with(|| InputField::new(4096));
        field.clear();
        field.set_focused(true);
        field.handle(&WindowEvent::TextInput(text.to_owned()));
        self.focused = Some(id.to_owned());
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
        self.input_with_hint(gpu, id, options, "")
    }

    pub fn input_with_hint<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        id: &str,
        options: InputOptions,
        hint: &str,
    ) -> anyhow::Result<String> {
        let font = self.font(Font::Regular)?;
        let s = self.style();
        let b = self.place_named(
            if self.divs.last().is_some_and(|div| div.width.is_none()) {
                240.0
            } else {
                self.div_width() - 2.0 * s.margin
            },
            s.font_size as f32 * 1.5 + 2.0 * s.padding,
            Some(id),
        );
        let s = self.animate_style((format!("input:{id}"), 0), s);
        let hit = self.hit_bounds(b);
        let visible = self.visible(b);
        if visible
            && (!self.modal_input || self.overlay)
            && self
                .pointer
                .is_some_and(|p| contains(scroll::intersect(b, self.clip_bounds()), p))
        {
            self.text_cursor = true;
        }
        if s.enabled && (visible || self.reveal_focus && self.focused.as_deref() == Some(id)) {
            self.register_control(id, b, FocusKind::Input, false);
        }

        let field = self
            .inputs
            .entry(id.into())
            .or_insert_with(|| InputField::new(4096));
        field.set_options(options);
        if visible
            && s.enabled
            && (!self.modal_input || self.overlay)
            && self.active_dropdown.is_none()
        {
            let response = dispatch_input(
                field,
                &mut self.focused,
                id,
                hit,
                &self.events,
                self.batch_pointer,
            );
            if response.submitted.is_some() {
                self.submitted_input = Some(id.to_owned());
            }

            if let Some(text) = response.copied {
                self.clipboard_actions.push(ClipboardAction::Copy(text));
            }

            if response.paste {
                self.clipboard_actions
                    .push(ClipboardAction::Paste(id.to_owned()));
            }
        } else {
            field.set_focused(false);
        }

        if !visible {
            return Ok(field.text().to_owned());
        }

        let next_blink = field.update_caret(self.frame_time, self.window_focused);
        self.ui_renderer.corner_box(
            [b.x, b.y, b.width, b.height],
            if self.rounded_corners {
                s.radius
            } else {
                0.0
            },
            s.rounded_corners,
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
        if field.text().is_empty() && !hint.is_empty() {
            self.text_renderer.color = self.theme.muted;
            let hint_result = self.text_renderer.render(
                b.x + s.padding,
                b.y + s.padding,
                hint,
                font,
                gpu,
                &mut self.ui_renderer,
            );
            self.ui_renderer.pop_clip();
            hint_result?;
        } else {
            self.ui_renderer.pop_clip();
        }

        result?;
        let value = field.text().to_owned();
        if let Some(next_blink) = next_blink {
            self.animate_until(next_blink);
        }

        Ok(value)
    }
}

fn tooltip_position(anchor: Bounds, window: Bounds, width: f32, height: f32) -> [f32; 2] {
    const GAP: f32 = 10.0;
    let right = anchor.x + anchor.width;
    let bottom = anchor.y + anchor.height;
    let window_right = window.x + window.width;
    let window_bottom = window.y + window.height;
    let x_center = anchor.x + (anchor.width - width) * 0.5;
    let y_center = anchor.y + (anchor.height - height) * 0.5;
    let vertical = [
        (window_bottom - bottom - height, [x_center, bottom + GAP]),
        (
            anchor.y - window.y - height,
            [x_center, anchor.y - GAP - height],
        ),
    ];
    let horizontal = [
        (window_right - right - width, [right + GAP, y_center]),
        (
            anchor.x - window.x - width,
            [anchor.x - GAP - width, y_center],
        ),
    ];
    let near_side = (anchor.x - window.x).min(window_right - right) < anchor.width
        || x_center < window.x
        || x_center + width > window_right;
    let (preferred, alternate) = if near_side {
        (horizontal, vertical)
    } else {
        (vertical, horizontal)
    };
    let [x, y] = preferred
        .into_iter()
        .chain(alternate)
        .find(|(space, _)| *space >= GAP)
        .or_else(|| {
            preferred
                .into_iter()
                .chain(alternate)
                .max_by(|a, b| a.0.total_cmp(&b.0))
        })
        .map_or([x_center, y_center], |(_, position)| position);
    [
        x.clamp(window.x, (window_right - width).max(window.x)),
        y.clamp(window.y, (window_bottom - height).max(window.y)),
    ]
}

fn dispatch_input(
    field: &mut InputField,
    focused: &mut Option<String>,
    id: &str,
    bounds: Bounds,
    events: &[WindowEvent],
    mut pointer: Option<CursorPosition>,
) -> super::widgets::input::InputResponse {
    let mut result = super::widgets::input::InputResponse::default();
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
                    field.restart_caret();
                } else if focused.as_deref() == Some(id) {
                    *focused = None;
                }

                field.set_focused(focused.as_deref() == Some(id));
            }

            _ => {}
        }

        let response = match event {
            WindowEvent::ClipboardText { target, text }
                if target == id && focused.as_deref() == Some(id) =>
            {
                field.handle(&WindowEvent::TextInput(text.clone()))
            }

            WindowEvent::FocusChanged(true) => {
                field.set_focused(focused.as_deref() == Some(id));
                continue;
            }

            _ => field.handle(event),
        };
        result.submitted = response.submitted.or(result.submitted);
        result.copied = response.copied.or(result.copied);
        result.paste |= response.paste;
    }

    result
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

#[cfg(test)]
mod tooltip_tests {
    use super::*;

    #[test]
    fn tooltips_use_the_side_with_clearance_at_window_edges() {
        let window = Bounds {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 200.0,
        };
        let mut anchor = Bounds {
            x: 16.0,
            y: 16.0,
            width: 40.0,
            height: 40.0,
        };
        assert_eq!(tooltip_position(anchor, window, 120.0, 28.0), [66.0, 22.0]);

        anchor.x = 344.0;
        assert_eq!(tooltip_position(anchor, window, 120.0, 28.0), [214.0, 22.0]);

        anchor.x = 180.0;
        assert_eq!(tooltip_position(anchor, window, 120.0, 28.0), [140.0, 66.0]);

        anchor.y = 150.0;
        assert_eq!(
            tooltip_position(anchor, window, 120.0, 28.0),
            [140.0, 112.0]
        );
    }
}
