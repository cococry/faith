mod vulkan;

use ash::{vk};

pub use vulkan::VulkanRenderer;

use crate::graphics::{BufferTarget, BufferUsage};

struct VulkanBuffer {
    raw: vk::Buffer,
    alloc: vk_mem::Allocation,

    size: usize,
    target: BufferTarget,
    usage: BufferUsage,

    vk_usage: vk::BufferUsageFlags,
    mem_props: vk::MemoryPropertyFlags,

    mapped: Option<*mut std::ffi::c_void>,
}

pub struct VulkanPipeline {
    pub raw: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub desc_layout: vk::DescriptorSetLayout,
    pub desc_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,
}
