mod cli;
mod client;
mod graphics;
mod platform;
mod profiles;
mod ui;

use clap::Parser;
use graphics::{GraphicsDevice, Renderer};
use platform::{Platform, WindowConfig, WindowEvent};
use profiles::Profile;
use ui::{
    framework::{DivOptions, Font, Fonts, Framework, Style},
    renderer::Image,
    theme::ThemeName,
    widgets::input::InputOptions,
};

// Icons are kept separate from the normal text fonts.
const MENU: &str = "\u{eb94}";
const ACCOUNTS: &str = "\u{eb99}";
const CHATS: &str = "\u{eac7}";
const SETTINGS: &str = "\u{eb51}";
const THEME: &str = "\u{eac6}";
const MORE: &str = "\u{ea7c}";
const ADD: &str = "\u{ea60}";
const LOCK: &str = "\u{ea75}";
const BACK: &str = "\u{eab5}";
const NEXT: &str = "\u{ea9c}";
const REFRESH: &str = "\u{eb37}";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Accounts,
    Create,
    Conversations,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
enum SettingsItem {
    Theme,
    Misc,
}

enum Action {
    None,
    Select(usize),
    Create(String),
    Choose,
    New,
    Retry,
    Chats,
    Settings,
    ToggleMenu,
    ListPage(usize),
    Conversation(faith_client::Id),
    Theme(ThemeName),
    SettingsItem(SettingsItem),
    Rounded(bool),
}

// State shared by the pages.
struct App {
    root: std::path::PathBuf,
    profiles: Vec<Profile>,
    selected: Option<Profile>,
    session: Option<client::Session>,
    page: Page,
    error: String,
    collapsed: bool,
    list_offset: usize,
    conversation: Option<faith_client::Id>,
    theme: ThemeName,
    settings_item: SettingsItem,
    rounded: bool,
}

// Account sessions and actions returned by the pages.
impl App {
    fn login(&mut self, profile: Profile, cli: &cli::Cli, platform: &Platform) {
        // Stop the old session before selecting another account.
        self.session = None;
        self.error.clear();
        self.conversation = None;
        self.list_offset = 0;
        self.page = Page::Conversations;
        self.selected = Some(profile.clone());

        // The client worker handles connection and authentication.
        match client::Session::start(
            faith_client::Config {
                host: cli.host.clone(),
                port: cli.port,
                server_name: cli.server_name.clone(),
                ca_file: cli.ca_file.clone(),
                identity_path: profile.path.join("identity.bin"),
                store: None,
            },
            platform.waker(),
        ) {
            Ok(session) => self.session = Some(session),
            Err(error) => self.error = error.to_string(),
        }
    }

    fn act(
        &mut self,
        action: Action,
        cli: &cli::Cli,
        platform: &Platform,
        ui: &mut Framework,
    ) -> anyhow::Result<()> {
        // Apply the action after the current frame has finished.
        match action {
            Action::None => return Ok(()),
            Action::Select(index) => self.login(self.profiles[index].clone(), cli, platform),
            Action::Create(alias) => match profiles::create(&self.root, &alias) {
                Ok(profile) => {
                    self.profiles.push(profile.clone());
                    self.profiles.sort_by(|a, b| a.alias.cmp(&b.alias));
                    self.login(profile, cli, platform);
                }

                Err(error) => self.error = error.to_string(),
            },

            // Refresh the profiles when opening the account list.
            Action::Choose => {
                self.error.clear();

                match profiles::discover(&self.root) {
                    Ok(mut profiles) => {
                        if let Some(selected) = &self.selected
                            && !profiles.iter().any(|p| p.path == selected.path)
                        {
                            profiles.push(selected.clone());
                        }

                        self.profiles = profiles;
                    }

                    Err(error) => self.error = error.to_string(),
                }

                self.page = if self.profiles.is_empty() {
                    Page::Create
                } else {
                    Page::Accounts
                };

                self.list_offset = 0;
            }

            // Start with an empty handle when opening the creation form.
            Action::New => {
                self.error.clear();
                ui.clear_input("new-profile-handle");
                self.page = Page::Create;
            }

            Action::Retry => {
                if let Some(profile) = self.selected.clone() {
                    self.login(profile, cli, platform);
                }
            }

            Action::Chats => {
                self.page = Page::Conversations;
                self.list_offset = 0;
            }

            Action::Settings => self.page = Page::Settings,
            Action::ToggleMenu => self.collapsed = !self.collapsed,
            Action::ListPage(offset) => self.list_offset = offset,
            Action::Conversation(id) => self.conversation = Some(id),
            Action::SettingsItem(item) => self.settings_item = item,
            Action::Rounded(enabled) => {
                self.rounded = enabled;
                ui.set_rounded_corners(enabled);
            }

            Action::Theme(theme) => {
                self.theme = theme;
                ui.set_theme(theme.colors());
            }
        }

        platform.request_redraw()?;
        Ok(())
    }
}

// A pane with its own layout and clipping region.
fn panel(ui: &mut Framework, x: f32, y: f32, w: f32, h: f32, surface: bool) -> anyhow::Result<()> {
    ui.set_position(x, y);
    ui.push_style(Style {
        background: Some(if surface {
            ui.theme().surface
        } else {
            ui.theme().background
        }),
        ..ui.style()
    });

    ui.div_begin(DivOptions {
        width: Some(w.max(0.0)),
        height: Some(h.max(0.0)),
        ..Default::default()
    })?;

    Ok(())
}

// Restore the parent pane and its style.
fn end_panel(ui: &mut Framework) {
    ui.div_end();
    ui.pop_style();
}

// Text using the current theme and the requested weight.
fn label(
    ui: &mut Framework,
    gpu: &mut Renderer,
    value: &str,
    size: u32,
    bold: bool,
    muted: bool,
    centered: bool,
) -> anyhow::Result<()> {
    ui.push_style(Style {
        font_size: size,
        color: if muted {
            ui.theme().muted
        } else {
            ui.theme().text
        },
        ..ui.style()
    });

    let font = Some(if bold {
        Font::Bold
    } else {
        Font::Regular
    });

    let result = if centered {
        ui.text_centered(gpu, value, font)
    } else {
        ui.text(gpu, value, font)
    };

    ui.pop_style();
    result
}

// Centered icon button for compact controls.
fn icon(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    selected: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        font_size: 20,
        padding: 4.0,
        radius: ui.corner_radii().icon,
        color: if selected {
            ui.theme().selected_icon
        } else {
            ui.theme().text
        },
        background: selected.then_some(ui.theme().button),
        ..ui.style()
    });

    ui.push_sizing(Some(40.0), Some(40.0));
    let result = ui.icon_button(gpu, id, glyph);
    ui.pop_sizing();
    ui.pop_style();
    result
}

// Full-width button with an icon and an optional selected badge.
fn row_button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    id: &str,
    glyph: &str,
    value: &str,
    selected: bool,
    primary: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        font_size: 16,
        padding: 14.0,
        radius: ui.corner_radii().row,
        color: if primary {
            ui.theme().primary_text
        } else if selected {
            ui.theme().selected_icon
        } else {
            ui.theme().button_text
        },
        background: Some(if primary {
            ui.theme().primary
        } else if selected {
            ui.theme().button
        } else {
            ui.theme().surface
        }),
        ..ui.style()
    });

    ui.push_sizing(Some(ui.div_width()), Some(48.0));
    let result = ui.icon_label_badge_button(
        gpu,
        id,
        glyph,
        value,
        (selected && glyph == ACCOUNTS).then_some("Selected"),
    );
    ui.pop_sizing();
    ui.pop_style();
    result
}

// The collapsible rail for accounts, chats and settings.
fn navigation(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    height: f32,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    ui.set_position(16.0, 16.0);

    if icon(ui, gpu, "nav-collapse", MENU, false)? {
        action = Action::ToggleMenu;
    }

    ui.set_position(16.0, 88.0);

    if icon(
        ui,
        gpu,
        "nav-accounts",
        ACCOUNTS,
        matches!(app.page, Page::Accounts | Page::Create),
    )? {
        action = Action::Choose;
    }

    ui.set_position(16.0, 144.0);

    if icon(ui, gpu, "nav-chats", CHATS, app.page == Page::Conversations)? {
        action = Action::Chats;
    }

    // Keep settings at the bottom of the rail.
    ui.set_position(16.0, (height - 64.0).max(200.0));

    if icon(
        ui,
        gpu,
        "nav-settings",
        SETTINGS,
        app.page == Page::Settings,
    )? {
        action = Action::Settings;
    }

    Ok(action)
}

// Shared heading, menu toggle and overflow button.
fn list_header(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    title: &str,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    let width = ui.div_width();

    // Keep the menu toggle available when the rail is hidden.
    if app.collapsed {
        ui.set_position(12.0, 16.0);

        if icon(ui, gpu, "list-expand", MENU, false)? {
            action = Action::ToggleMenu;
        }
    }

    ui.set_position(
        if app.collapsed {
            62.0
        } else {
            24.0
        },
        22.0,
    );
    ui.push_sizing(
        Some(
            (width
                - if app.collapsed {
                    118.0
                } else {
                    80.0
                })
            .max(1.0),
        ),
        None,
    );
    label(ui, gpu, title, 20, true, false, false)?;
    ui.pop_sizing();
    ui.set_position(width - 52.0, 16.0);
    let _ = icon(ui, gpu, "list-more", MORE, false)?;
    Ok(action)
}

// The available settings sections.
fn settings_list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    height: f32,
) -> anyhow::Result<Action> {
    let mut action = list_header(ui, gpu, app, "Settings")?;
    let width = ui.div_width();
    panel(
        ui,
        16.0,
        94.0,
        width - 32.0,
        (height - 110.0).max(48.0),
        true,
    )?;

    if row_button(
        ui,
        gpu,
        "settings-theme",
        THEME,
        "Theme",
        app.settings_item == SettingsItem::Theme,
        false,
    )? {
        action = Action::SettingsItem(SettingsItem::Theme);
    }

    ui.space(10.0);

    if row_button(
        ui,
        gpu,
        "settings-misc",
        "\u{eb52}",
        "Misc",
        app.settings_item == SettingsItem::Misc,
        false,
    )? {
        action = Action::SettingsItem(SettingsItem::Misc);
    }

    end_panel(ui);
    Ok(action)
}

// Profile and conversation rows in the left-hand list.
fn item_list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    snapshot: &client::Snapshot,
    height: f32,
) -> anyhow::Result<Action> {
    let mut action = list_header(
        ui,
        gpu,
        app,
        if app.page == Page::Conversations {
            "Conversations"
        } else {
            "Profiles"
        },
    )?;
    let width = ui.div_width();
    let chats = app.page == Page::Conversations;

    let count = if chats {
        snapshot.conversations.len()
    } else {
        app.profiles.len()
    };

    // Fit as many rows as the current window height allows.
    let per_page = ((height - 250.0) / 58.0).floor().max(1.0) as usize;
    let offset = app
        .list_offset
        .min(count.saturating_sub(1) / per_page * per_page);
    panel(
        ui,
        16.0,
        94.0,
        width - 32.0,
        (height - 226.0).max(60.0),
        true,
    )?;

    // An empty list still gets a heading and a short explanation.
    if count == 0 {
        ui.set_position(8.0, 14.0);
        label(
            ui,
            gpu,
            if chats {
                "A little quiet here"
            } else {
                "No profile yet"
            },
            16,
            true,
            false,
            false,
        )?;
        ui.set_position(8.0, 48.0);
        ui.push_sizing(Some((width - 64.0).max(1.0)), None);
        label(
            ui,
            gpu,
            if chats {
                "Your conversations will appear here."
            } else {
                "Your space for secure conversations starts with a profile."
            },
            14,
            false,
            true,
            false,
        )?;
        ui.pop_sizing();
    } else {
        // Render one row for each visible profile or conversation.
        for index in offset..(offset + per_page).min(count) {
            ui.set_position(0.0, (index - offset) as f32 * 58.0);

            if chats {
                let id = snapshot.conversations[index];
                let title = format!("Chat · {}", &id.to_string()[..8]);

                if row_button(
                    ui,
                    gpu,
                    &format!("conversation-{id}"),
                    CHATS,
                    &title,
                    app.conversation == Some(id),
                    false,
                )? {
                    action = Action::Conversation(id);
                }
            } else {
                let profile = &app.profiles[index];
                let selected = app
                    .selected
                    .as_ref()
                    .is_some_and(|p| p.path == profile.path);

                if row_button(
                    ui,
                    gpu,
                    &format!("profile-{}", profile.alias),
                    ACCOUNTS,
                    &profile.alias,
                    selected,
                    false,
                )? {
                    action = Action::Select(index);
                }
            }
        }
    }

    end_panel(ui);

    // Show paging controls when the list does not fit.
    if count > per_page {
        ui.set_position(16.0, height - 126.0);

        if offset > 0 && icon(ui, gpu, "list-previous", BACK, false)? {
            action = Action::ListPage(offset.saturating_sub(per_page));
        }

        ui.set_position(width - 56.0, height - 126.0);

        if offset + per_page < count && icon(ui, gpu, "list-next", NEXT, false)? {
            action = Action::ListPage(offset + per_page);
        }
    }

    // The list footer shows account status or the create button.
    panel(ui, 16.0, height - 70.0, width - 32.0, 54.0, true)?;

    if chats {
        if let Some(profile) = &app.selected {
            label(ui, gpu, &profile.alias, 15, true, false, false)?;
            label(
                ui,
                gpu,
                if snapshot.authorized {
                    "Connected securely"
                } else if snapshot.status.is_empty() {
                    "Connecting…"
                } else {
                    &snapshot.status
                },
                12,
                false,
                true,
                false,
            )?;
        } else if row_button(
            ui,
            gpu,
            "choose-profile",
            ACCOUNTS,
            "Choose a profile",
            false,
            false,
        )? {
            action = Action::Choose;
        }
    } else if row_button(
        ui,
        gpu,
        "create-profile",
        ADD,
        "Create New...",
        false,
        false,
    )? {
        action = Action::New;
    }

    end_panel(ui);
    Ok(action)
}

// The centered logo, name and tagline.
fn brand_page(ui: &mut Framework, gpu: &mut Renderer, logo: Image) -> anyhow::Result<()> {
    let width = ui.div_width();
    let height = ui.div_height();
    let content_width = (width - 40.0).max(1.0);
    let size = 100.0_f32.min(content_width);
    let top = ((height - 250.0) * 0.5).max(24.0);
    ui.set_position(20.0, top);
    ui.div_begin(DivOptions {
        width: Some(content_width),
        ..Default::default()
    })?;

    ui.set_position((content_width - size) * 0.5, 0.0);

    // Center the logo inside the text column.
    ui.image(logo, size, size)?;

    // Let wrapped headings push the tagline down.
    ui.space(32.0);
    label(ui, gpu, "Faith Messenger", 30, true, false, true)?;
    ui.space(12.0);
    label(
        ui,
        gpu,
        "Secure conversations, native and fast.",
        15,
        false,
        true,
        true,
    )?;
    ui.div_end();
    Ok(())
}

// The form for creating a local account profile.
fn profile_creation_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    has_profiles: bool,
) -> anyhow::Result<Action> {
    // Keep the form narrow and centered in the main pane.
    let width = (ui.div_width() - 64.0).clamp(1.0, 380.0);
    let x = (ui.div_width() - width) * 0.5;
    let y = ((ui.div_height() - 540.0) * 0.5).max(24.0);
    ui.set_position(x, y);
    ui.push_style(Style {
        background: Some(ui.theme().background),
        ..ui.style()
    });

    ui.div_begin(DivOptions {
        width: Some(width),
        ..Default::default()
    })?;

    ui.push_style(Style {
        font_size: 28,
        color: ui.theme().muted,
        ..ui.style()
    });

    ui.text(gpu, LOCK, Some(Font::Icons))?;
    ui.pop_style();
    ui.space(20.0);
    label(
        ui,
        gpu,
        "A fresh disposable identity",
        26,
        true,
        false,
        false,
    )?;
    ui.space(12.0);
    label(
        ui,
        gpu,
        "Create a profile for your private conversations. This is a local alias for your account.",
        15,
        false,
        true,
        false,
    )?;
    ui.space(28.0);
    label(ui, gpu, "Account handle", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(Style {
        padding: 12.0,
        font_size: 16,
        radius: ui.corner_radii().control,
        border_color: Some(ui.theme().border),
        background: Some(ui.theme().input),
        ..ui.style()
    });

    // Only allow characters that are valid in a profile handle.
    let alias = ui.input_with_options(
        gpu,
        "new-profile-handle",
        InputOptions {
            max_chars: Some(20),
            filter: Some(|c| c.is_ascii_alphanumeric() || c == '_'),
        },
    )?;
    ui.pop_style();
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Letters, numbers and underscores. Up to 20 characters.",
        12,
        false,
        true,
        false,
    )?;
    ui.space(24.0);
    let mut action = Action::None;

    // Create the profile when the completed button click is valid.
    if row_button(ui, gpu, "submit-create", ADD, "Create account", false, true)?
        && profiles::valid_alias(&alias)
    {
        action = Action::Create(alias);
    }

    if has_profiles {
        ui.space(12.0);

        if row_button(
            ui,
            gpu,
            "cancel-create",
            BACK,
            "Back to profiles",
            false,
            false,
        )? {
            action = Action::Choose;
        }
    }

    end_panel(ui);
    Ok(action)
}

// Account status and the conversation placeholder.
fn logged_in_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    snapshot: &client::Snapshot,
) -> anyhow::Result<Action> {
    let width = ui.div_width();
    panel(ui, 28.0, 24.0, width - 56.0, 160.0, false)?;
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

    // Display the identity reported by the client worker.
    if let Some(id) = snapshot.auth {
        label(ui, gpu, &format!("User ID: {id}"), 12, false, true, false)?;
    }

    if !snapshot.status.is_empty() {
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

    end_panel(ui);

    // Center the empty conversation view in its own column.
    let top = (ui.div_height() * 0.5 - 38.0).max(200.0);
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
    label(
        ui,
        gpu,
        if app.conversation.is_some() {
            "Your conversation"
        } else {
            "A space for your conversations"
        },
        23,
        true,
        false,
        true,
    )?;
    label(
        ui,
        gpu,
        if app.conversation.is_some() {
            "The message view is coming next."
        } else {
            "Choose a conversation from the list to get started."
        },
        14,
        false,
        true,
        true,
    )?;
    ui.div_end();
    let mut action = Action::None;

    if app.selected.is_some() && !snapshot.authorized {
        // Centered reconnect button with centered content.
        let button_width = (width - 56.0).min(220.0);
        panel(
            ui,
            (width - button_width) / 2.0,
            (ui.div_height() - 90.0).max(280.0),
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

// Small interface preferences.
fn misc_settings_page(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    let width = (ui.div_width() - 64.0).max(1.0);
    let height = (ui.div_height() - 72.0).max(1.0);
    panel(ui, 32.0, 36.0, width, height, false)?;
    label(ui, gpu, "Misc", 26, true, false, false)?;
    ui.space(12.0);
    label(
        ui,
        gpu,
        "Adjust details in the user interface.",
        15,
        false,
        true,
        false,
    )?;
    ui.space(36.0);
    label(ui, gpu, "Interface", 13, true, true, false)?;
    ui.space(10.0);

    // Apply the rounded-corner preference when the checkbox changes.
    let change = ui.checkbox(
        gpu,
        "settings-rounded-corners",
        "UI rounded corners",
        app.rounded,
    )?;
    ui.space(8.0);
    label(
        ui,
        gpu,
        "Adds rounded corners to elements in the interface.",
        13,
        false,
        true,
        false,
    )?;
    end_panel(ui);
    Ok(change.map_or(Action::None, Action::Rounded))
}

// The selected settings section and its controls.
fn settings_page(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    if app.settings_item == SettingsItem::Misc {
        return misc_settings_page(ui, gpu, app);
    }

    let width = (ui.div_width() - 64.0).max(1.0);
    let height = (ui.div_height() - 72.0).max(1.0);
    panel(ui, 32.0, 36.0, width, height, false)?;
    label(ui, gpu, "Theme", 26, true, false, false)?;
    ui.space(12.0);
    label(
        ui,
        gpu,
        "Choose the colors that feel like home.",
        15,
        false,
        true,
        false,
    )?;
    ui.space(36.0);
    label(ui, gpu, "Appearance", 13, true, true, false)?;
    ui.space(10.0);
    ui.push_style(Style {
        font_size: 15,
        padding: 12.0,
        background: Some(ui.theme().input),
        radius: ui.corner_radii().control,
        border_color: Some(ui.theme().border),
        ..ui.style()
    });

    ui.push_sizing(Some(width.min(340.0)), None);

    // Switch between the available themes without leaving the page.
    let choice = ui.dropdown(
        gpu,
        "settings-theme-dropdown",
        &["Dark (Gruvbox)", "Light (Blossom Light)"],
        if app.theme == ThemeName::Dark {
            0
        } else {
            1
        },
    )?;
    ui.pop_sizing();
    ui.pop_style();
    end_panel(ui);
    Ok(match choice {
        Some(0) => Action::Theme(ThemeName::Dark),
        Some(1) => Action::Theme(ThemeName::Light),
        _ => Action::None,
    })
}

// Arrange the navigation rail, item list and main content.
fn shell(ui: &mut Framework, gpu: &mut Renderer, app: &App, logo: Image) -> anyhow::Result<Action> {
    let width = ui.div_width();
    let height = ui.div_height();

    // The item list takes about a quarter of the available width.
    let rail = if app.collapsed {
        0.0
    } else {
        72.0
    };

    let remaining = (width - rail).max(1.0);
    let list = (remaining / 4.0)
        .clamp(220.0, 400.0)
        .min((remaining - 200.0).max(100.0));
    let content = (remaining - list).max(1.0);

    // Read the latest client state without making network requests.
    let snapshot = app
        .session
        .as_ref()
        .map_or_else(client::Snapshot::default, |s| s.snapshot());
    let mut action = Action::None;

    // Draw the rail only while it is expanded.
    if !app.collapsed {
        panel(ui, 0.0, 0.0, rail, height, true)?;
        action = navigation(ui, gpu, app, height)?;
        end_panel(ui);
    }

    panel(ui, rail, 0.0, list, height, true)?;

    // Settings has its own list beside profiles and conversations.
    let list_action = if app.page == Page::Settings {
        settings_list(ui, gpu, app, height)?
    } else {
        item_list(ui, gpu, app, &snapshot, height)?
    };

    if !matches!(list_action, Action::None) {
        action = list_action;
    }

    end_panel(ui);
    panel(ui, rail + list, 0.0, 1.0, height, true)?;
    ui.push_style(Style {
        background: Some(ui.theme().separator),
        ..ui.style()
    });

    ui.div_begin(DivOptions {
        width: Some(1.0),
        height: Some(height),
        ..Default::default()
    })?;

    ui.div_end();
    ui.pop_style();
    end_panel(ui);
    panel(ui, rail + list + 1.0, 0.0, content - 1.0, height, false)?;

    // Render the current page in the main content pane.
    let content_action = match app.page {
        Page::Accounts => {
            brand_page(ui, gpu, logo)?;
            Action::None
        }

        Page::Create => profile_creation_page(ui, gpu, !app.profiles.is_empty())?,
        Page::Conversations => logged_in_page(ui, gpu, app, &snapshot)?,
        Page::Settings => settings_page(ui, gpu, app)?,
    };

    if !matches!(content_action, Action::None) {
        action = content_action;
    }

    if !app.error.is_empty() {
        ui.set_position(28.0, (height - 42.0).max(0.0));
        ui.push_style(Style {
            font_size: 13,
            color: ui.theme().error,
            ..ui.style()
        });

        ui.push_sizing(Some((content - 56.0).max(1.0)), None);
        ui.text(gpu, &app.error, None)?;
        ui.pop_sizing();
        ui.pop_style();
    }

    end_panel(ui);
    Ok(action)
}

// Start the window and run the blocking UI loop.
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    // Read startup options and the local TLS configuration.
    let mut cli = cli::Cli::parse();
    cli.resolve_local_tls();
    let config = WindowConfig {
        backend: cli.platform,
        title: "Faith Messenger".into(),
        width: cli.width.max(800),
        height: cli.height.max(540),
    };

    // Create the window and select a graphics backend.
    let mut platform = Platform::new(&config)?;
    let mut renderer = Renderer::new(cli.graphics.into(), &platform)?;
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");

    // Register text and icon fonts; glyphs are loaded when needed.
    let mut ui = Framework::new(
        &mut renderer,
        &platform,
        Fonts {
            regular: assets.join("NotoSans-Regular.ttf"),
            bold: assets.join("NotoSans-Bold.ttf"),
            italic: None,
            icons: assets.join("nerd-fonts/SymbolsNerdFont-Regular.ttf"),
            fallbacks: [
                "NotoColorEmoji.ttf",
                "NotoSansArabic-Regular.ttf",
                "NotoSansHebrew-Regular.ttf",
            ]
            .iter()
            .map(|f| assets.join(f))
            .collect(),
        },
    )?;
    ui.set_theme(cli.theme.colors());
    let root = profiles::root()?;

    // Discover the accounts stored on this machine.
    let profiles = profiles::discover(&root)?;
    let mut app = App {
        root,
        profiles,
        selected: None,
        session: None,
        page: Page::Create,
        error: String::new(),
        collapsed: false,
        list_offset: 0,
        conversation: None,
        theme: cli.theme,
        settings_item: SettingsItem::Theme,
        rounded: false,
    };

    // Use an explicit profile, or automatically select the only account.
    if let Some(path) = &cli.profile {
        app.login(
            Profile {
                alias: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Account")
                    .to_owned(),
                path: path.clone(),
            },
            &cli,
            &platform,
        );
    } else if app.profiles.len() == 1 {
        app.login(app.profiles[0].clone(), &cli, &platform);
    } else if !app.profiles.is_empty() {
        app.page = Page::Accounts;
    }

    platform.request_redraw()?;
    let mut logo = None;

    // Wait for input or a client notification before drawing again.
    loop {
        let events = platform.poll_events()?;

        // Handle window shutdown and resizing first.
        for event in &events {
            match *event {
                WindowEvent::CloseRequested => return Ok(()),
                WindowEvent::Resized { width, height } if width > 0 && height > 0 => {
                    renderer.resize(width, height)
                }

                _ => {}
            }
        }

        let (w, h) = platform.size();

        if w == 0 || h == 0 || events.is_empty() {
            continue;
        }

        // Begin a new UI pass with the active theme.
        renderer.clear_color(ui.theme().background);
        renderer.begin_frame()?;
        ui.begin(&platform, &events);

        // Upload the logo once and reuse its image handle.
        let logo = match logo {
            Some(image) => image,
            None => {
                let image = ui.load_image(
                    &mut renderer,
                    &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/logo.png"),
                )?;
                logo = Some(image);
                image
            }
        };

        // Draw the shell, then apply any action returned by the widgets.
        let action = shell(&mut ui, &mut renderer, &app, logo)?;
        ui.end(&mut renderer)?;
        renderer.end_frame()?;

        app.act(action, &cli, &platform, &mut ui)?;
    }
}
