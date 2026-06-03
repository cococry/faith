extern crate khronos_egl as egl;
use std::ffi::c_void;

use anyhow::Ok;
use glow::HasContext;
use khronos_egl::{Display, Surface, Context};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind, VertexFormat};
use crate::graphics::opengl::{GlBuffer, GlPipeline, GlTexture, GlTextureKind};
use crate::platform::Platform;
use crate::platform::window::WindowHandleInfo;
use crate::graphics::{BufferDesc, BufferHandle, BufferTarget, BufferUsage, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc, TextureFormat, TextureHandle, VertexStepMode};

const MAX_VBOS: usize = 8;

pub struct OpenGLRenderer {
    gl: glow::Context,

    egl: egl::Instance<egl::Static>,
    egl_display: Display,
    egl_surface: Surface,
    egl_context: Context,

    width: u32,
    height: u32,

    global_vao: glow::NativeVertexArray,

    buffers: Vec<Option<GlBuffer>>,
    textures: Vec<Option<GlTexture>>,
    pipelines: Vec<Option<GlPipeline>>,

    current_pipeline: Option<PipelineHandle>,
    current_vbos: [Option<BufferHandle>; MAX_VBOS],
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

            egl::RED_SIZE, 8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE, 8,
            egl::ALPHA_SIZE, 0,
            egl::STENCIL_SIZE, 0,

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

        let global_vao = unsafe { 
            let vao = gl.create_vertex_array()
                .map_err(|e| anyhow::anyhow!("Failed to create VAO: {e}"))?; 
            gl.bind_vertex_array(Some(vao));
            vao
        };


        Ok(Self {
            gl,
            egl,
            egl_display,
            egl_surface,
            egl_context: context,
            width,
            height,

            global_vao,

            buffers: Vec::new(),
            textures: Vec::new(),
            pipelines: Vec::new(),
            current_pipeline: None,
            current_vbos: [None; MAX_VBOS],
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

    fn gl_texture_format(&self, format: TextureFormat) -> (u32, u32, u32) {
        match format {
            TextureFormat::Rgba8 => (
                glow::RGBA8,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
            ),
            TextureFormat::Alpha8 => (
                glow::R8,
                glow::RED,
                glow::UNSIGNED_BYTE,
            ),
        }
    }

    pub fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {

        unsafe {
            let raw = self.gl.create_texture()
                .map_err(|e| anyhow::anyhow!("Failed to create OpenGL texture: {e}"))?;

            let (internal_format, format, gl_type) = self.gl_texture_format(desc.format);

            self.gl.bind_texture(glow::TEXTURE_2D, Some(raw));

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal_format as i32,
                desc.width as i32,
                desc.height as i32,
                0,
                format,
                gl_type,
                glow::PixelUnpackData::Slice(data),
            );

            self.gl.bind_texture(glow::TEXTURE_2D, None);

            let handle = TextureHandle(self.textures.len() as u32);

            self.textures.push(Some(GlTexture { 
                raw, 
                width: desc.width,  
                height: desc.height, 
                format: desc.format,
                kind: super::GlTextureKind::Texture2D
            }));

            Ok(handle)
        }
    }

    pub fn write_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let tex = self.get_texture(texture)?;

        if x + width > tex.width || y + height > tex.height {
            anyhow::bail!("texture write out of bounds");
        }

        let raw = tex.raw;
        let format = tex.format;

        let (_, external_format, ty) = self.gl_texture_format(format);

        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(raw));
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x as i32,
                y as i32,
                width as i32,
                height as i32,
                external_format,
                ty,
                glow::PixelUnpackData::Slice(Some(data)),
            );
        }

        Ok(())
    }

    pub fn texture_gen_mipmap(
        &mut self, 
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        let tex = self.get_texture(texture)?;

        let raw = tex.raw;
        unsafe {
            let ty = if tex.kind == GlTextureKind::Texture2DArray {
                glow::TEXTURE_2D_ARRAY 
            } else {
                glow::TEXTURE_2D
            };
            self.gl.bind_texture(ty, Some(raw));
            self.gl.generate_mipmap(ty);
            self.gl.bind_texture(ty, None);
        }
        Ok(())
    }

    pub fn set_texture(
        &mut self,
        slot: u32,
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        let tex = self.get_texture(texture)?;

        unsafe {
            let ty = if tex.kind == GlTextureKind::Texture2DArray {
                glow::TEXTURE_2D_ARRAY 
            } else {
                glow::TEXTURE_2D
            };
            self.gl.active_texture(glow::TEXTURE0 + slot);
            self.gl.bind_texture(ty, Some(tex.raw));
        }

        Ok(())
    }


    pub fn create_texture_array(
        &mut self,
        desc: TextureArrayDesc,
    ) -> anyhow::Result<TextureHandle> {
        unsafe {
            let raw = self.gl.create_texture()
                .map_err(|e| anyhow::anyhow!("Failed to create OpenGL texture array: {e}"))?;

            let (internal_format, format, gl_type) = self.gl_texture_format(desc.format);

            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(raw));

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR_MIPMAP_LINEAR as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );

            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            self.gl.tex_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                internal_format as i32,
                desc.width as i32,
                desc.height as i32,
                desc.layers as i32,
                0,
                format,
                gl_type,
                glow::PixelUnpackData::Slice(None),
            );

            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);

            let handle = TextureHandle(self.textures.len() as u32);

            self.textures.push(Some(GlTexture { 
                raw, 
                width: desc.width,  
                height: desc.height, 
                format: desc.format,
                kind: super::GlTextureKind::Texture2DArray
            }));

            Ok(handle)
        }

    }

    fn write_texture_array_layer(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        layer: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> anyhow::Result<()> {
        let tex = self.get_texture(texture)?;

        if x + width > tex.width || y + height > tex.height {
            anyhow::bail!("texture write out of bounds");
        }

        let raw = tex.raw;

        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(raw));
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            let (_, external_format, ty) = self.gl_texture_format(tex.format);

            self.gl.tex_sub_image_3d(
                glow::TEXTURE_2D_ARRAY,
                0,
                x as i32,
                y as i32,
                layer as i32,
                width as i32,
                height as i32,
                1,
                external_format,
                ty,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );

            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        }

        Ok(())

    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        let vertex_shader = self.compile_shader(glow::VERTEX_SHADER, desc.vertex_source)?;
        let fragment_shader = self.compile_shader(glow::FRAGMENT_SHADER, desc.fragment_source)?;
        let program = self.link_program(&[vertex_shader, fragment_shader])?;


        self.pipelines.push(Some(GlPipeline { program, vert_layouts: desc.vert_layouts }));

        Ok(PipelineHandle((self.pipelines.len() - 1) as u32))
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

    fn get_texture(&self, handle: TextureHandle) -> anyhow::Result<&GlTexture> {
        self
            .textures
            .get(handle.0 as usize)
            .and_then(|handle| handle.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid texture handle: {:?}", handle))
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

    pub fn set_vertex_buffer(&mut self, handle: BufferHandle, binding: u32) -> anyhow::Result<()> {
        let binding_idx = binding as usize;

        if binding_idx >= self.current_vbos.len() {
            anyhow::bail!("vertex buffer binding {} is out of range", binding);
        }

        if self.current_vbos[binding_idx] == Some(handle) {
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
        
        let layout = {
            let pipeline = self.get_pipeline(crnt_pipeline)?;

            pipeline.vert_layouts.iter().find(|layout| layout.binding == binding)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("pipeline does not have vertex buffer layout for binding {}",
                        binding)
                })?
        };

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

        self.current_vbos[binding_idx] = Some(handle);

        Ok(())
    }

    pub fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        if self.current_pipeline == Some(handle) {
            return Ok(());
        }
        
        let pipeline = self.get_pipeline(handle)?;

        unsafe {
            self.gl.use_program(Some(pipeline.program));
            self.gl.bind_vertex_array(Some(self.global_vao));
        }
        self.current_pipeline = Some(handle);
        self.current_vbos = [None; MAX_VBOS];
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
        _binding: u32,
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
            let gl_target = self.gl_buffer_target(target);

            self.gl.bind_buffer(gl_target, Some(raw));
            self.gl.buffer_sub_data_u8_slice(gl_target, offset as i32, data);
        }

        // Conservative cache invalidation.
        match target {
            BufferTarget::Vertex => {
                self.current_vbos = [None; MAX_VBOS];
            }
            BufferTarget::Index => {
                self.current_ibo = None;
            }
            BufferTarget::Uniform => {}
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
    pub fn set_uniform_1i(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        value: i32,
    ) -> anyhow::Result<()> {
        self.set_pipeline(pipeline)?;

        let program = self.get_pipeline(pipeline)?.program;

        unsafe {
            if let Some(location) = self.gl.get_uniform_location(program, name) {
                self.gl.uniform_1_i32(Some(&location), value);
            }
        }

        Ok(())
    }


    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()> {
        unsafe {
            let offset_bytes =
                draw.index_offset as i32 * std::mem::size_of::<u32>() as i32;

            self.gl.draw_elements_instanced_base_vertex_base_instance(
                glow::TRIANGLES,
                draw.index_count as i32,
                glow::UNSIGNED_INT,
                offset_bytes,
                draw.inst_count as i32,
                draw.vertex_offset,
                draw.inst_offset,
            );
        }

        Ok(())
    }

    fn tex_kind_from_gl_kind(&self, kind: GlTextureKind) -> TextureKind {
        match kind {
            GlTextureKind::Texture2D => TextureKind::Texture2d, 
            GlTextureKind::Texture2DArray => TextureKind::TextureArray2d, 
        }
    }

    fn texture_get_kind(
        &mut self,
        texture: TextureHandle,
    ) -> anyhow::Result<TextureKind> {

        let tex = self.get_texture(texture)?;
        Ok(self.tex_kind_from_gl_kind(tex.kind))
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
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        OpenGLRenderer::write_buffer(self, handle, binding, offset, data)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        OpenGLRenderer::create_pipeline(self, desc)
    }

    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_pipeline(self, handle)
    }

    fn set_vertex_buffer(&mut self,handle: BufferHandle,  binding: u32) -> anyhow::Result<()> {
        OpenGLRenderer::set_vertex_buffer(self, handle, binding)
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
    fn set_uniform_1i(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        value: i32,
    ) -> anyhow::Result<()> {
        OpenGLRenderer::set_uniform_1i(self, pipeline, name, value)
    }

    fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()> {
        OpenGLRenderer::draw_indexed(self, draw)
    }

    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()> {
        OpenGLRenderer::draw_indexed_instanced(self, draw)
    }

    fn set_texture(
        &mut self,
        slot: u32,
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        OpenGLRenderer::set_texture(self, slot, texture)
    }

    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        OpenGLRenderer::create_texture(self, desc, data)
    }

    fn write_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> anyhow::Result<()> {
        OpenGLRenderer::write_texture(self, texture, x, y, width, height, data)
    }

    fn texture_gen_mipmap(
        &mut self, 
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        OpenGLRenderer::texture_gen_mipmap(self, texture)
    }
    fn create_texture_array(
        &mut self,
        desc: TextureArrayDesc,
    ) -> anyhow::Result<TextureHandle> {
        OpenGLRenderer::create_texture_array(self, desc)
    }
      fn write_texture_array_layer(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        layer: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> anyhow::Result<()> {
        OpenGLRenderer::write_texture_array_layer(
            self, texture,
            x,y, layer, width, height,
            pixels)
      }
      
      fn texture_get_kind(
          &mut self,
          texture: TextureHandle,
      ) -> anyhow::Result<TextureKind> {
        OpenGLRenderer::texture_get_kind(
            self, texture)
      }
}
