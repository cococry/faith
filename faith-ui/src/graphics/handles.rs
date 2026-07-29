/// Opaque handle to a graphics buffer created by the graphics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferHandle(pub(crate) u32);

/// Opaque handle to a texture created by the graphics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub(crate) u32);

/// Opaque handle to a graphics pipeline created by the graphics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineHandle(pub(crate) u32);

/// Opaque handle to a font created by `font.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontHandle(pub(crate) u32);
