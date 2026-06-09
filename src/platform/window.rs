use super::event::WindowEvent;
use crate::{
    cli::PlatformBackend,
    platform::{wayland::WaylandWaker, x11::X11Waker},
};
use tracing::{info, warn};

/// Window creation configuration.
///
/// Specifies the platform backend,
/// title and initial window size.
pub struct WindowConfig {
    pub backend: PlatformBackend,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Platform-independent waker handle used
/// to request redraws.
#[derive(Clone)]
#[allow(dead_code)]
pub struct PlatformWaker {
    inner: PlatformWakerInner,
}

#[derive(Clone)]
#[allow(dead_code)]
enum PlatformWakerInner {
    X11(X11Waker),
    Wayland(WaylandWaker),
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            backend: PlatformBackend::Auto,
            title: "Faith Messenger".to_string(),
            width: 1280,
            height: 720,
        }
    }
}

/// Platform-specific window implementation.
pub enum Platform {
    X11(super::x11::X11Platform),
    Wayland(super::wayland::WaylandPlatform),
}

/// Native window handle information used by
/// graphics backends.
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

impl PlatformWaker {
    #[allow(dead_code)]
    pub fn request_redraw(&self) -> anyhow::Result<()> {
        match &self.inner {
            PlatformWakerInner::X11(waker) => waker.request_redraw(),
            PlatformWakerInner::Wayland(waker) => waker.request_redraw(),
        }
    }
}

impl Platform {
    /// Creates a new platform window using the
    /// configured backend.
    ///
    /// Automatically chooses a supported backend
    /// when WindowConfig.backend is PlatformBackend::Auto.
    /// Automatic choosing prefers Wayland and falls
    /// back to X11.
    pub fn new(config: &WindowConfig) -> anyhow::Result<Self> {
        info!(
            "Window '{0}' at {1}x{2}",
            config.title, config.width, config.height
        );
        match config.backend {
            PlatformBackend::Auto => Self::new_auto(config),

            PlatformBackend::Wayland => {
                info!("using Wayland backend");
                Ok(Self::Wayland(super::wayland::WaylandPlatform::new(config)?))
            }

            PlatformBackend::X11 => {
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

    /// Polls and collects pending window events
    /// from the active platform backend.
    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        match self {
            Self::X11(platform) => platform.poll_events(),
            Self::Wayland(platform) => platform.poll_events(),
        }
    }

    /// Gets the native window handle from the
    /// active platform backend.
    pub fn native_handle(&self) -> WindowHandleInfo {
        match self {
            Self::X11(platform) => platform.native_handle(),
            Self::Wayland(platform) => platform.native_handle(),
        }
    }

    /// Gets the size of the active platform window
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::X11(platform) => platform.size(),
            Self::Wayland(platform) => platform.size(),
        }
    }

    /// Requests a redraw event to be sent by
    /// the active platform window (from a seperate
    /// thread / from outside the event loop).
    #[allow(dead_code)]
    pub fn request_redraw(&self) -> anyhow::Result<()> {
        self.waker().request_redraw()
    }

    /// Creates a platform-independent waker for
    /// requesting redraws from outside the active
    /// platform event loop.
    #[allow(dead_code)]
    pub fn waker(&self) -> PlatformWaker {
        match self {
            Platform::X11(platform) => PlatformWaker {
                inner: PlatformWakerInner::X11(platform.waker()),
            },

            Platform::Wayland(platform) => PlatformWaker {
                inner: PlatformWakerInner::Wayland(platform.waker()),
            },
        }
    }

    pub fn create_vulkan_surface(
        &self,
        entry: &ash::Entry,
        instance: &ash::Instance,
        have_ext_vk_khr_xcb_surface: bool,
        have_ext_vk_khr_wayland_surface: bool,
    ) -> anyhow::Result<ash::vk::SurfaceKHR> {
        match self {
            Self::X11(platform) => platform.create_vulkan_surface(
                entry,
                instance,
                have_ext_vk_khr_xcb_surface,
                have_ext_vk_khr_wayland_surface,
            ),
            Self::Wayland(platform) => platform.create_vulkan_surface(
                entry,
                instance,
                have_ext_vk_khr_xcb_surface,
                have_ext_vk_khr_wayland_surface,
            ),
        }
    }
}
