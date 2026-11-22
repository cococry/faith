use faith_client::Id;

use crate::{
    ACCOUNTS, ADD, Action, App, Font, Framework, GROUP, Profile, Renderer, Style, TitleEdit,
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
        let clicked = ui
            .action_row(
                gpu,
                &contact.auth,
                ActionRow {
                    title: &contact.display,
                    detail: auth,
                    icon: ACCOUNTS,
                    selected: selected == Some(contact.id),
                    badge: None,
                },
            )?
            .clicked;
        if let Some(at) = ui.context_press([x, y, ui.div_width(), 56.0]) {
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
    let width = (ui.div_width() - 40.0).clamp(1.0, 620.0);
    let height = (ui.div_height() - 38.0).max(1.0);
    ui.set_position((ui.div_width() - width) * 0.5, 20.0);
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

        if app.contact_mode == Mode::View {
            let width = ui.div_width();
            for (offset, id, glyph, tooltip, command) in [
                (
                    88.0,
                    "edit-contact",
                    EDIT,
                    "Edit local name",
                    Action::EditContact(contact.id),
                ),
                (
                    40.0,
                    "delete-contact",
                    DELETE,
                    "Delete contact",
                    Action::ConfirmDeleteContact(contact.id),
                ),
            ] {
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
        ui.space(16.0);
    } else {
        label(ui, gpu, "Contacts", 26, true, false, false)?;
        ui.space(8.0);
        label(
            ui,
            gpu,
            if app.contacts.entries.is_empty() {
                "Add a contact by Auth ID."
            } else {
                "Choose a contact to see their conversations."
            },
            14,
            false,
            true,
            false,
        )?;
    }

    if app.contact_mode == Mode::Add {
        label(ui, gpu, "Auth ID", 14, true, false, false)?;
        ui.space(6.0);
        let auth = ui.input_with_hint(
            gpu,
            "contact-auth",
            InputOptions {
                max_chars: Some(32),
                filter: Some(|c| c.is_ascii_hexdigit()),
                ..Default::default()
            },
            "32-character Auth ID",
        )?;
        ui.space(12.0);
        let id = auth
            .parse::<Id>()
            .ok()
            .filter(|id| app.contacts.get(*id).is_none());

        label(ui, gpu, "Local name", 14, true, false, false)?;
        ui.space(6.0);
        let name = ui.input_with_hint(
            gpu,
            "contact-name",
            InputOptions {
                max_chars: Some(NAME_MAX),
                filter: Some(|c| !c.is_control()),
                ..Default::default()
            },
            "Enter a local name",
        )?;
        ui.space(10.0);
        ui.push_style(Style {
            enabled: id.is_some() && !name.trim().is_empty(),
            ..ui.style()
        });
        let save = row_button(ui, gpu, "save-contact", EDIT, "Save contact", false, false)?;
        ui.pop_style();
        if let Some(id) = id
            && !name.trim().is_empty()
            && (save || ui.input_submitted("contact-name"))
        {
            action = Action::SaveContact(id, name);
        }

        if row_button(ui, gpu, "cancel-contact-edit", "", "Cancel", false, false)? {
            action = Action::CancelContactEdit;
        }
    }

    if let Some(contact) = contact {
        if app.contact_mode == Mode::Delete {
            label(
                ui,
                gpu,
                "Delete this local contact?",
                14,
                true,
                false,
                false,
            )?;
            if row_button(
                ui,
                gpu,
                "confirm-delete-contact",
                DELETE,
                "Delete contact",
                false,
                false,
            )? {
                action = Action::DeleteContact(contact.id);
            }

            if row_button(ui, gpu, "cancel-contact-delete", "", "Cancel", false, false)? {
                action = Action::CancelContactEdit;
            }
        }

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
