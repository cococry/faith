mod opengl;

pub use opengl::OpenGLRenderer;

use crate::graphics::{BufferTarget, device::VertexBufferLayout};

struct GlBuffer {
    raw: glow::NativeBuffer,
    target: BufferTarget,
    size: usize,
}

struct GlTexture {
    raw: glow::NativeTexture,
    width: u32,
    height: u32,
}

struct GlPipeline {
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    vert_layouts: Vec<VertexBufferLayout> 
}
