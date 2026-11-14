use crate::platform::window::WindowHandleInfo;

use super::event::WindowEvent;
use super::keyboard::Keyboard;
use super::mouse::{CursorPosition, Mouse, MouseButton, ScrollDelta};
use super::window::WindowConfig;
use xkbcommon::xkb;

extern crate anyhow;
extern crate x11;
extern crate x11rb;

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use ash::vk::{XcbSurfaceCreateInfoKHR, XlibSurfaceCreateInfoKHR};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, CreateWindowAux, EventMask, PixmapEnum, PropMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::eventfd::{EfdFlags, EventFd};
use nix::unistd::read;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use x11::xlib::{_XDisplay, XDefaultScreen, XOpenDisplay};
use x11::xlib_xcb::{XEventQueueOwner::XCBOwnsEventQueue, XGetXCBConnection, XSetEventQueueOwner};

/// X11 window platform implementation.
///
/// Owns the native Xlib display, XCB connection,
/// X11 window and wake file descriptor used to
/// drive the window event loop.
pub struct X11Platform {
    width: u32,
    height: u32,

    // Native Xlib display used for interop with
    // APIs that require an X11 display pointer.
    xdisplay: NonNull<_XDisplay>,

    xcb_conn: XCBConnection,
    window: u32,
    wm_delete_window: u32,
    pub(crate) mouse: Mouse,
    keyboard: Keyboard,
    context: xkb::Context,
    keyboard_device: i32,
    pending_input: Vec<WindowEvent>,

    // Eventfd used to wake the blocking X11
    // event poll from another thread.
    wake_fd: Arc<EventFd>,
}

/// Waker handle for requesting redraws from
/// outside the X11 event loop. (thread safe)
#[derive(Clone)]
pub struct X11Waker {
    #[allow(dead_code)]
    wake_fd: Arc<EventFd>,
}

impl X11Waker {
    /// Unix implementation to request a redraw in the
    /// X11 platform window.
    ///
    /// Uses nix::unistd::write to wake up the self.wake_fd
    /// file descriptor.
    #[allow(dead_code)]
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
    /// Creates a new X11 platform window.
    ///
    /// Opens the X display, creates the XCB window,
    /// configures window manager protocols and title
    /// for the window maps the window and initializes
    /// the wake file descriptor used by the event loop.
    pub fn new(window_config: &WindowConfig) -> anyhow::Result<Self> {
        let conn: XCBConnection;
        let screen_num: i32;

        // Open X display, we need the raw X display
        // aside from the XCB connection because of
        // EGL interop.
        let raw_display = unsafe { XOpenDisplay(ptr::null()) };
        let xdisplay =
            NonNull::new(raw_display).ok_or_else(|| anyhow::anyhow!("Failed to open X display"))?;

        unsafe {
            // Set event queue owner to XCB to use
            // XCB later.
            XSetEventQueueOwner(xdisplay.as_ptr(), XCBOwnsEventQueue);

            let conn_ptr = XGetXCBConnection(xdisplay.as_ptr());
            if conn_ptr.is_null() {
                anyhow::bail!("Failed to get XCB connection pointer from X display.");
            }

            screen_num = XDefaultScreen(xdisplay.as_ptr());

            // We get the XCB connection from raw connection
            // pointer from Xlib
            conn = XCBConnection::from_raw_xcb_connection(conn_ptr, false)
                .expect("Failed to create x11rb connection from raw connection pointer");
        }

        let setup = conn.setup();

        // Gets the default screen to get the root window
        let screen = setup
            .roots
            .get(screen_num as usize)
            .expect("Failed to find the default screen in XCB setup roots");

        let win_id = conn.generate_id()?;

        // Create the window
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
                        | EventMask::FOCUS_CHANGE
                        | EventMask::POINTER_MOTION
                        | EventMask::BUTTON_PRESS
                        | EventMask::BUTTON_RELEASE
                        | EventMask::ENTER_WINDOW
                        | EventMask::LEAVE_WINDOW,
                ),
        )?;

        // Set window manager protocol atoms.
        // Title & close atoms
        let wm_protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
        let wm_delete_window = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;

        conn.change_property32(
            PropMode::REPLACE,
            win_id,
            wm_protocols,
            AtomEnum::ATOM,
            &[wm_delete_window],
        )?;

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

        // Create the wake FD to be used in poll_events() and
        // request_redraw() later.
        let wake_fd = Arc::new(EventFd::from_value_and_flags(
            0,
            EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC,
        )?);

        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let (mut major, mut minor, mut event, mut error) = (0, 0, 0, 0);
        anyhow::ensure!(
            xkb::x11::setup_xkb_extension(
                &conn,
                1,
                0,
                xkb::x11::SetupXkbExtensionFlags::NoFlags,
                &mut major,
                &mut minor,
                &mut event,
                &mut error
            ),
            "XKB extension unavailable"
        );
        let keyboard_device = xkb::x11::get_core_keyboard_device_id(&conn);
        anyhow::ensure!(keyboard_device >= 0, "XKB keyboard unavailable");
        let keymap = xkb::x11::keymap_new_from_device(
            &context,
            &conn,
            keyboard_device,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
        anyhow::ensure!(!keymap.get_raw_ptr().is_null(), "XKB keymap unavailable");
        let state = xkb::x11::state_new_from_device(&keymap, &conn, keyboard_device);
        anyhow::ensure!(!state.get_raw_ptr().is_null(), "XKB state unavailable");
        let keyboard = Keyboard::new(&context, keymap, state);
        use x11rb::protocol::xkb::{self as protocol, EventType, MapPart};
        protocol::select_events(
            &conn,
            keyboard_device as u16,
            EventType::default(),
            EventType::NEW_KEYBOARD_NOTIFY | EventType::MAP_NOTIFY | EventType::STATE_NOTIFY,
            MapPart::from(0xffu8),
            MapPart::from(0xffu8),
            &protocol::SelectEventsAux::default(),
        )?
        .check()?;
        protocol::per_client_flags(
            &conn,
            keyboard_device as u16,
            protocol::PerClientFlag::DETECTABLE_AUTO_REPEAT,
            protocol::PerClientFlag::DETECTABLE_AUTO_REPEAT,
            protocol::BoolCtrl::default(),
            protocol::BoolCtrl::default(),
            protocol::BoolCtrl::default(),
        )?
        .reply()?;
        conn.flush()?;

        Ok(Self {
            width: window_config.width,
            height: window_config.height,
            xdisplay,
            xcb_conn: conn,
            window: win_id,
            wm_delete_window,
            mouse: Mouse::default(),
            keyboard,
            context,
            keyboard_device,
            pending_input: Vec::new(),
            wake_fd,
        })
    }

    fn collect_event(
        &mut self,
        raw_ev: Event,
        close_requested: &mut bool,
        redraw_requested: &mut bool,
        latest_resize: &mut Option<(u32, u32)>,
    ) {
        // Simply respond to a single event
        match raw_ev {
            Event::ClientMessage(e) if e.data.as_data32()[0] == self.wm_delete_window => {
                *close_requested = true;
            }

            Event::Expose(_) => {
                *redraw_requested = true;
            }

            Event::ConfigureNotify(e) => {
                self.width = e.width as u32;
                self.height = e.height as u32;

                *latest_resize = Some((self.width, self.height));
            }

            Event::EnterNotify(e) => {
                self.pending_input.push(WindowEvent::MouseEntered {
                    position: CursorPosition {
                        x: e.event_x.into(),
                        y: e.event_y.into(),
                    },
                });
            }
            Event::LeaveNotify(_) => self.pending_input.push(WindowEvent::MouseLeft),
            Event::MotionNotify(e) => {
                self.pending_input.push(WindowEvent::MouseMoved {
                    position: CursorPosition {
                        x: e.event_x.into(),
                        y: e.event_y.into(),
                    },
                });
            }
            Event::ButtonPress(e) => {
                self.pending_input.push(x11_button_event(e.detail, true));
            }
            Event::ButtonRelease(e) if !(4..=7).contains(&e.detail) => {
                self.pending_input.push(x11_button_event(e.detail, false));
            }
            Event::KeyPress(e) => {
                self.pending_input
                    .extend(self.keyboard.press(e.detail as u32));
            }

            Event::XkbStateNotify(e) => {
                self.keyboard.state.update_mask(
                    e.base_mods.into(),
                    e.latched_mods.into(),
                    e.locked_mods.into(),
                    e.base_group as u32,
                    e.latched_group as u32,
                    e.locked_group.into(),
                );
            }

            Event::XkbMapNotify(_) | Event::XkbNewKeyboardNotify(_) => {
                let keymap = xkb::x11::keymap_new_from_device(
                    &self.context,
                    &self.xcb_conn,
                    self.keyboard_device,
                    xkb::KEYMAP_COMPILE_NO_FLAGS,
                );
                if !keymap.get_raw_ptr().is_null() {
                    let state = xkb::x11::state_new_from_device(
                        &keymap,
                        &self.xcb_conn,
                        self.keyboard_device,
                    );
                    if !state.get_raw_ptr().is_null() {
                        self.keyboard = Keyboard::new(&self.context, keymap, state);
                    }
                }
            }

            Event::FocusIn(_) => {
                self.keyboard.state = xkb::x11::state_new_from_device(
                    &self.keyboard.keymap,
                    &self.xcb_conn,
                    self.keyboard_device,
                );
                self.pending_input.push(WindowEvent::FocusChanged(true));
            }

            Event::FocusOut(_) => {
                self.keyboard.reset();
                self.pending_input.push(WindowEvent::FocusChanged(false));
            }

            _ => {}
        }
    }

    fn drain_wake_fd(&self) -> anyhow::Result<()> {
        let mut buf = [0u8; 8];

        // drain the fd, yeah.
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

    /// Polls and collects pending X11 window
    /// events.
    ///
    /// Blocks until either an X11 event arrives
    /// or the wake file descriptor is signaled.
    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        let mut close_requested = false;
        let mut redraw_requested = false;
        let mut latest_resize: Option<(u32, u32)> = None;

        // Collect any already queued events
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

        // Respond to potentially queued events
        let mut events = std::mem::take(&mut self.pending_input);

        if close_requested {
            events.push(WindowEvent::CloseRequested);
            return Ok(events);
        }

        if let Some((width, height)) = latest_resize.take() {
            events.push(WindowEvent::Resized { width, height });
            redraw_requested = true;
        }

        if redraw_requested {
            events.push(WindowEvent::RedrawRequested);
            return Ok(events);
        }

        if !events.is_empty() {
            return Ok(events);
        }

        // Get raw X11 FD
        let x11_fd_raw = self.xcb_conn.as_raw_fd();

        let x11_fd = unsafe { BorrowedFd::borrow_raw(x11_fd_raw) };

        // Set up FDs to poll on. Those being
        // the X11 FD and the wake FD.
        let mut poll_fds = [
            PollFd::new(x11_fd, PollFlags::POLLIN),
            PollFd::new(self.wake_fd.as_fd(), PollFlags::POLLIN),
        ];

        // Poll on the file descriptors
        match poll(&mut poll_fds, PollTimeout::NONE) {
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        }
        for fd in &poll_fds {
            anyhow::ensure!(
                !fd.revents()
                    .unwrap_or(PollFlags::empty())
                    .intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL),
                "X11 connection closed"
            );
        }

        // Repond to potential events on either
        // of the file descriptors

        let mut close_requested = false;
        let mut redraw_requested = false;
        let mut latest_resize: Option<(u32, u32)> = None;

        // Respond to a requested redraw event
        if poll_fds[1]
            .revents()
            .unwrap_or(PollFlags::empty())
            .contains(PollFlags::POLLIN)
        {
            self.drain_wake_fd()?;
            redraw_requested = true;
        }

        // Collect potential X11 events
        if poll_fds[0]
            .revents()
            .unwrap_or(PollFlags::empty())
            .contains(PollFlags::POLLIN)
        {
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

        // Respond to X11 potentially collected events
        let mut events = std::mem::take(&mut self.pending_input);

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

    /// Returns the current X11 window size.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the native X11 window handle
    /// information used by graphics backends.
    pub fn native_handle(&self) -> WindowHandleInfo {
        WindowHandleInfo::X11 {
            display: self.xdisplay.as_ptr() as *mut c_void,
            window: self.window as u64,
        }
    }

    /// Creates a waker handle that can request a
    /// redraw from outside the X11 event loop.
    #[allow(dead_code)]
    pub fn waker(&self) -> X11Waker {
        X11Waker {
            wake_fd: self.wake_fd.clone(),
        }
    }

    pub fn create_vulkan_surface(
        &self,
        entry: &ash::Entry,
        instance: &ash::Instance,
        have_ext_vk_khr_xcb_surface: bool,
        _have_ext_vk_khr_wayland_surface: bool,
    ) -> anyhow::Result<ash::vk::SurfaceKHR> {
        if have_ext_vk_khr_xcb_surface {
            let create_info: XcbSurfaceCreateInfoKHR = XcbSurfaceCreateInfoKHR {
                connection: self.xcb_conn.get_raw_xcb_connection(),
                window: self.window,
                ..Default::default()
            };

            let xcb_surface_inst = ash::khr::xcb_surface::Instance::new(entry, instance);

            tracing::info!("Created Vulkan X11/XCB surface successfully.");
            unsafe { Ok(xcb_surface_inst.create_xcb_surface(&create_info, None)?) }
        } else {
            let create_info: XlibSurfaceCreateInfoKHR = XlibSurfaceCreateInfoKHR {
                dpy: self.xdisplay.as_ptr() as *mut c_void,
                window: self.window as u64,
                ..Default::default()
            };

            let xlib_surface_inst = ash::khr::xlib_surface::Instance::new(entry, instance);

            tracing::info!("Created Vulkan X11/Xlib surface successfully.");

            unsafe { Ok(xlib_surface_inst.create_xlib_surface(&create_info, None)?) }
        }
    }
}

impl Drop for X11Platform {
    fn drop(&mut self) {
        let _ = self.xcb_conn.destroy_window(self.window);
        let _ = self.xcb_conn.flush();
        // XCBConnection borrows this connection (should_drop=false).
        unsafe {
            x11::xlib::XCloseDisplay(self.xdisplay.as_ptr());
        }
    }
}

/// Core X11 represents wheel steps as button presses; releases are ignored.
fn x11_button_event(detail: u8, pressed: bool) -> WindowEvent {
    let delta = match detail {
        4 => Some((0.0, -1.0)),
        5 => Some((0.0, 1.0)),
        6 => Some((-1.0, 0.0)),
        7 => Some((1.0, 0.0)),
        _ => None,
    };
    if let Some((x, y)) = delta {
        return WindowEvent::MouseScrolled {
            delta: ScrollDelta::Lines { x, y },
        };
    }
    let button = match detail {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        8 => MouseButton::Back,
        9 => MouseButton::Forward,
        other => MouseButton::Other(other.into()),
    };
    if pressed {
        WindowEvent::MouseButtonPressed { button }
    } else {
        WindowEvent::MouseButtonReleased { button }
    }
}
