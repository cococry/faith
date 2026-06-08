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
    FontHandle,
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
    TextureDesc,
    TextureFormat,
    UniformBindingType
};

pub mod image;

pub use image::ImageData;

pub mod font;

pub use font::FontManager;
