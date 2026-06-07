mod vulkan;

use ash::{vk};

pub use vulkan::VulkanRenderer;

use crate::graphics::{BufferTarget};

struct VulkanBuffer {
    raw: vk::Buffer,
    alloc: vk_mem::Allocation,
    size: usize,
    target: BufferTarget, 
}

struct VulkanPipeline {
    raw: vk::Pipeline,
}
