/// Opaque handle to a GPU buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferHandle(pub(crate) u32);

/// Opaque handle to a GPU texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub(crate) u32);

/// Opaque handle to a GPU pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineHandle(pub(crate) u32);

/// Opaque handle to a loaded font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontHandle(pub(crate) u32);
