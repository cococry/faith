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
}

/// A single-line, UTF-8 input widget. Editing is grapheme-aware; platform text
/// commits are distinct from navigation keys. No timer or rendering loop lives
/// here. The caller redraws its bounds only when a response reports a change.
pub struct InputField {
    text: String,
    cursor: usize,
    focused: bool,
    max_bytes: usize,
}
impl InputField {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            focused: true,
            max_bytes,
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
    pub fn handle(&mut self, event: &WindowEvent) -> InputResponse {
        let mut response = InputResponse::default();
        match event {
            WindowEvent::FocusChanged(focused) => {
                response.changed = self.focused != *focused;
                self.focused = *focused;
            }
            WindowEvent::TextInput(text) if self.focused => {
                let text: String = text.chars().filter(|c| !c.is_control()).collect();
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
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn render<G: GraphicsDevice>(
        &self,
        bounds: Bounds,
        font: FontHandle,
        text: &mut TextRenderer,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        ui.quad(
            [bounds.x, bounds.y, bounds.width, bounds.height],
            Color::rgba(0.88, 0.91, 0.95, 1.0),
        )?;

        let available = (bounds.width - 24.0).max(1.0);

        let mut boundaries: Vec<usize> = self.text.grapheme_indices(true).map(|(i, _)| i).collect();

        boundaries.push(self.text.len());
        let cursor = boundaries.binary_search(&self.cursor).unwrap_or(0);
        let mut lo = 0;
        let mut hi = cursor;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if text
                .measure(&self.text[boundaries[mid]..self.cursor], font)?
                .width
                > available - 3.0
            {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let start = boundaries[lo];
        let mut lo = cursor;
        let mut hi = boundaries.len() - 1;
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if text
                .measure(&self.text[start..boundaries[mid]], font)?
                .width
                <= available
            {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let shown = &self.text[start..boundaries[lo]];
        let y = bounds.y + 10.0;
        text.render(bounds.x + 10.0, y, shown, font, gpu, ui)?;
        if self.focused {
            let caret = text.measure(&self.text[start..self.cursor], font)?.width;
            ui.quad(
                [
                    bounds.x + 10.0 + caret,
                    y,
                    1.5,
                    (bounds.height - 20.0).max(1.0),
                ],
                Color::rgba(0.1, 0.15, 0.25, 1.0),
            )?;
        }
        Ok(())
    }
}
