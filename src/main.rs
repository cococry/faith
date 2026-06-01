
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

        for events in events {
            match events {
                WindowEvent::CloseRequested => {
                    running = false;
                }
                WindowEvent::RedrawRequested => {
                    renderer.clear_color(Color::rgba(1.0, 1.0, 1.0, 1.0));
                    renderer.begin_frame();
                    renderer.end_frame()?;
                }
                WindowEvent::Resized {width, height} => {
                    renderer.resize(width, height);
                }
                _ev => {
                }
            }
        }
    }
    Ok(())
}
