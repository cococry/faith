mod cli;
mod graphics;
mod platform;
mod ui;

use crate::{graphics::GraphicsDevice, ui::TextRenderer};

use clap::Parser;
use cli::Cli;

use platform::{Platform, WindowConfig, WindowEvent};
use tracing_subscriber::EnvFilter;

use crate::{
    graphics::{Color, Renderer},
    ui::UIRenderer,
};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("faith=info")),
        )
        .init();

    tracing::info!("Starting Faith client...");

    let cli = Cli::parse();

    let conf = WindowConfig {
        backend: cli.platform,
        title: WindowConfig::default().title,
        width: cli.width,
        height: cli.height,
    };

    let mut platform = Platform::new(&conf)?;
    let mut renderer = Renderer::new(cli.graphics.into(), &platform)?;
    let mut ui = UIRenderer::new(&mut renderer, conf.width, conf.height)?;
    let mut text = TextRenderer::new()?;

    let base_font = text.load_font("assets/NotoSans-Regular.ttf", 24)?;
    let bold_font = text.load_font("assets/NotoSans-Bold.ttf", 24)?;

    text.load_font("assets/NotoColorEmoji.ttf", 24)?;
    text.load_font("assets/NotoSansArabic-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansHebrew-Regular.ttf", 24)?;

    let mut running = true;

    while running {
        let events = platform.poll_events()?;

        let mut should_redraw = false;

        for event in events {
            match event {
                WindowEvent::CloseRequested => {
                    running = false;
                }

                WindowEvent::Resized { width, height } => {
                    renderer.resize(width, height);
                }

                WindowEvent::RedrawRequested => {
                    should_redraw = true;
                }

                _ => {}
            }
        }

        if running && should_redraw {
            renderer.clear_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
            renderer.begin_frame()?;

            let (width, height) = platform.size();

            ui.begin(width, height);

            text.render(
                50.0,
                50.0,
                "Faith Instant Messaging",
                bold_font,
                &mut renderer,
                &mut ui,
            )?;

            let dim1 = text.measure("❌️", base_font)?;
            text.render(
                width as f32 - dim1.width - 50.0,
                50.0,
                "❌️",
                base_font,
                &mut renderer,
                &mut ui,
            )?;

            let dim = text.measure("Send message", base_font)?;
            println!("{}x{}", dim.width, dim.height);
            text.render(
                (width as f32 - dim.width) / 2.0,
                height as f32 - dim.height - 50.0,
                "Send message",
                base_font,
                &mut renderer,
                &mut ui,
            )?;

            ui.end(&mut renderer)?;
            renderer.end_frame()?;
        }
    }
    Ok(())
}
