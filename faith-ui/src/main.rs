mod cli;
mod client;
mod graphics;
mod platform;
mod profiles;
mod ui;

use clap::Parser;
use graphics::{Color, GraphicsDevice, Renderer};
use platform::{Platform, WindowConfig, WindowEvent};
use profiles::Profile;
use ui::{
    framework::{DivOptions, Font, Fonts, Framework, Style},
    widgets::input::InputOptions,
};

const PROFILES_PER_PAGE: usize = 4;

enum Page {
    Select(usize),
    Create,
    LoggedIn,
}

enum Action {
    None,
    Select(usize),
    Create(String),
    Choose,
    New,
    Retry,
    ListPage(usize),
}

struct App {
    root: std::path::PathBuf,
    profiles: Vec<Profile>,
    selected: Option<Profile>,
    session: Option<client::Session>,
    page: Page,
    error: String,
}

impl App {
    fn login(&mut self, profile: Profile, cli: &cli::Cli, platform: &Platform) {
        self.session = None;
        self.error.clear();
        self.page = Page::LoggedIn;
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
                self.session = None;
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

                self.page = Page::Select(0);
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
            Action::ListPage(offset) => self.page = Page::Select(offset),
        }

        platform.request_redraw()?;
        Ok(())
    }
}

fn heading(ui: &mut Framework, gpu: &mut Renderer, title: &str) -> anyhow::Result<()> {
    ui.push_style(Style {
        font_size: 27,
        margin: 8.0,
        ..ui.style()
    });
    ui.text(gpu, title, Some(Font::Bold))?;
    ui.pop_style();
    Ok(())
}

fn muted(ui: &mut Framework, gpu: &mut Renderer, value: &str) -> anyhow::Result<()> {
    ui.push_style(Style {
        font_size: 14,
        margin: 8.0,
        color: Color::rgba(0.40, 0.43, 0.48, 1.0),
        ..ui.style()
    });
    ui.text(gpu, value, None)?;
    ui.pop_style();
    Ok(())
}

fn button(
    ui: &mut Framework,
    gpu: &mut Renderer,
    label: &str,
    primary: bool,
) -> anyhow::Result<bool> {
    ui.push_style(Style {
        padding: 12.0,
        margin: 6.0,
        background: Some(if primary {
            Color::rgba(0.82, 0.88, 1.0, 1.0)
        } else {
            Color::rgba(0.94, 0.95, 0.97, 1.0)
        }),
        ..ui.style()
    });
    ui.push_sizing(Some((ui.div_width() - 12.0).max(0.0)), None);
    let result = ui.button(gpu, label);
    ui.pop_sizing();
    ui.pop_style();
    result
}

fn profile_selection_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    app: &App,
    offset: usize,
) -> anyhow::Result<Action> {
    heading(ui, gpu, "Choose your account")?;
    muted(ui, gpu, "Select a local profile to use for your session.")?;
    let mut action = Action::None;
    for (index, profile) in app
        .profiles
        .iter()
        .enumerate()
        .skip(offset)
        .take(PROFILES_PER_PAGE)
    {
        if button(ui, gpu, &profile.alias, false)? {
            action = Action::Select(index);
        }
    }

    if app.profiles.is_empty() {
        muted(ui, gpu, "No profiles yet. Create your first account below.")?;
    }

    if offset > 0 && button(ui, gpu, "Previous accounts", false)? {
        action = Action::ListPage(offset.saturating_sub(PROFILES_PER_PAGE));
    }

    if offset + PROFILES_PER_PAGE < app.profiles.len() && button(ui, gpu, "More accounts", false)? {
        action = Action::ListPage(offset + PROFILES_PER_PAGE);
    }

    if button(ui, gpu, "Create New...", true)? {
        action = Action::New;
    }

    Ok(action)
}

fn profile_creation_page(
    ui: &mut Framework,
    gpu: &mut Renderer,
    has_profiles: bool,
) -> anyhow::Result<Action> {
    heading(ui, gpu, "Create your account")?;
    muted(
        ui,
        gpu,
        "Choose your account handle. This handle will be a local alias for a securely generated auth-ID.",
    )?;
    muted(ui, gpu, "Handle · 1–20 letters, numbers, or underscores")?;
    ui.push_style(Style {
        padding: 10.0,
        margin: 8.0,
        border_color: Some(Color::rgba(0.7, 0.73, 0.78, 1.0)),
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
    let mut action = Action::None;
    if button(ui, gpu, "Create account", true)? && profiles::valid_alias(&alias) {
        action = Action::Create(alias);
    }

    if has_profiles && button(ui, gpu, "Back to accounts", false)? {
        action = Action::Choose;
    }

    Ok(action)
}

fn logged_in_page(ui: &mut Framework, gpu: &mut Renderer, app: &App) -> anyhow::Result<Action> {
    heading(ui, gpu, "Your account")?;
    if let Some(profile) = &app.selected {
        ui.text(gpu, &profile.alias, Some(Font::Bold))?;
    }

    if let Some(session) = &app.session {
        let snapshot = session.snapshot();
        let identity = snapshot.auth.map_or_else(
            || "Generating account identity…".to_owned(),
            |id| format!("User ID: {id}"),
        );
        muted(ui, gpu, &identity)?;
        let status = if snapshot.status.is_empty() {
            "Connecting…"
        } else {
            &snapshot.status
        };
        ui.push_style(Style {
            margin: 8.0,
            color: if snapshot.authorized {
                Color::rgba(0.12, 0.46, 0.28, 1.0)
            } else {
                Color::rgba(0.55, 0.32, 0.12, 1.0)
            },
            ..ui.style()
        });
        ui.text(gpu, status, None)?;
        ui.pop_style();
        if let Some(notice) = snapshot.notices.last() {
            muted(ui, gpu, notice)?;
        }
    }

    let mut action = Action::None;
    if button(ui, gpu, "Reconnect", false)? {
        action = Action::Retry;
    }

    if button(ui, gpu, "Switch account", false)? {
        action = Action::Choose;
    }

    Ok(action)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let mut cli = cli::Cli::parse();
    cli.resolve_local_tls();
    let config = WindowConfig {
        backend: cli.platform,
        title: "Faith".into(),
        width: cli.width.max(320),
        height: cli.height.max(240),
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
    let root = profiles::root()?;
    let profiles = profiles::discover(&root)?;
    let mut app = App {
        root,
        profiles,
        selected: None,
        session: None,
        page: Page::Create,
        error: String::new(),
    };
    if let Some(path) = &cli.profile {
        let alias = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Account")
            .to_owned();
        app.login(
            Profile {
                alias,
                path: path.clone(),
            },
            &cli,
            &platform,
        );
    } else if app.profiles.len() == 1 {
        app.login(app.profiles[0].clone(), &cli, &platform);
    } else if !app.profiles.is_empty() {
        app.page = Page::Select(0);
    }

    platform.request_redraw()?;
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

        renderer.clear_color(Color::rgba(0.96, 0.97, 0.98, 1.0));
        renderer.begin_frame()?;
        ui.begin(&platform, &events);
        let width = (ui.div_width() - 32.0).clamp(1.0, 480.0);
        ui.set_position(
            (ui.div_width() - width) / 2.0,
            ((ui.div_height() - 560.0) / 2.0).max(16.0),
        );
        ui.push_style(Style {
            padding: 20.0,
            background: Some(Color::rgba(1.0, 1.0, 1.0, 1.0)),
            ..ui.style()
        });
        ui.div_begin(DivOptions {
            width: Some(width),
            ..Default::default()
        })?;
        let action = match app.page {
            Page::Select(offset) => profile_selection_page(&mut ui, &mut renderer, &app, offset)?,
            Page::Create => {
                profile_creation_page(&mut ui, &mut renderer, !app.profiles.is_empty())?
            }
            Page::LoggedIn => logged_in_page(&mut ui, &mut renderer, &app)?,
        };
        if !app.error.is_empty() {
            ui.push_style(Style {
                margin: 8.0,
                font_size: 14,
                color: Color::rgba(0.7, 0.16, 0.18, 1.0),
                ..ui.style()
            });
            ui.text(&mut renderer, &app.error, None)?;
            ui.pop_style();
        }

        ui.div_end();
        ui.pop_style();
        ui.end(&mut renderer)?;
        renderer.end_frame()?;
        app.act(action, &cli, &platform, &mut ui)?;
    }
}
