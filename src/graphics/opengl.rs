extern crate khronos_egl as egl;
use std::ffi::c_void;

use glow::HasContext;
use khronos_egl::{Display, Surface, Context};

use crate::platform::Platform;
use crate::platform::window::WindowHandleInfo;
use crate::graphics::Color;


pub struct OpenGLRenderer {
    gl: glow::Context,

    egl: egl::Instance<egl::Static>,
    egl_display: Display, 
    egl_surface: Surface, 
    egl_context: Context, 

    width: u32,
    height: u32,
}

impl OpenGLRenderer {
    pub fn new(platform: &Platform) -> anyhow::Result<Self> {
       
        let handle = platform.native_handle();
        let native_display = match handle {
            WindowHandleInfo::X11 { display, .. } => display,
            WindowHandleInfo::Wayland { display, .. } => display,
        };

        let native_window = match handle {
            WindowHandleInfo::X11 { window, .. } => window as usize as *mut c_void,
            WindowHandleInfo::Wayland { egl_win, .. } => egl_win,
        };

        let egl = egl::Instance::new(egl::Static);
        let egl_display = unsafe { egl.get_display(native_display as *mut c_void) 
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
                native_window,
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
            egl_context: context,
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
    if width == 0 || height == 0 {
        return;
    }

    self.width = width;
    self.height = height;

    unsafe {
        self.gl.viewport(0, 0, width as i32, height as i32);
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

impl Drop for OpenGLRenderer {
    fn drop(&mut self) {
        let _ = self.egl.make_current(
            self.egl_display,
            None,
            None,
            None,
        );

        let _ = self.egl.destroy_surface(
            self.egl_display,
            self.egl_surface,
        );

        let _ = self.egl.destroy_context(
            self.egl_display,
            self.egl_context,
        );

        let _ = self.egl.terminate(self.egl_display);
    }
}
