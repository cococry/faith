mod opengl;

pub use opengl::OpenGLRenderer;

use crate::graphics::{BufferTarget, TextureFormat, device::VertexBufferBindingLayout};
/// OpenGL buffer resource.
struct GlBuffer {
    raw: glow::NativeBuffer,
    target: BufferTarget,
    size: usize,
}

/// Type of OpenGL texture resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlTextureKind {
    Texture2D,
    Texture2DArray,
}

/// OpenGL texture resource.
struct GlTexture {
    raw: glow::NativeTexture,
    width: u32,
    height: u32,
    pub format: TextureFormat,
    kind: GlTextureKind
}

/// OpenGL graphics pipeline resource.
///
/// Stores the linked shader program and the 
/// vertex buffer layouts used when binding 
/// vertex buffers.
/// Vertex array is not stored per pipeline, 
/// the vertex array is pipeline-global in 
/// the OpenGL backend. 
#[derive(Clone)]
struct GlPipeline {
    program: glow::NativeProgram,
    vert_layouts: Vec<VertexBufferBindingLayout> 
}
