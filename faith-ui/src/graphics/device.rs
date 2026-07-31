/// Type of GPU buffer to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTarget {
    Unspecified,
    Vertex,
    Index,
}

/// Expected update frequency of a GPU buffer.
#[derive(Debug, Clone, Copy)]
pub enum BufferUsage {
    Static,
    Dynamic,
    Staging,
}

/// Description used to create a GPU buffer.
#[derive(Debug, Clone, Copy)]
pub struct BufferDesc<'a> {
    pub target: BufferTarget,
    pub usage: BufferUsage,
    pub size: usize,
    pub data: Option<&'a [u8]>,
}

/// Parameters for an indexed instanced draw
/// call.
#[derive(Debug, Clone, Copy)]
pub struct DrawIndexedInstanced {
    pub index_count: u32,
    pub index_offset: u32,
    pub vertex_offset: i32,
    pub inst_count: u32,
    pub inst_offset: u32,
}

/// Vertex attribute data format.
#[derive(Debug, Clone, Copy)]
pub enum VertexFormat {
    Float32x2,
    Float32x4,
}

/// Whether a vertex buffer advances per
/// vertex or per instance.
#[derive(Debug, Clone, Copy)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

/// One vertex attribute consumed by a shader.
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub location: u32,
    pub offset: u32,
    pub format: VertexFormat,
}

/// Vertex buffer layout description.
///
/// Describes how vertex or instance data is
/// read from a buffer binding.
#[derive(Debug, Clone)]
pub struct VertexBufferBindingLayout {
    pub stride: u32,
    pub binding: u32,
    pub step_mode: VertexStepMode,
    pub attrs: Vec<VertexAttribute>,
}

/// The data type of a uniform.
///
/// Describes the data type of a uniform binding,
/// corresponding to the binding's data type in
/// the shader.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum UniformBindingType {
    #[default]
    Sampler2dArray,
    Vec2,
}

/// A shader stage in which a uniform is read.
///
/// Describes a shader stage in which a uniform
/// will be read in the shader pipeline.
#[derive(Debug, Clone, Copy, Default)]
pub enum UniformBindingShaderStage {
    #[default]
    Vertex,
    Fragment,
}

#[derive(Default, Clone)]
pub struct UniformBinding {
    /// The binding for UniformBindingType::Sampler2dArray
    /// uniforms
    pub binding: i32,
    pub name: String,
    pub ty: UniformBindingType,
    pub stage: UniformBindingShaderStage,
    /// Uloaded for uniforms with float type data.
    /// UniformBindingType::Vec2 uploads
    /// `f_data[0]` and `f_data[1]`.
    pub f_data: [f32; 4],
}

/// Describes a builtin shader pipeline
///
/// This enum is used in PipelineDesc to
/// specify pre-defined, builtin shader
/// pipelines for a graphics pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinShaderPipeline {
    UiQuadAtlas,
    UiQuadDedicated,
}

/// Description used to create a graphics
/// pipeline.
pub struct PipelineDesc {
    pub shader: BuiltinShaderPipeline,

    pub vert_bindings: Vec<VertexBufferBindingLayout>,
    pub uniform_bindings: Vec<UniformBinding>,
}

/// Description used to create a 2D texture
/// array.
pub struct TextureArrayDesc {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub format: TextureFormat,
}

/// Pixel format of a GPU texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    Rgba8,
}

/// Description used to create a 2D texture.
#[derive(Debug, Clone, Copy)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

/// Arguments for GraphicsDevice::write_texture_array_layer
pub struct TextureArrayWrite {
    pub texture: TextureHandle,
    pub x: u32,
    pub y: u32,
    pub layer: u32,
    pub width: u32,
    pub height: u32,
}

pub use crate::graphics::{BufferHandle, Color, PipelineHandle, TextureHandle};

/// Common graphics device interface.
///
/// Implemented by backend-specific renderers
/// and used by higher-level rendering code to
/// create resources, upload data, bind state
/// and submit draw calls.
pub trait GraphicsDevice {
    /// Resizes the graphics viewport.
    fn resize(&mut self, width: u32, height: u32);

    /// Sets the color used when clearing the
    /// current frame.
    fn clear_color(&mut self, color: Color);

    /// Begins a new graphics frame.
    fn begin_frame(&mut self) -> anyhow::Result<()>;

    /// Ends the current graphics frame and
    /// presents it to the window surface.
    fn end_frame(&mut self) -> anyhow::Result<()>;

    /// Creates a GPU buffer from the given
    /// buffer description.
    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle>;

    /// Writes data into an existing GPU buffer
    /// at the given byte offset.
    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()>;

    /// Creates a graphics pipeline from shader
    /// sources and vertex buffer layouts.
    fn create_pipeline(&mut self, desc: PipelineDesc) -> anyhow::Result<PipelineHandle>;

    /// Binds the active graphics pipeline.
    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()>;

    /// Binds a vertex buffer to the given vertex
    /// buffer binding.
    fn set_vertex_buffer(&mut self, handle: BufferHandle, binding: u32) -> anyhow::Result<()>;

    /// Binds an index buffer.
    fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()>;

    /// Sets a vec2 float uniform on a pipeline.
    fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()>;

    /// Submits an indexed instanced draw call.
    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()>;

    /// Creates a 2D texture and optionally uploads
    /// initial pixel data.
    #[allow(dead_code)]
    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle>;

    /// Binds a texture to the given texture slot.
    fn set_texture(&mut self, slot: u32, texture: TextureHandle) -> anyhow::Result<()>;

    /// Creates a 2D texture array.
    fn create_texture_array(&mut self, desc: TextureArrayDesc) -> anyhow::Result<TextureHandle>;

    /// Writes pixel data into one layer of a 2D
    /// texture array.
    fn write_texture_array_layer(
        &mut self,
        write: TextureArrayWrite,
        pixels_rgba: &[u8],
    ) -> anyhow::Result<()>;
}
