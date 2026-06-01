extern crate khronos_egl as egl;
use std::ffi::c_void;

use anyhow::Ok;
use glow::HasContext;
use khronos_egl::{Display, Surface, Context};

use crate::graphics::device::VertexFormat;
use crate::graphics::opengl::{GlBuffer, GlPipeline, GlTexture};
use crate::platform::Platform;
use crate::platform::window::WindowHandleInfo;
use crate::graphics::{BufferDesc, BufferHandle, BufferTarget, BufferUsage, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, VertexStepMode};

pub struct OpenGLRenderer {
    gl: glow::Context,

    egl: egl::Instance<egl::Static>,
    egl_display: Display,
    egl_surface: Surface,
    egl_context: Context,

    width: u32,
    height: u32,

    buffers: Vec<Option<GlBuffer>>,
    textures: Vec<Option<GlTexture>>,
    pipelines: Vec<Option<GlPipeline>>,

    current_pipeline: Option<PipelineHandle>,
    current_vbo: Option<BufferHandle>,
    current_ibo: Option<BufferHandle>,
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

        unsafe {

            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);

            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

            gl.viewport(0, 0, width as i32, height as i32);
        }

        Ok(Self {
            gl,
            egl,
            egl_display,
            egl_surface,
            egl_context: context,
            width,
            height,

            buffers: Vec::new(),
            textures: Vec::new(),
            pipelines: Vec::new(),
            current_pipeline: None,
            current_vbo: None,
            current_ibo: None,
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
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        self.egl.swap_buffers(self.egl_display, self.egl_surface)?;
        Ok(())
    }

    fn gl_buffer_target(&self, target: BufferTarget) -> u32 {
        match target {
            BufferTarget::Vertex => glow::ARRAY_BUFFER,
            BufferTarget::Index => glow::ELEMENT_ARRAY_BUFFER,
            BufferTarget::Uniform => glow::UNIFORM_BUFFER,
        }
    }

    fn gl_buffer_usage(&self, usage: BufferUsage) -> u32 {
        match usage {
            BufferUsage::Static => glow::STATIC_DRAW,
            BufferUsage::Dynamic => glow::DYNAMIC_DRAW,
            BufferUsage::Stream => glow::STREAM_DRAW,
        }
    }
    
    pub fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        unsafe {
            let raw_buf = self.gl.create_buffer().map_err(|e| anyhow::anyhow!("failed to create OpenGL buffer: {e}"))?;

            let target = self.gl_buffer_target(desc.target);
            let usage = self.gl_buffer_usage(desc.usage);

            self.gl.bind_buffer(target, Some(raw_buf));
            self.gl.buffer_data_size(target, desc.size as i32, usage); 
            self.gl.bind_buffer(target, None);

            let handle = BufferHandle(self.buffers.len() as u32);

            self.buffers.push(Some( GlBuffer { 
                raw: raw_buf, target: desc.target, size: desc.size
            }));

            Ok(handle)
        }
    }

    fn compile_shader(&self, kind: u32, source: &str) -> anyhow::Result<glow::NativeShader> {
        unsafe {
            let shader = self.gl.create_shader(kind)
                .map_err(|e| anyhow::anyhow!("Failed to create shader: {e}"))?;

            self.gl.shader_source(shader, source);
            self.gl.compile_shader(shader);

            if !self.gl.get_shader_compile_status(shader) {
                let log = self.gl.get_shader_info_log(shader);
                self.gl.delete_shader(shader);
                anyhow::bail!("Shader compilation failed: {log}");
            }

            Ok(shader)
        }
    }

    fn link_program(&self, shaders: &[glow::NativeShader]) -> anyhow::Result<glow::NativeProgram> {
        unsafe {
            let program = self.gl.create_program()
                .map_err(|e| anyhow::anyhow!("Failed to create shader program: {e}"))?;

            for &shader in shaders {
                self.gl.attach_shader(program, shader);
            }

            self.gl.link_program(program);

            if !self.gl.get_program_link_status(program) {
                let log = self.gl.get_program_info_log(program);
                for &shader in shaders {
                    self.gl.detach_shader(program, shader);
                    self.gl.delete_shader(shader);
                }

                self.gl.delete_program(program);
                anyhow::bail!("Shader program linking failed: {log}");
            }
            
            for &shader in shaders {
                self.gl.detach_shader(program, shader);
                self.gl.delete_shader(shader);
            }

            Ok(program)
        }
    }

    fn gl_vertex_format(&self, format: VertexFormat) -> (i32, u32, bool) {
        match format {
            VertexFormat::Float32 => (1, glow::FLOAT, false),
            VertexFormat::Float32x2 => (2, glow::FLOAT, false),
            VertexFormat::Float32x3 => (3, glow::FLOAT, false),
            VertexFormat::Float32x4 => (4, glow::FLOAT, false),

            VertexFormat::Uint32 => (1, glow::UNSIGNED_INT, false),
            VertexFormat::Uint32x2 => (2, glow::UNSIGNED_INT, false),
            VertexFormat::Uint32x3 => (3, glow::UNSIGNED_INT, false),
            VertexFormat::Uint32x4 => (4, glow::UNSIGNED_INT, false),

            VertexFormat::Unorm8x4 => (4, glow::UNSIGNED_BYTE, true),
        }
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        unsafe {
            let vertex_shader = self.compile_shader(glow::VERTEX_SHADER, desc.vertex_source)?;
            let fragment_shader = self.compile_shader(glow::FRAGMENT_SHADER, desc.fragment_source)?;
            let program = self.link_program(&[vertex_shader, fragment_shader])?;

            let vao = self.gl.create_vertex_array()
                .map_err(|e| anyhow::anyhow!("Failed to create VAO: {e}"))?; 

            self.pipelines.push(Some(GlPipeline { program, vao, vert_layout: desc.vert_layout }));

            Ok(PipelineHandle((self.pipelines.len() - 1) as u32))
        }
    }
    pub fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()> {
        unsafe {
            let offset_bytes = draw.index_offset as i32 * std::mem::size_of::<u32>() as i32;

            self.gl.draw_elements_base_vertex(
                glow::TRIANGLES,
                draw.index_count as i32,
                glow::UNSIGNED_INT,
                offset_bytes,
                draw.vertex_offset,
            );
        }

        Ok(())
    }

    fn get_pipeline(&self, handle: PipelineHandle) -> anyhow::Result<&GlPipeline> {
        self.pipelines
            .get(handle.0 as usize)
            .and_then(|pipeline| pipeline.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid pipeline handle: {:?}", handle))
    }

    fn get_pipeline_mut(&mut self, handle: PipelineHandle) -> anyhow::Result<&mut GlPipeline> {
        self.pipelines
            .get_mut(handle.0 as usize)
            .and_then(|pipeline| pipeline.as_mut())
            .ok_or_else(|| anyhow::anyhow!("invalid pipeline handle: {:?}", handle))
    }

    fn get_buffer(&self, handle: BufferHandle) -> anyhow::Result<&GlBuffer> {
        self.buffers
            .get(handle.0 as usize)
            .and_then(|buffer| buffer.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid buffer handle: {:?}", handle))
    } 

    pub fn set_vertex_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        if self.current_vbo == Some(handle) {
            return Ok(());
        }
        let crnt_pipeline = self.current_pipeline
            .ok_or_else(|| anyhow::anyhow!("set_vertex_buffer called before set_pipeline"))?;

        let buffer_raw = {
            let buffer = self.get_buffer(handle)?;

            if buffer.target != BufferTarget::Vertex {
                anyhow::bail!("Trying to bind non-vertex buffer as vertex buffer");
            }
            buffer.raw
        };
        
        let layout = &self.get_pipeline(crnt_pipeline)?.vert_layout;

        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer_raw));

               for attr in &layout.attrs {
                let (components, gl_type, normalized) = self.gl_vertex_format(attr.format);

                self.gl.enable_vertex_attrib_array(attr.location);

                match attr.format {
                    VertexFormat::Uint32
                        | VertexFormat::Uint32x2
                        | VertexFormat::Uint32x3
                        | VertexFormat::Uint32x4 => {
                            self.gl.vertex_attrib_pointer_i32(
                                attr.location,
                                components,
                                gl_type,
                                layout.stride as i32,
                                attr.offset as i32
                            );
                        }
                    _ => {
                        self.gl.vertex_attrib_pointer_f32(
                            attr.location,
                            components,
                            gl_type,
                            normalized,
                            layout.stride as i32,
                            attr.offset as i32
                        );
                    }
                }

                let divisor = match layout.step_mode {
                    VertexStepMode::Instance => 1,
                    VertexStepMode::Vertex => 0
                };

                self.gl.vertex_attrib_divisor(attr.location, divisor);
            }

        }

        self.current_vbo = Some(handle);

        Ok(())
    }

    pub fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        if self.current_pipeline == Some(handle) {
            return Ok(());
        }
        
        let pipeline = self.get_pipeline(handle)?;

        unsafe {
            self.gl.use_program(Some(pipeline.program));
            self.gl.bind_vertex_array(Some(pipeline.vao));
        }
        self.current_pipeline = Some(handle);
        self.current_vbo = None;
        self.current_ibo = None;

        Ok(())
    }

    pub fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        if self.current_ibo == Some(handle) {
            return Ok(());
        }

        let buffer = self.get_buffer(handle)?;
        if buffer.target != BufferTarget::Index {
            anyhow::bail!("Trying to bind non-index buffer as index buffer");
        }

        let buffer_raw = buffer.raw;
        unsafe {
            self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffer_raw));
        }

        self.current_ibo = Some(handle);

        Ok(())
    }

    pub fn write_buffer(
        &mut self, 
        handle: BufferHandle,
        offset: usize, 
        data: &[u8],
    ) -> anyhow::Result<()> {
        let (raw, target, size) = {
            let buffer = self.get_buffer(handle)?;
            (buffer.raw, buffer.target, buffer.size)
        };

        if offset + data.len() > size {
            anyhow::bail!(
                "buffer write out of bounds: offset={}, len={}, buffer_size={}",
                offset,
                data.len(),
                size
            );
        }

        unsafe {
            if self.current_vbo != Some(handle) {
                self.gl.bind_buffer(self.gl_buffer_target(target), Some(raw));
                self.current_vbo = Some(handle);
            }
            self.gl.buffer_sub_data_u8_slice(self.gl_buffer_target(target), offset as i32, data);
        }

        Ok(())
    }

    pub fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str, 
        x: f32, 
        y: f32 
    ) -> anyhow::Result<()> {
        let program = self.get_pipeline(pipeline)?.program;

        unsafe {
            if self.current_pipeline != Some(pipeline) {
                self.gl.use_program(Some(program));
                self.current_pipeline = Some(pipeline);
            }
            if let Some(location) = self.gl.get_uniform_location(program, name) {
                self.gl.uniform_2_f32(Some(&location), x, y);
            }
        }

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


impl GraphicsDevice for OpenGLRenderer {
    fn resize(&mut self, width: u32, height: u32) {
        OpenGLRenderer::resize(self, width, height);
    }

    fn clear_color(&mut self, color: Color) {
        OpenGLRenderer::clear_color(self, color);
    }

    fn begin_frame(&mut self) {
        OpenGLRenderer::begin_frame(self);
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        OpenGLRenderer::end_frame(self)
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        OpenGLRenderer::create_buffer(self, desc)
    }

    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        OpenGLRenderer::write_buffer(self, handle, offset, data)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        OpenGLRenderer::create_pipeline(self, desc)
    }

    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_pipeline(self, handle)
    }

    fn set_vertex_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_vertex_buffer(self, handle)
    }

    fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_index_buffer(self, handle)
    }

    fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        OpenGLRenderer::set_uniform_2f(self, pipeline, name, x, y)
    }

    fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()> {
        OpenGLRenderer::draw_indexed(self, draw)
    }
}
