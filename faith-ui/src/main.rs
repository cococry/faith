mod cli;
mod client;
mod graphics;
mod platform;
mod ui;

use crate::{
    graphics::{Color, GraphicsDevice, Renderer},
    ui::{
        TextRenderer, UIRenderer,
        input::{Bounds, InputField},
        text::TextLayoutOptions,
    },
};
use clap::Parser;
use cli::Cli;
use platform::{Platform, WindowConfig, WindowEvent};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("faith=info")),
        )
        .init();
    let cli = Cli::parse();
    anyhow::ensure!(
        cli.width >= 320 && cli.height >= 240,
        "initial window must be at least 320×240"
    );
    let conf = WindowConfig {
        backend: cli.platform,
        title: "Faith Messenger".into(),
        width: cli.width,
        height: cli.height,
    };
    let mut platform = Platform::new(&conf)?;
    let mut renderer = Renderer::new(cli.graphics.into(), &platform)?;
    let mut ui = UIRenderer::new(&mut renderer, conf.width, conf.height)?;
    let mut text = TextRenderer::new()?;
    // Asset resolution is independent of the directory from which Faith starts.
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let font = text.load_font(assets.join("NotoSans-Regular.ttf").to_str().unwrap(), 18)?;
    let bold = text.load_font(assets.join("NotoSans-Bold.ttf").to_str().unwrap(), 20)?;
    for name in [
        "NotoColorEmoji.ttf",
        "NotoSansArabic-Regular.ttf",
        "NotoSansHebrew-Regular.ttf",
    ] {
        text.load_font(assets.join(name).to_str().unwrap(), 18)?;
    }
    let session = client::Session::start(
        faith_client::Config {
            host: cli.host,
            port: cli.port,
            server_name: cli.server_name,
            ca_file: cli.ca_file,
            identity_path: cli.profile.join("identity.bin"),
            store: None,
        },
        platform.waker(),
    )?;
    let mut input = InputField::new(faith_client::MAX_TEXT);
    let mut snapshot = session.snapshot();
    let mut scroll = 0.0f32;
    let mut local_error = String::new();
    let mut awaiting_submission = None;
    let mut deferred_input = std::collections::VecDeque::new();
    platform.request_redraw()?;
    loop {
        // Blocks indefinitely at idle. Network notifications and external timer
        // threads can wake this same loop through PlatformWaker's eventfd.
        let events = if awaiting_submission.is_none() && !deferred_input.is_empty() {
            deferred_input.drain(..).collect()
        } else {
            platform.poll_events()?
        };
        let mut redraw = false;
        for event in events {
            match &event {
                WindowEvent::CloseRequested => return Ok(()),
                WindowEvent::Resized { width, height } => {
                    if *width > 0 && *height > 0 {
                        renderer.resize(*width, *height);
                    }
                    redraw = true;
                }
                WindowEvent::RedrawRequested => redraw = true,
                WindowEvent::KeyPressed {
                    key: platform::event::Key::PageUp,
                    ..
                } => {
                    scroll += 240.0;
                    redraw = true;
                }
                WindowEvent::KeyPressed {
                    key: platform::event::Key::PageDown,
                    ..
                } => {
                    scroll = (scroll - 240.0).max(0.0);
                    redraw = true;
                }
                _ => {}
            }
            if awaiting_submission.is_some()
                && matches!(
                    event,
                    WindowEvent::TextInput(_) | WindowEvent::KeyPressed { .. }
                )
            {
                if deferred_input.len() < 8192 {
                    deferred_input.push_back(event);
                } else {
                    local_error = "Input queue full; wait for the current submission".into();
                    redraw = true;
                }
                continue;
            }
            let response = input.handle(&event);
            redraw |= response.changed;
            if let Some(line) = response.submitted {
                if line.trim() == "/quit" {
                    return Ok(());
                }
                match session.submit(line) {
                    Ok(id) => {
                        awaiting_submission = Some(id);
                        local_error.clear();
                        scroll = 0.0;
                    }
                    Err(error) => {
                        local_error = error.to_string();
                    }
                }
                redraw = true;
            }
        }
        if let Some(latest) = session.snapshot_since(snapshot.generation) {
            if let Some((id, accepted)) = latest.submission
                && awaiting_submission == Some(id)
            {
                if accepted {
                    input.clear();
                }
                awaiting_submission = None;
            }
            if latest.status.starts_with("Client stopped:") {
                awaiting_submission = None;
            }
            snapshot = latest;
            redraw = true;
        }
        if !redraw {
            continue;
        }
        let (width, height) = platform.size();
        if width == 0 || height == 0 {
            continue;
        }
        renderer.clear_color(Color::rgba(0.98, 0.98, 0.99, 1.0));
        renderer.begin_frame()?;
        ui.begin(width, height);
        let width = width as f32;
        let height = height as f32;
        let available = (width - 32.0).max(1.0);
        let identity = snapshot
            .auth
            .map_or_else(|| "Loading identity…".into(), |id| format!("Your ID: {id}"));
        text.render(16.0, 12.0, &identity, bold, &mut renderer, &mut ui)?;
        let status = if local_error.is_empty() {
            &snapshot.status
        } else {
            &local_error
        };
        text.render(16.0, 42.0, status, font, &mut renderer, &mut ui)?;
        let selected = snapshot.selected.map_or_else(
            || "No conversation. Try /new AUTH_ID … or /list".into(),
            |id| {
                format!(
                    "Conversation: {id} | {} participants{}",
                    snapshot.members.len(),
                    if snapshot.active { "" } else { " (inactive)" }
                )
            },
        );
        text.render(16.0, 68.0, &selected, font, &mut renderer, &mut ui)?;
        let input_y = (height - 64.0).max(180.0);
        let notice_count = snapshot.notices.len().min(8);
        let history_bottom = (input_y - 12.0 - 24.0 * notice_count as f32).max(100.0);
        let layouts = snapshot
            .messages
            .iter()
            .map(|message| {
                text.measure_with_options(
                    message,
                    font,
                    TextLayoutOptions {
                        max_width: available,
                    },
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let total: f32 = layouts.iter().map(|layout| layout.height + 8.0).sum();
        scroll = scroll.min((total - (history_bottom - 100.0)).max(0.0));
        let mut y = history_bottom + scroll;
        for (message, layout) in snapshot.messages.iter().zip(&layouts).rev() {
            y -= layout.height + 8.0;
            if y < history_bottom {

                text.render_viewport(
                    ([16.0, y], [100.0, history_bottom], available),
                    message,
                    font,
                    &mut renderer,
                    &mut ui,
                )?;
            }
            if y + layout.height < 100.0 {
                break;
            }
        }
        for (i, notice) in snapshot
            .notices
            .iter()
            .rev()
            .take(notice_count)
            .rev()
            .enumerate()
        {
            text.render(
                16.0,
                history_bottom + 4.0 + i as f32 * 24.0,
                notice,
                font,
                &mut renderer,
                &mut ui,
            )?;
        }
        input.render(
            Bounds {
                x: 16.0,
                y: input_y,
                width: available,
                height: 48.0,
            },
            font,
            &mut text,
            &mut renderer,
            &mut ui,
        )?;
        ui.end(&mut renderer)?;
        renderer.end_frame()?;
    }
}
