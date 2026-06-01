use super::event::WindowEvent;
use crate::cli::Backend;
use tracing::{info, warn};

pub struct WindowConfig {
    pub backend: Backend, 
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            title: "Faith Messenger".to_string(),
            width: 1280, 
            height: 720, 
        }
    }
}

pub enum Platform {
    X11(super::x11::X11Platform),
    Wayland(super::wayland::WaylandPlatform)
}

pub enum WindowHandleInfo {
    X11 {
        display: *mut std::ffi::c_void,
        window: u64,
    },
    Wayland { 
        display: *mut std::ffi::c_void,
        egl_win: *mut std::ffi::c_void,
    },
}

impl Platform {
    pub fn new(config: &WindowConfig) -> anyhow::Result<Self> {
        info!("Window '{0}' at {1}x{2}", config.title, config.width, config.height);
        match config.backend {
            Backend::Auto => Self::new_auto(config),

            Backend::Wayland => {
                info!("using Wayland backend");
                Ok(Self::Wayland(super::wayland::WaylandPlatform::new(config)?))
            }

            Backend::X11 => {
                info!("using X11 backend");
                Ok(Self::X11(super::x11::X11Platform::new(config)?))
            }
        }
    }

    fn new_auto(config: &WindowConfig) -> anyhow::Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            match super::wayland::WaylandPlatform::new(config) {
                Ok(platform) => {
                    info!("using Wayland backend");
                    return Ok(Self::Wayland(platform));
                }
                Err(err) => {
                    warn!("Wayland windowing failed, falling back to X11: {err}");
                }
            }
        }

        info!("using X11 backend");
        Ok(Self::X11(super::x11::X11Platform::new(config)?))
    } 

    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        match self {
            Self::X11(platform) => platform.poll_events(),
            Self::Wayland(platform) => platform.poll_events()
        }
    }

    pub fn native_handle(&self) -> WindowHandleInfo {
        match self {
            Self::X11(platform) => platform.native_handle(),
            Self::Wayland(platform) => platform.native_handle()
        }
    }

    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::X11(platform) => platform.size(),
            Self::Wayland(platform) => platform.size()
        }
    }
}
