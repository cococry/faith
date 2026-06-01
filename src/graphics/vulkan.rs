
use crate::platform::Platform;
use crate::graphics::Color;

pub struct VulkanRenderer;

impl VulkanRenderer {
    pub fn new(_platform: &Platform) -> anyhow::Result<Self> {
        anyhow::bail!("Vulkan rendering backend not implemented yet");
    }
    pub fn resize(&mut self, _width: u32, _height: u32) {
    }

    pub fn begin_frame(&mut self) {
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan rendering backend not implemented yet");
    }
    pub fn clear_color(&mut self, _color: Color) {
    }
}
