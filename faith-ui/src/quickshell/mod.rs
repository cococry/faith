pub mod catalog;
mod rank;

use std::collections::HashMap;

use crate::{
    Action, App,
    graphics::Renderer,
    platform::{WindowEvent, event::Key, mouse::MouseButton},
    ui::{
        framework::{ActionRow, DivOptions, Framework, Style},
        widgets::input::InputOptions,
    },
};
use catalog::{Context, Entry, Kind};

const QUERY: &str = "quickshell-query";
const ROW: f32 = 56.0;

struct SearchResult {
    index: usize,
    score: i32,
    path: Vec<usize>,
    title: String,
}

struct Match<'a> {
    entry: &'a Entry,
    score: i32,
    path: Vec<usize>,
}

fn find<'a>(
    entry: &'a Entry,
    tokens: &[Vec<char>],
    inherited: &[Option<i32>],
) -> Option<Match<'a>> {
    let own: Vec<_> = tokens
        .iter()
        .map(|token| {
            entry
                .primary
                .score(token)
                .into_iter()
                .chain(
                    entry
                        .terms
                        .iter()
                        .filter_map(|field| field.score(token).map(|score| score - 500)),
                )
                .max()
        })
        .collect();
    let scores: Vec<_> = own
        .iter()
        .enumerate()
        .map(|(index, score)| (*score).max(inherited.get(index).copied().flatten()))
        .collect();
    /* a descendant must contribute a better match, not just inherit a generic parent query */
    let specific =
        inherited.is_empty() || own.iter().zip(inherited).any(|(own, parent)| own > parent);
    let mut best = if specific {
        scores
            .iter()
            .copied()
            .sum::<Option<i32>>()
            .map(|score| Match {
                entry,
                score: score / tokens.len() as i32,
                path: Vec::new(),
            })
    } else {
        None
    };
    if let Some(children) = &entry.children {
        for (index, child) in children.iter().enumerate() {
            if let Some(mut candidate) = find(child, tokens, &scores) {
                let better = best.as_ref().is_none_or(|best| {
                    candidate.score > best.score
                        || (candidate.score == best.score
                            && !best.path.is_empty()
                            && (&candidate.entry.label, &candidate.entry.id)
                                < (&best.entry.label, &best.entry.id))
                });
                if better {
                    candidate.path.insert(0, index);
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn descendant<'a>(mut entry: &'a Entry, path: &[usize]) -> &'a Entry {
    for &index in path {
        entry = &entry.children.as_ref().unwrap()[index];
    }
    entry
}

#[derive(Default)]
struct Usage {
    count: u16,
    last: u64,
}

struct Level {
    entries: Vec<Entry>,
    query: String,
    selected: usize,
    placeholder: String,
    parent: usize,
}

#[derive(Default)]
pub struct Shell {
    open: bool,
    presence: crate::ui::animation::Presence,
    entries: Vec<Entry>,
    context: Option<Context>,
    query: String,
    results: Vec<SearchResult>,
    selected: usize,
    reveal: bool,
    usage: HashMap<String, Usage>,
    clock: u64,
    levels: Vec<Level>,
    placeholder: String,
}

impl Shell {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_visible(&self) -> bool {
        self.open || self.presence.visible()
    }

    pub fn open(&mut self, app: &App, zoom: u32, ui: &mut Framework) {
        self.levels.clear();
        self.placeholder = "Run an action directly".into();
        self.entries = catalog::build(app, zoom);
        self.context = Some(Context::from_app(app));
        self.query.clear();
        self.selected = 0;
        self.reveal = true;
        self.open = true;
        self.search("");
        ui.clear_input(QUERY);
    }

    fn search(&mut self, query: &str) {
        let (scope, text) = query
            .split_once(':')
            .map_or((None, query), |(prefix, text)| {
                let scope = match prefix.trim().to_lowercase().as_str() {
                    "profile" | "profiles" | "account" => Some(Kind::Profile),
                    "chat" | "conversation" => Some(Kind::Conversation),
                    "settings" | "setting" => Some(Kind::Settings),
                    "theme" => Some(Kind::Theme),
                    _ => None,
                };
                if scope.is_some() {
                    (scope, text)
                } else {
                    (None, query)
                }
            });
        let normalized = rank::normalize(text);
        let words: Vec<_> = normalized.split_whitespace().collect();
        let tokens: Vec<Vec<char>> = words
            .iter()
            .filter(|word| {
                words.len() == 1 || !matches!(**word, "go" | "to" | "open" | "with" | "the")
            })
            .take(8)
            .map(|word| word.chars().take(32).collect())
            .collect();
        let context = self.context.as_ref().unwrap();
        let mut results = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if scope.is_some_and(|kind| kind != entry.kind) {
                continue;
            }

            let matched = if tokens.is_empty() {
                Match {
                    entry,
                    score: 0,
                    path: Vec::new(),
                }
            } else if let Some(matched) = find(entry, &tokens, &[]) {
                matched
            } else {
                continue;
            };
            let target = matched.entry;
            let usage = self.usage.get(&target.id).map_or(0, |usage| {
                i32::from(usage.count.min(5)) * 8
                    + (60_u64.saturating_sub(self.clock.saturating_sub(usage.last) * 4)) as i32
            });
            let score = if tokens.is_empty() {
                entry.priority
            } else {
                matched.score + target.priority.min(200) / 5
            };
            /* we prefer themes for generic theme and selector queries, even from settings */
            let generic_selector = !normalized.is_empty() && "selector".starts_with(&normalized);
            let preference =
                if entry.id == "select-theme" && (normalized == "theme" || generic_selector) {
                    300
                } else {
                    0
                };
            let mut title = entry.label.clone();
            let mut parent = entry;
            for &index in &matched.path {
                parent = &parent.children.as_ref().unwrap()[index];
                title.push_str(" | ");
                title.push_str(&parent.label);
            }
            results.push(SearchResult {
                index,
                score: score + target.context_bonus(context) + usage + preference,
                path: matched.path,
                title,
            });
        }

        results.sort_unstable_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| self.entries[a.index].id.cmp(&self.entries[b.index].id))
        });
        self.results = results;
        self.query = query.to_owned();
        self.selected = 0;
        self.reveal = true;
    }

    fn back(&mut self, ui: &mut Framework) -> bool {
        let Some(level) = self.levels.pop() else {
            return false;
        };
        let children = std::mem::replace(&mut self.entries, level.entries);
        self.entries[level.parent].children = Some(children);
        self.placeholder = level.placeholder;
        self.search(&level.query);
        self.selected = level.selected.min(self.results.len().saturating_sub(1));
        ui.set_input(QUERY, &self.query);
        ui.animate_until(std::time::Instant::now());
        true
    }

    fn enter(&mut self, index: usize, ui: &mut Framework) {
        let children = self.entries[index].children.take().unwrap();
        let placeholder = format!(
            "Search {}",
            self.entries[index]
                .label
                .to_lowercase()
                .trim_start_matches("select ")
        );
        let entries = std::mem::replace(&mut self.entries, children);
        self.levels.push(Level {
            parent: index,
            entries,
            query: self.query.clone(),
            selected: self.selected,
            placeholder: std::mem::replace(&mut self.placeholder, placeholder),
        });
        self.search("");
        ui.clear_input(QUERY);
        ui.animate_until(std::time::Instant::now());
    }

    fn activate(&mut self, index: usize, ui: &mut Framework) -> Option<Action> {
        let result = self.results.get(index)?;
        let entry = descendant(&self.entries[result.index], &result.path);
        if entry.children.is_some() {
            let index = result.index;
            let path = result.path.clone();
            self.enter(index, ui);
            for index in path {
                self.selected = self
                    .results
                    .iter()
                    .position(|result| result.index == index)
                    .unwrap_or(0);
                self.enter(index, ui);
            }
            return None;
        }
        self.clock += 1;
        let usage = self.usage.entry(entry.id.clone()).or_default();
        usage.count = usage.count.saturating_add(1);
        usage.last = self.clock;
        self.open = false;
        Some(entry.command.clone())
    }

    pub fn render(
        &mut self,
        ui: &mut Framework,
        gpu: &mut Renderer,
    ) -> anyhow::Result<Option<Action>> {
        if self.open {
            ui.begin_overlay()?;
        } else {
            ui.begin_passive_overlay()?;
        }

        let mut presence = ui.presence(&mut self.presence, self.open);
        ui.overlay_opacity(presence);
        if !self.levels.is_empty()
            && ui.events().iter().any(|event| {
                matches!(
                    event,
                    WindowEvent::KeyPressed {
                        key: Key::Left,
                        control: true
                    }
                )
            })
        {
            self.back(ui);
            ui.end_overlay()?;
            return Ok(None);
        }
        let width = (ui.div_width() - 32.0).clamp(1.0, 640.0);
        let count = self.results.len().max(1);
        let rows = (((ui.div_height() - 72.0) / ROW).floor().max(1.0) as usize)
            .min(8)
            .min(count);
        let empty = self.results.is_empty();
        let list_height = if empty {
            ui.push_style(Style {
                font_size: 13,
                ..ui.style()
            });
            let measured =
                ui.wrapped_text_size("No matching actions", None, (width - 24.0).max(1.0));
            ui.pop_style();
            measured?.1 + 16.0
        } else {
            rows as f32 * ROW
        };
        let bottom_inset = if empty {
            1.0
        } else {
            ui.corner_radii().popup.max(1.0)
        };
        let height = if empty {
            48.0
        } else {
            56.0
        } + list_height
            + bottom_inset;
        let x = ((ui.div_width() - width) * 0.5).max(0.0);
        let top = ((ui.div_height() - height) * 0.5).max(0.0);
        let pointer = ui.pointer_position();
        let outside = pointer.is_some_and(|p| {
            p.x < x as f64
                || p.x >= (x + width) as f64
                || p.y < top as f64
                || p.y >= (top + height) as f64
        });
        if ui.events().iter().any(|event| {
            matches!(
                event,
                WindowEvent::KeyPressed {
                    key: Key::Escape,
                    ..
                }
            ) || (outside
                && matches!(
                    event,
                    WindowEvent::MouseButtonPressed {
                        button: MouseButton::Left
                    }
                ))
        }) {
            let escape = ui.events().iter().any(|event| {
                matches!(
                    event,
                    WindowEvent::KeyPressed {
                        key: Key::Escape,
                        ..
                    }
                )
            });
            if !escape || !self.back(ui) {
                self.open = false;
                ui.ignore_overlay_input();
                presence = ui.presence(&mut self.presence, false);
                ui.overlay_opacity(presence);
            }
        }

        ui.set_position(x, top);
        ui.push_style(Style {
            padding: 0.0,
            background: Some(ui.theme().surface),
            border_color: Some(ui.theme().border),
            radius: ui.corner_radii().popup,
            rounded_corners: [true; 4],
            ..ui.style()
        });
        ui.div_begin_reveal(
            "quickshell",
            DivOptions {
                width: Some(width),
                height: Some(height),
                ..Default::default()
            },
            presence,
        )?;
        ui.push_style(Style {
            padding: 0.0,
            background: None,
            border_color: None,
            font_size: 12,
            color: ui.theme().muted,
            ..ui.style()
        });
        ui.push_style(Style {
            font_size: 16,
            padding: 12.0,
            color: ui.theme().text,
            background: Some(ui.theme().input),
            border_color: Some(ui.theme().border),
            radius: ui.corner_radii().popup,
            rounded_corners: [self.levels.is_empty(), true, false, false],
            ..ui.style()
        });
        let nested = !self.levels.is_empty();
        let mut go_back = false;
        if nested {
            ui.set_position(1.0, 1.0);
            ui.push_style(Style {
                font_size: 20,
                padding: 10.0,
                background: Some(ui.theme().surface),
                hover_color: Some(crate::ui::widgets::button::highlight(
                    ui.theme().button,
                    ui.theme().button_text,
                )),
                hover_text_color: Some(ui.theme().button_text),
                click_color: Some(ui.theme().primary),
                click_text_color: Some(ui.theme().primary_text),
                border_color: None,
                rounded_corners: [true, false, false, false],
                ..ui.style()
            });
            ui.push_sizing(Some(42.0), Some(46.0));
            go_back = ui.icon_button(gpu, "quickshell-back", crate::BACK)?;
            ui.pop_sizing();
            ui.pop_style();
            ui.set_position(44.0, 0.0);
            ui.push_sizing(Some((width - 44.0).max(1.0)), None);
        }
        if self.open {
            ui.focus_control(QUERY, false);
        }
        let input_origin = ui.position();
        let query = ui.input_with_options(
            gpu,
            QUERY,
            InputOptions {
                max_chars: Some(96),
                filter: None,
            },
        )?;
        if nested {
            ui.pop_sizing();
        }
        ui.pop_style();
        if query.is_empty() {
            let after_input = ui.position();
            ui.set_position(input_origin.0 + 14.0, input_origin.1 + 12.0);
            ui.push_style(Style {
                font_size: 16,
                color: ui.theme().muted,
                ..ui.style()
            });
            ui.text(gpu, &self.placeholder, None)?;
            ui.pop_style();
            ui.set_position(after_input.0, after_input.1);
        }

        if self.open && query != self.query {
            self.search(&query);
            ui.animate_until(std::time::Instant::now());
        }

        let mut execute = false;
        let mut mouse_moved = false;
        for event in ui.events() {
            match event {
                WindowEvent::KeyPressed {
                    key: Key::Enter, ..
                } => {
                    execute = true;
                    mouse_moved = false;
                }
                WindowEvent::KeyPressed { key, .. } => {
                    mouse_moved = false;
                    let count = self.results.len();
                    if count == 0 {
                        continue;
                    }

                    match key {
                        Key::Down => self.selected = (self.selected + 1) % count,
                        Key::Up => self.selected = (self.selected + count - 1) % count,
                        Key::PageDown => self.selected = (self.selected + rows).min(count - 1),
                        Key::PageUp => self.selected = self.selected.saturating_sub(rows),
                        _ => continue,
                    }

                    self.reveal = true;
                }
                WindowEvent::FocusNext { backwards } => {
                    mouse_moved = false;
                    let count = self.results.len();
                    if count > 0 {
                        self.selected = if *backwards {
                            (self.selected + count - 1) % count
                        } else {
                            (self.selected + 1) % count
                        };
                        self.reveal = true;
                    }
                }
                WindowEvent::MouseMoved { .. } => mouse_moved = true,
                WindowEvent::TextInput(_) => mouse_moved = false,
                _ => {}
            }
        }

        ui.space(if self.results.is_empty() {
            0.0
        } else {
            8.0
        });
        /* we keep rows and scrollbar tracks inside the panel border and bottom curve */
        let list_width = (ui.div_width() - 2.0).max(0.0);
        let list_y = ui.position().1;
        ui.set_position(1.0, list_y);
        ui.div_begin_id(
            "quickshell-results",
            DivOptions {
                width: Some(list_width),
                height: Some(list_height),
                scrollable: true,
            },
        )?;
        if std::mem::take(&mut self.reveal) {
            ui.reveal_row(self.selected, ROW);
        }

        let visible = ui.visible_rows(ROW, self.results.len());
        let end = visible.end;
        ui.space(visible.start as f32 * ROW);
        let mut clicked = None;
        for rank in visible {
            let result = &self.results[rank];
            let entry = descendant(&self.entries[result.index], &result.path);
            let response = ui.action_row(
                gpu,
                &format!("quickshell:{}", entry.id),
                ActionRow {
                    title: &result.title,
                    detail: &entry.detail,
                    icon: entry.icon(),
                    selected: rank == self.selected,
                },
            )?;
            if response.hovered && mouse_moved && self.selected != rank {
                self.selected = rank;
                ui.animate_until(std::time::Instant::now());
            }

            if response.clicked {
                clicked = Some(rank);
            }
        }

        if end < self.results.len() {
            ui.space((self.results.len() - end) as f32 * ROW);
        }

        if self.results.is_empty() {
            ui.set_position(12.0, 8.0);
            ui.push_style(Style {
                font_size: 13,
                color: ui.theme().muted,
                ..ui.style()
            });
            ui.push_sizing(Some((ui.div_width() - 24.0).max(1.0)), None);
            let result = ui.text(gpu, "No matching actions", None);
            ui.pop_sizing();
            ui.pop_style();
            result?;
            ui.space(8.0);
        }

        ui.div_end();
        ui.pop_style();
        ui.div_end();
        ui.pop_style();
        ui.end_overlay()?;
        Ok(if go_back {
            self.back(ui);
            None
        } else if let Some(rank) = clicked {
            self.activate(rank, ui)
        } else if execute {
            self.activate(self.selected, ui)
        } else {
            None
        })
    }
}
