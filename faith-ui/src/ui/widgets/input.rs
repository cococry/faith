use crate::{
    graphics::{FontHandle, GraphicsDevice},
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
}

#[derive(Clone, Copy, Default)]
pub struct InputOptions {
    pub max_chars: Option<usize>,
    pub filter: Option<fn(char) -> bool>,
}

/// A single-line, UTF-8 input widget. Editing is grapheme-aware; platform text
/// commits are distinct from navigation keys. No timer or rendering loop lives
/// here. The caller redraws its bounds only when a response reports a change.
pub struct InputField {
    options: InputOptions,
    text: String,
    cursor: usize,
    focused: bool,
    max_bytes: usize,
    scroll_x: f32,
}
impl InputField {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            options: InputOptions::default(),
            text: String::new(),
            cursor: 0,
            focused: true,
            max_bytes,
            scroll_x: 0.0,
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
                self.focused = *focused;
            }
            WindowEvent::TextInput(text) if self.focused => {
                let remaining = self.options.max_chars.map_or(usize::MAX, |max| {
                    max.saturating_sub(self.text.chars().count())
                });
                let text: String = text
                    .chars()
                    .filter(|c| {
                        !c.is_control() && self.options.filter.is_none_or(|filter| filter(*c))
                    })
                    .take(remaining)
                    .collect();
                if !text.is_empty() && self.text.len() + text.len() <= self.max_bytes {
                    self.text.insert_str(self.cursor, &text);
                    self.cursor += text.len();
                    // A combining commit can join adjacent graphemes. Keep the
                    // cursor on a valid grapheme boundary after insertion.
                    if let Some((next, _)) = self
                        .text
                        .grapheme_indices(true)
                        .find(|(i, _)| *i >= self.cursor)
                    {
                        self.cursor = next;
                    } else {
                        self.cursor = self.text.len();
                    }
                    response.changed = true;
                }
            }
            WindowEvent::KeyPressed { key, control } if self.focused => {
                let before = self.cursor;
                match key {
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
                    Key::Left => self.cursor = if *control { 0 } else { self.previous() },
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
                response.changed |= self.cursor != before;
            }
            _ => {}
        }
        if !self.text.is_empty()
            && !self
                .text
                .grapheme_indices(true)
                .any(|(i, _)| i == self.cursor)
        {
            self.cursor = self
                .text
                .grapheme_indices(true)
                .find(|(i, _)| *i > self.cursor)
                .map_or(self.text.len(), |(i, _)| i);
        }
        response
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll_x = 0.0;
    }
    pub fn render<G: GraphicsDevice>(
        &mut self,
        bounds: Bounds,
        style: super::super::framework::Style,
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
            text.render(x - self.scroll_x, y, &self.text, font, gpu, ui)?;
            if self.focused {
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
