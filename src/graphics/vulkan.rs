
use crate::graphics::device::DrawIndexedInstanced;
use crate::platform::Platform;
use crate::graphics::{BufferDesc, BufferHandle, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle};

pub struct VulkanRenderer;

impl VulkanRenderer {
    pub fn new(_platform: &Platform) -> anyhow::Result<Self> {
        anyhow::bail!("Vulkan rendering backend not implemented yet");
    }
}

impl GraphicsDevice for VulkanRenderer {
    fn resize(&mut self, _width: u32, _height: u32) {}

    fn clear_color(&mut self, _color: Color) {}

    fn begin_frame(&mut self) {}

    fn end_frame(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan renderer not implemented yet")
    }

    fn create_buffer(&mut self, _desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        anyhow::bail!("Vulkan create_buffer not implemented yet")
    }

    fn write_buffer(
        &mut self,
        _handle: BufferHandle,
        _binding: u32,
        _offset: usize,
        _data: &[u8],
    ) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan write_buffer not implemented yet")
    }

    fn create_pipeline(&mut self, _desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        anyhow::bail!("Vulkan create_pipeline not implemented yet")
    }

    fn set_pipeline(&mut self, _handle: PipelineHandle) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan set_pipeline not implemented yet")
    }

    fn set_vertex_buffer(&mut self, _handle: BufferHandle, _binding: u32) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan set_vertex_buffer not implemented yet")
    }

    fn set_index_buffer(&mut self, _handle: BufferHandle) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan set_index_buffer not implemented yet")
    }

    fn set_uniform_2f(
        &mut self,
        _pipeline: PipelineHandle,
        _name: &str,
        _x: f32,
        _y: f32,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan set_uniform_2f not implemented yet")
    }

    fn draw_indexed(&mut self, _draw: DrawIndexed) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan draw_indexed not implemented yet")
    }
    
    fn draw_indexed_instanced(&mut self, _draw: DrawIndexedInstanced) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan draw_indexed_instanced not implemented yet")
    }
}
