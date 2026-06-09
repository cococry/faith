mod vulkan;

use ash::{vk};

pub use vulkan::VulkanRenderer;

use crate::graphics::{BufferTarget, BufferUsage, TextureFormat};

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

    pub push_constant_size: u32,
    pub push_constant_stage_flags: vk::ShaderStageFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VulkanTextureKind {
    Texture2D,
    Texture2DArray,
}

pub struct VulkanTexture {
    pub image: vk::Image,
    pub alloc: vk_mem::Allocation,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,

    pub width: u32,
    pub height: u32,
    pub layers: u32,

    pub format: TextureFormat,
    pub vk_format: vk::Format,
    pub kind: VulkanTextureKind,

    pub layout: vk::ImageLayout,
}
