use crate::graphics::GraphicsDevice;
use crate::graphics::opengl::OpenGLRenderer;
use crate::graphics::vulkan::VulkanRenderer;
use crate::platform::Platform;
use crate::graphics::{
    BufferDesc,
    BufferHandle,
    Color,
    DrawIndexed,
    PipelineDesc,
    PipelineHandle,
};

pub enum GraphicsBackend {
    OpenGL,
    Vulkan,
}

pub enum Renderer {
    OpenGL(OpenGLRenderer),
    Vulkan(VulkanRenderer),
}

impl Renderer {
    pub fn new(backend: GraphicsBackend, platform: &Platform) -> anyhow::Result<Self> {
        match backend {
            GraphicsBackend::OpenGL => {
                Ok(Self::OpenGL(OpenGLRenderer::new(platform)?))
            }
            GraphicsBackend::Vulkan => {
                Ok(Self::Vulkan(VulkanRenderer::new(platform)?))
            }
        }
    }

    fn device(&self) -> &dyn GraphicsDevice {
        match self {
            Self::OpenGL(renderer) => renderer,
            Self::Vulkan(renderer) => renderer,
        }
    }

    fn device_mut(&mut self) -> &mut dyn GraphicsDevice {
        match self {
            Self::OpenGL(renderer) => renderer,
            Self::Vulkan(renderer) => renderer,
        }
    }

      pub fn resize(&mut self, width: u32, height: u32) {
        self.device_mut().resize(width, height);
    }

    pub fn clear_color(&mut self, color: Color) {
        self.device_mut().clear_color(color);
    }

    pub fn begin_frame(&mut self) {
        self.device_mut().begin_frame();
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        self.device_mut().end_frame()
    }

    pub fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        self.device_mut().create_buffer(desc)
    }

    pub fn write_buffer(
        &mut self,
        handle: BufferHandle,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut().write_buffer(handle, offset, data)
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        self.device_mut().create_pipeline(desc)
    }

    pub fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        self.device_mut().set_pipeline(handle)
    }

    pub fn set_vertex_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        self.device_mut().set_vertex_buffer(handle)
    }

    pub fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        self.device_mut().set_index_buffer(handle)
    }

    pub fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        self.device_mut().set_uniform_2f(pipeline, name, x, y)
    }

    pub fn draw_indexed(&mut self, draw: DrawIndexed) -> anyhow::Result<()> {
        self.device_mut().draw_indexed(draw)
    }
}
