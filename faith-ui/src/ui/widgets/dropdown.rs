use super::{
    button,
    input::{Bounds, InputField, InputResponse},
};
use crate::{
    graphics::FontHandle,
    platform::{
        WindowEvent,
        event::Key,
        mouse::{CursorPosition, MouseButton},
    },
    ui::{
        scroll::contains,
        search::{Text, normalize},
    },
};
use std::sync::Arc;

pub const DOWN: &str = "\u{eab4}";
pub const UP: &str = "\u{eab7}";
pub const CHECK: &str = "\u{eab2}";
pub const ROW_HEIGHT: f32 = 38.0;
pub const SEARCH_HEIGHT: f32 = 30.0;

pub struct State {
    pub open: bool,
    pub presence: crate::ui::animation::Presence,
    header: button::State,
    cursor: Option<usize>,
    pub reveal: bool,
    options: Arc<[String]>,
    fields: Vec<Text>,
    pub search: InputField,
    query: String,
    order: Arc<[usize]>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            presence: Default::default(),
            header: Default::default(),
            cursor: None,
            reveal: false,
            options: Default::default(),
            fields: Vec::new(),
            search: InputField::new(4096),
            query: String::new(),
            order: Default::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Options<'a> {
    Borrowed(&'a [&'a str]),
    Shared(&'a Arc<[String]>),
}

impl<'a> Options<'a> {
    pub fn len(self) -> usize {
        match self {
            Self::Borrowed(values) => values.len(),
            Self::Shared(values) => values.len(),
        }
    }

    pub fn get(self, index: usize) -> Option<&'a str> {
        match self {
            Self::Borrowed(values) => values.get(index).copied(),
            Self::Shared(values) => values.get(index).map(String::as_str),
        }
    }
}

pub struct Response {
    pub hovered: bool,
    pub pressed: bool,
}

pub fn popup_bounds(header: Bounds, count: usize, window: [f32; 2]) -> Bounds {
    let width = header.width.min((window[0] - 16.0).max(0.0));
    let below = header.y + header.height + 4.0;
    let space_below = (window[1] - below - 8.0).max(0.0);
    let space_above = (header.y - 12.0).max(0.0);
    let count = count.max(1);
    let header_height = SEARCH_HEIGHT + 8.0;
    let desired = count as f32 * ROW_HEIGHT + header_height;
    let open_below = desired <= space_below || space_below >= space_above;
    let available = if open_below {
        space_below
    } else {
        space_above
    };
    let rows = (((available - header_height) / ROW_HEIGHT).floor().max(0.0) as usize).min(count);
    let height = (rows as f32 * ROW_HEIGHT + header_height).min(available);
    let x = header.x.clamp(8.0, (window[0] - width - 8.0).max(8.0));
    let y = if open_below {
        below
    } else {
        header.y - height - 4.0
    };

    Bounds {
        x,
        y,
        width,
        height,
    }
}

impl State {
    pub fn close(&mut self) {
        self.open = false;
        self.search.set_focused(false);
        self.cursor = None;
    }

    pub fn open_with_selection(&mut self, selected: usize) {
        self.open = true;
        self.search.clear();
        self.search.set_focused(true);
        self.query.clear();
        self.filter();
        self.cursor = Some(selected);
    }

    pub fn keyboard(
        &mut self,
        focused: bool,
        selected: usize,
        events: &[WindowEvent],
        id: &str,
        clipboard: &mut InputResponse,
    ) -> (Option<usize>, bool) {
        if self.open && self.cursor.is_none() && self.query.is_empty() {
            self.open_with_selection(selected);
        }

        for event in events.iter().filter(|_| focused) {
            match event {
                WindowEvent::TextInput(text) if !self.open && text == " " => {
                    self.open_with_selection(selected)
                }
                WindowEvent::KeyPressed {
                    key: Key::Enter, ..
                } => {
                    if self.open {
                        let choice = self
                            .cursor
                            .and_then(|cursor| self.order.get(cursor).copied());
                        if choice.is_some() {
                            self.close();
                            return (choice, false);
                        }
                    } else {
                        self.open_with_selection(selected);
                    }
                }
                WindowEvent::KeyPressed {
                    key: Key::Escape, ..
                } => {
                    if self.open {
                        self.close();
                    } else {
                        return (None, true);
                    }
                }
                WindowEvent::KeyPressed {
                    key: key @ (Key::Up | Key::Down | Key::PageUp | Key::PageDown),
                    ..
                } if self.open && !self.order.is_empty() => {
                    let count = self.order.len();
                    let cursor = self.cursor.unwrap_or(0).min(count - 1);
                    let next = match key {
                        Key::Down => (cursor + 1) % count,
                        Key::Up => (cursor + count - 1) % count,
                        Key::PageDown => (cursor + 5).min(count - 1),
                        Key::PageUp => cursor.saturating_sub(5),
                        _ => unreachable!(),
                    };
                    self.cursor = Some(next);
                    self.reveal = true;
                }
                WindowEvent::ClipboardText { target, text } if self.open && target == id => {
                    let response = self.search.paste(text);
                    self.update_query(response.changed);
                }
                WindowEvent::TextInput(_) | WindowEvent::KeyPressed { .. } if self.open => {
                    let response = self.search.handle(event);
                    clipboard.copied = response.copied.or(clipboard.copied.take());
                    clipboard.paste |= response.paste;
                    self.update_query(response.changed);
                }
                _ => {}
            }
        }

        (None, false)
    }

    fn update_query(&mut self, changed: bool) {
        if changed && self.search.text() != self.query {
            self.query.clear();
            self.query.push_str(self.search.text());
            self.filter();
        }
    }

    fn filter(&mut self) {
        let normalized = normalize(&self.query);
        if normalized.is_empty() {
            self.order = (0..self.options.len()).collect();
        } else {
            if self.fields.is_empty() {
                self.fields = self.options.iter().map(|value| Text::new(value)).collect();
            }

            let tokens: Vec<Vec<char>> = normalized
                .split_whitespace()
                .map(|token| token.chars().collect())
                .collect();
            let mut ranked: Vec<_> = self
                .fields
                .iter()
                .enumerate()
                .filter_map(|(index, field)| {
                    let score = tokens
                        .iter()
                        .map(|token| field.score(token))
                        .sum::<Option<i32>>()?;
                    Some((std::cmp::Reverse(score), index))
                })
                .collect();
            ranked.sort_unstable();
            self.order = ranked.into_iter().map(|(_, index)| index).collect();
        }

        self.cursor = (!self.order.is_empty()).then_some(0);
        self.reveal = true;
    }

    pub fn sync_options(&mut self, options: Options<'_>) {
        match options {
            Options::Borrowed(values) => {
                if self.options.len() == values.len()
                    && self.options.iter().zip(values).all(|(a, b)| a == b)
                {
                    return;
                }

                self.options = values.iter().map(|value| (*value).to_owned()).collect();
            }
            Options::Shared(values) => {
                if Arc::ptr_eq(&self.options, values) {
                    return;
                }

                self.options = values.clone();
            }
        }

        self.fields.clear();
        if self.open {
            self.filter();
        }
    }

    pub fn row_count(&self) -> usize {
        self.order.len()
    }

    pub fn contents(&self) -> Contents {
        Contents {
            options: self.options.clone(),
            rows: self.order.clone(),
            highlighted: self.cursor,
        }
    }

    pub fn update(
        &mut self,
        header: Bounds,
        popup: Bounds,
        mut pointer: Option<CursorPosition>,
        events: &[WindowEvent],
    ) -> Response {
        for event in events {
            match event {
                WindowEvent::MouseEntered { position } | WindowEvent::MouseMoved { position } => {
                    pointer = Some(*position)
                }
                WindowEvent::MouseLeft => pointer = None,
                WindowEvent::MouseCancelled | WindowEvent::FocusChanged(false) => {
                    /* scrollbars consume pointer events without dismissing the popup */
                    self.header = button::State::default();

                    if matches!(event, WindowEvent::FocusChanged(false)) {
                        self.close();
                    }
                }

                WindowEvent::MouseButtonPressed {
                    button: MouseButton::Left,
                    ..
                } if self.open && !contains(header, pointer) && !contains(popup, pointer) => {
                    self.close()
                }
                _ => {}
            }

            if self
                .header
                .update(header, pointer, std::slice::from_ref(event))
                .clicked
            {
                if self.open {
                    self.close();
                } else {
                    self.open = true;
                }
            }
        }

        let response = self.header.update(header, pointer, &[]);
        Response {
            hovered: response.hovered,
            pressed: response.pressed,
        }
    }
}

pub struct Contents {
    pub options: Arc<[String]>,
    pub rows: Arc<[usize]>,
    pub highlighted: Option<usize>,
}

pub struct Popup {
    pub id: String,
    pub bounds: Bounds,
    pub contents: Contents,
    pub selected: usize,
    pub reveal: bool,
    pub font: FontHandle,
    pub icons: FontHandle,
    pub font_size: u32,
    pub presence: f32,
    pub interactive: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key) -> WindowEvent {
        WindowEvent::KeyPressed {
            key,
            control: false,
            shift: false,
        }
    }

    fn keyboard(dropdown: &mut State, events: &[WindowEvent]) -> (Option<usize>, bool) {
        dropdown.keyboard(true, 0, events, "test", &mut InputResponse::default())
    }

    #[test]
    fn fuzzy_search_returns_the_original_option_index() {
        let mut dropdown = State::default();
        let options = ["Bloodvoid", "Catppuccin", "Gruvbox", "Nord"];
        dropdown.sync_options(Options::Borrowed(&options));
        dropdown.open_with_selection(0);
        keyboard(&mut dropdown, &[WindowEvent::TextInput("grbx".into())]);
        assert_eq!(dropdown.contents().rows.as_ref(), &[2]);
        assert_eq!(
            keyboard(&mut dropdown, &[key(Key::Enter)]),
            (Some(2), false)
        );
    }

    #[test]
    fn search_edits_at_the_caret_and_routes_clipboard_to_the_dropdown() {
        let mut dropdown = State::default();
        let options = ["Nord", "Gruvbox"];
        dropdown.sync_options(Options::Borrowed(&options));
        dropdown.open_with_selection(1);
        let mut clipboard = InputResponse::default();
        dropdown.keyboard(
            true,
            1,
            &[
                WindowEvent::TextInput("ord".into()),
                key(Key::Home),
                WindowEvent::TextInput("N".into()),
                key(Key::SelectAll),
                key(Key::Copy),
                key(Key::Paste),
                WindowEvent::ClipboardText {
                    target: "other".into(),
                    text: "ignored".into(),
                },
                WindowEvent::ClipboardText {
                    target: "test".into(),
                    text: "grbx".into(),
                },
            ],
            "test",
            &mut clipboard,
        );

        assert_eq!(clipboard.copied.as_deref(), Some("Nord"));
        assert!(clipboard.paste);
        assert_eq!(dropdown.search.text(), "grbx");
        assert_eq!(dropdown.contents().rows.as_ref(), &[1]);
    }

    #[test]
    fn an_empty_search_result_stays_open_and_can_be_corrected() {
        let mut dropdown = State::default();
        let options = Arc::from(["DejaVu Sans".to_owned(), "Noto Sans".to_owned()]);
        dropdown.sync_options(Options::Shared(&options));
        dropdown.open_with_selection(0);
        keyboard(
            &mut dropdown,
            &[WindowEvent::TextInput("zzzz".into()), key(Key::Enter)],
        );
        assert!(dropdown.open);
        assert_eq!(dropdown.row_count(), 0);
        keyboard(
            &mut dropdown,
            &[
                key(Key::SelectAll),
                WindowEvent::TextInput("nt sans".into()),
            ],
        );
        assert_eq!(dropdown.contents().rows.first(), Some(&1));
        assert_eq!(
            keyboard(&mut dropdown, &[key(Key::Enter)]),
            (Some(1), false)
        );
        dropdown.open_with_selection(0);
        assert!(dropdown.search.text().is_empty());
    }
}
