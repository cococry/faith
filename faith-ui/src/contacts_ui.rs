use faith_client::Id;

use crate::{
    ACCOUNTS, ADD, Action, App, Font, Framework, GROUP, Profile, Renderer, Style, TitleEdit,
    chat_ui::{command_button, form_input},
    contacts::{DELETE, EDIT, NAME_MAX},
    header_icon, inline_title_editor, label, list_header, row_button,
    ui::{
        framework::{ActionRow, DivOptions},
        widgets::input::InputOptions,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    View,
    Add,
    Edit,
    Delete,
}

pub struct ConversationLink {
    pub key: String,
    pub profile: Profile,
    pub id: Id,
    pub title: String,
    pub detail: String,
}

pub fn list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    height: f32,
    selected: Option<Id>,
) -> anyhow::Result<Action> {
    let (mut action, top) = list_header(ui, gpu, app, "Contacts")?;
    let width = (ui.div_width() - 32.0).max(1.0);
    ui.set_position(16.0, top);
    ui.div_begin_id(
        "contacts-list",
        DivOptions {
            width: Some(width),
            height: Some((height - top - 70.0).max(1.0)),
            scrollable: true,
        },
    )?;
    let entries = &app.contacts.entries;
    if entries.is_empty() {
        ui.set_position(0.0, 14.0);
        label(ui, gpu, "No contacts.", 14, false, true, true)?;
    }

    let visible = ui.visible_rows(58.0, entries.len());
    let end = visible.end;
    ui.space(visible.start as f32 * 58.0);
    for index in visible {
        let contact = &entries[index];
        let short_auth;
        let auth = if width < 305.0 {
            short_auth = format!("{}…{}", &contact.auth[..6], &contact.auth[26..]);
            &short_auth
        } else {
            &contact.auth
        };
        let (x, y) = ui.position();
        let confirming = app.contact_delete_in_list == Some(contact.id);
        ui.push_style(Style {
            enabled: !confirming,
            ..ui.style()
        });
        let row = ui.action_row_with_space(
            gpu,
            &contact.auth,
            ActionRow {
                title: &contact.display,
                detail: auth,
                icon: ACCOUNTS,
                selected: selected == Some(contact.id),
                badge: None,
            },
            if confirming {
                72.0
            } else {
                0.0
            },
        );
        ui.pop_style();
        let clicked = row?.clicked;
        if confirming {
            ui.set_position((width - 76.0).max(0.0), y + 12.0);
            let delete = header_icon(
                ui,
                gpu,
                "delete-contact-row",
                DELETE,
                "Confirm delete contact",
                32.0,
            )?;
            ui.set_position((width - 36.0).max(0.0), y + 12.0);
            let cancel = header_icon(
                ui,
                gpu,
                "cancel-contact-row-delete",
                "\u{ea76}",
                "Cancel deletion",
                32.0,
            )?;
            if cancel {
                action = Action::CancelContactListDelete;
            } else if delete {
                action = Action::DeleteContact(contact.id);
            }
        } else if let Some(at) = ui.context_press([x, y, ui.div_width(), 56.0]) {
            action = Action::OpenRowMenu(crate::RowMenu::Contact(contact.id), at);
        } else if clicked {
            action = Action::OpenContact(contact.id);
        }
    }

    ui.space((entries.len() - end) as f32 * 58.0);
    ui.div_end();

    ui.set_position(16.0, (height - 70.0).max(top));
    if row_button(ui, gpu, "add-contact", ADD, "Add contact", false, false)? {
        action = Action::AddContact;
    }

    Ok(action)
}

pub fn page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    selected: Option<Id>,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    let contact = selected.and_then(|id| app.contacts.get(id));
    let empty =
        contact.is_none() && app.contact_mode == Mode::View && app.contacts.entries.is_empty();
    let (inset, max_width, top, bottom) = if empty {
        (48.0, 520.0, 24.0, 74.0)
    } else if app.contact_mode == Mode::Add {
        (48.0, 490.0, 14.0, 38.0)
    } else {
        (40.0, 620.0, 20.0, 38.0)
    };
    let width = (ui.div_width() - inset).clamp(1.0, max_width);
    let height = (ui.div_height() - bottom).max(1.0);
    ui.set_position((ui.div_width() - width) * 0.5, top);
    ui.div_begin_id(
        contact.map_or("contact-home", |contact| contact.auth.as_str()),
        DivOptions {
            width: Some(width),
            height: Some(height),
            scrollable: true,
        },
    )?;

    if let Some(contact) = contact {
        let header_y = ui.position().1;
        ui.push_style(Style {
            font_size: 52,
            ..ui.style()
        });
        ui.text(gpu, ACCOUNTS, Some(Font::Icons))?;
        ui.pop_style();

        let buttons = match app.contact_mode {
            Mode::View => Some([
                (
                    "edit-contact",
                    EDIT,
                    "Edit local name",
                    Action::EditContact(contact.id),
                ),
                (
                    "delete-contact",
                    DELETE,
                    "Delete contact",
                    Action::ConfirmDeleteContact(contact.id),
                ),
            ]),
            Mode::Delete => Some([
                (
                    "confirm-delete-contact",
                    DELETE,
                    "Confirm delete contact",
                    Action::DeleteContact(contact.id),
                ),
                (
                    "cancel-contact-delete",
                    "\u{ea76}",
                    "Cancel deletion",
                    Action::CancelContactEdit,
                ),
            ]),
            _ => None,
        };
        if let Some(buttons) = buttons {
            let width = ui.div_width();
            for (offset, (id, glyph, tooltip, command)) in [88.0, 40.0].into_iter().zip(buttons) {
                ui.set_position((width - offset).max(0.0), header_y);
                if header_icon(ui, gpu, id, glyph, tooltip, 40.0)? {
                    action = command;
                }
            }
        }

        ui.space(10.0);
        if app.contact_mode == Mode::Edit {
            action = match inline_title_editor(
                ui,
                gpu,
                "contact-name",
                "Nickname",
                (Font::Bold, 26),
                NAME_MAX,
                true,
            )? {
                TitleEdit::Cancel => Action::CancelContactEdit,
                TitleEdit::Submit(name) => Action::SaveContact(contact.id, name),
                TitleEdit::None => Action::None,
            };
        } else {
            label(ui, gpu, &contact.display, 26, true, false, false)?;
        }

        ui.space(6.0);
        label(ui, gpu, &contact.auth, 14, false, true, false)?;
        ui.space(12.0);
    } else if app.contact_mode == Mode::Add {
        label(ui, gpu, "Add contact", 26, true, false, false)?;
        ui.space(22.0);
    } else if empty {
        label(ui, gpu, "Contacts", 25, true, false, false)?;
        ui.space(20.0);
        crate::empty_state(
            ui,
            gpu,
            "contacts-empty",
            ACCOUNTS,
            "You don't have any contacts.",
        )?;
    } else {
        label(ui, gpu, "Contacts", 26, true, false, false)?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            "Choose a contact to see their conversations.",
            14,
            false,
            true,
            false,
        )?;
    }

    if app.contact_mode == Mode::Add {
        let auth = form_input(
            ui,
            gpu,
            "Auth ID",
            "contact-auth",
            "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
            InputOptions {
                max_chars: Some(32),
                filter: Some(|c| c.is_ascii_hexdigit()),
            },
            true,
        )?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            "Paste the 32-character Auth ID from your contact.",
            12,
            false,
            true,
            false,
        )?;
        ui.space(16.0);
        let name = form_input(
            ui,
            gpu,
            "Nickname",
            "contact-name",
            "Enter a nickname",
            InputOptions {
                max_chars: Some(NAME_MAX),
                filter: Some(|c| !c.is_control()),
            },
            true,
        )?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            "Nicknames are only visible to you. You can change them at any time.",
            12,
            false,
            true,
            false,
        )?;
        let id = auth
            .parse::<Id>()
            .ok()
            .filter(|id| app.contacts.get(*id).is_none());
        let can_save = id.is_some() && !name.trim().is_empty();

        ui.space(20.0);
        let y = ui.position().1;
        let button_width = ((width - 8.0) * 0.5).clamp(1.0, 76.0);
        ui.set_position((width - 2.0 * button_width - 8.0).max(0.0), y);
        ui.push_sizing(Some(button_width), Some(36.0));
        let cancel = command_button(ui, gpu, "cancel-contact-edit", "", "Cancel", true, false);
        ui.pop_sizing();

        ui.set_position((width - button_width).max(0.0), y);
        ui.push_sizing(Some(button_width), Some(36.0));
        let save = command_button(ui, gpu, "save-contact", "", "Save", can_save, true);
        ui.pop_sizing();
        let save = save?;

        if cancel? {
            action = Action::CancelContactEdit;
        } else if let Some(id) = id
            && can_save
            && (save || ui.input_submitted("contact-name"))
        {
            action = Action::SaveContact(id, name);
        }
    }

    if contact.is_some() {
        ui.space(20.0);
        label(ui, gpu, "Conversations", 18, true, false, false)?;
        ui.space(8.0);
        let links = &app.contact_links;
        let visible = ui.visible_rows_from(ui.position().1, 58.0, links.len());
        let end = visible.end;
        ui.space(visible.start as f32 * 58.0);
        for link in &links[visible] {
            if ui
                .action_row(
                    gpu,
                    &link.key,
                    ActionRow {
                        title: &link.title,
                        detail: &link.detail,
                        icon: GROUP,
                        selected: false,
                        badge: None,
                    },
                )?
                .clicked
            {
                action = Action::OpenConversation {
                    profile: link.profile.clone(),
                    id: link.id,
                };
            }
        }

        ui.space((links.len() - end) as f32 * 58.0);
        if links.is_empty() {
            label(
                ui,
                gpu,
                "No conversations with this contact",
                14,
                false,
                true,
                false,
            )?;
        }
    }

    ui.div_end();
    Ok(action)
}
