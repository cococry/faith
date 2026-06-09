use anyhow::Context;

use crate::graphics::{
    BufferDesc, BufferHandle, Color, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc,
    TextureHandle,
};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureArrayWrite};
use crate::graphics::opengl::OpenGLRenderer;
use crate::graphics::vulkan::VulkanRenderer;
use crate::platform::Platform;

/// Available graphics rendering backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsBackend {
    Auto,
    OpenGL,
    Vulkan,
}

/// Graphics-backend renderer implementation.
pub enum Renderer {
    OpenGL(Box<OpenGLRenderer>),
    Vulkan(Box<VulkanRenderer>),
}

impl Renderer {
    pub fn new(backend: GraphicsBackend, platform: &Platform) -> anyhow::Result<Self> {
        match backend {
            GraphicsBackend::Auto => match Self::try_new_vulkan(platform) {
                Ok(renderer) => {
                    tracing::info!("Selected Vulkan rendering backend");
                    Ok(renderer)
                }

                Err(vk_err) => {
                    tracing::warn!(
                        "Vulkan backend unavailable, falling back to OpenGL: {vk_err:?}"
                    );

                    match Self::try_new_opengl(platform) {
                        Ok(renderer) => {
                            tracing::info!("Selected OpenGL rendering backend");
                            Ok(renderer)
                        }

                        Err(gl_err) => Err(anyhow::anyhow!(
                            "No graphics backend could be initialized.\n\
                                     Vulkan error: {vk_err:?}\n\
                                     OpenGL error: {gl_err:?}"
                        )),
                    }
                }
            },

            GraphicsBackend::Vulkan => {
                Self::try_new_vulkan(platform).context("failed to initialize Vulkan backend")
            }

            GraphicsBackend::OpenGL => {
                Self::try_new_opengl(platform).context("failed to initialize OpenGL backend")
            }
        }
    }

    fn try_new_vulkan(platform: &Platform) -> anyhow::Result<Self> {
        Ok(Self::Vulkan(Box::new(VulkanRenderer::new(platform)?)))
    }

    fn try_new_opengl(platform: &Platform) -> anyhow::Result<Self> {
        Ok(Self::OpenGL(Box::new(OpenGLRenderer::new(platform)?)))
    }

    fn device_mut(&mut self) -> &mut dyn GraphicsDevice {
        match self {
            Self::OpenGL(renderer) => renderer.as_mut(),
            Self::Vulkan(renderer) => renderer.as_mut(),
        }
    }
}

impl GraphicsDevice for Renderer {
    fn resize(&mut self, width: u32, height: u32) {
        self.device_mut().resize(width, height);
    }

    fn clear_color(&mut self, color: Color) {
        self.device_mut().clear_color(color);
    }

    fn begin_frame(&mut self) -> anyhow::Result<()> {
        self.device_mut().begin_frame()
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        self.device_mut().end_frame()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        self.device_mut().create_buffer(desc)
    }

    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut()
            .write_buffer(handle, binding, offset, data)
    }

    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        self.device_mut().create_texture(desc, data)
    }

    fn set_texture(&mut self, slot: u32, texture: TextureHandle) -> anyhow::Result<()> {
        self.device_mut().set_texture(slot, texture)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> anyhow::Result<PipelineHandle> {
        self.device_mut().create_pipeline(desc)
    }

    fn set_pipeline(&mut self, handle: PipelineHandle) -> anyhow::Result<()> {
        self.device_mut().set_pipeline(handle)
    }

    fn set_vertex_buffer(&mut self, handle: BufferHandle, binding: u32) -> anyhow::Result<()> {
        self.device_mut().set_vertex_buffer(handle, binding)
    }

    fn set_index_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        self.device_mut().set_index_buffer(handle)
    }

    fn set_uniform_2f(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        self.device_mut().set_uniform_2f(pipeline, name, x, y)
    }

    fn draw_indexed_instanced(&mut self, draw: DrawIndexedInstanced) -> anyhow::Result<()> {
        self.device_mut().draw_indexed_instanced(draw)
    }

    fn create_texture_array(&mut self, desc: TextureArrayDesc) -> anyhow::Result<TextureHandle> {
        self.device_mut().create_texture_array(desc)
    }

    fn write_texture_array_layer(
        &mut self,
        write: TextureArrayWrite,
        pixels_rgba: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut()
            .write_texture_array_layer(write, pixels_rgba)
    }
}
