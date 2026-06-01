use crate::platform::Platform;
use crate::graphics::Color;

pub enum GraphicsBackend {
    OpenGL,
    Vulkan
}

pub enum Renderer {
    OpenGL(super::opengl::OpenGLRenderer),
    Vulkan(super::vulkan::VulkanRenderer),
}

impl Renderer {
    pub fn new(backend: GraphicsBackend, platform: &Platform) -> anyhow::Result<Self> {
        match backend {
            GraphicsBackend::OpenGL => {
                Ok(Self::OpenGL(super::opengl::OpenGLRenderer::new(platform)?))
            }
            GraphicsBackend::Vulkan => {
                Ok(Self::Vulkan(super::vulkan::VulkanRenderer::new(platform)?))
            }
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::OpenGL(render) => render.resize(width, height),
            Self::Vulkan(render) => render.resize(width, height),
        }
    }

    pub fn clear_color(&mut self, color: Color) {
        match self {
            Self::OpenGL(render) => render.clear_color(color),
            Self::Vulkan(render) => render.clear_color(color),
        }
    }
    
    pub fn begin_frame(&mut self) {
        match self {
            Self::OpenGL(render) => render.begin_frame(),
            Self::Vulkan(render) => render.begin_frame(),
        }
    }
    
    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        match self {
            Self::OpenGL(render) => render.end_frame(),
            Self::Vulkan(render) => render.end_frame(),
        }
    }
}
