use std::time::{Duration, Instant};

use crate::{
    graphics::{Color, FontHandle, GraphicsDevice},
    platform::event::{Key, WindowEvent},
    ui::{TextRenderer, UIRenderer},
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

/// A single-line, UTF-8 input widget. Editing is grapheme-aware; platform text
/// commits are distinct from navigation keys. The caller schedules caret redraws.
pub struct InputField {
    options: InputOptions,
    text: String,
    cursor: usize,
    selected: bool,
    focused: bool,
    max_bytes: usize,
    scroll_x: f32,
    caret_since: Option<Instant>,
    caret_visible: bool,
}

impl InputField {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            options: InputOptions::default(),
            text: String::new(),
            cursor: 0,
            selected: false,
            focused: true,
            max_bytes,
            scroll_x: 0.0,
            caret_since: None,
            caret_visible: false,
        }
    }

    pub fn set_options(&mut self, options: InputOptions) {
        self.options = options;
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

    pub fn handle(&mut self, event: &WindowEvent) -> InputResponse {
        let mut response = InputResponse::default();

        match event {
            WindowEvent::FocusChanged(focused) => {
                response.changed = self.focused != *focused;
                self.set_focused(*focused);
            }

            WindowEvent::TextInput(text) if self.focused => {
                let existing = if self.selected {
                    ""
                } else {
                    &self.text
                };
                let remaining = self.options.max_chars.map_or(usize::MAX, |max| {
                    max.saturating_sub(existing.chars().count())
                });

                let mut bytes = self.max_bytes.saturating_sub(existing.len());
                let text: String = text
                    .chars()
                    .filter(|c| {
                        !c.is_control() && self.options.filter.is_none_or(|filter| filter(*c))
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

                if !text.is_empty() {
                    if self.selected {
                        self.clear();
                    }

                    self.text.insert_str(self.cursor, &text);
                    self.cursor += text.len();
                    response.changed = true;
                }
            }

            WindowEvent::KeyPressed { key, control } if self.focused => {
                let before = self.cursor;

                match key {
                    Key::SelectAll => {
                        self.selected = !self.text.is_empty();
                        self.cursor = self.text.len();
                        response.changed = true;
                    }

                    Key::Copy if !self.text.is_empty() => response.copied = Some(self.text.clone()),
                    Key::Paste => response.paste = true,
                    Key::Backspace | Key::Delete if self.selected => {
                        self.clear();
                        response.changed = true;
                    }

                    Key::Left if self.selected => self.cursor = 0,
                    Key::Right if self.selected => self.cursor = self.text.len(),

                    Key::Enter if !self.text.trim().is_empty() => {
                        response.submitted = Some(self.text.clone());
                        response.changed = true;
                    }

                    Key::Backspace if self.cursor > 0 => {
                        let start = if *control {
                            self.text[..self.cursor]
                                .trim_end()
                                .char_indices()
                                .rev()
                                .find(|(_, c)| c.is_whitespace())
                                .map_or(0, |(i, c)| i + c.len_utf8())
                        } else {
                            self.previous()
                        };

                        self.text.replace_range(start..self.cursor, "");
                        self.cursor = start;
                        response.changed = true;
                    }

                    Key::Delete if self.cursor < self.text.len() => {
                        let end = self.next();
                        self.text.replace_range(self.cursor..end, "");
                        response.changed = true;
                    }

                    Key::Left => {
                        self.cursor = if *control {
                            0
                        } else {
                            self.previous()
                        }
                    }

                    Key::Right => {
                        self.cursor = if *control {
                            self.text.len()
                        } else {
                            self.next()
                        }
                    }

                    Key::Home => self.cursor = 0,
                    Key::End => self.cursor = self.text.len(),
                    Key::Escape => {
                        self.text.clear();
                        self.cursor = 0;
                        response.changed = true;
                    }

                    _ => {}
                }

                if matches!(
                    key,
                    Key::Left | Key::Right | Key::Home | Key::End | Key::Escape
                ) {
                    response.changed |= self.selected;
                    self.selected = false;
                }

                response.changed |= self.cursor != before;
            }

            _ => {}
        }

        // we can end up inside a grapheme when text joins across the caret
        if response.changed && self.cursor < self.text.len() {
            self.cursor = self
                .text
                .grapheme_indices(true)
                .find(|(i, _)| *i >= self.cursor)
                .map_or(self.text.len(), |(i, _)| i);
        }

        if response.changed {
            self.restart_caret();
        }

        response
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.restart_caret();
        }

        self.focused = focused;
        if !focused {
            self.selected = false;
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
        self.selected = false;
        self.restart_caret();
    }

    pub fn render<G: GraphicsDevice>(
        &mut self,
        bounds: Bounds,
        style: super::super::framework::Style,
        selection_color: Color,
        font: FontHandle,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let available = (bounds.width - style.padding * 2.0).max(0.0);
        let caret_width = 1.5_f32.min(available);
        let caret = text.caret_x(&self.text, font, self.cursor)?;
        self.scroll_x = scroll_to_caret(self.scroll_x, caret, (available - caret_width).max(0.0));
        let x = bounds.x + style.padding;
        let y = bounds.y + style.padding;
        let height = (bounds.height - style.padding * 2.0).max(0.0);
        ui.push_clip([x, y, available, height]);
        let result = (|| -> anyhow::Result<()> {
            if self.focused && self.selected {
                ui.quad([x - self.scroll_x, y, caret, height], selection_color)?;
            }

            text.render(x - self.scroll_x, y, &self.text, font, gpu, ui)?;

            if self.caret_visible {
                ui.quad(
                    [x + caret - self.scroll_x, y, caret_width, height],
                    style.color,
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
        let shortcut = |key| WindowEvent::KeyPressed { key, control: true };

        field.handle(&shortcut(Key::SelectAll));
        assert_eq!(
            field.handle(&shortcut(Key::Copy)).copied.as_deref(),
            Some("draft")
        );
        assert!(field.handle(&shortcut(Key::Paste)).paste);

        field.handle(&WindowEvent::TextInput("ééé".into()));
        assert_eq!(field.text(), "éé");

        field.handle(&shortcut(Key::SelectAll));
        field.handle(&WindowEvent::FocusChanged(false));
        assert!(!field.handle(&shortcut(Key::Paste)).paste);
    }
}
