use crate::platform::wayland::WaylandWaker;
use crate::platform::window::WindowHandleInfo;

use super::event::WindowEvent;
use super::window::WindowConfig;

extern crate x11;
extern crate x11rb;
extern crate anyhow;

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use nix::unistd::write;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, CreateWindowAux, EventMask, PixmapEnum, PropMode, WindowClass
};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;
use x11rb::COPY_DEPTH_FROM_PARENT;

use x11::xlib::{_XDisplay, XDefaultScreen, XOpenDisplay};
use x11::xlib_xcb::{XGetXCBConnection, XSetEventQueueOwner, XEventQueueOwner::XCBOwnsEventQueue};
use nix::sys::eventfd::{EfdFlags, EventFd};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::unistd::read;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};



pub struct X11Platform {
    width: u32,
    height: u32,

    xdisplay: NonNull<_XDisplay>, 

    xcb_conn: XCBConnection,
    window: u32,
    wm_delete_window: u32,
    wake_fd: Arc<EventFd> 
}


#[derive(Clone)]
pub struct X11Waker {
    wake_fd: Arc<EventFd>,
}
impl X11Waker {
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


impl X11Platform {
    pub fn new(window_config: &WindowConfig) -> anyhow::Result<Self> {
        let conn: XCBConnection; 
        let screen_num: i32;
        
        let raw_display = unsafe { XOpenDisplay(ptr::null()) };
        let xdisplay = NonNull::new(raw_display)
            .ok_or_else(|| anyhow::anyhow!("Failed to open X display"))?;
        unsafe {
            XSetEventQueueOwner(xdisplay.as_ptr(), XCBOwnsEventQueue);

            let conn_ptr = XGetXCBConnection(xdisplay.as_ptr());
            if conn_ptr.is_null() {
                anyhow::bail!("Failed to get XCB connection pointer from X display.");
            }
            
            screen_num = XDefaultScreen(xdisplay.as_ptr());

            conn = XCBConnection::from_raw_xcb_connection(conn_ptr, false)
                .expect("Failed to create x11rb connection from raw connection pointer");
        }

        let setup = conn.setup();
        let screen = setup.roots.iter().nth(screen_num as usize).
            expect("Failed to find the default screen in XCB setup roots");

        let win_id = conn.generate_id()?;

        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win_id,
            screen.root,
            0,
            0,
            window_config.width as u16,
            window_config.height as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
            .background_pixmap(PixmapEnum::NONE)
            .event_mask(
                EventMask::EXPOSURE
                | EventMask::STRUCTURE_NOTIFY
                | EventMask::KEY_PRESS
                | EventMask::KEY_RELEASE
            )
        )?;

        let wm_protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
        let wm_delete_window = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;

        conn.change_property32(PropMode::REPLACE, win_id, 
            wm_protocols, AtomEnum::ATOM, &[wm_delete_window])?;


        let wm_name = conn.intern_atom(false, b"WM_NAME")?.reply()?.atom;
        let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
        let utf8_string = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;

        conn.change_property8(
            PropMode::REPLACE,
            win_id,
            wm_name,
            AtomEnum::STRING,
            window_config.title.as_bytes(),
        )?;

        conn.change_property8(
            PropMode::REPLACE,
            win_id,
            net_wm_name,
            utf8_string,
            window_config.title.as_bytes(),
        )?;

        conn.map_window(win_id)?;
        conn.flush()?;

        
        let wake_fd = Arc::new(EventFd::from_value_and_flags(
            0,
            EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC,
        )?);

        Ok(Self {
            width: window_config.width,
            height: window_config.height,
            xdisplay: xdisplay, 
            xcb_conn: conn,
            window: win_id,
            wm_delete_window: wm_delete_window,
            wake_fd
        })
    }

    fn collect_event(
        &mut self,
        raw_ev: Event,
        close_requested: &mut bool,
        redraw_requested: &mut bool,
        latest_resize: &mut Option<(u32, u32)>,
    ) {
        match raw_ev {
            Event::ClientMessage(e) => {
                if e.data.as_data32()[0] == self.wm_delete_window {
                    *close_requested = true;
                }
            }

            Event::Expose(_) => {
                *redraw_requested = true;
            }

            Event::ConfigureNotify(e) => {
                self.width = e.width as u32;
                self.height = e.height as u32;

                *latest_resize = Some((self.width, self.height));
            }

            Event::KeyPress(_e) => {
                // later
            }

            Event::KeyRelease(_e) => {
                // later
            }

            _ => {}
        }
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

        let x11_fd_raw = self.xcb_conn.as_raw_fd();

        let x11_fd = unsafe {
            BorrowedFd::borrow_raw(x11_fd_raw)
        };

        let mut poll_fds = [
            PollFd::new(x11_fd, PollFlags::POLLIN),
            PollFd::new(self.wake_fd.as_fd(), PollFlags::POLLIN),
        ];
        
        // block forever until X11 event or app wake.
        poll(&mut poll_fds, PollTimeout::NONE)?;
        
        let mut close_requested = false;
        let mut redraw_requested = false;
        let mut latest_resize: Option<(u32, u32)> = None;

        if poll_fds[1].revents().unwrap_or(PollFlags::empty()).contains(PollFlags::POLLIN) {
            self.drain_wake_fd()?;
            redraw_requested = true;
        }

        if poll_fds[0].revents().unwrap_or(PollFlags::empty()).contains(PollFlags::POLLIN) {

            // Drain pending events, but don't generate one app event per raw X11 event.
            while let Some(ev) = self.xcb_conn.poll_for_event()? {
                self.collect_event(
                    ev,
                    &mut close_requested,
                    &mut redraw_requested,
                    &mut latest_resize,
                );

                if close_requested {
                    break;
                }
            }
        }

        let mut events = Vec::new();

        if close_requested {
            events.push(WindowEvent::CloseRequested);
            return Ok(events);
        }

        if let Some((width, height)) = latest_resize {
            events.push(WindowEvent::Resized { width, height });
            redraw_requested = true;
        }

        if redraw_requested {
            events.push(WindowEvent::RedrawRequested);
        }

        Ok(events)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn native_handle(&self) -> WindowHandleInfo {
        WindowHandleInfo::X11 {
            display: self.xdisplay.as_ptr() as *mut c_void,
            window: self.window as u64
        }
    }

    pub fn waker(&self) -> X11Waker {
        X11Waker {
            wake_fd: self.wake_fd.clone(),
        }
    }

}
