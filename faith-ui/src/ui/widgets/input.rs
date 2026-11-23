use std::{
    collections::VecDeque,
    ops::Range,
    time::{Duration, Instant},
};

use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    platform::{
        event::{Key, WindowEvent},
        mouse::{CursorPosition, MouseButton, ScrollDelta},
    },
    ui::{TextRenderer, UIRenderer, text::PreservedLayout},
};

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct InputResponse {
    pub changed: bool,
    pub submitted: Option<String>,
    pub copied: Option<String>,
    pub paste: bool,
}

#[derive(Clone, Copy, Default)]
pub struct InputOptions {
    pub max_chars: Option<usize>,
    pub filter: Option<fn(char) -> bool>,
}

pub struct InputPaint {
    pub padding: f32,
    pub caret: Color,
    pub selection: Color,
}

pub struct InputHit {
    pub bounds: Bounds,
    pub origin_x: f32,
    pub origin_y: f32,
    pub viewport_height: f32,
    pub font: FontHandle,
}

struct EditSnapshot {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Backspace,
    Delete,
}

#[derive(Default)]
pub struct MouseSelection {
    pub drag_anchor: Option<Range<usize>>,
    last_click: Option<(Instant, CursorPosition, u8)>,
}

impl MouseSelection {
    pub fn click_count(&mut self, position: CursorPosition, shift: bool) -> u8 {
        if shift {
            self.last_click = None;
            return 1;
        }

        let now = Instant::now();
        let count = self
            .last_click
            .filter(|(at, last, _)| {
                now.duration_since(*at) < Duration::from_millis(400)
                    && (position.x - last.x).abs() <= 4.0
                    && (position.y - last.y).abs() <= 4.0
            })
            .map_or(1, |(_, _, count)| count % 3 + 1);
        self.last_click = Some((now, position, count));
        count
    }

    pub fn click_range(text: &str, cursor: usize, count: u8) -> Range<usize> {
        match count {
            2 => Self::word_at(text, cursor),
            3 => 0..text.len(),
            _ => cursor..cursor,
        }
    }

    pub fn drag_to(text: &str, anchor: Range<usize>, cursor: usize) -> (usize, usize) {
        let target = if anchor.is_empty() {
            cursor..cursor
        } else {
            Self::word_at(text, cursor)
        };
        if cursor < anchor.start {
            (anchor.end, target.start)
        } else {
            (anchor.start, target.end.max(anchor.end))
        }
    }

    fn word_at(text: &str, cursor: usize) -> Range<usize> {
        text.split_word_bound_indices()
            .find(|(start, word)| {
                cursor < start + word.len() || cursor == start + word.len() && cursor == text.len()
            })
            .map_or(cursor..cursor, |(start, word)| start..start + word.len())
    }
}

/// A UTF-8 input widget. Editing is grapheme-aware; platform text
/// commits are distinct from navigation keys. The caller schedules caret redraws.
pub struct InputField {
    options: InputOptions,
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    undo: VecDeque<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    edit_group: Option<(EditKind, Instant)>,
    focused: bool,
    mouse_selection: MouseSelection,
    max_bytes: usize,
    scroll_x: f32,
    scroll_y: f32,
    reveal_caret: bool,
    viewport_height: f32,
    wrap_width: Option<f32>,
    caret_row: Option<usize>,
    preferred_x: Option<f32>,
    caret_since: Option<Instant>,
    caret_visible: bool,
}

impl InputField {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            options: InputOptions::default(),
            text: String::new(),
            cursor: 0,
            anchor: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            edit_group: None,
            focused: true,
            mouse_selection: MouseSelection::default(),
            max_bytes,
            scroll_x: 0.0,
            scroll_y: 0.0,
            reveal_caret: true,
            viewport_height: 0.0,
            wrap_width: None,
            caret_row: None,
            preferred_x: None,
            caret_since: None,
            caret_visible: false,
        }
    }

    pub fn set_options(&mut self, options: InputOptions) {
        self.options = options;
    }

    pub fn set_wrap_width(&mut self, width: Option<f32>) {
        if self.wrap_width != width {
            self.wrap_width = width;
            self.reveal_caret = true;
            self.caret_row = None;
            self.preferred_x = None;
        }
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |g| self.cursor + g.len())
    }

    fn selection(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }

    fn word_left(&self) -> usize {
        self.text[..self.cursor]
            .unicode_word_indices()
            .next_back()
            .map_or(0, |(start, _)| start)
    }

    fn word_right(&self) -> usize {
        self.text[self.cursor..]
            .unicode_word_indices()
            .next()
            .map_or(self.text.len(), |(start, word)| {
                self.cursor + start + word.len()
            })
    }

    fn boundary_at_or_after(&self, byte: usize) -> usize {
        let mut cursor = unicode_segmentation::GraphemeCursor::new(byte, self.text.len(), true);
        if cursor.is_boundary(&self.text, 0).unwrap_or(false) {
            byte
        } else {
            cursor
                .next_boundary(&self.text, 0)
                .ok()
                .flatten()
                .unwrap_or(self.text.len())
        }
    }

    fn move_to(&mut self, cursor: usize, extend: bool) -> bool {
        let cursor = self.boundary_at_or_after(cursor);
        let anchor = extend.then_some(self.anchor.unwrap_or(self.cursor));
        let changed = self.anchor != anchor || self.cursor != cursor;
        self.anchor = anchor;
        self.cursor = cursor;
        self.caret_row = None;
        self.preferred_x = None;
        self.edit_group = None;
        changed
    }

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    fn replace(&mut self, range: Range<usize>, text: &str, group: Option<EditKind>) -> bool {
        if self.text[range.clone()] != *text {
            let now = Instant::now();
            let coalesce = group.is_some_and(|kind| {
                self.edit_group.is_some_and(|(previous, at)| {
                    previous == kind && now.duration_since(at) < Duration::from_secs(1)
                })
            });

            if !coalesce {
                if self.undo.len() == 100 {
                    self.undo.pop_front();
                }

                self.undo.push_back(self.snapshot());
            }

            self.redo.clear();
            self.edit_group = group.map(|kind| (kind, now));
            self.text.replace_range(range.clone(), text);
        }

        self.anchor = None;
        self.mouse_selection.drag_anchor = None;
        self.caret_row = None;
        self.preferred_x = None;
        self.cursor = self.boundary_at_or_after(range.start + text.len());
        true
    }

    fn restore_history(&mut self, redo: bool) -> bool {
        self.edit_group = None;
        let entry = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        };
        let Some(entry) = entry else {
            return false;
        };

        let previous = self.snapshot();
        if redo {
            self.undo.push_back(previous);
        } else {
            self.redo.push(previous);
        }

        self.text = entry.text;
        self.mouse_selection.drag_anchor = None;
        self.caret_row = None;
        self.preferred_x = None;
        self.cursor = entry.cursor;
        self.anchor = entry.anchor;
        true
    }

    fn insert(&mut self, text: &str, typing: bool) -> bool {
        let range = self.selection();
        let mut bytes = self.max_bytes.saturating_sub(self.text.len() - range.len());
        let remaining = self.options.max_chars.map_or(usize::MAX, |max| {
            max.saturating_sub(
                self.text[..range.start].chars().count() + self.text[range.end..].chars().count(),
            )
        });
        let normalized;
        let multiline = self.wrap_width.is_some();
        let text = if multiline && text.contains('\r') {
            normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            &normalized
        } else {
            text
        };
        let text: String = text
            .chars()
            .filter(|c| {
                (!c.is_control() || multiline && matches!(c, '\n' | '\t'))
                    && self.options.filter.is_none_or(|filter| filter(*c))
            })
            .take(remaining)
            .take_while(|c| {
                if c.len_utf8() > bytes {
                    return false;
                }

                bytes -= c.len_utf8();
                true
            })
            .collect();

        let group = (typing && range.is_empty()).then_some(EditKind::Typing);
        !text.is_empty() && self.replace(range, &text, group)
    }

    pub fn paste(&mut self, text: &str) -> InputResponse {
        let changed = self.focused && self.insert(text, false);
        if changed {
            self.reveal_caret = true;
            self.restart_caret();
        }

        InputResponse {
            changed,
            ..InputResponse::default()
        }
    }

    pub fn handle(&mut self, event: &WindowEvent) -> InputResponse {
        let mut response = InputResponse::default();

        match event {
            WindowEvent::FocusChanged(focused) => {
                response.changed = self.focused != *focused;
                self.set_focused(*focused);
            }

            WindowEvent::TextInput(text) if self.focused => {
                response.changed = self.insert(text, true);
            }

            WindowEvent::KeyPressed {
                key,
                control,
                shift,
            } if self.focused => {
                let selected = self.selection();
                match key {
                    Key::SelectAll => {
                        response.changed = self.selection() != (0..self.text.len())
                            || self.cursor != self.text.len();
                        self.anchor = (!self.text.is_empty()).then_some(0);
                        self.cursor = self.text.len();
                        self.caret_row = None;
                        self.preferred_x = None;
                        self.edit_group = None;
                    }

                    Key::Copy | Key::Cut if !selected.is_empty() => {
                        response.copied = Some(self.text[selected.clone()].to_owned());
                        if *key == Key::Cut {
                            response.changed = self.replace(selected, "", None);
                        }
                    }

                    Key::Paste => {
                        self.edit_group = None;
                        response.paste = true;
                    }

                    Key::Undo | Key::Redo => {
                        response.changed = self.restore_history(*key == Key::Redo)
                    }

                    Key::Enter if self.wrap_width.is_some() && *shift => {
                        response.changed = self.insert("\n", false);
                    }

                    Key::Enter if !self.text.trim().is_empty() => {
                        self.edit_group = None;
                        response.submitted = Some(self.text.clone());
                        response.changed = true;
                    }

                    Key::Backspace | Key::Delete => {
                        let (range, group) = if !selected.is_empty() {
                            (selected, None)
                        } else if *key == Key::Backspace {
                            let start = if *control {
                                self.word_left()
                            } else {
                                self.previous()
                            };
                            (
                                self.boundary_at_or_after(start)..self.cursor,
                                (!control).then_some(EditKind::Backspace),
                            )
                        } else {
                            let end = if *control {
                                self.word_right()
                            } else {
                                self.next()
                            };
                            (
                                self.cursor..self.boundary_at_or_after(end),
                                (!control).then_some(EditKind::Delete),
                            )
                        };

                        if !range.is_empty() {
                            response.changed = self.replace(range, "", group);
                        }
                    }

                    Key::Left | Key::Right | Key::Home | Key::End => {
                        let collapse = !shift && !control && !selected.is_empty();
                        let cursor = match key {
                            Key::Left if collapse => selected.start,
                            Key::Right if collapse => selected.end,
                            Key::Left if *control => self.word_left(),
                            Key::Right if *control => self.word_right(),
                            Key::Left => self.previous(),
                            Key::Right => self.next(),
                            Key::Home => 0,
                            Key::End => self.text.len(),
                            _ => unreachable!(),
                        };

                        response.changed = self.move_to(cursor, *shift);
                    }

                    Key::Escape if !selected.is_empty() => {
                        response.changed = self.move_to(self.cursor, false);
                    }

                    _ => {}
                }
            }

            _ => {}
        }

        if response.changed {
            self.reveal_caret = true;
            self.restart_caret();
        }

        response
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.reveal_caret |= focused;
            self.restart_caret();
        }

        self.focused = focused;
        if !focused {
            self.mouse_selection.drag_anchor = None;
            self.mouse_selection.last_click = None;
            self.anchor = None;
            self.edit_group = None;
        }
    }

    pub fn restart_caret(&mut self) {
        self.caret_since = None;
    }

    pub fn update_caret(&mut self, now: Instant, window_focused: bool) -> Option<Instant> {
        const HALF_PERIOD_MS: u128 = 500;

        if !self.focused || !window_focused {
            self.restart_caret();
            self.caret_visible = false;
            return None;
        }

        let since = *self.caret_since.get_or_insert(now);
        let elapsed = now.saturating_duration_since(since).as_millis();
        self.caret_visible = (elapsed / HALF_PERIOD_MS).is_multiple_of(2);
        let next = (elapsed / HALF_PERIOD_MS + 1) * HALF_PERIOD_MS;
        Some(since + Duration::from_millis(next as u64))
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.caret_row = None;
        self.preferred_x = None;
        self.anchor = None;
        self.mouse_selection.drag_anchor = None;
        self.mouse_selection.last_click = None;
        self.undo.clear();
        self.redo.clear();
        self.edit_group = None;
        self.restart_caret();
    }

    fn visual_row(&self, layout: &PreservedLayout) -> usize {
        self.caret_row
            .filter(|&row| {
                layout.lines.get(row).is_some_and(|line| {
                    (line.source.start..=line.source.end).contains(&self.cursor)
                })
            })
            .unwrap_or_else(|| layout.row_at(self.cursor))
    }

    pub fn visual_key(
        &mut self,
        event: &WindowEvent,
        hit: &InputHit,
        text: &mut TextRenderer,
    ) -> anyhow::Result<Option<InputResponse>> {
        let (
            Some(width),
            WindowEvent::KeyPressed {
                key,
                control: false,
                shift,
            },
        ) = (self.wrap_width, event)
        else {
            return Ok(None);
        };

        if !self.focused || !matches!(key, Key::Up | Key::Down | Key::Home | Key::End) {
            return Ok(None);
        }

        let layout = text.preserved_layout(&self.text, hit.font, width)?;
        let row = self.visual_row(&layout);
        let current = &layout.lines[row];
        let mut preferred_x = None;
        let (target, cursor) = match key {
            Key::Home => (row, current.source.start),
            Key::End => (row, current.source.end),
            Key::Up | Key::Down => {
                let target = if *key == Key::Up {
                    row.saturating_sub(1)
                } else {
                    (row + 1).min(layout.lines.len() - 1)
                };

                if target == row {
                    let edge = if *key == Key::Up {
                        current.source.start
                    } else {
                        current.source.end
                    };
                    (row, edge)
                } else {
                    let x = match self.preferred_x {
                        Some(x) => x,
                        None => text.caret_x(
                            &current.display,
                            hit.font,
                            current.display_offset(self.cursor),
                        )?,
                    };

                    preferred_x = Some(x);
                    let line = &layout.lines[target];
                    let offset = text.caret_at_x(&line.display, hit.font, x, true)?;
                    (target, line.source_offset(offset))
                }
            }

            _ => unreachable!(),
        };

        let changed = self.move_to(cursor, *shift) || target != row;
        self.caret_row = Some(target);
        self.preferred_x = preferred_x;
        if changed {
            self.reveal_caret = true;
            self.restart_caret();
        }

        Ok(Some(InputResponse {
            changed,
            ..InputResponse::default()
        }))
    }

    fn pointer_caret(
        &self,
        position: CursorPosition,
        hit: &InputHit,
        text: &mut TextRenderer,
        nearest: bool,
    ) -> anyhow::Result<(usize, Option<usize>)> {
        let x = position.x as f32 - hit.origin_x;
        let Some(width) = self.wrap_width else {
            return Ok((
                text.caret_at_x(&self.text, hit.font, x + self.scroll_x, nearest)?,
                None,
            ));
        };

        let layout = text.preserved_layout(&self.text, hit.font, width)?;
        let y = position.y as f32 - hit.origin_y + self.scroll_y;
        let (cursor, row) = text.preserved_caret_at(&layout, hit.font, [x, y], nearest)?;
        Ok((cursor, Some(row)))
    }

    pub fn pointer_event(
        &mut self,
        event: &WindowEvent,
        pointer: Option<CursorPosition>,
        hit: &InputHit,
        text: &mut TextRenderer,
    ) -> anyhow::Result<()> {
        let Bounds {
            x,
            y,
            width,
            height,
        } = hit.bounds;
        match event {
            WindowEvent::MouseButtonPressed {
                button: MouseButton::Left,
                shift,
            } => {
                let Some(position) = pointer.filter(|p| {
                    let [px, py] = [p.x as f32, p.y as f32];
                    px >= x && px < x + width && py >= y && py < y + height
                }) else {
                    return Ok(());
                };

                let count = self.mouse_selection.click_count(position, *shift);
                let (cursor, row) = self.pointer_caret(position, hit, text, count != 2)?;
                if *shift {
                    let anchor = self.anchor.unwrap_or(self.cursor);
                    self.move_to(cursor, true);
                    self.caret_row = row;
                    self.mouse_selection.drag_anchor = Some(anchor..anchor);
                    self.reveal_caret = true;
                    self.restart_caret();
                    return Ok(());
                }

                let range = MouseSelection::click_range(&self.text, cursor, count);
                self.move_to(range.start, false);
                if !range.is_empty() {
                    self.move_to(range.end, true);
                } else {
                    self.caret_row = row;
                }

                self.mouse_selection.drag_anchor = Some(self.selection());
                self.reveal_caret = true;
                self.restart_caret();
            }

            WindowEvent::MouseMoved { position } => {
                if let Some(anchor) = self.mouse_selection.drag_anchor.clone() {
                    let by_word = !anchor.is_empty();
                    let (cursor, row) = self.pointer_caret(*position, hit, text, !by_word)?;
                    let (anchor, cursor) = MouseSelection::drag_to(&self.text, anchor, cursor);
                    self.anchor = Some(anchor);
                    self.move_to(cursor, true);

                    self.caret_row = row;
                    self.reveal_caret = true;
                    self.restart_caret();
                }
            }

            WindowEvent::MouseScrolled { delta } => {
                if let Some(width) = self.wrap_width
                    && pointer.is_some_and(|p| {
                        let [px, py] = [p.x as f32, p.y as f32];
                        px >= x
                            && px < x + hit.bounds.width
                            && py >= y
                            && py < y + hit.bounds.height
                    })
                {
                    let layout = text.preserved_layout(&self.text, hit.font, width)?;
                    let change = match delta {
                        ScrollDelta::Lines { y, .. } => *y as f32 * layout.line_height * 3.0,
                        ScrollDelta::Pixels { y, .. } => *y as f32,
                    };
                    self.scroll_y = (self.scroll_y + change)
                        .clamp(0.0, (layout.height - hit.viewport_height).max(0.0));
                    self.reveal_caret = false;
                }
            }

            WindowEvent::MouseButtonReleased {
                button: MouseButton::Left,
            }
            | WindowEvent::MouseCancelled => self.mouse_selection.drag_anchor = None,

            _ => {}
        }

        Ok(())
    }

    fn render_multiline<G: GraphicsDevice>(
        &mut self,
        bounds: Bounds,
        paint: InputPaint,
        font: FontHandle,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let width = (bounds.width - 2.0 * paint.padding).max(0.0);
        let height = (bounds.height - 2.0 * paint.padding).max(0.0);
        let layout = text.preserved_layout(&self.text, font, self.wrap_width.unwrap())?;
        let row = self.visual_row(&layout);
        let line = &layout.lines[row];
        let caret_x = text.caret_x(&line.display, font, line.display_offset(self.cursor))?;
        let caret_y = row as f32 * layout.line_height;
        self.scroll_y = self.scroll_y.min((layout.height - height).max(0.0));
        if self.reveal_caret || self.viewport_height != height {
            self.scroll_y = scroll_to_caret(
                self.scroll_y,
                caret_y,
                (height - layout.line_height).max(0.0),
            );
            self.reveal_caret = false;
        }

        self.viewport_height = height;

        let x = bounds.x + paint.padding;
        let y = bounds.y + paint.padding;
        let top = y - self.scroll_y;
        ui.push_clip([x, y, width, height]);
        let result = (|| -> anyhow::Result<()> {
            let selection = self.selection();
            if self.focused && !selection.is_empty() {
                text.render_preserved_selection(
                    [x, top],
                    &layout,
                    font,
                    selection,
                    paint.selection,
                    ui,
                )?;
            }

            text.render_preserved([x, top], &layout, gpu, ui)?;
            if self.caret_visible {
                let thickness = 1.5_f32.min(width);
                ui.quad(
                    [
                        x + caret_x.min(width - thickness),
                        top + caret_y,
                        thickness,
                        layout.line_height,
                    ],
                    paint.caret,
                )?;
            }

            Ok(())
        })();
        ui.pop_clip();
        result
    }

    pub fn render<G: GraphicsDevice>(
        &mut self,
        bounds: Bounds,
        paint: InputPaint,
        font: FontHandle,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        if self.wrap_width.is_some() {
            return self.render_multiline(bounds, paint, font, text, gpu, ui);
        }

        let available = (bounds.width - paint.padding * 2.0).max(0.0);
        let caret_width = 1.5_f32.min(available);
        let caret = text.caret_x(&self.text, font, self.cursor)?;
        self.scroll_x = scroll_to_caret(self.scroll_x, caret, (available - caret_width).max(0.0));
        let x = bounds.x + paint.padding;
        let y = bounds.y + paint.padding;
        let height = (bounds.height - paint.padding * 2.0).max(0.0);
        ui.push_clip([x, y, available, height]);
        let result = (|| -> anyhow::Result<()> {
            let selected = self.selection();
            if self.focused && !selected.is_empty() {
                for [left, right] in text.selection_spans(&self.text, font, selected)? {
                    ui.quad(
                        [x + left - self.scroll_x, y, right - left, height],
                        paint.selection,
                    )?;
                }
            }

            if !self.text.is_empty() {
                let text_height = text.measure(&self.text, font)?.height;
                let text_y = y + (height - text_height).max(0.0) * 0.5;
                text.render(x - self.scroll_x, text_y, &self.text, font, gpu, ui)?;
            }

            if self.caret_visible {
                ui.quad(
                    [x + caret - self.scroll_x, y, caret_width, height],
                    paint.caret,
                )?;
            }

            Ok(())
        })();
        ui.pop_clip();
        result
    }
}

fn scroll_to_caret(scroll: f32, caret: f32, viewport: f32) -> f32 {
    if caret < scroll {
        caret.max(0.0)
    } else if caret > scroll + viewport {
        (caret - viewport).max(0.0)
    } else {
        scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(field: &mut InputField, key: Key, control: bool, shift: bool) -> InputResponse {
        field.handle(&WindowEvent::KeyPressed {
            key,
            control,
            shift,
        })
    }

    #[test]
    fn composer_keeps_paste_indentation_and_submits_the_exact_draft() {
        let mut field = InputField::new(4096);
        field.set_wrap_width(Some(120.0));
        field.paste("first\r\n\r\n  second\r\tthird");
        assert_eq!(field.text(), "first\n\n  second\n\tthird");

        let newline = key(&mut field, Key::Enter, false, true);
        assert!(newline.changed);
        assert!(newline.submitted.is_none());
        assert_eq!(field.text(), "first\n\n  second\n\tthird\n");
        key(&mut field, Key::Undo, true, false);
        assert_eq!(field.text(), "first\n\n  second\n\tthird");
        key(&mut field, Key::Redo, true, true);
        assert_eq!(
            key(&mut field, Key::Enter, false, false)
                .submitted
                .as_deref(),
            Some("first\n\n  second\n\tthird\n"),
        );

        let mut single_line = InputField::new(4096);
        single_line.paste("one\r\n\ttwo");
        assert_eq!(single_line.text(), "onetwo");
    }

    #[test]
    fn composer_navigation_and_pointer_use_visual_rows() -> anyhow::Result<()> {
        let mut text = TextRenderer::new()?;
        let font = text.load_primary_font(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/NotoSans-Bold.ttf"),
            16,
            0,
        )?;
        let wrap_width = text.caret_x("abcdef", font, 3)? + 1.0;
        let mut field = InputField::new(4096);
        field.set_wrap_width(Some(wrap_width));
        field.paste("abcdef\n\nxyz");
        let layout = text.preserved_layout(field.text(), font, wrap_width)?;
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| line.display.as_str())
                .collect::<Vec<_>>(),
            ["abc", "def", "", "xyz"]
        );

        let hit = InputHit {
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 120.0,
            },
            origin_x: 4.0,
            origin_y: 4.0,
            viewport_height: 112.0,
            font,
        };
        let click = WindowEvent::MouseButtonPressed {
            button: MouseButton::Left,
            shift: false,
        };
        let position = CursorPosition {
            x: f64::from(hit.origin_x + wrap_width),
            y: f64::from(hit.origin_y + layout.line_height * 0.5),
        };
        field.pointer_event(&click, Some(position), &hit, &mut text)?;
        assert_eq!(field.cursor, 3);
        assert_eq!(field.visual_row(&layout), 0);

        for (key, row, byte) in [
            (Key::Down, 1, 6),
            (Key::Down, 2, 7),
            (Key::Home, 2, 7),
            (Key::Down, 3, 8),
        ] {
            field.visual_key(
                &WindowEvent::KeyPressed {
                    key,
                    control: false,
                    shift: false,
                },
                &hit,
                &mut text,
            )?;
            assert_eq!(field.visual_row(&layout), row);
            assert_eq!(field.cursor, byte);
        }

        Ok(())
    }

    #[test]
    fn mouse_places_the_caret_and_selects_text_by_drag_or_click() -> anyhow::Result<()> {
        let mut text = TextRenderer::new()?;
        let font = text.load_primary_font(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/NotoSans-Bold.ttf"),
            16,
            0,
        )?;
        let hit = InputHit {
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 24.0,
            },
            origin_x: 4.0,
            origin_y: 0.0,
            viewport_height: 24.0,
            font,
        };
        let mut field = InputField::new(128);
        field.paste("hello world");
        let at = |text: &mut TextRenderer, offset| -> anyhow::Result<CursorPosition> {
            let x = text.caret_x("hello world", font, offset)? + hit.origin_x;
            Ok(CursorPosition {
                x: f64::from(x),
                y: 12.0,
            })
        };
        let from = at(&mut text, 1)?;
        let to = at(&mut text, 4)?;
        let press = |shift| WindowEvent::MouseButtonPressed {
            button: MouseButton::Left,
            shift,
        };
        let release = WindowEvent::MouseButtonReleased {
            button: MouseButton::Left,
        };
        field.pointer_event(&press(false), Some(from), &hit, &mut text)?;
        assert_eq!(field.cursor, 1);
        field.pointer_event(
            &WindowEvent::MouseMoved { position: to },
            Some(to),
            &hit,
            &mut text,
        )?;
        field.pointer_event(&release, Some(to), &hit, &mut text)?;
        assert_eq!(&field.text()[field.selection()], "ell");

        field.pointer_event(&press(true), Some(from), &hit, &mut text)?;
        field.pointer_event(&release, Some(from), &hit, &mut text)?;
        assert!(field.selection().is_empty());
        for expected in ["", "hello", "hello world"] {
            field.pointer_event(&press(false), Some(from), &hit, &mut text)?;
            field.pointer_event(&release, Some(from), &hit, &mut text)?;
            assert_eq!(&field.text()[field.selection()], expected);
        }

        Ok(())
    }

    #[test]
    fn word_selection_cut_undo_and_redo_preserve_the_rest_of_the_draft() {
        let mut field = InputField::new(128);
        field.paste("café, 世界!");
        key(&mut field, Key::Home, false, false);
        key(&mut field, Key::Right, true, true);
        assert_eq!(
            key(&mut field, Key::Copy, true, false).copied.as_deref(),
            Some("café")
        );
        assert_eq!(
            key(&mut field, Key::Cut, true, false).copied.as_deref(),
            Some("café")
        );
        assert_eq!(field.text(), ", 世界!");

        key(&mut field, Key::Undo, true, false);
        assert_eq!(field.text(), "café, 世界!");
        assert_eq!(&field.text()[field.selection()], "café");
        key(&mut field, Key::Redo, true, true);
        assert_eq!(field.text(), ", 世界!");
        assert!(key(&mut field, Key::Copy, true, false).copied.is_none());
    }

    #[test]
    fn ctrl_delete_and_backspace_remove_words_in_both_directions() {
        let mut field = InputField::new(128);
        field.paste("one, two three");
        key(&mut field, Key::Home, false, false);
        key(&mut field, Key::Right, true, false);
        key(&mut field, Key::Delete, true, false);
        assert_eq!(field.text(), "one three");
        key(&mut field, Key::End, false, false);
        key(&mut field, Key::Backspace, true, false);
        assert_eq!(field.text(), "one ");
    }

    #[test]
    fn shift_selection_keeps_graphemes_whole_and_escape_preserves_text() {
        let mut field = InputField::new(128);
        field.paste("a👩‍💻e\u{301}");
        key(&mut field, Key::Left, false, true);
        assert_eq!(&field.text()[field.selection()], "e\u{301}");
        key(&mut field, Key::Left, false, true);
        assert_eq!(&field.text()[field.selection()], "👩‍💻e\u{301}");
        key(&mut field, Key::Right, false, true);
        assert_eq!(&field.text()[field.selection()], "e\u{301}");
        key(&mut field, Key::Escape, false, false);
        assert!(field.selection().is_empty());
        assert_eq!(field.text(), "a👩‍💻e\u{301}");
    }

    #[test]
    fn typing_coalesces_and_paste_starts_a_separate_undo_step() {
        let mut field = InputField::new(128);
        field.handle(&WindowEvent::TextInput("a".into()));
        field.handle(&WindowEvent::TextInput("b".into()));
        field.paste("c");
        key(&mut field, Key::Undo, true, false);
        assert_eq!(field.text(), "ab");
        key(&mut field, Key::Undo, true, false);
        assert_eq!(field.text(), "");
        key(&mut field, Key::Redo, true, false);
        assert_eq!(field.text(), "ab");
        field.handle(&WindowEvent::TextInput("d".into()));
        assert!(!key(&mut field, Key::Redo, true, false).changed);
        field.clear();
        assert!(!key(&mut field, Key::Undo, true, false).changed);
    }

    #[test]
    fn caret_blinks_only_while_the_window_and_field_are_focused() {
        let mut field = InputField::new(32);
        let start = Instant::now();
        let half_period = Duration::from_millis(500);

        assert_eq!(field.update_caret(start, true), Some(start + half_period));
        assert!(field.caret_visible);
        assert_eq!(
            field.update_caret(start + half_period, true),
            Some(start + half_period * 2)
        );
        assert!(!field.caret_visible);

        assert_eq!(field.update_caret(start + half_period, false), None);
        assert!(!field.caret_visible);
        field.update_caret(start + half_period * 2, true);
        assert!(field.caret_visible);

        field.set_focused(false);
        assert_eq!(field.update_caret(start + half_period * 3, true), None);
        assert!(!field.caret_visible);
    }

    #[test]
    fn clipboard_shortcuts_replace_the_selection_and_keep_utf8_within_the_limit() {
        let mut field = InputField::new(5);
        field.handle(&WindowEvent::TextInput("draft".into()));
        let shortcut = |key| WindowEvent::KeyPressed {
            key,
            control: true,
            shift: false,
        };

        field.handle(&shortcut(Key::SelectAll));
        assert_eq!(
            field.handle(&shortcut(Key::Copy)).copied.as_deref(),
            Some("draft")
        );
        assert!(field.handle(&shortcut(Key::Paste)).paste);

        field.paste("ééé");
        assert_eq!(field.text(), "éé");

        field.handle(&shortcut(Key::SelectAll));
        field.handle(&WindowEvent::FocusChanged(false));
        assert!(!field.handle(&shortcut(Key::Paste)).paste);
    }
}
