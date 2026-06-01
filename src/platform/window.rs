use super::event::WindowEvent;

pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { 
            title: "Messenger".to_string(),
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
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            match super::wayland::WaylandPlatform::new(config) {
                Ok(platform) => return Ok(Self::Wayland(platform)),
                Err(err) => eprintln!("Wayland windowing failed, failling back to X11: {err}") 
            }
        }

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
