
extern crate x11;
extern crate x11rb;
extern crate glow;
extern crate khronos_egl as egl;

use std::ptr;
use std::ffi::c_void; 

use x11rb::connection::Connection;
use x11rb::errors::ConnectError;
use x11rb::protocol::{Event, xproto::*};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::xcb_ffi::XCBConnection;

use x11::xlib::{_XDisplay, XDefaultScreen, XOpenDisplay};
use x11::xlib_xcb::{XGetXCBConnection, XSetEventQueueOwner, XEventQueueOwner::XCBOwnsEventQueue};

use glow::HasContext;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn: Result<XCBConnection, ConnectError>;
    let screen_num: i32;
    let xdisplay: *mut _XDisplay; 
    unsafe {
        xdisplay = XOpenDisplay(ptr::null());
        if xdisplay.is_null() {
            panic!("Failed to open X display.");
        }

        XSetEventQueueOwner(xdisplay, XCBOwnsEventQueue);

        let conn_ptr = XGetXCBConnection(xdisplay);
        if conn_ptr.is_null() {
            panic!("Failed to get XCB connection pointer from X display.");
        }

        screen_num = XDefaultScreen(xdisplay);

        conn = XCBConnection::from_raw_xcb_connection(conn_ptr, false);
    }

    let conn = conn.expect("Failed to create x11rb connection from raw connection pointer");
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
        1280,
        720,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
        .background_pixmap(PixmapEnum::NONE)
        .event_mask(EventMask::EXPOSURE),
    )?;

    let egl = egl::Instance::new(egl::Static);
    let egl_display = unsafe { egl.get_display(xdisplay as *mut c_void) 
    }.expect("Failed to get EGL display from raw X display");
    
    egl.initialize(egl_display)?;

    egl.bind_api(egl::OPENGL_API)?;

    let attributes = [
        egl::SURFACE_TYPE, egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE, egl::OPENGL_BIT,
        egl::RED_SIZE, 8,
        egl::GREEN_SIZE, 8,
        egl::BLUE_SIZE, 8,
        egl::ALPHA_SIZE, 8,
        egl::DEPTH_SIZE, 24,
        egl::STENCIL_SIZE, 8,

        egl::NONE
    ];

    let config = egl
        .choose_first_config(egl_display, &attributes)?
        .expect("unable to find an appropriate ELG configuration");

    let context_attributes = [
        egl::CONTEXT_MAJOR_VERSION, 4,
        egl::CONTEXT_MINOR_VERSION, 0,
        egl::CONTEXT_OPENGL_PROFILE_MASK,
        egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
        egl::NONE
    ];

    let context = egl.create_context(egl_display, 
        config, None, &context_attributes)?;

    conn.map_window(win_id)?;
    conn.flush()?;

    let surface = unsafe {
        egl.create_window_surface(
            egl_display, 
            config, 
            win_id as usize as *mut c_void, 
            None)
    }?;

    egl.make_current(egl_display, Some(surface), Some(surface), Some(context))?;

    let gl = unsafe {
        glow::Context::from_loader_function(|name| {
            egl.get_proc_address(name)
                .map_or(std::ptr::null(), |f| f as *const c_void)
        })
    };
    unsafe {
        gl.viewport(0, 0, 1280, 720);
        gl.clear_color(0.1, 0.1, 0.1, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    }

    egl.swap_buffers(egl_display, surface)?;

    conn.flush()?;
    loop {
        let event = conn.wait_for_event()?; 

        match event {
            Event::Expose(expose) => {
                unsafe {
                    gl.viewport(0, 0, expose.width as i32, expose.height as i32);
                    gl.clear_color(0.1, 0.1, 0.1, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                }

                egl.swap_buffers(egl_display, surface)?;

            }
            _ => todo!()
        }
    }
}
