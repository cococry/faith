
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTarget {
    Vertex,
    Index,
    Uniform,
}

#[derive(Debug, Clone, Copy)]
pub enum BufferUsage {
    Static,
    Dynamic,
    Stream,
}

#[derive(Debug, Clone, Copy)]
pub struct BufferDesc {
    pub target: BufferTarget,
    pub usage: BufferUsage,
    pub size: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct DrawIndexed {
    pub index_count: u32,
    pub index_offset: u32,
    pub vertex_offset: i32,
}

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

#[derive(Debug, Clone, Copy)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

#[derive(Debug, Clone)]
pub struct VertexAttribute {
    /// Shader location.
    pub location: u32,

    /// Byte offset inside the vertex struct.
    pub offset: u32,

    /// Data format of the attribute.
    pub format: VertexFormat,
}

pub struct VertexBufferLayout {
    // Size of one vertex/instance in bytes
    pub stride : u32,

    // Per-vertex or per-instance.
    pub step_mode: VertexStepMode,

    // attributes of the vertex/instance
    pub attrs: Vec<VertexAttribute>,
}

pub struct PipelineDesc<'a> {
    pub vertex_source: &'a str,
    pub fragment_source: &'a str,

    pub vert_layout: VertexBufferLayout

}

use crate::graphics::{
    BufferHandle,
    Color,
    PipelineHandle,
};

pub trait GraphicsDevice {
    fn resize(&mut self, width: u32, height: u32);

    fn clear_color(&mut self, color: Color);

    fn begin_frame(&mut self);

    fn end_frame(&mut self) -> anyhow::Result<()>;

    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle>;

    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()>;

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle>;

    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()>;

    fn set_vertex_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()>;

    fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()>;

    fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()>;

    fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()>;
}
