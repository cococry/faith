use std::ffi::c_void;

use crate::platform::window::WindowHandleInfo;
use super::event::WindowEvent;
use super::window::WindowConfig;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, protocol::{wl_compositor, wl_registry, wl_surface}
};

use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};


pub struct WaylandState {
    running: bool,
    width: u32,
    height: u32,
    title: String,

    surface: Option<wl_surface::WlSurface>, 
    wm_base: Option<xdg_wm_base::XdgWmBase>, 
    xdg_surface: Option<xdg_surface::XdgSurface>, 
    toplevel: Option<xdg_toplevel::XdgToplevel>, 
    egl_win: Option<wayland_egl::WlEglSurface>,
    configured: bool,

    pending_events: Vec<WindowEvent>,
    pending_resize: Option<(u32, u32)>,
}

pub struct WaylandPlatform {
    conn: Connection,
    ev_queue: EventQueue<WaylandState>,
    state: WaylandState
}

#[derive(Clone, Copy, Debug)]
struct WaylandHandler;

impl WaylandState {
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

                    let surface = compositor.create_surface(qh, WaylandHandler);

                    state.surface = Some(surface);


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

                    if state.surface.is_some() && state.xdg_surface.is_none() {
                        state.init_xdg_surface(qh);
                    }
                }

                _ => {}
            }
        }
    }
}

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
                state.running = false;
                state.pending_events.push(WindowEvent::CloseRequested);
            }

            xdg_toplevel::Event::Configure { width, height, .. } => {
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
            running: true, 
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
        Ok(Self {
            conn,
            ev_queue,
            state
        })
    }

    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        while self.state.pending_events.is_empty() && self.state.running {
            self.ev_queue.blocking_dispatch(&mut self.state)?;

            while self.ev_queue.dispatch_pending(&mut self.state)? > 0 {}

            self.conn.flush()?;
        }

        Ok(std::mem::take(&mut self.state.pending_events))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.state.width, self.state.height)
    }

    pub fn native_handle(&self) -> WindowHandleInfo {
        WindowHandleInfo::Wayland {
            display: self.conn.backend().display_ptr() as *mut c_void, 
            egl_win: self.state.egl_win 
                .as_ref()
                .expect("Missing wl_egl_window")
                .ptr() as *mut c_void
        }
    }
}
