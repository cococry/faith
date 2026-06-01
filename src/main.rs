
mod platform;
mod graphics;

use platform::{Platform, WindowConfig, WindowEvent};

use crate::graphics::{GraphicsBackend, Renderer, Color};


fn main() -> anyhow::Result<()> { 
    let conf = WindowConfig::default(); 

    let mut platform = Platform::new(&conf)?;

    let mut renderer = Renderer::new(GraphicsBackend::OpenGL, &platform)?;

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
            renderer.end_frame()?;
        }
    }
    Ok(())
}
