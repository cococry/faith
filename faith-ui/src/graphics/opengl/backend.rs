extern crate khronos_egl as egl;
use std::ffi::c_void;

use anyhow::Ok;
use glow::HasContext;
use khronos_egl::{Context, Display, Surface};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, VertexFormat};
use crate::graphics::opengl::{GlBuffer, GlPipeline, GlTexture, GlTextureKind};
use crate::graphics::{
    BufferDesc, BufferHandle, BufferTarget, BufferUsage, BuiltinShaderPipeline, Color,
    GraphicsDevice, PipelineDesc, PipelineHandle, TextureArrayWrite, TextureDesc, TextureFormat,
    TextureHandle, VertexStepMode,
};
use crate::platform::Platform;
use crate::platform::window::WindowHandleInfo;

const MAX_VBOS: usize = 8;

pub const UI_QUAD_VERTEX_SHADER: &str = r#"
#version 330 core

layout(location = 0) in vec2 a_local_pos;
layout(location = 1) in vec2 a_local_uv;

layout(location = 2) in vec4 i_rect;
layout(location = 3) in vec4 i_color;
layout(location = 4) in vec4 i_uv;
layout(location = 5) in vec4 i_params;

uniform vec2 u_screen_size;

out vec2 v_local_pos;
out vec2 v_uv;
out vec4 v_color;
out vec4 v_rect;
out vec4 v_params;
flat out int v_layer;
flat out int v_kind;

void main() {
    vec2 pixel_pos = i_rect.xy + a_local_pos * i_rect.zw;

    vec2 ndc = vec2(
        (pixel_pos.x / u_screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / u_screen_size.y) * 2.0
    );

    vec2 uv_min = i_uv.xy;
    vec2 uv_max = i_uv.zw;

    v_local_pos = a_local_pos;
    v_uv = mix(uv_min, uv_max, a_local_uv);
    v_color = i_color;
    v_rect = i_rect;
    v_params = i_params;

    v_layer = int(i_params.z + 0.5);
    v_kind = int(v_params.w + 0.5);

    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

pub const UI_QUAD_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec2 v_uv;
in vec4 v_color;

flat in int v_layer;
flat in int v_kind;

uniform sampler2DArray u_texture_array;

out vec4 out_color;

void main() {
    // 0 = solid rect
    // 1 = text glyph alpha mask
    // 2 = emoji/atlas image 

    if (v_kind == 0) {
        out_color = v_color;
        return;
    }

    vec4 tex_color = texture(u_texture_array, vec3(v_uv, float(v_layer)));

    if (v_kind == 1) {
        // text glyph: use texture alpha only
        out_color = vec4(v_color.rgb, v_color.a * tex_color.a);
    } else {
        // emoji/image: use full RGBA texture
        out_color = tex_color * v_color;
    }
}
"#;

pub const UI_QUAD_FRAGMENT_SHADER_DEDICATED: &str = r#"
#version 330 core

in vec2 v_uv;
in vec4 v_color;

uniform sampler2D u_texture;

out vec4 out_color;

void main() {
    vec4 tex_color = texture(u_texture, v_uv);
    out_color = tex_color * v_color;
}
"#;

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
        tracing::info!("Using OpenGL rendering backend");

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
        let egl_display = unsafe { egl.get_display(native_display) }
            .ok_or_else(|| anyhow::anyhow!("failed to get EGL display from native display"))?;

        egl.initialize(egl_display)?;

        egl.bind_api(egl::OPENGL_API)?;

        let attributes = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            0,
            egl::STENCIL_SIZE,
            0,
            egl::NONE,
        ];

        let config = egl
            .choose_first_config(egl_display, &attributes)?
            .ok_or_else(|| anyhow::anyhow!("unable to find an appropriate EGL configuration"))?;

        let major = 4;
        let minor = 0;
        let context_attributes = [
            egl::CONTEXT_MAJOR_VERSION,
            major,
            egl::CONTEXT_MINOR_VERSION,
            minor,
            egl::CONTEXT_OPENGL_PROFILE_MASK,
            egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];

        let context = egl.create_context(egl_display, config, None, &context_attributes)?;

        tracing::info!(
            "Created OpenGL Core profile context (version {}.{})",
            major,
            minor
        );

        let egl_surface =
            unsafe { egl.create_window_surface(egl_display, config, native_window, None) }?;

        egl.make_current(
            egl_display,
            Some(egl_surface),
            Some(egl_surface),
            Some(context),
        )?;

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
            let vao = gl
                .create_vertex_array()
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

    pub fn begin_frame(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        Ok(())
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        self.egl.swap_buffers(self.egl_display, self.egl_surface)?;
        Ok(())
    }

    fn gl_buffer_target(&self, target: BufferTarget) -> u32 {
        match target {
            BufferTarget::Vertex => glow::ARRAY_BUFFER,
            BufferTarget::Index => glow::ELEMENT_ARRAY_BUFFER,
            BufferTarget::Unspecified => 0,
        }
    }

    fn gl_buffer_usage(&self, usage: BufferUsage) -> u32 {
        match usage {
            BufferUsage::Static => glow::STATIC_DRAW,
            BufferUsage::Dynamic => glow::DYNAMIC_DRAW,
            BufferUsage::Staging => glow::DYNAMIC_DRAW,
        }
    }

    pub fn create_buffer(&mut self, desc: BufferDesc<'_>) -> anyhow::Result<BufferHandle> {
        unsafe {
            let raw = self
                .gl
                .create_buffer()
                .map_err(|e| anyhow::anyhow!("failed to create OpenGL buffer: {e}"))?;

            let target = self.gl_buffer_target(desc.target);
            let usage = self.gl_buffer_usage(desc.usage);

            self.gl.bind_buffer(target, Some(raw));

            if let Some(data) = desc.data {
                self.gl.buffer_data_u8_slice(target, data, usage);
            } else {
                self.gl.buffer_data_size(target, desc.size as i32, usage);
            }

            self.gl.bind_buffer(target, None);

            let handle = BufferHandle(self.buffers.len() as u32);

            self.buffers.push(Some(GlBuffer {
                raw,
                target: desc.target,
                size: desc.size,
            }));

            Ok(handle)
        }
    }

    fn compile_shader(&self, kind: u32, source: &str) -> anyhow::Result<glow::NativeShader> {
        unsafe {
            let shader = self
                .gl
                .create_shader(kind)
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
            let program = self
                .gl
                .create_program()
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
            VertexFormat::Float32x2 => (2, glow::FLOAT, false),
            VertexFormat::Float32x4 => (4, glow::FLOAT, false),
        }
    }

    fn gl_texture_format(&self, format: TextureFormat) -> (u32, u32, u32) {
        match format {
            TextureFormat::Rgba8 => (glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE),
        }
    }

    #[allow(dead_code)]
    pub fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        unsafe {
            let raw = self
                .gl
                .create_texture()
                .map_err(|e| anyhow::anyhow!("Failed to create OpenGL texture: {e}"))?;

            let (internal_format, format, gl_type) = self.gl_texture_format(desc.format);

            self.gl.bind_texture(glow::TEXTURE_2D, Some(raw));

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
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
                kind: super::GlTextureKind::Texture2D,
            }));

            Ok(handle)
        }
    }

    pub fn set_texture(&mut self, slot: u32, texture: TextureHandle) -> anyhow::Result<()> {
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
            let raw = self
                .gl
                .create_texture()
                .map_err(|e| anyhow::anyhow!("Failed to create OpenGL texture array: {e}"))?;

            let (internal_format, format, gl_type) = self.gl_texture_format(desc.format);

            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(raw));

            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D_ARRAY,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
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
                kind: super::GlTextureKind::Texture2DArray,
            }));

            Ok(handle)
        }
    }

    fn write_texture_array_layer(
        &mut self,
        write: TextureArrayWrite,
        pixels_rgba: &[u8],
    ) -> anyhow::Result<()> {
        let tex = self.get_texture(write.texture)?;

        if write.x + write.width > tex.width || write.y + write.height > tex.height {
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
                write.x as i32,
                write.y as i32,
                write.layer as i32,
                write.width as i32,
                write.height as i32,
                1,
                external_format,
                ty,
                glow::PixelUnpackData::Slice(Some(pixels_rgba)),
            );

            self.gl.bind_texture(glow::TEXTURE_2D_ARRAY, None);
        }

        Ok(())
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc) -> anyhow::Result<PipelineHandle> {
        let (vertex_source, fragment_source) = match desc.shader {
            BuiltinShaderPipeline::UiQuadAtlas => (UI_QUAD_VERTEX_SHADER, UI_QUAD_FRAGMENT_SHADER),
            BuiltinShaderPipeline::UiQuadDedicated => {
                (UI_QUAD_VERTEX_SHADER, UI_QUAD_FRAGMENT_SHADER_DEDICATED)
            }
        };

        let vertex_shader = self.compile_shader(glow::VERTEX_SHADER, vertex_source)?;
        let fragment_shader = self.compile_shader(glow::FRAGMENT_SHADER, fragment_source)?;
        let program = self.link_program(&[vertex_shader, fragment_shader])?;

        unsafe { self.gl.use_program(Some(program)) };

        let pipeline = Some(GlPipeline {
            program,
            vert_layouts: desc.vert_bindings,
        });
        self.pipelines.push(pipeline);

        let pipeline = PipelineHandle((self.pipelines.len() - 1) as u32);

        for uniform in desc.uniform_bindings {
            match uniform.ty {
                crate::graphics::device::UniformBindingType::Vec2 => {
                    self.set_uniform_2f(
                        pipeline,
                        &uniform.name,
                        uniform.f_data[0],
                        uniform.f_data[1],
                    )?;
                }
                crate::graphics::device::UniformBindingType::Sampler2dArray => {
                    self.set_uniform_1i(pipeline, &uniform.name, uniform.binding)?;
                }
            }
        }
        Ok(pipeline)
    }

    fn get_texture(&self, handle: TextureHandle) -> anyhow::Result<&GlTexture> {
        self.textures
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

        let crnt_pipeline = self
            .current_pipeline
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

            pipeline
                .vert_layouts
                .iter()
                .find(|layout| layout.binding == binding)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "pipeline does not have vertex buffer layout for binding {}",
                        binding
                    )
                })?
        };

        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer_raw));

            for attr in &layout.attrs {
                let (components, gl_type, normalized) = self.gl_vertex_format(attr.format);

                self.gl.enable_vertex_attrib_array(attr.location);

                self.gl.vertex_attrib_pointer_f32(
                    attr.location,
                    components,
                    gl_type,
                    normalized,
                    layout.stride as i32,
                    attr.offset as i32,
                );

                let divisor = match layout.step_mode {
                    VertexStepMode::Instance => 1,
                    VertexStepMode::Vertex => 0,
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
            self.gl
                .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(buffer_raw));
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
            self.gl
                .buffer_sub_data_u8_slice(gl_target, offset as i32, data);
        }

        // Conservative cache invalidation.
        match target {
            BufferTarget::Vertex => {
                self.current_vbos = [None; MAX_VBOS];
            }
            BufferTarget::Index => {
                self.current_ibo = None;
            }
            _ => {}
        }

        Ok(())
    }

    pub fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
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
            let offset_bytes = draw.index_offset as i32 * std::mem::size_of::<u32>() as i32;

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
}

impl Drop for OpenGLRenderer {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.egl_display, None, None, None);

        let _ = self.egl.destroy_surface(self.egl_display, self.egl_surface);

        let _ = self.egl.destroy_context(self.egl_display, self.egl_context);

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

    fn begin_frame(&mut self) -> anyhow::Result<()> {
        OpenGLRenderer::begin_frame(self)
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        OpenGLRenderer::end_frame(self)
    }

    fn create_buffer(&mut self, desc: BufferDesc<'_>) -> anyhow::Result<BufferHandle> {
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

    fn create_pipeline(&mut self, desc: PipelineDesc) -> anyhow::Result<PipelineHandle> {
        OpenGLRenderer::create_pipeline(self, desc)
    }

    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_pipeline(self, handle)
    }

    fn set_vertex_buffer(&mut self, handle: BufferHandle, binding: u32) -> anyhow::Result<()> {
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

    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()> {
        OpenGLRenderer::draw_indexed_instanced(self, draw)
    }

    fn set_texture(&mut self, slot: u32, texture: TextureHandle) -> anyhow::Result<()> {
        OpenGLRenderer::set_texture(self, slot, texture)
    }

    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        OpenGLRenderer::create_texture(self, desc, data)
    }

    fn create_texture_array(&mut self, desc: TextureArrayDesc) -> anyhow::Result<TextureHandle> {
        OpenGLRenderer::create_texture_array(self, desc)
    }

    fn write_texture_array_layer(
        &mut self,
        write: TextureArrayWrite,
        pixels_rgba: &[u8],
    ) -> anyhow::Result<()> {
        OpenGLRenderer::write_texture_array_layer(self, write, pixels_rgba)
    }
}
