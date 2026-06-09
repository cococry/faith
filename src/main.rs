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

use crate::{graphics::{Color, Renderer}, ui::UIRenderer};

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
        backend:    cli.platform,
        title: WindowConfig::default().title,
        width: cli.width,
        height: cli.height,
    };

    let mut platform    = Platform::new(&conf)?;
    let mut renderer    = Renderer::new(cli.graphics.into(), &platform)?;
    let mut ui          = UIRenderer::new(&mut renderer, conf.width, conf.height)?;
    let mut text        = TextRenderer::new()?; 


    text.load_font("assets/NotoColorEmoji.ttf", 24)?;
    let base_font = text.load_font("assets/NotoSans-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansArabic-Regular.ttf", 24)?;
    text.load_font("assets/NotoSansHebrew-Regular.ttf", 24)?;
    let mixed_hebrew_arabic = "تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه. تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه. تَتَحَدَّث اَلْأَبْيَات عَنْ لَحْظَة وَدَاع يَسْتَغْرِب فِيهَا اَلشَّاعِر أَنْ لَا يَبْكِي مِنْ أَلَم اَلْفِرَاق، وَيَصِف حَالَة اَلْمُودِعِينَ وَبَعْضهمْ يَتَكَلَّم فِي حِين يَكْتَفِي اَلْمُحِبُّونَ بِالصَّمْتِ، لِأَنَّ حَالهمْ تَظْهَر عِشْقهمْ أَكْثَر مِمَّا يَسْتَطِيعُونَ اَلتَّعْبِير عَنْهُ بِالْكَلَامِ. وَيُقَسِّم فِي آخَر اَلْأَبْيَات عَلَى أَنَّ تَوَقُّف دَمْعه لَا يَعْنِي نِهَايَة حَيّه."; 

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

            let img_w  = img.size[0] as f32 / 5.0;
            let img_h = img.size[1] as f32 / 5.0;
            ui.image(20.0, 20.0, img_w, img_h, img)?;
            let layout = text.render_wrapped(
                20.0,
                img_h + 20.0 + 20.0,
                mixed_hebrew_arabic,
                base_font,
                width as f32 - 40.0,
                &mut renderer,
                &mut ui,
            )?;

            text.render_wrapped(
                20.0,
                img_h + 20.0 + 20.0 + layout.height,
                "Lorem ipsum dolor sit amet, consectetuer adipiscing elit. Aenean commodo ligula eget dolor. Aenean massa. Cum sociis natoque penatibus et magnis dis parturient montes, nascetur ridiculus mus. Donec quam felis, ultricies nec, pellentesque eu, pretium quis, sem. Nulla consequat massa quis enim. Donec pede justo, fringilla vel, aliquet nec, vulputate eget, arcu. In enim justo, rhoncus ut, imperdiet a, venenatis vitae, justo. Nullam dictum felis eu pede mollis pretium. Integer tincidunt. Cras dapibus. Vivamus elementum semper nisi. Aenean vulputate eleifend tellus. Aenean leo ligula, porttitor eu, consequat vitae, eleifend ac, enim. Aliquam lorem ante, dapibus in, viverra quis, feugiat a, tellus. Phasellus viverra nulla ut metus varius laoreet. Quisque rutrum. Aenean imperdiet. Etiam ultricies nisi vel augue. Curabitur ullamcorper ultricies nisi. Nam eget dui. Etiam rhoncus. Maecenas tempus, tellus eget condimentum rhoncus, sem quam semper libero, sit amet adipiscing sem neque sed ipsum. Nam quam nunc, blandit vel, luctus pulvinar, hendrerit id, lorem. Maecenas nec odio et ante tincidunt tempus. Donec vitae sapien ut libero venenatis faucibus. Nullam quis ante. Etiam sit amet orci eget eros faucibus tincidunt. Duis leo. Sed fringilla mauris sit amet nibh. Donec sodales sagittis magna. Sed consequat, leo eget bibendum sodales, augue velit cursus nunc,",
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
