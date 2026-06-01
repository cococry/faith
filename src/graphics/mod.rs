mod renderer;
mod opengl;
mod vulkan;

pub use renderer::{GraphicsBackend, Renderer};
pub mod color;
pub use color::Color; 

pub mod handles;
pub use handles::{
    BufferHandle,
    TextureHandle,
    PipelineHandle,
};
pub mod device;
pub use device::{
    BufferTarget,
    BufferUsage,
    BufferDesc,
    DrawIndexed,
    PipelineDesc,
    VertexStepMode,
    GraphicsDevice,
    DrawIndexedInstanced,
};
