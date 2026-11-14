mod cli;
mod client;
mod graphics;
mod platform;
mod ui;

use clap::Parser;
use graphics::{Color, GraphicsDevice, Renderer};
use platform::{Platform, WindowConfig, WindowEvent};
use ui::framework::{Font, Fonts, Framework, Style};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cli = cli::Cli::parse();
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

    let mut submitted_handle: Option<String> = None;
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
        let width = (ui.div_width() - 32.0).clamp(1.0, 440.0);
        ui.set_position(
            (ui.div_width() - width) / 2.0,
            ((ui.div_height() - 300.0) / 2.0).max(0.0),
        );
        ui.push_style(Style {
            padding: 24.0,
            ..ui.style()
        });
        ui.div_begin(width, 300.0, false)?;
        ui.push_style(Style {
            font_size: 28,
            margin: 8.0,
            ..ui.style()
        });

        ui.text(&mut renderer, "Your Account", Some(Font::Bold))?;
        ui.pop_style();
        ui.push_style(Style {
            font_size: 14,
            margin: 8.0,
            color: Color::rgba(0.4, 0.43, 0.48, 1.0),
            ..ui.style()
        });
        ui.text(&mut renderer, "Handle", None)?;
        ui.pop_style();
        ui.push_style(Style {
            padding: 10.0,
            margin: 8.0,
            border_color: Some(Color::rgba(0.7, 0.73, 0.78, 1.0)),
            ..ui.style()
        });
        let handle = ui.input(&mut renderer, "handle")?;

        ui.pop_style();
        ui.push_style(Style {
            padding: 12.0,
            margin: 8.0,
            background: Some(Color::rgba(0.83, 0.88, 1.0, 1.0)),
            ..ui.style()
        });
        if ui.button(&mut renderer, "Done")? && !handle.trim().is_empty() {
            submitted_handle = Some(handle);
        }
        ui.pop_style();
        if let Some(handle) = &submitted_handle {
            ui.text(&mut renderer, &format!("Welcome, {handle}"), None)?;
        }
        ui.div_end();
        ui.pop_style();
        ui.end(&mut renderer)?;
        renderer.end_frame()?;
    }
}
