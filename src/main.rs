mod platform;
mod graphics;
mod cli;
mod ui;

use std::time::Instant;

use crate::{graphics::{GraphicsDevice}, ui::TextRenderer};

use clap::Parser;
use cli::Cli;

use platform::{Platform, WindowConfig, WindowEvent};
use tracing_subscriber::EnvFilter;

use crate::{graphics::{Color, GraphicsBackend, Renderer}, ui::UIRenderer};

fn main() -> anyhow::Result<()> { 
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("faith=info")),
        )
        .init();

    tracing::info!("Starting Faith client...");

    let cli = Cli::parse();

    let conf = WindowConfig{
        backend:    cli.backend,
        title:      WindowConfig::default().title,
        width:      WindowConfig::default().width,
        height:     WindowConfig::default().height,
    };

    let mut platform    = Platform::new(&conf)?;
    let mut renderer    = Renderer::new(GraphicsBackend::OpenGL, &platform)?;
    let mut ui          = UIRenderer::new(&mut renderer, conf.width, conf.height)?;
    let mut text        = TextRenderer::new()?; 


    text.load_font("assets/NotoColorEmoji.ttf", 24)?;
    let base_font = text.load_font("assets/NotoSans-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansArabic-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansHebrew-Regular.ttf", 24)?;
    let mixed_hebrew_arabic = "Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct طريق 👩🏽‍💻 engine كبير 🇩🇪 cache صغير 🏳️‍🌈 shape سريع 😂 line هادئ 🥲 wrap موسيقى 🚀 font قهوة ✨ Hello مرحبا 😀 world العربية ❤️ cafe العالم 👍🏽 naive السَّلامُ 👩🏽‍💻 render عَلَيْكُمْ 🇩🇪 layout صباح 🏳️‍🌈 glyph نور 😂 emoji ليلة 🥲 Latin جميلة 🚀 test قلب ✨ mixed كتاب 😀 text مدينة ❤️ fast صديق 👍🏽 correct"; 
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
            let start = Instant::now();
            renderer.clear_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
            renderer.begin_frame();

            let (width, height) = platform.size();

            ui.begin(width, height);
      
            


            text.render_wrapped(
                20.0,
                20.0,
                mixed_hebrew_arabic,
                base_font,
                width as f32 - 40.0,
                &mut renderer,
                &mut ui,
            )?;


            let elapsed = start.elapsed();

            println!("rendering took: {:.3} ms", elapsed.as_secs_f64() * 1000.0);

            ui.end(&mut renderer)?;

            renderer.end_frame()?;
        }
    }
    Ok(())
}
