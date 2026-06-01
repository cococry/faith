mod opengl;

pub use opengl::OpenGLRenderer;

use crate::graphics::{BufferTarget, TextureFormat, device::VertexBufferLayout};

struct GlBuffer {
    raw: glow::NativeBuffer,
    target: BufferTarget,
    size: usize,
}

struct GlTexture {
    raw: glow::NativeTexture,
    width: u32,
    height: u32,
    pub format: TextureFormat,
}

struct GlPipeline {
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vert_layouts: Vec<VertexBufferLayout> 
}
