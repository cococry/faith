use faith_client::Id;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

use crate::{
    ADD, Action, App, CHATS, GROUP, NEW_CHAT, REFRESH, Renderer, client, end_panel, icon, label,
    panel, row_button,
    ui::{
        framework::{DivOptions, Font, Framework, Style},
        widgets::input::InputOptions,
    },
};

pub const RENAME_INPUT: &str = "conversation-name";

pub enum Submission {
    Create,
    Send(String),
}

pub struct State {
    pub snapshot: client::Snapshot,
    pub submission: Option<(u64, Submission)>,
    pub partners: Vec<(Id, String)>,
    pub naming: bool,
    pub renaming: Option<Id>,
    pub require_username: bool,
    pub disclose_username: Option<bool>,
    pub request_disclosure: HashMap<Id, bool>,
    pub session: u64,
    pub footer_hover: RefCell<crate::ui::animation::Presence>,
    form: u64,
    history_layout: RefCell<Option<HistoryLayout>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            snapshot: client::Snapshot::default(),
            submission: None,
            partners: Vec::new(),
            naming: false,
            renaming: None,
            require_username: false,
            disclose_username: None,
            request_disclosure: HashMap::new(),
            session: 0,
            footer_hover: RefCell::default(),
            form: 0,
            history_layout: RefCell::new(None),
        }
    }
}

impl State {
    pub fn reset(&mut self) {
        *self = Self {
            session: self.session + 1,
            ..Self::default()
        };
    }

    pub fn invalidate_history_layout(&mut self) {
        self.history_layout.get_mut().take();
    }

    pub fn contact_capacity(&self) -> bool {
        self.partners.len() < faith_client::MAX_MEMBERS - 1
    }

    pub fn contact_selected(&self, id: Id) -> bool {
        self.partners.iter().any(|(partner, _)| *partner == id)
    }

    pub fn toggle_contact(&mut self, id: Id) {
        if let Some(index) = self.partners.iter().position(|(partner, _)| *partner == id) {
            self.partners.remove(index);
        } else if let Some(partner) = self.valid_partner(id, "") {
            self.partners.push(partner);
        }
    }

    pub fn creation_step(&self) -> (u64, bool) {
        (self.form, self.naming)
    }

    pub fn title_id(&self) -> String {
        format!("chat-title-{}-{}", self.session, self.form)
    }

    pub fn partner_id(&self) -> String {
        format!("chat-partner-id-{}-{}", self.session, self.form)
    }

    fn partner_name(&self) -> String {
        format!("chat-partner-name-{}-{}", self.session, self.form)
    }

    pub fn composer_id(&self, chat: Id) -> String {
        format!("chat-composer-{}-{chat}", self.session)
    }

    pub fn clear_form(&mut self, ui: &mut Framework) {
        ui.clear_input(&self.title_id());

        self.clear_partner_inputs(ui);
        self.partners.clear();
        self.naming = false;
        self.require_username = false;
        self.disclose_username = None;
        self.form += 1;
    }

    fn clear_partner_inputs(&self, ui: &mut Framework) {
        ui.clear_input(&self.partner_id());
        ui.clear_input(&self.partner_name());
    }

    pub fn editing_creation(&self) -> bool {
        !matches!(self.submission, Some((_, Submission::Create)))
    }

    pub fn discloses_username(&self) -> bool {
        self.require_username
            || self
                .disclose_username
                .unwrap_or(self.snapshot.auto_disclose)
    }

    pub fn shares_on_request(&self, id: Id) -> bool {
        self.request_disclosure
            .get(&id)
            .copied()
            .unwrap_or(self.snapshot.auto_disclose)
    }

    fn valid_partner(&self, id: Id, name: &str) -> Option<(Id, String)> {
        (self.contact_capacity()
            && self.snapshot.auth != Some(id)
            && !name.chars().any(char::is_control)
            && !self.partners.iter().any(|(existing, _)| *existing == id))
        .then(|| (id, name.trim().to_owned()))
    }

    pub fn add_partner(&mut self, ui: &mut Framework, partner: (Id, String)) {
        self.partners.push(partner);
        self.clear_partner_inputs(ui);
        ui.focus_input(&self.partner_id());
    }

    pub fn default_title(&self, name_for: impl Fn(&Id, &str) -> String) -> String {
        let mut title = self
            .partners
            .iter()
            .take(3)
            .map(|(id, name)| name_for(id, name))
            .collect::<Vec<_>>()
            .join(", ");
        if self.partners.len() > 3 {
            title.push_str(" & more");
        }

        title
    }
}

pub fn partner_label(id: &Id, name: &str) -> String {
    if name.is_empty() {
        format!("{}…", &id.to_string()[..4])
    } else {
        name.to_owned()
    }
}

fn input_style(ui: &Framework, enabled: bool) -> Style {
    Style {
        background: Some(ui.theme().input),
        border_color: Some(ui.theme().border),
        padding: 9.0,
        radius: ui.corner_radii().control,
        font_size: 14,
        enabled,
        ..ui.style()
    }
}

fn bottom_inset(ui: &Framework, app: &App, normal: f32) -> f32 {
    if ui.zoom_percent() != 100 {
        64.0
    } else if app.error.is_empty() {
        normal
    } else {
        54.0
    }
}

fn ready(app: &App) -> bool {
    app.chat.snapshot.authorized
        && !app.chat.snapshot.pending
        && !app.chat.snapshot.delivering
        && app.chat.submission.is_none()
}

fn compact_button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    text: &str,
    enabled: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        enabled,
        padding: 8.0,
        font_size: 13,
        radius: ui.corner_radii().control,
        background: Some(ui.theme().button),
        color: ui.theme().button_text,
        ..ui.style()
    });
    let clicked = ui.button(gpu, text);
    ui.pop_style();
    clicked
}

pub fn creation(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    let form = &app.chat;
    let naming = form.naming;
    let editing = form.editing_creation();
    let width = (ui.div_width() - 48.0).clamp(1.0, 490.0);
    let x = (ui.div_width() - width) * 0.5;
    let bottom = (ui.div_height() - bottom_inset(ui, app, 24.0)).max(0.0);
    let footer = (bottom - 42.0).max(26.0);
    let title_id = form.title_id();
    let mut action = Action::None;
    let mut title = String::new();
    let mut partner = None;
    let can_continue;

    ui.set_position(x, 14.0);
    ui.div_begin_id(
        if naming {
            "chat-creation-name"
        } else {
            "chat-creation-partners"
        },
        DivOptions {
            width: Some(width),
            height: Some((footer - 28.0).max(1.0)),
            scrollable: true,
        },
    )?;
    label(
        ui,
        gpu,
        if naming {
            "Name your conversation"
        } else {
            "New conversation"
        },
        25,
        true,
        false,
        false,
    )?;
    ui.space(24.0);

    if naming {
        let size = 76.0_f32.min(ui.div_width());
        ui.set_position((ui.div_width() - size) * 0.5, ui.position().1);
        ui.push_style(Style {
            background: Some(ui.theme().primary),
            radius: size * 0.5,
            ..ui.style()
        });
        ui.div_begin(DivOptions {
            width: Some(size),
            height: Some(size),
            ..Default::default()
        })?;
        ui.push_style(Style {
            font_size: 32,
            color: ui.theme().primary_text,
            ..ui.style()
        });
        let icon_height = ui.text_size(GROUP, Some(Font::Icons))?.1;
        ui.set_position(0.0, (size - icon_height) * 0.5);
        ui.text_centered(gpu, GROUP, Some(Font::Icons))?;
        ui.pop_style();
        ui.div_end();
        ui.pop_style();
        ui.space(24.0);

        title = form_input(
            ui,
            gpu,
            "Conversation name",
            &title_id,
            "Name your group",
            InputOptions {
                max_chars: Some(256),
                ..Default::default()
            },
            editing,
        )?;
        if editing && form.snapshot.username.is_some() {
            ui.space(12.0);
            ui.push_style(Style {
                font_size: 13,
                enabled: !form.require_username,
                ..ui.style()
            });
            let disclose = ui.compact_checkbox(
                gpu,
                "disclose-conversation-username",
                "Disclose username",
                form.discloses_username(),
            );
            ui.pop_style();
            if let Some(value) = disclose? {
                action = Action::DiscloseConversationUsername(value);
            }

            crate::disclosure_text(
                ui,
                gpu,
                "disclose-username-explanation",
                "Your username will be shared with everyone you invite.",
                form.discloses_username(),
            )?;
            ui.push_style(Style {
                font_size: 13,
                ..ui.style()
            });
            let require = ui.compact_checkbox(
                gpu,
                "require-conversation-username",
                "Require username disclosure",
                form.require_username,
            );
            ui.pop_style();
            if let Some(value) = require? {
                action = Action::RequireConversationUsername(value);
            }

            crate::disclosure_text(
                ui,
                gpu,
                "require-username-explanation",
                "Members will automatically share their username when entering.",
                form.require_username,
            )?;
        }

        can_continue = !title.trim().is_empty() && ready(app);
        if !editing {
            ui.space(12.0);
            label(ui, gpu, "Creating conversation…", 13, false, true, false)?;
        } else if !ready(app) {
            ui.space(12.0);
            label(ui, gpu, &form.snapshot.status, 13, false, true, false)?;
        }
    } else {
        let auth_id = form.partner_id();
        let name_id = form.partner_name();
        let auth = form_input(
            ui,
            gpu,
            "Auth ID",
            &auth_id,
            "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
            InputOptions {
                max_chars: Some(32),
                ..Default::default()
            },
            true,
        )?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            "Paste the 32-character Auth ID from your partner.",
            12,
            false,
            true,
            false,
        )?;

        ui.space(16.0);
        let name = form_input(
            ui,
            gpu,
            "Local contact name (optional)",
            &name_id,
            "Enter a local name",
            InputOptions {
                max_chars: Some(20),
                ..Default::default()
            },
            true,
        )?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            "A local name saves this Auth ID to Contacts when you create the conversation.",
            12,
            false,
            true,
            false,
        )?;

        let entered_auth = auth.trim();
        let parsed_auth = entered_auth.parse::<Id>().ok();
        partner = parsed_auth.and_then(|id| form.valid_partner(id, &name));
        let draft_empty = entered_auth.is_empty() && name.trim().is_empty();
        can_continue = partner.is_some() || draft_empty && !form.partners.is_empty();
        ui.space(12.0);
        let button_y = ui.position().1;
        let inline = ui.div_width() >= 300.0;
        if inline {
            ui.push_sizing(Some(118.0), None);
        }

        let add = command_button(
            ui,
            gpu,
            "chat-add-partner",
            ADD,
            "Add partner",
            partner.is_some(),
            false,
        )?;
        if inline {
            ui.pop_sizing();
        }

        if let Some((id, name)) = partner.as_ref()
            && (add || ui.input_submitted(&auth_id) || ui.input_submitted(&name_id))
        {
            action = Action::AddChatPartner(*id, name.clone());
        }

        if inline {
            ui.set_position(128.0, button_y);
        } else {
            ui.space(8.0);
        }

        if command_button(
            ui,
            gpu,
            "add-from-contacts",
            crate::contacts::ICON,
            "Add from contacts",
            editing,
            false,
        )? {
            action = Action::OpenContactPicker;
        }

        if !entered_auth.is_empty() && parsed_auth.is_none() {
            ui.space(8.0);
            label(
                ui,
                gpu,
                "Enter a valid 32-character hex Auth ID from a partner. Ask them to send them theirs for you to paste it here.",
                12,
                false,
                true,
                false,
            )?;
        }

        if !form.partners.is_empty() {
            ui.space(22.0);
            label(ui, gpu, "Partners", 13, true, false, false)?;
            ui.space(8.0);
            let mut x = 0.0;
            let mut y = ui.position().1;
            for (index, (id, name)) in form.partners.iter().enumerate() {
                let title = app.partner_label(id, name);
                ui.push_style(Style {
                    font_size: 13,
                    ..ui.style()
                });
                let width = (ui.text_size(&title, None)?.0 + 48.0).min(ui.div_width());
                ui.pop_style();
                if x > 0.0 && x + width > ui.div_width() {
                    x = 0.0;
                    y += 40.0;
                }

                ui.set_position(x, y);
                ui.push_sizing(Some(width), Some(32.0));
                if command_button(
                    ui,
                    gpu,
                    &format!("chat-remove-{id}"),
                    "\u{ea76}",
                    &title,
                    true,
                    false,
                )? {
                    action = Action::RemoveChatPartner(index);
                }

                ui.pop_sizing();
                x += width + 8.0;
            }
        }
    }

    ui.div_end();
    if !naming {
        ui.set_position(x, footer);
        if command_button(ui, gpu, "chat-cancel", "", "Cancel", editing, false)? {
            action = Action::CancelConversation;
        }
    }

    let button_width = (if naming {
        185.0_f32
    } else {
        112.0
    })
    .min(width);
    ui.set_position(x + width - button_width, footer);
    ui.push_sizing(Some(button_width), Some(42.0));
    let advance = command_button(
        ui,
        gpu,
        "chat-advance",
        if naming {
            NEW_CHAT
        } else {
            ""
        },
        if naming {
            "Create conversation"
        } else {
            "Continue"
        },
        can_continue,
        true,
    )?;
    ui.pop_sizing();
    if can_continue && (advance || naming && ui.input_submitted(&title_id)) {
        action = if naming {
            Action::CreateConversation {
                title: title.trim().to_owned(),
                partners: form.partners.clone(),
                policy: client::InvitationPolicy {
                    required: form.require_username,
                    disclose: form.snapshot.username.is_some() && form.discloses_username(),
                },
                automatic_title: title.trim() == app.default_conversation_title(),
            }
        } else {
            Action::NameNewConversation(partner)
        };
    }

    Ok(action)
}

pub fn form_input(
    ui: &mut Framework,
    gpu: &mut Renderer,
    title: &str,
    id: &str,
    hint: &str,
    options: InputOptions,
    enabled: bool,
) -> anyhow::Result<String> {
    label(ui, gpu, title, 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(Style {
        enabled,
        ..crate::profile_input_style(ui)
    });
    let result = ui.input_with_hint(gpu, id, options, hint);
    ui.pop_style();
    result
}

pub fn command_button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    title: &str,
    enabled: bool,
    primary: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        enabled,
        focus_when_disabled: matches!(id, "chat-add-partner" | "chat-advance"),
        focus_outline: matches!(id, "chat-add-partner" | "chat-advance" | "chat-cancel"),
        font_size: 13,
        icon_font_size: Some(15),
        padding: 10.0,
        radius: 20.0,
        center_button_content: true,
        background: Some(if primary && enabled {
            ui.theme().primary
        } else {
            ui.theme().button
        }),
        color: if !enabled {
            ui.theme().muted
        } else if primary {
            ui.theme().primary_text
        } else {
            ui.theme().button_text
        },
        ..ui.style()
    });
    let result = ui.icon_label_button(gpu, id, glyph, title);
    ui.pop_style();
    result
}

pub fn requests(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    let Some(profile) = &app.selected else {
        return Ok(Action::None);
    };
    let snapshot = &app.chat.snapshot;
    let width = (ui.div_width() - 48.0).clamp(1.0, 520.0);
    ui.set_position((ui.div_width() - width) * 0.5, 24.0);
    ui.div_begin_id(
        "incoming-message-requests",
        DivOptions {
            width: Some(width),
            height: Some((ui.div_height() - 74.0).max(1.0)),
            scrollable: true,
        },
    )?;
    label(ui, gpu, "Message requests", 25, true, false, false)?;
    ui.space(20.0);

    if let Some(notice) = snapshot.notices.last() {
        ui.div_begin_id(
            "request-notice",
            DivOptions {
                width: Some(ui.div_width()),
                ..Default::default()
            },
        )?;
        label(ui, gpu, notice, 12, false, true, false)?;
        ui.div_end();
        ui.space(12.0);
    }

    if snapshot.requests.is_empty() {
        crate::empty_state(
            ui,
            gpu,
            "requests-empty",
            crate::REQUESTS,
            "No pending message requests",
        )?;
    }

    let mut action = Action::None;
    let has_username = snapshot.username.is_some();
    let can_respond = snapshot.authorized && snapshot.responding.is_none();

    for request in &*snapshot.requests {
        let id = request.conversation;
        ui.push_style(Style {
            background: Some(ui.theme().surface),
            radius: ui.corner_radii().row,
            padding: 10.0,
            ..ui.style()
        });
        ui.div_begin_id(
            &format!("request-{id}"),
            DivOptions {
                width: Some(ui.div_width()),
                ..Default::default()
            },
        )?;
        let width = ui.div_width();
        let button_size = 32.0_f32.min(width * 0.25);
        let actions_x = (width - 2.0 * button_size - 6.0).max(0.0);
        ui.div_begin_id(
            "request-details",
            DivOptions {
                width: Some((actions_x - 8.0).max(1.0)),
                ..Default::default()
            },
        )?;
        let requester;
        let name = if request.username.is_empty() {
            requester = request.requester.to_string();
            &requester
        } else {
            &request.username
        };
        label(
            ui,
            gpu,
            app.contact_name(request.requester, name),
            14,
            true,
            false,
            false,
        )?;

        let mut share = has_username && app.chat.shares_on_request(id);
        if request.required {
            ui.space(3.0);
            let policy = if snapshot.reject_required {
                "Automatically declined by your profile"
            } else if has_username {
                "Accepting shares your username"
            } else {
                "Username required to accept"
            };
            label(ui, gpu, policy, 12, false, true, false)?;
        } else if has_username {
            ui.push_style(Style {
                font_size: 12,
                enabled: can_respond,
                ..ui.style()
            });
            let change = ui.compact_checkbox(
                gpu,
                &format!("request-disclosure-{id}"),
                "Share my username",
                share,
            );
            ui.pop_style();
            if let Some(value) = change? {
                share = value;
                action = Action::RequestDisclosure(id, value);
            }
        }

        ui.div_end();
        let y = ((ui.position().1 - button_size) * 0.5).max(0.0);
        let respond = |accept, disclose| Action::RespondToRequest {
            profile: profile.clone(),
            conversation: id,
            accept,
            disclose,
        };
        let can_accept =
            can_respond && (!request.required || has_username && !snapshot.reject_required);
        ui.set_position(actions_x, y);
        ui.push_sizing(Some(button_size), Some(button_size));
        ui.push_style(Style {
            enabled: can_accept,
            tooltip: Some(if request.required || share {
                "Accept and share your username"
            } else {
                "Accept privately"
            }),
            font_size: 16,
            radius: if ui.rounded_corners_enabled() {
                button_size * 0.5
            } else {
                0.0
            },
            background: Some(if can_accept {
                ui.theme().primary
            } else {
                ui.theme().button
            }),
            color: if can_accept {
                ui.theme().primary_text
            } else {
                ui.theme().muted
            },
            ..ui.style()
        });
        let accept = ui.icon_button(gpu, &format!("accept-request-{id}"), "\u{eab2}");
        ui.pop_style();
        if accept? {
            action = respond(true, share);
        }

        ui.set_position(actions_x + button_size + 6.0, y);
        ui.push_style(Style {
            enabled: can_respond,
            tooltip: Some("Reject request"),
            font_size: 16,
            radius: if ui.rounded_corners_enabled() {
                button_size * 0.5
            } else {
                0.0
            },
            background: Some(ui.theme().button),
            color: if can_respond {
                ui.theme().button_text
            } else {
                ui.theme().muted
            },
            hover_text_color: Some(ui.theme().error),
            ..ui.style()
        });
        let reject = ui.icon_button(gpu, &format!("reject-request-{id}"), "\u{ea76}");
        ui.pop_style();
        ui.pop_sizing();
        if reject? {
            action = respond(false, false);
        }

        end_panel(ui);
        ui.space(6.0);
    }

    ui.div_end();
    Ok(action)
}

pub fn pending_invites(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
) -> anyhow::Result<Action> {
    let snapshot = &app.chat.snapshot;
    let width = (ui.div_width() - 48.0).clamp(1.0, 560.0);
    ui.set_position((ui.div_width() - width) * 0.5, 24.0);
    ui.div_begin_id(
        "outgoing-pending-invites",
        DivOptions {
            width: Some(width),
            height: Some((ui.div_height() - 74.0).max(1.0)),
            scrollable: true,
        },
    )?;
    label(ui, gpu, "Pending invites", 25, true, false, false)?;
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Track who has responded to each conversation invite.",
        13,
        false,
        true,
        false,
    )?;
    ui.space(18.0);

    for (index, invite) in snapshot.pending_invites.iter().enumerate() {
        let id = format!("{}-{}", invite.conversation, invite.recipient);
        ui.push_style(Style {
            background: Some(ui.theme().surface),
            radius: ui.corner_radii().row,
            padding: 12.0,
            ..ui.style()
        });
        ui.div_begin_id(
            &format!("pending-invite-{id}"),
            DivOptions {
                width: Some(ui.div_width()),
                ..Default::default()
            },
        )?;
        let row_width = ui.div_width();
        ui.div_begin_id(
            &format!("pending-invite-details-{id}"),
            DivOptions {
                width: Some((row_width - 112.0).max(1.0)),
                ..Default::default()
            },
        )?;
        let title = invite.title_members.as_ref().map_or_else(
            || std::borrow::Cow::Borrowed(invite.title.as_str()),
            |members| app.member_title(members, None, &invite.title),
        );
        label(ui, gpu, &title, 14, true, false, false)?;
        ui.space(4.0);
        label(
            ui,
            gpu,
            &format!(
                "Invite to {}",
                app.contact_name(invite.recipient, &invite.recipient_name)
            ),
            12,
            false,
            true,
            false,
        )?;
        ui.div_end();

        let (status, color) = match invite.accepted {
            Some(true) => ("Accepted", ui.theme().success),
            Some(false) => ("Declined", ui.theme().error),
            None => ("Waiting", ui.theme().muted),
        };
        ui.set_position((row_width - 104.0).max(0.0), 0.0);
        ui.push_style(Style {
            font_size: 11,
            color,
            background: Some(ui.theme().input),
            radius: if ui.rounded_corners_enabled() {
                14.0
            } else {
                0.0
            },
            padding: 7.0,
            ..ui.style()
        });
        ui.push_sizing(Some(88.0), Some(28.0));
        ui.div_begin_id(
            &format!("pending-invite-status-{id}"),
            DivOptions {
                width: Some(88.0),
                height: Some(28.0),
                ..Default::default()
            },
        )?;
        ui.text_centered(gpu, status, None)?;
        ui.div_end();
        ui.pop_sizing();
        ui.pop_style();
        ui.div_end();
        ui.pop_style();
        if index + 1 < snapshot.pending_invites.len() {
            ui.space(8.0);
        }
    }

    let mut action = Action::None;
    if snapshot.pending
        && let Some(profile) = &app.selected
    {
        ui.space(20.0);
        let declined = snapshot
            .pending_invites
            .iter()
            .any(|invite| invite.accepted == Some(false));
        if compact_button(
            ui,
            gpu,
            "Retry invites",
            snapshot.authorized && !declined && !snapshot.delivering,
        )? {
            action = Action::RetryDelivery(profile.clone());
        }
        if compact_button(ui, gpu, "Cancel pending conversation", !snapshot.delivering)? {
            action = Action::DiscardDelivery(profile.clone());
        }
    }

    ui.div_end();
    Ok(action)
}

fn conversation_overview(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    snapshot: &client::Snapshot,
) -> anyhow::Result<Action> {
    let width = ui.div_width();
    ui.set_position(28.0, 24.0);
    ui.div_begin(DivOptions {
        width: Some((width - 56.0).max(1.0)),
        ..Default::default()
    })?;
    label(
        ui,
        gpu,
        app.selected
            .as_ref()
            .map_or("Conversations", |p| p.alias.as_str()),
        20,
        true,
        false,
        false,
    )?;

    if !snapshot.status.is_empty() && !snapshot.pending {
        ui.push_style(Style {
            font_size: 13,
            color: if snapshot.authorized {
                ui.theme().success
            } else {
                ui.theme().warning
            },
            ..ui.style()
        });

        ui.text(gpu, &snapshot.status, None)?;
        ui.pop_style();
    }

    if let Some(notice) = snapshot.notices.last() {
        label(ui, gpu, notice, 12, false, true, false)?;
    }

    let mut action = Action::None;

    ui.div_end();
    let header_bottom = ui.position().1;

    // Center the empty conversation view in its own column.
    let top = (ui.div_height() * 0.5 - 38.0)
        .max(200.0)
        .max(header_bottom + 16.0);
    ui.set_position(24.0, top);
    ui.div_begin(DivOptions {
        width: Some((width - 48.0).max(1.0)),
        ..Default::default()
    })?;

    ui.push_style(Style {
        font_size: 36,
        color: ui.theme().muted,
        ..ui.style()
    });

    ui.text_centered(gpu, CHATS, Some(Font::Icons))?;
    ui.pop_style();
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Open a conversation to get started.",
        14,
        false,
        true,
        true,
    )?;
    ui.div_end();

    if app.selected.is_some() && !snapshot.authorized {
        // Centered reconnect button with centered content.
        let button_width = (width - 56.0).min(220.0);
        panel(
            ui,
            (width - button_width) / 2.0,
            (ui.div_height() - 90.0)
                .max(280.0)
                .max(ui.position().1 + 16.0),
            button_width,
            52.0,
            false,
        )?;

        ui.push_style(Style {
            center_button_content: true,
            ..ui.style()
        });

        let reconnect = row_button(ui, gpu, "reconnect", REFRESH, "Reconnect", false, false);
        ui.pop_style();

        if reconnect? {
            action = Action::Retry;
        }

        end_panel(ui);
    }

    Ok(action)
}

pub fn conversation(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    show_clipboard: bool,
) -> anyhow::Result<(Action, Option<[f32; 2]>)> {
    let snapshot = &app.chat.snapshot;
    let Some(profile) = &app.selected else {
        return Ok((conversation_overview(ui, gpu, app, snapshot)?, None));
    };
    let Some(chat) = snapshot
        .conversations
        .iter()
        .find(|chat| Some(chat.id) == app.conversation)
    else {
        return Ok((conversation_overview(ui, gpu, app, snapshot)?, None));
    };
    let width = ui.div_width();
    let height = ui.div_height();
    let inner_width = (width - 40.0).max(1.0);
    let mut action = Action::None;
    ui.set_position(20.0, 12.0);
    ui.div_begin(DivOptions {
        width: Some(inner_width),
        ..Default::default()
    })?;
    let partner = chat.direct_partner();
    let contact = partner.filter(|&member| app.contacts.get(member).is_some());
    let contact_action = match partner {
        Some(member) if app.can_save_partner(member) => Some((
            "Save a contact",
            Action::PromptSaveConversationContact {
                profile: profile.clone(),
                id: chat.id,
                member,
            },
        )),
        None if chat
            .members
            .iter()
            .any(|&member| app.can_save_partner(member)) =>
        {
            Some(("Save a contact", Action::OpenConversationContacts))
        }
        _ => None,
    };
    let button_width = 108.0_f32.min(inner_width * 0.5);
    let (x, y) = ui.position();
    let title_width = if contact_action.is_some() {
        (inner_width - button_width - 8.0).max(1.0)
    } else {
        inner_width
    };
    ui.push_sizing(Some(title_width), None);
    let title = app.conversation_title(chat);
    if let Some(member) = contact {
        ui.push_style(Style {
            font_size: 20,
            ..ui.style()
        });
        let clicked = ui.text_link(gpu, "conversation-title-contact", &title, Font::Bold);
        ui.pop_style();
        if clicked? {
            action = Action::OpenContact(member);
        }
    } else {
        label(ui, gpu, &title, 20, true, false, false)?;
    }
    ui.pop_sizing();
    if let Some(at) = ui.context_press([x, y, title_width, ui.position().1 - y]) {
        action = Action::OpenRowMenu(
            crate::RowMenu::Conversation {
                profile: profile.clone(),
                id: chat.id,
            },
            at,
        );
    }

    if let Some((title, command)) = contact_action {
        ui.set_position(inner_width - button_width, y);
        ui.push_sizing(Some(button_width), Some(32.0));
        let save = command_button(ui, gpu, "conversation-contact", "", title, true, false);
        ui.pop_sizing();
        if save? {
            action = command;
        }
    }

    if !snapshot.status.is_empty()
        && (!snapshot.authorized || snapshot.pending || snapshot.delivering)
    {
        label(ui, gpu, &snapshot.status, 13, false, true, false)?;
    }

    if !snapshot.authorized
        && !snapshot.status.is_empty()
        && icon(ui, gpu, "reconnect", REFRESH, false, "Reconnect")?
    {
        action = Action::Retry;
    }

    ui.div_end();
    let header = ui.position().1 + 10.0;

    let bottom = (height - bottom_inset(ui, app, 18.0)).max(0.0);
    let send_width = 44.0_f32.min(inner_width);
    let input_width = (inner_width - send_width - 8.0).max(1.0);
    let composer_id = app.chat.composer_id(chat.id);
    let composer_style = Style {
        radius: 22.0,
        padding: 12.0,
        margin: 0.0,
        ..input_style(ui, app.chat.submission.is_none())
    };
    ui.push_style(composer_style);
    let composer_height = ui.composer_height(&composer_id, input_width, (bottom - header) * 0.45);
    ui.pop_style();
    let composer_height = composer_height?;
    let composer = (bottom - 3.0 - composer_height).max(header);
    let clipboard_position = show_clipboard.then_some([20.0, (composer - 24.0).max(0.0)]);
    let history_bottom = clipboard_position.map_or(composer, |position| position[1]);
    let history = (history_bottom - header - 9.0).max(0.0);
    if history > 0.0
        && let Some(member) = messages(ui, gpu, app, chat, [20.0, header, inner_width, history])?
    {
        action = if app.contacts.get(member).is_some() {
            Action::OpenContact(member)
        } else {
            Action::PromptSaveConversationContact {
                profile: profile.clone(),
                id: chat.id,
                member,
            }
        };
    }

    panel(ui, 20.0, composer, input_width, composer_height, false)?;
    ui.push_style(composer_style);
    ui.push_sizing(None, Some(composer_height));
    let body = ui.composer(gpu, &composer_id);
    ui.pop_sizing();
    ui.pop_style();
    end_panel(ui);
    let body = body?;
    ui.set_position(28.0 + input_width, composer + composer_height - 44.0);
    ui.push_sizing(Some(send_width), Some(44.0));
    let can_send = ready(app) && !body.trim().is_empty();
    ui.push_style(Style {
        font_size: 18,
        radius: 22.0,
        padding: 0.0,
        background: Some(ui.theme().primary),
        color: ui.theme().primary_text,
        enabled: can_send,
        ..ui.style()
    });
    let send = ui.icon_button(gpu, "chat-send", "")?;
    ui.pop_style();
    ui.pop_sizing();

    if can_send && (send || ui.input_submitted(&composer_id)) {
        action = Action::SendMessage { id: chat.id, body };
    }

    Ok((action, clipboard_position))
}

const MESSAGE_ACTION_SIZE: f32 = 24.0;
const MESSAGE_ACTION_GAP: f32 = 4.0;

struct Bubble {
    y: f32,
    height: f32,
    width: f32,
    actions: f32,
    view_contact: bool,
}

struct HistoryLayout {
    messages: Arc<[client::Message]>,
    limit: f32,
    scale: f32,
    extent: f32,
    bubbles: Vec<Bubble>,
}

impl HistoryLayout {
    fn measure(
        ui: &mut Framework,
        app: &App,
        chat: &client::Chat,
        limit: f32,
    ) -> anyhow::Result<Self> {
        let snapshot = &app.chat.snapshot;
        let padding = limit.min(20.0) * 0.5;
        let content = (limit - padding * 2.0).max(1.0);
        let mut extent = 0.0;
        let group = chat.direct_partner().is_none();
        let mut bubbles = Vec::with_capacity(snapshot.messages.len());
        for message in snapshot.messages.iter() {
            let author = author(app, message);
            ui.push_style(Style {
                font_size: 14,
                ..ui.style()
            });
            let body = ui.message_text_size(&message.body, content);
            ui.pop_style();
            let other = group && snapshot.auth.is_some_and(|auth| auth != message.sender);
            let view_contact = other && app.contacts.get(message.sender).is_some();
            let inset = if view_contact {
                2.0 * crate::ui::framework::TEXT_LINK_INSET
            } else {
                0.0
            };
            ui.push_style(Style {
                font_size: 12,
                ..ui.style()
            });
            let name = ui.wrapped_text_size(author, Some(Font::Bold), (content - inset).max(1.0));
            ui.pop_style();
            let (body, name) = (body?, name?);
            let width = (body.0.max(name.0 + inset).ceil() + 2.0 * padding + 1.0).min(limit);
            let height = body.1 + name.1 + 2.0 * padding;
            let actions = if other && !view_contact && chat.members.contains(&message.sender) {
                MESSAGE_ACTION_SIZE + MESSAGE_ACTION_GAP
            } else {
                0.0
            };
            bubbles.push(Bubble {
                y: extent,
                height,
                width,
                actions,
                view_contact,
            });
            extent += height + actions + 8.0;
        }

        Ok(Self {
            messages: snapshot.messages.clone(),
            limit,
            scale: ui.layout_scale(),
            extent,
            bubbles,
        })
    }
}

fn author<'a>(app: &'a App, message: &'a client::Message) -> &'a str {
    if app.chat.snapshot.auth == Some(message.sender) {
        "You"
    } else {
        app.contact_name(message.sender, &message.author)
    }
}

fn messages(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    chat: &client::Chat,
    [x, y, width, height]: [f32; 4],
) -> anyhow::Result<Option<Id>> {
    let snapshot = &app.chat.snapshot;
    let mut sender = None;
    ui.set_position(x, y);
    ui.div_begin_id(
        &app.chat.composer_id(chat.id),
        DivOptions {
            width: Some(width),
            height: Some(height),
            scrollable: true,
        },
    )?;
    ui.follow_scroll_end();
    ui.message_selection(&app.chat.composer_id(chat.id), |row| {
        (snapshot.selected == Some(chat.id))
            .then(|| {
                snapshot
                    .messages
                    .iter()
                    .find(|message| message.row_id == row)
            })
            .flatten()
            .map(|message| message.body.as_str())
    })?;

    if snapshot.selected == Some(chat.id) {
        let available = (ui.div_width() - 12.0).max(1.0);
        let limit = (available * 0.77).clamp(1.0, 520.0);
        if snapshot.messages.is_empty() {
            label(ui, gpu, "No messages yet", 13, false, true, true)?;
        }

        let mut cache = app.chat.history_layout.borrow_mut();
        if cache.as_ref().is_none_or(|layout| {
            layout.limit != limit
                || layout.scale != ui.layout_scale()
                || !Arc::ptr_eq(&layout.messages, &snapshot.messages)
        }) {
            *cache = Some(HistoryLayout::measure(ui, app, chat, limit)?);
        }

        let layout = cache.as_ref().unwrap();
        let body_wrap_width = (limit - limit.min(20.0)).max(1.0);
        let top = ui.scroll_offset()[1];
        let start = layout
            .bubbles
            .partition_point(|bubble| bubble.y + bubble.height + bubble.actions < top);
        let end = layout
            .bubbles
            .partition_point(|bubble| bubble.y <= top + height);
        ui.space(layout.extent);
        for (message, bubble) in snapshot.messages[start..end]
            .iter()
            .zip(&layout.bubbles[start..end])
        {
            let mine = snapshot.auth == Some(message.sender);
            ui.set_position(
                if mine {
                    available - bubble.width
                } else {
                    0.0
                },
                bubble.y,
            );
            let row_id = message.row_id.to_string();
            ui.push_style(Style {
                padding: limit.min(20.0) * 0.5,
                radius: ui.corner_radii().row,
                background: Some(if mine {
                    ui.theme().primary
                } else {
                    ui.theme().surface
                }),
                ..ui.style()
            });
            ui.div_begin_id(
                &row_id,
                DivOptions {
                    width: Some(bubble.width),
                    height: Some(bubble.height),
                    ..Default::default()
                },
            )?;
            ui.push_style(Style {
                padding: 0.0,
                background: None,
                ..ui.style()
            });
            if mine {
                ui.push_style(Style {
                    font_size: 12,
                    color: ui.theme().primary_text,
                    ..ui.style()
                });
                ui.text(gpu, author(app, message), Some(Font::Bold))?;
                ui.pop_style();
                ui.push_style(Style {
                    font_size: 14,
                    color: ui.theme().primary_text,
                    ..ui.style()
                });
                ui.message_text(gpu, message.row_id, &message.body, body_wrap_width)?;
                ui.pop_style();
            } else {
                let name = author(app, message);
                if bubble.view_contact {
                    ui.push_style(Style {
                        font_size: 12,
                        color: ui.theme().muted,
                        ..ui.style()
                    });
                    let clicked =
                        ui.text_link(gpu, &format!("message-author-{row_id}"), name, Font::Bold);
                    ui.pop_style();
                    if clicked? {
                        sender = Some(message.sender);
                    }
                } else {
                    label(ui, gpu, name, 12, true, true, false)?;
                }

                ui.push_style(Style {
                    font_size: 14,
                    ..ui.style()
                });
                ui.message_text(gpu, message.row_id, &message.body, body_wrap_width)?;
                ui.pop_style();
            }

            ui.pop_style();
            ui.div_end();
            ui.pop_style();

            if bubble.actions > 0.0
                && ui.region_hovered([
                    0.0,
                    bubble.y,
                    bubble.width.max(MESSAGE_ACTION_SIZE).min(available),
                    bubble.height + bubble.actions,
                ])
            {
                ui.set_position(0.0, bubble.y + bubble.height + MESSAGE_ACTION_GAP);
                ui.push_style(Style {
                    tooltip: Some("Save a contact"),
                    font_size: 14,
                    radius: MESSAGE_ACTION_SIZE * 0.5,
                    color: ui.theme().text,
                    background: Some(ui.theme().button),
                    ..ui.style()
                });
                ui.push_sizing(
                    Some(MESSAGE_ACTION_SIZE.min(available)),
                    Some(MESSAGE_ACTION_SIZE),
                );
                let clicked = ui.icon_button(
                    gpu,
                    &format!("message-contact-{row_id}"),
                    crate::contacts::ICON,
                );
                ui.pop_sizing();
                ui.pop_style();

                if clicked? {
                    sender = Some(message.sender);
                }
            }
        }
    }

    ui.div_end();
    Ok(sender)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_picker_toggles_recipients_and_keeps_membership_valid() {
        let mut state = State::default();
        let own = Id([0; 16]);
        state.snapshot.auth = Some(own);
        state.toggle_contact(own);
        assert!(state.partners.is_empty());

        for i in 1..faith_client::MAX_MEMBERS {
            state.toggle_contact(Id([i as u8; 16]));
        }

        let first = Id([1; 16]);
        let extra = Id([250; 16]);
        assert!(state.contact_selected(first));
        assert!(!state.contact_capacity());
        state.toggle_contact(extra);
        assert!(!state.contact_selected(extra));

        state.toggle_contact(first);
        assert!(!state.contact_selected(first));
        state.toggle_contact(extra);
        assert!(state.contact_selected(extra));
        assert!(!state.contact_capacity());
    }

    #[test]
    fn group_title_uses_three_partners_and_shortens_unnamed_ids() {
        let state = State {
            partners: vec![
                (Id([1; 16]), "Alice".into()),
                (Id([2; 16]), String::new()),
                (Id([3; 16]), "Caren".into()),
                (Id([4; 16]), "Dana".into()),
            ],
            ..State::default()
        };

        assert_eq!(
            state.default_title(partner_label),
            "Alice, 0202…, Caren & more"
        );
    }
}
