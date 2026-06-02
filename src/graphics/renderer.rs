
use anyhow::anyhow;

use crate::graphics::{
    BufferDesc,
    BufferHandle,
    Color,
    DrawIndexed,
    GraphicsDevice,
    PipelineDesc,
    PipelineHandle,
    TextureDesc,
    TextureHandle,
};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind};
use crate::graphics::opengl::OpenGLRenderer;
use crate::graphics::vulkan::VulkanRenderer;
use crate::platform::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsBackend {
    OpenGL,
    Vulkan,
}

pub enum Renderer {
    OpenGL(OpenGLRenderer),
    Vulkan(VulkanRenderer),
}

impl Renderer {
    pub fn new(
        backend: GraphicsBackend,
        platform: &Platform,
    ) -> anyhow::Result<Self> {
        match backend {
            GraphicsBackend::OpenGL => {
                Ok(Self::OpenGL(OpenGLRenderer::new(platform)?))
            }

            GraphicsBackend::Vulkan => {
                Ok(Self::Vulkan(VulkanRenderer::new(platform)?))
            }
        }
    }

    fn device_mut(&mut self) -> &mut dyn GraphicsDevice {
        match self {
            Self::OpenGL(renderer) => renderer,
            Self::Vulkan(renderer) => renderer,
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

    fn begin_frame(&mut self) {
        self.device_mut().begin_frame();
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        self.device_mut().end_frame()
    }

    fn create_buffer(
        &mut self,
        desc: BufferDesc,
    ) -> anyhow::Result<BufferHandle> {
        self.device_mut().create_buffer(desc)
    }

    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut().write_buffer(handle, binding, offset, data)
    }

    fn create_texture(
        &mut self,
        desc: TextureDesc,
        data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        self.device_mut().create_texture(desc, data)
    }

    fn write_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut()
            .write_texture(texture, x, y, width, height, data)
    }

    fn set_texture(
        &mut self,
        slot: u32,
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        self.device_mut().set_texture(slot, texture)
    }

    fn create_pipeline(
        &mut self,
        desc: PipelineDesc<'_>,
    ) -> anyhow::Result<PipelineHandle> {
        self.device_mut().create_pipeline(desc)
    }

    fn set_pipeline(
        &mut self,
        handle: PipelineHandle,
    ) -> anyhow::Result<()> {
        self.device_mut().set_pipeline(handle)
    }

    fn set_vertex_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
    ) -> anyhow::Result<()> {
        self.device_mut().set_vertex_buffer(handle, binding)
    }

    fn set_index_buffer(
        &mut self,
        handle: BufferHandle,
    ) -> anyhow::Result<()> {
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
    fn set_uniform_1i(
        &mut self,
        pipeline: PipelineHandle,
        name: &str,
        value: i32,
    ) -> anyhow::Result<()> {
        self.device_mut().set_uniform_1i(pipeline, name, value)
    }


    fn draw_indexed(
        &mut self,
        draw: DrawIndexed,
    ) -> anyhow::Result<()> {
        self.device_mut().draw_indexed(draw)
    }

    fn draw_indexed_instanced(
        &mut self,
        draw: DrawIndexedInstanced,
    ) -> anyhow::Result<()> {
        self.device_mut().draw_indexed_instanced(draw)
    }

    fn texture_gen_mipmap(
        &mut self, 
        texture: TextureHandle,
    ) -> anyhow::Result<()> {
        self.device_mut().texture_gen_mipmap(texture)
    }
    fn create_texture_array(
        &mut self,
        desc: TextureArrayDesc,
    ) -> anyhow::Result<TextureHandle> {
        self.device_mut().create_texture_array(desc)
    }

    fn write_texture_array_layer(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        layer: u32,
        width: u32,
        height: u32,
        pixels_rgba: &[u8],
    ) -> anyhow::Result<()> {
        self.device_mut().write_texture_array_layer(
            texture, x, y, layer, 
            width, height, pixels_rgba
        )
    }

    fn texture_get_kind(
        &mut self,
        texture: TextureHandle,
    ) -> anyhow::Result<TextureKind> {
        self.device_mut().texture_get_kind(texture)
    }


}
