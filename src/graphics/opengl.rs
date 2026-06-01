extern crate khronos_egl as egl;
use std::ffi::c_void;

use glow::HasContext;
use khronos_egl::{Display, Surface};

use crate::platform::Platform;
use crate::platform::window::WindowHandleInfo;
use crate::graphics::Color;


pub struct OpenGLRenderer {
    gl: glow::Context,

    egl: egl::Instance<egl::Static>,
    egl_display: Display, 
    egl_surface: Surface, 

    width: u32,
    height: u32,
}

impl OpenGLRenderer {
    pub fn new(platform: &Platform) -> anyhow::Result<Self> {

        let egl = egl::Instance::new(egl::Static);
        let egl_display = unsafe { egl.get_display(match platform.native_handle() {
            WindowHandleInfo::X11 { display, .. } => {
                display
            },
            WindowHandleInfo::Wayland { display, .. } => {
                display
            },
        }as *mut c_void) 
        }.expect("Failed to get EGL display from raw X display");


        egl.initialize(egl_display)?;

        egl.bind_api(egl::OPENGL_API)?;

        let attributes = [
            egl::SURFACE_TYPE, egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE, egl::OPENGL_BIT,

            egl::BUFFER_SIZE, 32,
            egl::RED_SIZE, 8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE, 8,
            egl::ALPHA_SIZE, 8,
            egl::DEPTH_SIZE, 24,
            egl::STENCIL_SIZE, 8,

            egl::NONE,
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

        let egl_surface = unsafe {
            egl.create_window_surface(
                egl_display, 
                config,
                match platform.native_handle() {
                    WindowHandleInfo::X11 { display: _, window } => {
                        window as usize as *mut c_void
                    },
                    WindowHandleInfo::Wayland { display, .. } => {
                        display
                    },
                },
                None)
        }?;

        egl.make_current(egl_display, Some(egl_surface), Some(egl_surface), Some(context))?;

        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name)
                    .map_or(std::ptr::null(), |f| f as *const c_void)
            })
        };

        let (width, height) = platform.size();

        Ok(Self {
            gl,
            egl,
            egl_display,
            egl_surface,
            width,
            height,
        })
    }

    pub fn clear_color(&mut self, color: Color) {
        unsafe {
            self.gl.clear_color(color.r, color.g, color.b, color.a);
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;

        let surface_width = self
            .egl
            .query_surface(self.egl_display, self.egl_surface, egl::WIDTH)
            .unwrap_or(width as i32);

        let surface_height = self
            .egl
            .query_surface(self.egl_display, self.egl_surface, egl::HEIGHT)
            .unwrap_or(height as i32);

        self.width = surface_width as u32;
        self.height = surface_height as u32;

        unsafe {
            self.gl.viewport(0, 0, surface_width, surface_height);
        }
    }

    pub fn begin_frame(&mut self) {
        unsafe {
            self.gl.viewport(0, 0, self.width as i32, self.height as i32); 
            self.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        self.egl.swap_buffers(self.egl_display, self.egl_surface)?;
        Ok(())
    }

}
