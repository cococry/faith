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
    widgets::input::InputOptions,
};

const MENU: &str = "\u{eb94}";
const ACCOUNTS: &str = "\u{eb99}";
const CHATS: &str = "\u{eac7}";
const SETTINGS: &str = "\u{eb51}";
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
}

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
}

impl App {
    fn login(&mut self, profile: Profile, cli: &cli::Cli, platform: &Platform) {
        self.session = None;
        self.error.clear();
        self.conversation = None;
        self.list_offset = 0;
        self.page = Page::Conversations;
        self.selected = Some(profile.clone());
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
        }
        platform.request_redraw()?;
        Ok(())
    }
}

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

fn end_panel(ui: &mut Framework) {
    ui.div_end();
    ui.pop_style();
}

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
    let font = Some(if bold { Font::Bold } else { Font::Regular });
    let result = if centered {
        ui.text_centered(gpu, value, font)
    } else {
        ui.text(gpu, value, font)
    };
    ui.pop_style();
    result
}

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

fn item_list(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    snapshot: &client::Snapshot,
    height: f32,
) -> anyhow::Result<Action> {
    let mut action = Action::None;
    let width = ui.div_width();
    if app.collapsed {
        ui.set_position(12.0, 16.0);
        if icon(ui, gpu, "list-expand", MENU, false)? {
            action = Action::ToggleMenu;
        }
    }
    ui.set_position(if app.collapsed { 62.0 } else { 24.0 }, 22.0);
    ui.push_sizing(
        Some((width - if app.collapsed { 118.0 } else { 80.0 }).max(1.0)),
        None,
    );
    label(
        ui,
        gpu,
        if app.page == Page::Conversations {
            "Conversations"
        } else {
            "Profiles"
        },
        20,
        true,
        false,
        false,
    )?;
    ui.pop_sizing();
    ui.set_position(width - 52.0, 16.0);
    let _ = icon(ui, gpu, "list-more", MORE, false)?;
    let chats = app.page == Page::Conversations;
    let count = if chats {
        snapshot.conversations.len()
    } else {
        app.profiles.len()
    };
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

fn brand_page(ui: &mut Framework, gpu: &mut Renderer, logo: Image) -> anyhow::Result<()> {
    let width = ui.div_width();
    let height = ui.div_height();
    let size = 100.0_f32.min((width - 48.0).max(32.0));
    let top = ((height - 250.0) * 0.5).max(24.0);
    ui.set_position((width - size) * 0.5, top);
    ui.image(logo, size, size)?;
    ui.set_position(20.0, top + 132.0);
    ui.push_sizing(Some((width - 40.0).max(1.0)), None);
    label(ui, gpu, "Faith Messenger", 30, true, false, true)?;
    ui.pop_sizing();
    ui.set_position(24.0, top + 186.0);
    ui.push_sizing(Some((width - 48.0).max(1.0)), None);
    label(
        ui,
        gpu,
        "Secure conversations, native and fast.",
        15,
        false,
        true,
        true,
    )?;
    ui.pop_sizing();
    Ok(())
}

fn profile_creation_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    has_profiles: bool,
) -> anyhow::Result<Action> {
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
        "Create a profile for your private conversations.",
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
        border_color: Some(ui.theme().border),
        background: Some(ui.theme().input),
        ..ui.style()
    });
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
    let top = (ui.div_height() * 0.5 - 38.0).max(200.0);
    ui.set_position(24.0, top);
    ui.push_sizing(Some((width - 48.0).max(1.0)), None);
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
    ui.pop_sizing();
    let mut action = Action::None;
    if app.selected.is_some() && !snapshot.authorized {
        panel(
            ui,
            28.0,
            (ui.div_height() - 90.0).max(280.0),
            (width - 56.0).min(220.0),
            52.0,
            false,
        )?;
        if row_button(ui, gpu, "reconnect", REFRESH, "Reconnect", false, false)? {
            action = Action::Retry;
        }
        end_panel(ui);
    }
    Ok(action)
}

fn settings_page(ui: &mut Framework, gpu: &mut Renderer) -> anyhow::Result<()> {
    ui.set_position(32.0, 36.0);
    label(ui, gpu, "Settings", 26, true, false, false)?;
    ui.set_position(32.0, 98.0);
    ui.push_sizing(Some((ui.div_width() - 64.0).max(1.0)), None);
    label(
        ui,
        gpu,
        "Your preferences will live here.",
        15,
        false,
        true,
        false,
    )?;
    label(
        ui,
        gpu,
        "For now, choose a theme at startup with --theme light or --theme dark.",
        14,
        false,
        true,
        false,
    )?;
    ui.pop_sizing();
    Ok(())
}

fn shell(ui: &mut Framework, gpu: &mut Renderer, app: &App, logo: Image) -> anyhow::Result<Action> {
    let width = ui.div_width();
    let height = ui.div_height();
    let rail = if app.collapsed { 0.0 } else { 72.0 };
    let remaining = (width - rail).max(1.0);
    let list = (remaining / 3.0)
        .clamp(220.0, 400.0)
        .min((remaining - 200.0).max(100.0));
    let content = (remaining - list).max(1.0);
    let snapshot = app
        .session
        .as_ref()
        .map_or_else(client::Snapshot::default, |s| s.snapshot());
    let mut action = Action::None;
    if !app.collapsed {
        panel(ui, 0.0, 0.0, rail, height, true)?;
        action = navigation(ui, gpu, app, height)?;
        end_panel(ui);
    }
    panel(ui, rail, 0.0, list, height, true)?;
    let list_action = item_list(ui, gpu, app, &snapshot, height)?;
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
    let content_action = match app.page {
        Page::Accounts => {
            brand_page(ui, gpu, logo)?;
            Action::None
        }
        Page::Create => profile_creation_page(ui, gpu, !app.profiles.is_empty())?,
        Page::Conversations => logged_in_page(ui, gpu, app, &snapshot)?,
        Page::Settings => {
            settings_page(ui, gpu)?;
            Action::None
        }
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

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let mut cli = cli::Cli::parse();
    cli.resolve_local_tls();
    let config = WindowConfig {
        backend: cli.platform,
        title: "Faith Messenger".into(),
        width: cli.width.max(800),
        height: cli.height.max(540),
    };
    let mut platform = Platform::new(&config)?;
    let mut renderer = Renderer::new(cli.graphics.into(), &platform)?;
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
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
    };
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
    loop {
        let events = platform.poll_events()?;
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
        renderer.clear_color(ui.theme().background);
        renderer.begin_frame()?;
        ui.begin(&platform, &events);
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
        let action = shell(&mut ui, &mut renderer, &app, logo)?;
        ui.end(&mut renderer)?;
        renderer.end_frame()?;
        app.act(action, &cli, &platform, &mut ui)?;
    }
}
