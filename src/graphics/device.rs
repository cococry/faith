// Type of GPU buffer to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTarget {
    Vertex,
    Index,
    Uniform,
}

// Expected update frequency of a GPU buffer.
#[derive(Debug, Clone, Copy)]
pub enum BufferUsage {
    Static,
    Dynamic,
    Stream,
}

// Description used to create a GPU buffer.
#[derive(Debug, Clone, Copy)]
pub struct BufferDesc {
    pub target: BufferTarget,
    pub usage: BufferUsage,
    pub size: usize,
}

// Parameters for an indexed draw call.
#[derive(Debug, Clone, Copy)]
pub struct DrawIndexed {
    pub index_count: u32,
    pub index_offset: u32,
    pub vertex_offset: i32,
}

// Parameters for an indexed instanced draw 
// call.
#[derive(Debug, Clone, Copy)]
pub struct DrawIndexedInstanced {
    pub index_count: u32,
    pub index_offset: u32,
    pub vertex_offset: i32,
    pub inst_count: u32,
    pub inst_offset: u32,
}

// Vertex attribute data format.
#[derive(Debug, Clone, Copy)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint32x2,
    Uint32x3,
    Uint32x4,
    Unorm8x4,
}

// Whether a vertex buffer advances per 
// vertex or per instance.
#[derive(Debug, Clone, Copy)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

// One vertex attribute consumed by a shader.
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub location: u32,
    pub offset: u32,
    pub format: VertexFormat,
}

// Vertex buffer layout description.
//
// Describes how vertex or instance data is 
// read from a buffer binding.
#[derive(Debug, Clone)]
pub struct VertexBufferLayout {
    pub stride : u32,
    pub binding: u32,
    pub step_mode: VertexStepMode,
    pub attrs: Vec<VertexAttribute>,
}

// Description used to create a graphics 
// pipeline.
pub struct PipelineDesc<'a> {
    pub vertex_source: &'a str,
    pub fragment_source: &'a str,

    pub vert_layouts: Vec<VertexBufferLayout>
}

// Description used to create a 2D texture 
// array.
pub struct TextureArrayDesc {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub format: TextureFormat,
}

// Type of GPU texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKind {
    Texture2d,
    TextureArray2d,
}

// Pixel format of a GPU texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat{
    Rgba8,
    Alpha8,
}


// Description used to create a 2D texture.
#[derive(Debug, Clone, Copy)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

pub use crate::graphics::{
    BufferHandle,
    Color,
    PipelineHandle, TextureHandle,
};

// Common graphics device interface.
//
// Implemented by backend-specific renderers 
// and used by higher-level rendering code to 
// create resources, upload data, bind state 
// and submit draw calls.
pub trait GraphicsDevice {
    // Resizes the graphics viewport.
    fn resize(&mut self, width: u32, height: u32);

    // Sets the color used when clearing the 
    // current frame.
    fn clear_color(&mut self, color: Color);

    // Begins a new graphics frame.
    fn begin_frame(&mut self);

    // Ends the current graphics frame and 
    // presents it to the window surface.
    fn end_frame(&mut self) -> anyhow::Result<()>;

    // Creates a GPU buffer from the given 
    // buffer description.
    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle>;

    // Writes data into an existing GPU buffer 
    // at the given byte offset.
    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()>;

    // Creates a graphics pipeline from shader 
    // sources and vertex buffer layouts.
    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle>;

    // Binds the active graphics pipeline.
    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()>;

    // Binds a vertex buffer to the given vertex 
    // buffer binding.
    fn set_vertex_buffer(&mut self, handle: BufferHandle, binding: u32) -> anyhow::Result<()>;

    // Binds an index buffer.
    fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()>;

    // Sets a vec2 float uniform on a pipeline.
    fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()>;

    // Sets an integer uniform on a pipeline.
    fn set_uniform_1i(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        value: i32,
    ) -> anyhow::Result<()>;

    // Submits an indexed draw call.
    fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()>;

    // Submits an indexed instanced draw call.
    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()>;

    // Creates a 2D texture and optionally uploads 
    // initial pixel data.
    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle>;

    // Writes pixel data into an existing 2D 
    // texture region.
    fn write_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8]) -> anyhow::Result<()>;

    // Binds a texture to the given texture slot.
    fn set_texture(&mut self,
        slot: u32,
        texture: TextureHandle)
        -> anyhow::Result<()>;

    // Generates mipmaps for a texture.
    fn texture_gen_mipmap(
        &mut self, 
        texture: TextureHandle,
    ) -> anyhow::Result<()>;

    // Creates a 2D texture array.
    fn create_texture_array(
        &mut self,
        desc: TextureArrayDesc,
    ) -> anyhow::Result<TextureHandle>;

    // Writes pixel data into one layer of a 2D 
    // texture array.
    fn write_texture_array_layer(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        layer: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> anyhow::Result<()>;

    // Returns the kind of texture represented by 
    // the given texture handle.
    fn texture_get_kind(
        &mut self,
        texture: TextureHandle,
    ) -> anyhow::Result<TextureKind>;

    // Returns the current graphics surface size.
    fn size(&self) -> (u32, u32);
}
