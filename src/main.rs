mod platform;
mod graphics;
mod cli;
mod ui;

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


    text.load_font("assets/NotoColorEmoji.ttf", 48)?;
    let base_font = text.load_font("assets/NotoSans-Regular.ttf", 48)?;
    text.load_font("assets/NotoSansArabic-Regular.ttf", 48)?;
    text.load_font("assets/NotoSansHebrew-Regular.ttf", 48)?;

    let hard_text = "Latin: office affine fluff ffi fj AVATAR — Arabic: السَّلامُ عَلَيْكُمْ مرحبًا بِالعالَمِ\n\
لا إلهَ إلّا اللهُ اللغةُ العربيّةُ جميلةٌ شكرًا جَزِيلًا لَا لَأ لِإ لآ ﷺ ؟،؛\n\
Hebrew: שָׁלוֹם עֲלֵיכֶם עברית מְנֻקֶּדֶת אֱלֹהִים תּוֹרָה ״שלום״\n\
Mixed RTL/LTR: abc مرحبًا 123 שָׁלוֹם DEF لا إلهَ إلّا اللهُ xyz\n\
Emoji: 😀 😄 🚀 ❤️ ♥️ ☺️ ✨ 🔥 🌍 ✅ 🥹 🫠 🫶 🫰 🫨\n\
ZWJ: 👨‍👩‍👧‍👦 👩‍❤️‍👨 👩‍❤️‍💋‍👨 🧑‍💻 👨🏿‍🚀 🧑🏽‍🦱 🧑🏽‍💻\n\
Flags/keycaps: 🇩🇪 🇺🇸 🇯🇵 🇸🇦 🇮🇱 1️⃣ 2️⃣ 3️⃣ #️⃣ *️⃣\n\
Skin tones: 👋🏻 👋🏼 👋🏽 👋🏾 👋🏿 🤌🏻 🫱🏾‍🫲🏼\n\
Fallback mix: Hello مرحبًا 😀 עברית 🇩🇪 flags ♥️❤️ keycaps 1️⃣2️⃣3️⃣ done ✅";

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
            renderer.begin_frame();

            let (width, height) = platform.size();

            ui.begin(width, height);
            
            text.render(
                20.0,
                20.0,
                hard_text,

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
