mod cli;
mod graphics;
mod platform;
mod ui;

use std::time::Instant;

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

    text.load_font("assets/NotoColorEmoji.ttf", 24)?;
    let base_font = text.load_font("assets/NotoSans-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansArabic-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansHebrew-Regular.ttf", 24)?;
    let mixed_hebrew_arabic = "Lorem ipsum dolor eris meris keris 😀😎🚀🌟🎉🔥🍕🐱🌈💡  تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه. تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه. تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه.";

    let img = ui.load_image(&mut renderer, "assets/large.png")?;

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

            let layout_start = Instant::now();

            ui.begin(width, height);

            let img_w = img.size[0] as f32 / 5.0;
            let img_h = img.size[1] as f32 / 5.0;
            ui.image(20.0, 20.0, img_w, img_h, img)?;
            let layout = text.render_wrapped(
                [20.0, img_h + 20.0 + 20.0],
                mixed_hebrew_arabic,
                base_font,
                width as f32 - 40.0,
                &mut renderer,
                &mut ui,
            )?;

            text.render_wrapped(
                [20.0, img_h + 20.0 + 20.0 + layout.height],
                &format!(
                    "Lorem ipsum dolor. The text above is {}x{}px large.",
                    layout.width as u32, layout.height as u32
                ),
                base_font,
                width as f32 - 40.0,
                &mut renderer,
                &mut ui,
            )?;

            let layout_time = layout_start.elapsed();

            let submit_start = Instant::now();

            ui.end(&mut renderer)?;
            renderer.end_frame()?;

            let submit_time = submit_start.elapsed();

            println!(
                "text/ui build: {:.3} ms | backend submit: {:.3} ms",
                layout_time.as_secs_f64() * 1000.0,
                submit_time.as_secs_f64() * 1000.0,
            );
        }
    }
    Ok(())
}
