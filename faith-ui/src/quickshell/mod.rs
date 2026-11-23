pub mod catalog;
use crate::ui::search as rank;

use std::collections::HashMap;

use crate::{
    Action, App, Profile,
    graphics::Renderer,
    platform::{WindowEvent, event::Key, mouse::MouseButton},
    ui::{
        framework::{ActionRow, DivOptions, Framework, Style},
        widgets::input::InputOptions,
    },
};
use catalog::{Context, Entry, Kind};

const QUERY: &str = "quickshell-query";
const NAME: &str = "quickshell-name";
const ROW: f32 = 56.0;

struct SearchResult {
    index: usize,
    match_score: i32,
    implicit_delete: bool,
    destination: u8,
    score: i32,
    path: Vec<usize>,
    title: String,
}

struct Match<'a> {
    entry: &'a Entry,
    score: i32,
    path: Vec<usize>,
}

fn destination_rank(entry: &Entry, context: &Context) -> u8 {
    match &entry.command {
        Action::OpenConversation { .. } => 2 + u8::from(entry.owner == context.profile),
        Action::OpenProfile(_) | Action::OpenContact(_) => 0,
        _ if entry.id == "profile-conversations" => 2,
        _ => 1,
    }
}

fn is_delete_result(entry: &Entry) -> bool {
    matches!(
        entry.command,
        Action::ConfirmDeleteProfile(_)
            | Action::DeleteProfile(_)
            | Action::ConfirmDeleteContact(_)
            | Action::ConfirmDeleteContactInList(_)
            | Action::DeleteContact(_)
    ) || matches!(entry.id.as_str(), "delete-profile" | "delete-contact")
}

fn find<'a>(
    entry: &'a Entry,
    tokens: &[Vec<char>],
    inherited: &[Option<i32>],
    context: &Context,
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
            if let Some(mut candidate) = find(child, tokens, &scores, context) {
                let better = best.as_ref().is_none_or(|best| {
                    candidate.score > best.score
                        || (candidate.score == best.score
                            && !best.path.is_empty()
                            && destination_rank(best.entry, context)
                                .cmp(&destination_rank(candidate.entry, context))
                                .then_with(|| {
                                    (&candidate.entry.label, &candidate.entry.id)
                                        .cmp(&(&best.entry.label, &best.entry.id))
                                })
                                .is_lt())
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
    contact_picker: bool,
    loading_fonts: bool,
    save_contact_target: Option<(Profile, faith_client::Id, faith_client::Id)>,
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
        self.contact_picker = false;
        self.loading_fonts = false;
        self.save_contact_target = None;
        self.levels.clear();
        self.placeholder = "Run...".into();
        self.entries = catalog::build(app, zoom);
        self.context = Some(Context::from_app(app));
        self.query.clear();
        self.selected = 0;
        self.reveal = true;
        self.open = true;
        self.search("");
        ui.clear_input(QUERY);
    }

    pub fn open_font_picker(&mut self, app: &App, ui: &mut Framework) {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches!(entry.command, Action::OpenFontPicker))
        else {
            return;
        };

        self.loading_fonts = app.system_fonts.loading();
        self.entries[index].children = Some(if self.loading_fonts {
            Vec::new()
        } else {
            catalog::font_choices(app)
        });
        self.open = true;
        self.enter(index, ui);
    }

    pub fn finish_font_picker(&mut self, app: &App) {
        if self.open && self.loading_fonts {
            self.entries = catalog::font_choices(app);
            self.loading_fonts = false;
            let query = self.query.clone();
            self.search(&query);
        }
    }

    pub fn open_contact_picker(&mut self, app: &App, ui: &mut Framework) {
        let Some(picker) = catalog::contact_picker(app) else {
            return;
        };

        self.open_collection(app, ui, picker);
    }

    pub fn open_conversation_contacts(&mut self, app: &App, ui: &mut Framework) {
        if let Some(collection) = catalog::conversation_contacts(app) {
            self.open_collection(app, ui, collection);
        }
    }

    fn open_collection(&mut self, app: &App, ui: &mut Framework, collection: Entry) {
        self.contact_picker = collection.id == "add-contact-to-conversation";
        self.loading_fonts = false;
        self.save_contact_target = None;
        self.levels.clear();
        self.placeholder = collection.label;
        self.entries = collection.children.unwrap_or_default();
        self.context = Some(Context::from_app(app));
        self.query.clear();
        self.selected = 0;
        self.reveal = true;
        self.open = true;
        self.search("");
        ui.clear_input(QUERY);
    }

    pub fn prompt_save_contact(
        &mut self,
        ui: &mut Framework,
        profile: Profile,
        id: faith_client::Id,
        member: faith_client::Id,
        username: Option<&str>,
    ) {
        self.levels.clear();
        self.open = true;
        self.contact_picker = false;
        self.save_contact_target = Some((profile, id, member));
        ui.clear_input(NAME);
        if let Some(username) = username {
            ui.set_input(NAME, username);
        }

        ui.animate_until(std::time::Instant::now());
    }

    fn search(&mut self, query: &str) {
        let (scope, text) = query
            .split_once(':')
            .map_or((None, query), |(prefix, text)| {
                let scope = match prefix.trim().to_lowercase().as_str() {
                    "profile" | "profiles" | "account" => Some(Kind::Profile),
                    "chat" | "conversation" => Some(Kind::Conversation),
                    "contact" | "contacts" => Some(Kind::Contact),
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
        let requests_deletion = words.iter().any(|word| {
            word.len() >= 3
                && ["delete", "remove", "erase"]
                    .iter()
                    .any(|verb| verb.starts_with(word))
        });
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
            } else if let Some(matched) = find(entry, &tokens, &[], context) {
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
                target.priority.min(200) / 5
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
                match_score: matched.score + preference,
                implicit_delete: !requests_deletion && is_delete_result(target),
                destination: destination_rank(target, context),
                score: score + target.context_bonus(context) + usage,
                path: matched.path,
                title,
            });
        }

        results.sort_unstable_by(|a, b| {
            a.implicit_delete
                .cmp(&b.implicit_delete)
                .then_with(|| b.match_score.cmp(&a.match_score))
                .then_with(|| b.destination.cmp(&a.destination))
                .then_with(|| b.score.cmp(&a.score))
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
        if matches!(self.entries[level.parent].command, Action::OpenFontPicker) {
            self.loading_fonts = false;
        } else {
            self.entries[level.parent].children = Some(children);
        }

        self.placeholder = level.placeholder;
        self.contact_picker = false;
        self.search(&level.query);
        self.selected = level.selected.min(self.results.len().saturating_sub(1));
        ui.set_input(QUERY, &self.query);
        ui.animate_until(std::time::Instant::now());
        true
    }

    fn enter(&mut self, index: usize, ui: &mut Framework) {
        self.contact_picker = self.entries[index].id == "add-contact-to-conversation";
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
        if matches!(entry.command, Action::ToggleContactPartner(_)) {
            return Some(entry.command.clone());
        }

        self.clock += 1;
        let usage = self.usage.entry(entry.id.clone()).or_default();
        usage.count = usage.count.saturating_add(1);
        usage.last = self.clock;
        self.open = false;
        Some(entry.command.clone())
    }

    fn render_save_contact(
        &mut self,
        ui: &mut Framework,
        gpu: &mut Renderer,
        [x, y, width, height]: [f32; 4],
    ) -> anyhow::Result<Option<Action>> {
        let Some((profile, id, member)) = &self.save_contact_target else {
            return Ok(None);
        };
        ui.set_position(x, y);
        ui.push_style(Style {
            padding: 16.0,
            background: Some(ui.theme().surface),
            border_color: Some(ui.theme().border),
            radius: ui.corner_radii().popup,
            ..ui.style()
        });
        ui.div_begin_id(
            "name-popup",
            DivOptions {
                width: Some(width),
                height: Some(height),
                ..Default::default()
            },
        )?;
        crate::label(ui, gpu, "Save a contact", 15, true, false, false)?;
        ui.space(12.0);

        let width = ui.div_width();
        let done_width = width.min(200.0) * 0.35;
        let row_y = ui.position().1;
        ui.push_style(Style {
            font_size: 14,
            padding: 9.0,
            color: ui.theme().text,
            background: Some(ui.theme().input),
            border_color: Some(ui.theme().border),
            radius: ui.corner_radii().control,
            enabled: self.open,
            ..ui.style()
        });
        ui.push_sizing(Some((width - done_width - 8.0).max(1.0)), Some(40.0));
        if self.open {
            ui.focus_control(NAME, false);
        }

        let name = ui.input_with_hint(
            gpu,
            NAME,
            InputOptions {
                max_chars: Some(crate::contacts::NAME_MAX),
                filter: None,
            },
            "Name",
        )?;
        ui.pop_sizing();
        ui.pop_style();

        let name = name.trim();
        let enabled = self.open && !name.is_empty();
        ui.set_position(width - done_width, row_y);
        ui.push_style(Style {
            font_size: 13,
            color: if enabled {
                ui.theme().primary_text
            } else {
                ui.theme().muted
            },
            background: Some(if enabled {
                ui.theme().primary
            } else {
                ui.theme().button
            }),
            radius: ui.corner_radii().control,
            center_button_content: true,
            enabled,
            ..ui.style()
        });
        ui.push_sizing(Some(done_width), Some(40.0));
        let done = ui.icon_label_button(gpu, "quickshell-save-contact", "", "Save")?;
        ui.pop_sizing();
        ui.pop_style();
        ui.div_end();
        ui.pop_style();

        if enabled && (done || ui.input_submitted(NAME)) {
            self.open = false;
            return Ok(Some(Action::SaveConversationContact {
                profile: profile.clone(),
                id: *id,
                member: *member,
                name: name.to_owned(),
            }));
        }

        Ok(None)
    }

    pub fn render(
        &mut self,
        ui: &mut Framework,
        gpu: &mut Renderer,
        app: &App,
    ) -> anyhow::Result<Option<Action>> {
        if self.open {
            ui.begin_overlay()?;
        } else {
            ui.begin_passive_overlay()?;
        }

        let mut presence = ui.presence(&mut self.presence, self.open);
        ui.overlay_opacity(presence);
        let naming = self.save_contact_target.is_some();

        if !naming
            && !self.levels.is_empty()
            && !ui.input_focused()
            && ui.events().iter().any(|event| {
                matches!(
                    event,
                    WindowEvent::KeyPressed {
                        key: Key::Left,
                        control: true,
                        shift: false,
                    }
                )
            })
        {
            self.back(ui);
            ui.end_overlay()?;
            return Ok(None);
        }
        let width = (ui.div_width() - 32.0).clamp(
            1.0,
            if naming {
                400.0
            } else {
                640.0
            },
        );
        let count = self.results.len().max(1);
        let rows = (((ui.div_height() - 72.0) / ROW).floor().max(1.0) as usize)
            .min(8)
            .min(count);
        let empty = self.results.is_empty();
        let empty_label = if self.loading_fonts {
            "Finding system fonts…"
        } else {
            "No matching actions"
        };

        let list_height = if naming {
            0.0
        } else if empty {
            ui.push_style(Style {
                font_size: 13,
                ..ui.style()
            });
            let measured = ui.wrapped_text_size(empty_label, None, (width - 24.0).max(1.0));
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
        let height = if naming {
            108.0
        } else {
            (if empty {
                48.0
            } else {
                56.0
            }) + list_height
                + bottom_inset
        };
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
                        button: MouseButton::Left,
                        ..
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
            if !escape || self.contact_picker || !self.back(ui) {
                self.open = false;
                ui.ignore_overlay_input();
                presence = ui.presence(&mut self.presence, false);
                ui.overlay_opacity(presence);
            }
        }

        if naming {
            let action = self.render_save_contact(ui, gpu, [x, top, width, height])?;
            ui.end_overlay()?;
            return Ok(action);
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
                tooltip: Some("Back"),
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
        let query = ui.input_with_hint(
            gpu,
            QUERY,
            InputOptions {
                max_chars: Some(96),
                filter: None,
            },
            &self.placeholder,
        )?;
        if nested {
            ui.pop_sizing();
        }

        ui.pop_style();

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
            let checked = match entry.command {
                Action::ToggleContactPartner(id) => Some(app.chat.contact_selected(id)),
                _ => None,
            };
            let detail = if checked == Some(false) && !app.chat.contact_capacity() {
                "Conversation is full"
            } else {
                &entry.detail
            };
            let response = ui.action_row_with_checkbox(
                gpu,
                &format!("quickshell:{}", entry.id),
                ActionRow {
                    title: &result.title,
                    detail,
                    icon: entry.icon(),
                    selected: rank == self.selected,
                    badge: None,
                },
                checked,
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
            let result = ui.text(gpu, empty_label, None);
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

#[cfg(test)]
mod ranking_tests {
    use super::*;

    #[test]
    fn profile_deletion_ranks_below_navigation_for_a_name_only() {
        let profile = Profile {
            alias: "luca".into(),
            path: "luca".into(),
        };
        let id = faith_client::Id([1; 16]);
        let mut conversations = Entry::conversation(&profile, id, "Open conversation");
        conversations.id = "profile-conversations".into();
        conversations.command = Action::None;
        conversations.children = Some(vec![Entry::conversation(&profile, id, "Simon")]);

        let mut delete_profile = Entry::profile(&profile);
        delete_profile.id = "delete-profile:luca".into();
        delete_profile.label = profile.alias.clone();
        delete_profile.command = Action::ConfirmDeleteProfile(profile.clone());
        let mut delete_profiles = Entry::profile(&profile);
        delete_profiles.id = "delete-profile".into();
        delete_profiles.label = "Delete a profile".into();
        delete_profiles.primary = rank::Text::new("delete profile");
        delete_profiles.terms.clear();
        delete_profiles.command = Action::None;
        delete_profiles.children = Some(vec![delete_profile]);

        let mut shell = Shell {
            entries: vec![delete_profiles, Entry::profile(&profile), conversations],
            context: Some(Context {
                kind: Kind::Profile,
                profile: Some(profile.path.clone()),
            }),
            ..Default::default()
        };

        shell.search("luca");
        assert_eq!(
            shell
                .results
                .iter()
                .map(|result| result.index)
                .collect::<Vec<_>>(),
            [1, 2, 0]
        );
        assert_eq!(shell.results[2].path, [0]);

        shell.search("delete luca");
        assert_eq!(shell.results[0].index, 0);
        assert_eq!(shell.results[0].path, [0]);

        shell.search("delete profile");
        assert_eq!(shell.results[0].index, 0);
        assert!(shell.results[0].path.is_empty());
    }

    #[test]
    fn conversation_title_beats_a_profile_with_the_same_name() {
        let profile = |alias: &str| Profile {
            alias: alias.into(),
            path: alias.into(),
        };
        let active = profile("hello123");
        let other = profile("other");
        let id = faith_client::Id([1; 16]);
        let active_chat = Entry::conversation(&active, id, "hello123");
        let other_chat = Entry::conversation(&other, id, "hello123");
        let mut parent = Entry::conversation(&other, id, "Open conversation");
        parent.id = "profile-conversations".into();
        parent.command = Action::None;
        parent.children = Some(vec![other_chat, active_chat]);

        let mut shell = Shell {
            entries: vec![Entry::profile(&active), parent],
            context: Some(Context {
                kind: Kind::Profile,
                profile: Some(active.path.clone()),
            }),
            ..Default::default()
        };
        shell.search("hello123");
        assert_eq!(shell.results[0].index, 1);
        assert_eq!(shell.results[0].path, [1]);

        shell.context.as_mut().unwrap().profile = Some(other.path.clone());
        shell.search("hello123");
        assert_eq!(shell.results[0].index, 1);
        assert_eq!(shell.results[0].path, [0]);

        shell.search("open conversation");
        assert!(shell.results[0].path.is_empty());
    }
}
