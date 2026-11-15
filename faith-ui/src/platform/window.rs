use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use super::{event::WindowEvent, mouse::Mouse};
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
    timing: FrameTiming,
}

#[derive(Clone)]
#[allow(dead_code)]
enum PlatformWakerInner {
    X11(X11Waker),
    Wayland(WaylandWaker),
}

#[derive(Clone)]
pub(crate) struct FrameTiming(Arc<AtomicU64>);

impl Default for FrameTiming {
    fn default() -> Self {
        // Leave timing unresolved until the platform reports a refresh rate
        // or an animation actually needs the fallback interval.
        Self(Arc::new(AtomicU64::new(0)))
    }
}

impl FrameTiming {
    pub fn set_refresh_millihz(&self, refresh: u64) {
        if let Some(interval) = 1_000_000_000_000u64.checked_div(refresh) {
            self.0.store(interval, Ordering::Relaxed);
        }
    }

    fn interval(&self) -> Duration {
        let mut interval = self.0.load(Ordering::Relaxed);
        if interval == 0 {
            const FALLBACK: u64 = 1_000_000_000 / 60;
            interval = match self.0.compare_exchange(
                0,
                FALLBACK,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    warn!(
                        "Monitor refresh rate unavailable; falling back to 60 Hz animation timing"
                    );
                    FALLBACK
                }
                Err(detected) => detected,
            };
        }
        Duration::from_nanos(interval.max(1))
    }
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
    X11(Box<super::x11::X11Platform>),
    Wayland(Box<super::wayland::WaylandPlatform>),
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
    pub fn frame_interval(&self) -> Duration {
        self.timing.interval()
    }

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
                Ok(Self::Wayland(Box::new(
                    super::wayland::WaylandPlatform::new(config)?,
                )))
            }

            PlatformBackend::X11 => {
                info!("using X11 backend");
                Ok(Self::X11(Box::new(super::x11::X11Platform::new(config)?)))
            }
        }
    }

    fn new_auto(config: &WindowConfig) -> anyhow::Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            match super::wayland::WaylandPlatform::new(config) {
                Ok(platform) => {
                    info!("using Wayland backend");
                    return Ok(Self::Wayland(Box::new(platform)));
                }

                Err(err) => {
                    warn!("Wayland windowing failed, falling back to X11: {err}");
                }
            }
        }

        info!("using X11 backend");
        Ok(Self::X11(Box::new(super::x11::X11Platform::new(config)?)))
    }

    /// Polls and collects pending window events
    /// from the active platform backend.
    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        let events = match self {
            Self::X11(platform) => platform.poll_events()?,
            Self::Wayland(platform) => platform.poll_events()?,
        };

        let mouse = match self {
            Self::X11(platform) => &mut platform.mouse,
            Self::Wayland(platform) => &mut platform.mouse,
        };

        mouse.begin_frame();

        for event in &events {
            mouse.handle(event);
        }

        Ok(events)
    }

    /// Pointer snapshot for the most recent poll_events() batch.
    #[allow(dead_code)]
    pub fn mouse(&self) -> &Mouse {
        match self {
            Self::X11(platform) => &platform.mouse,
            Self::Wayland(platform) => &platform.mouse,
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

    pub fn scale_factor(&self) -> f32 {
        match self {
            Self::X11(platform) => platform.scale_factor(),
            Self::Wayland(platform) => platform.scale_factor(),
        }
    }

    /// Converts native pointer coordinates to framebuffer pixels.
    pub fn pointer_scale(&self) -> f32 {
        match self {
            Self::X11(_) => 1.0,
            Self::Wayland(_) => self.scale_factor(),
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
                timing: platform.frame_timing.clone(),
            },

            Platform::Wayland(platform) => PlatformWaker {
                inner: PlatformWakerInner::Wayland(platform.waker()),
                timing: platform.frame_timing().clone(),
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
