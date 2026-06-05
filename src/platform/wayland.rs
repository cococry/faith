// Implementation following: https://github.com/Smithay/wayland-rs/blob/master/wayland-client/examples/simple_window.rs
//
use std::{ffi::c_void, os::fd::AsFd, ptr, sync::Arc};

use crate::platform::window::WindowHandleInfo;
use super::event::WindowEvent;
use super::window::WindowConfig;

use ash::vk::WaylandSurfaceCreateInfoKHR;
use nix::{poll::{PollFd, PollFlags, PollTimeout, poll}, sys::eventfd::{EfdFlags, EventFd}};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, protocol::{wl_compositor, wl_registry, wl_surface}
};

use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use nix::unistd::read;

/// Wayland window state.
///
/// Stores the current Wayland objects & state,
/// as well as pending events to be collected by 
/// Wayland protocol handler callbacks.
pub struct WaylandState {
    width: u32,
    height: u32,
    title: String,

    surface: Option<wl_surface::WlSurface>, 
    wm_base: Option<xdg_wm_base::XdgWmBase>, 
    xdg_surface: Option<xdg_surface::XdgSurface>, 
    toplevel: Option<xdg_toplevel::XdgToplevel>, 
    egl_win: Option<wayland_egl::WlEglSurface>,

    // Whether the initial xdg_surface configure 
    // event has been received and acknowledged.
    configured: bool,

    pending_events: Vec<WindowEvent>,
    pending_resize: Option<(u32, u32)>,
}

/// Wayland window platform implementation.
///
/// Owns the Wayland connection, event queue, 
/// window state and wake file descriptor used 
/// to drive the window event loop.
pub struct WaylandPlatform {
    conn: Connection,
    ev_queue: EventQueue<WaylandState>,
    state: WaylandState,

    // Eventfd used to wake the blocking Wayland 
    // event poll from another thread.
    wake_fd: Arc<EventFd>
}

/// Marker type used as user data for Wayland 
/// protocol dispatch handlers. (thread safe)
#[derive(Clone, Copy, Debug)]
struct WaylandHandler;


/// Waker handle for requesting redraws from 
/// outside the Wayland event loop.
#[derive(Clone)]
pub struct WaylandWaker {
    wake_fd: Arc<EventFd>,
}


impl WaylandWaker {
    // Unix implementation to request a redraw in the 
    // Wayland platform window.
    //
    // Uses nix::unistd::write to wake up the self.wake_fd 
    // file descriptor.

    pub fn request_redraw(&self) -> anyhow::Result<()> {
        let val: u64 = 1;
        let bytes = val.to_ne_bytes();

        match nix::unistd::write(&*self.wake_fd, &bytes) {
            Ok(_) => Ok(()),

            Err(nix::errno::Errno::EAGAIN) => Ok(()),

            Err(err) => Err(err.into()),
        }
    }
}

impl WaylandState {
    /// Called when the XDG surface is initialized.
    /// At this point, we can set the tile of the toplevel.
    fn init_xdg_surface(&mut self, qh: &QueueHandle<WaylandState>) {
        let wm_base = self.wm_base.as_ref().unwrap();
        let base_surface = self.surface.as_ref().unwrap();

        let xdg_surface = wm_base.get_xdg_surface(base_surface, qh, WaylandHandler);
        let toplevel = xdg_surface.get_toplevel(qh, WaylandHandler);
        toplevel.set_title(self.title.clone());

        base_surface.commit();

        self.xdg_surface = Some(xdg_surface); 
        self.toplevel = Some(toplevel); 
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, WaylandHandler> for WaylandState {
    fn event(
        _state: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &WaylandHandler,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // We need to respond to a 'ping' with a 'pong' 
        // in order to make the wayland compositor know 
        // that our window is still active.
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, WaylandHandler> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &WaylandHandler,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_compositor" => {
                    let compositor = registry.bind::<wl_compositor::WlCompositor, _, _>(
                        name,
                        4,
                        qh,
                        WaylandHandler,
                    );

                    let surface     = compositor.create_surface(qh, WaylandHandler);
                    state.surface   = Some(surface);

                    // Call our init handler when the xdg surface has spawned 
                    if state.wm_base.is_some() && state.xdg_surface.is_none() {
                        state.init_xdg_surface(qh);
                    }
                }

                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(
                        name,
                        1,
                        qh,
                        WaylandHandler,
                    );

                    state.wm_base = Some(wm_base);

                    // Call our init handler when the xdg surface has spawned 
                    if state.surface.is_some() && state.xdg_surface.is_none() {
                        state.init_xdg_surface(qh);
                    }
                }

                _ => {}
            }
        }
    }
}

/// Not needed
impl Dispatch<wl_compositor::WlCompositor, WaylandHandler> for WaylandState {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &WaylandHandler,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Not needed
impl Dispatch<wl_surface::WlSurface, WaylandHandler> for WaylandState {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &WaylandHandler,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_surface::XdgSurface, WaylandHandler> for WaylandState {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &WaylandHandler,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);

            if state.egl_win.is_none() {
                let surface = state.surface.as_ref().unwrap();

                // We need to create an EGL window for OpenGL/EGL 
                // interop on Wayland.
                let egl_win = wayland_egl::WlEglSurface::new(
                    surface.id(),
                    state.width as i32,
                    state.height as i32,
                )
                    .expect("failed to create wl_egl_window");

                state.egl_win = Some(egl_win);
            }

            if let Some((width, height)) = state.pending_resize.take() {
                if let Some(egl_win) = state.egl_win.as_mut() {
                    egl_win.resize(width as i32, height as i32, 0, 0);
                }

                state.pending_events.push(WindowEvent::Resized { width, height });
            }

            state.configured = true;
            state.pending_events.push(WindowEvent::RedrawRequested);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, WaylandHandler> for WaylandState {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &WaylandHandler,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Close => {
                // respond to close event 
                state.pending_events.push(WindowEvent::CloseRequested);
            }

            xdg_toplevel::Event::Configure { width, height, .. } => {
                // respond to configure event.
                // basically a resize event
                if width > 0 && height > 0 {
                    state.width = width as u32;
                    state.height = height as u32;
                    state.pending_resize = Some((width as u32, height as u32));
                }
            }

            _ => {}
        }
    }
}

impl WaylandPlatform {
    pub fn new(config: &WindowConfig) -> anyhow::Result<Self> {
        let conn = Connection::connect_to_env()?;
        let mut ev_queue = conn.new_event_queue::<WaylandState>();
        let handle = ev_queue.handle();


        let mut state = WaylandState {
            width: config.width, 
            height: config.height, 
            title: config.title.clone(),
            surface: None,
            egl_win: None,
            wm_base: None,
            xdg_surface: None,
            toplevel: None,
            configured: false,
            pending_events: Vec::new(),
            pending_resize: None,
        };

        conn.display().get_registry(&handle, WaylandHandler); 

        ev_queue.roundtrip(&mut state)?;

        while !state.configured {
            ev_queue.blocking_dispatch(&mut state)?;
        }
        let wake_fd = Arc::new(EventFd::from_value_and_flags(
            0,
            EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC,
        )?);


        Ok(Self {
            conn,
            ev_queue,
            state,
            wake_fd
        })
    }

    fn drain_wake_fd(&self) -> anyhow::Result<()> {
        let mut buf = [0u8; 8];

        loop {
            match read(&self.wake_fd, &mut buf) {
                Ok(_) => {
                    continue;
                }

                Err(nix::errno::Errno::EAGAIN) => {
                    break;
                }

                Err(err) => {
                    return Err(err.into());
                }
            }
        }

        Ok(())
    }


    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {

        // Collect any already queued events 
        while self.ev_queue.dispatch_pending(&mut self.state)? > 0 {}

        // If there are still any queued/pending events, 
        // do not block yet and return.
        if !self.state.pending_events.is_empty() {
            return Ok(std::mem::take(&mut self.state.pending_events));
        }


        self.conn.flush()?;

        // prepare_read() returns None when events became pending between
        // dispatch_pending() and prepare_read(). In that case, dispatch again.
        let guard = loop {
            if let Some(guard) = self.conn.prepare_read() {
                break guard;
            }

            while self.ev_queue.dispatch_pending(&mut self.state)? > 0 {}
            self.conn.flush()?;
        };

        // Get Wayland FD
        let wl_fd = guard.connection_fd();

        // Set up FDs to poll on. Those being 
        // the Wayland FD and the wake FD.
        let mut poll_fds = [
            PollFd::new(wl_fd, PollFlags::POLLIN),
            PollFd::new(self.wake_fd.as_fd(), PollFlags::POLLIN),
        ];

        // Poll on the file descriptors
        poll(&mut poll_fds, PollTimeout::NONE)?;

        // Repond to potential events on either 
        // of the file descriptors 

        let wake_readable = poll_fds[1]
            .revents()
            .unwrap_or(PollFlags::empty())
            .contains(PollFlags::POLLIN);

        let wl_readable = poll_fds[0]
            .revents()
            .unwrap_or(PollFlags::empty())
            .contains(PollFlags::POLLIN);

        if wl_readable {
            guard.read()?;
        } else {
            // Dropping the guard cancels the prepared read.
            drop(guard);
        }


        // Respond to a requested redraw event
        if wake_readable {
            self.drain_wake_fd()?;
            self.state
                .pending_events
                .push(WindowEvent::RedrawRequested);
        }

        // respond to pending Wayland events
        while self.ev_queue.dispatch_pending(&mut self.state)? > 0 {}

        self.conn.flush()?;

        Ok(std::mem::take(&mut self.state.pending_events))

    }

    /// Returns the current Wayland window size.
    pub fn size(&self) -> (u32, u32) {
        (self.state.width, self.state.height)
    }

    /// Returns the native Wayland window handle 
    /// information used by graphics backends.
    pub fn native_handle(&self) -> WindowHandleInfo {
        WindowHandleInfo::Wayland {
            display: self.conn.backend().display_ptr() as *mut c_void, 
            egl_win: self.state.egl_win 
                .as_ref()
                .expect("Missing wl_egl_window")
                .ptr() as *mut c_void
        }
    }

    /// Creates a waker handle that can request a 
    /// redraw from outside the Wayland event loop.
    pub fn waker(&self) -> WaylandWaker {
        WaylandWaker {
            wake_fd: self.wake_fd.clone(),
        }
    }

    pub fn create_vulkan_surface(
        &self,
        entry: &ash::Entry,
        instance: &ash::Instance, 
        _have_ext_vk_khr_xcb_surface: bool,
        have_ext_vk_khr_wayland_surface: bool,
    ) -> anyhow::Result<ash::vk::SurfaceKHR> {
        if !have_ext_vk_khr_wayland_surface {
            anyhow::bail!("Wayland: Vulkan instance missing VK_KHR_wayland_surface extension")
        } else {
            let raw_surf_ptr = self.state.surface.clone().unwrap().id().as_ptr() as *mut c_void;
           
            let create_info: WaylandSurfaceCreateInfoKHR = WaylandSurfaceCreateInfoKHR {
                display: self.conn.backend().display_ptr() as *mut c_void,
                surface: raw_surf_ptr, 
                ..Default::default()
            };

            let wl_surface_inst = ash::khr::wayland_surface::Instance::new(entry, instance); 

            unsafe {
                Ok(wl_surface_inst.create_wayland_surface(&create_info, None)?)
            }

        }
    }

}
