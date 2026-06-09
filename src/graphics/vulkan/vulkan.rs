use std::ffi::{CString, c_char, c_void};
use std::fs::File;
use std::io::Read;

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind, UniformBindingShaderStage, VertexAttribute, VertexBufferBindingLayout, VertexFormat};
use crate::graphics::vulkan::{VulkanBuffer, VulkanPipeline};
use crate::platform::{Platform};
use crate::graphics::{BufferDesc, BufferHandle, BufferTarget, BufferUsage, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc, TextureHandle, UniformBindingType, VertexStepMode};

use anyhow::Context;
use ash::vk::{ColorComponentFlags, DescriptorPoolCreateFlags, Extent2D, ImageViewCreateInfo, PipelineCache, QueueFlags };
use ash::{Entry, vk};
use vk_mem::{Alloc, AllocationCreateInfo};

const MAX_STAGING_RING_MEM: vk::DeviceSize = 1024 * 1024 * 256;
const FRAME_COUNT: usize = 2;

#[derive(Default)]
struct PendingResize {
    width: u32,
    height: u32,
    pending: bool,
}

pub struct StagingRing {
    buf: BufferHandle,
    head: vk::DeviceSize,
    cap: vk::DeviceSize
}

pub struct UploadContext {
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence
}

pub struct VulkanRenderer {
    instance: ash::Instance,
    logical_device: ash::Device,

    surface: vk::SurfaceKHR,
    surface_inst: ash::khr::surface::Instance,

    phys_dev: vk::PhysicalDevice,
    phys_dev_limits: vk::PhysicalDeviceLimits,

    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    graphics_queue_family_idx: u32,
    present_queue_family_idx: u32,

    swapchain: Swapchain,
    swapchain_dev:  ash::khr::swapchain::Device,

    frameloop: Frameloop,
    allocator: Option<vk_mem::Allocator>,

    pending_resize: PendingResize,

    width: u32,
    height: u32,

    skip_render: bool,
    clear_color: [f32; 4],

    buffers: Vec<Option<VulkanBuffer>>,
    pipelines: Vec<Option<VulkanPipeline>>,

    staging_ring: StagingRing,
    upload_ctx: UploadContext,
}

#[derive(Default)]
struct SwapchainInfo {
    present_modes: Vec<vk::PresentModeKHR>,
    surface_fmts: Vec<vk::SurfaceFormatKHR>,
    capabilities: vk::SurfaceCapabilitiesKHR,

    ideal_present_mode: vk::PresentModeKHR,
    ideal_surface_fmt: vk::SurfaceFormatKHR,
    ideal_depth_fmt: vk::Format,
}


#[derive(Default)]
struct Swapchain {
    img_count: u32,

    info: SwapchainInfo,

    handle: vk::SwapchainKHR, 
    images: Vec<vk::Image>,
    extent: Extent2D, 

    img_views: Vec<vk::ImageView>,
    img_views_depth: Vec<vk::ImageView>,
    depth_images: Vec<vk::Image>,
    depth_image_allocs: Vec<vk_mem::Allocation>,
    imgs_in_flight: Vec<vk::Fence>,

    image_idx: usize 
}

#[derive(Default)]
struct Frameloop {
    pub frame_idx: usize,
    pub crnt_pass: vk::RenderPass,
    pub fbs: Vec<vk::Framebuffer>,
    frames: [Frame; FRAME_COUNT], 
}

#[derive(Default)]
pub struct Frame {
    pub cmd_pool: vk::CommandPool,
    pub cmd_buf: vk::CommandBuffer,

    pub image_available: vk::Semaphore,
    pub render_finished_per_image: Vec<vk::Semaphore>,
    pub in_flight_fence: vk::Fence,
}

impl VulkanRenderer {
    fn check_exts(
        entry: &ash::Entry, 
    ) -> anyhow::Result<(bool, bool, bool, bool, bool)> {
        let exts = unsafe { entry.enumerate_instance_extension_properties(None)? };

        if exts.is_empty() {
            anyhow::bail!("No Vulkan instance extensions are supported.");
        }

        let mut have_ext_vk_khr_surface: bool = false;
        let mut have_ext_vk_khr_xcb_surface: bool = false;
        let mut have_ext_vk_khr_xlib_surface: bool = false;
        let mut have_ext_vk_khr_wayland_surface: bool = false;

        for ext in exts {
            match ext.extension_name_as_c_str()?.to_str()? {
                "VK_KHR_surface"            => have_ext_vk_khr_surface = true,
                "VK_KHR_xcb_surface"        => have_ext_vk_khr_xcb_surface = true,
                "VK_KHR_xlib_surface"       => have_ext_vk_khr_xlib_surface = true,
                "VK_KHR_wayland_surface"    => have_ext_vk_khr_wayland_surface = true,
                &_ => {}
            } 
        }

        Ok((true,
                have_ext_vk_khr_surface,
                have_ext_vk_khr_xcb_surface, 
                have_ext_vk_khr_xlib_surface, 
                have_ext_vk_khr_wayland_surface, 
        ))
    }

    fn pick_phys_device(
        instance: &ash::Instance,
        phys_dev: vk::PhysicalDevice,
        present_queue_family_idx: u32, 
        graphics_queue_family_idx: u32, 
    ) -> anyhow::Result<vk::PhysicalDeviceLimits> {
        let props = unsafe { instance.get_physical_device_properties(phys_dev) };

        tracing::info!(
            "Picked Vulkan physical device: (name: {}, API version: {}.{}.{}, 
            driver version: {}.{}.{}, present queue: {}, graphics queue: {})",
            props.device_name_as_c_str()?.to_str()?,
            vk::api_version_major(props.api_version), 
            vk::api_version_minor(props.api_version), 
            vk::api_version_patch(props.api_version), 
            vk::api_version_major(props.driver_version), 
            vk::api_version_minor(props.driver_version), 
            vk::api_version_patch(props.driver_version), 
            present_queue_family_idx,
            graphics_queue_family_idx
        );

        Ok(props.limits)
    }

    fn pick_phys_device_and_device_queues(
        instance: &ash::Instance,
        surface_inst: &ash::khr::surface::Instance,
        surface: &vk::SurfaceKHR,
    ) -> anyhow::Result<(u32, u32, vk::PhysicalDevice, vk::PhysicalDeviceLimits)> {
        let phys_devs = unsafe { instance.enumerate_physical_devices()? };

        let mut graphics_queue_family_idx: isize = -1;
        let mut present_queue_family_idx: isize = -1;

        for phys_dev in phys_devs {
            let queue_family_props = unsafe { 
                instance.get_physical_device_queue_family_properties(phys_dev) 
            };

            for (queue_family_index, queue_family) in queue_family_props.iter().enumerate() {
                if queue_family.queue_flags.contains(QueueFlags::GRAPHICS) {
                    graphics_queue_family_idx = queue_family_index as isize;
                }
                let device_supports_presentation = unsafe {
                    surface_inst.get_physical_device_surface_support(
                        phys_dev,
                        queue_family_index as u32,
                        *surface
                    )?
                };

                if device_supports_presentation {
                    present_queue_family_idx = queue_family_index as isize; 
                } 
            }

            if graphics_queue_family_idx >= 0 && present_queue_family_idx >= 0 {
                let limits = Self::pick_phys_device(instance, 
                    phys_dev, 
                    present_queue_family_idx as u32, 
                    graphics_queue_family_idx as u32)?;
                return Ok((
                        graphics_queue_family_idx as u32, 
                        present_queue_family_idx as u32, 
                        phys_dev,
                        limits
                ));
            }
        } 

        anyhow::bail!(
            "None of your GPUs support all Vulkan features needed for 
            rendering in the application window. Either a physical device (GPU) 
            queue for graphics or presentation is missing.");
    }

    fn string_vec_to_cstrings(
        vec: &Vec<String> 
    ) -> Vec<CString> {
        vec.iter()
            .map(|s| CString::new(s.as_str()).expect("String contains Nullbyte"))
            .collect()
    }

    fn create_logical_device(
        instance: &ash::Instance,
        phys_dev: &vk::PhysicalDevice,
        present_queue_family_idx: u32,
        graphics_queue_family_idx: u32
    ) -> anyhow::Result<(ash::Device, vk::Queue, vk::Queue)> {

        let queue_priority = [1.0_f32];

        let mut queues = Vec::new();

        queues.push(vk::DeviceQueueCreateInfo {
            queue_family_index: graphics_queue_family_idx,
            queue_count: 1,
            p_queue_priorities: queue_priority.as_ptr(),
            ..Default::default()
        });

        if graphics_queue_family_idx != present_queue_family_idx {
            queues.push(vk::DeviceQueueCreateInfo {
                queue_family_index: present_queue_family_idx,
                queue_count: 1,
                p_queue_priorities: queue_priority.as_ptr(),
                ..Default::default()
            });
        }

        let enabled_exts_vec: Vec<String> = vec!["VK_KHR_swapchain".to_string()];

        let enabled_exts = Self::string_vec_to_cstrings(&enabled_exts_vec);

        let enabled_exts_final: Vec<*const c_char> = enabled_exts 
            .iter()
            .map(|cs| cs.as_ptr())
            .collect();

        let device_info = vk::DeviceCreateInfo {
            enabled_extension_count: enabled_exts_final.len() as u32,
            pp_enabled_extension_names: enabled_exts_final.as_ptr(), 
            p_queue_create_infos: queues.as_ptr(),
            queue_create_info_count: queues.len() as u32,
            ..Default::default()
        };

        let logical_device = unsafe { instance.create_device(*phys_dev, &device_info, None)? };

        let graphics_queue = unsafe {
            logical_device.get_device_queue(graphics_queue_family_idx as u32, 0)
        };

        let present_queue = unsafe {
            logical_device.get_device_queue(present_queue_family_idx as u32, 0)
        };

        tracing::info!("Initialized Vulkan logical device (graphics queue index: {}, present queue index: {})",
        graphics_queue_family_idx, present_queue_family_idx);

        Ok((logical_device, graphics_queue, present_queue))
    }

    fn get_ideal_swapchain_surface_fmt(
        surface_fmts: &Vec<vk::SurfaceFormatKHR>
    ) -> anyhow::Result<vk::SurfaceFormatKHR> {
        for fmt in surface_fmts {
            if fmt.format == vk::Format::B8G8R8_SRGB && 
                fmt.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR {
                    return Ok(*fmt);
            }
        }
        if surface_fmts.is_empty() {
            anyhow::bail!("Physical device returned no surface formats.");
        }
        Ok(surface_fmts[0])
    }

    fn get_ideal_swapchain_present_mode(
        modes: &Vec<vk::PresentModeKHR>
    ) -> vk::PresentModeKHR {
        for mode in modes {
            if *mode == vk::PresentModeKHR::MAILBOX {
                return *mode;
            } 
        }
        vk::PresentModeKHR::FIFO
    }

    fn get_swapchain_extent(
        swap_info: &SwapchainInfo, 
        width: u32, 
        height: u32, 
    ) -> Extent2D {
        if swap_info.capabilities.current_extent.width != u32::MAX {
            return swap_info.capabilities.current_extent;
        }

        let mut extent = Extent2D {
            width, height
        };

        extent.width    = std::cmp::min(width, 
            swap_info.capabilities.max_image_extent.width); 
        extent.height   = std::cmp::min(height, 
            swap_info.capabilities.max_image_extent.height); 

        extent.width    = std::cmp::max(width, 
            swap_info.capabilities.min_image_extent.width); 
        extent.height   = std::cmp::max(height, 
            swap_info.capabilities.min_image_extent.height); 

        extent
    }

    fn get_swapchain_info(
        instance: &ash::Instance,
        phys_dev: &vk::PhysicalDevice,
        surface_inst: &ash::khr::surface::Instance,
        surface: &vk::SurfaceKHR,
    ) -> anyhow::Result<SwapchainInfo> {
        let capabilities    = unsafe { surface_inst.get_physical_device_surface_capabilities(*phys_dev, *surface)? };
        let surface_fmts    = unsafe { surface_inst.get_physical_device_surface_formats(*phys_dev, *surface)? };
        let present_modes   = unsafe { surface_inst.get_physical_device_surface_present_modes(*phys_dev, *surface)? };

        let ideal_present_mode  = Self::get_ideal_swapchain_present_mode(&present_modes);
        let ideal_surface_fmt   = Self::get_ideal_swapchain_surface_fmt(&surface_fmts)?;
        let ideal_depth_fmt     = Self::get_ideal_swapchain_depth_fmt(instance, phys_dev);

        Ok(SwapchainInfo { 
            present_modes, 
            surface_fmts, 
            capabilities,
            ideal_present_mode, 
            ideal_surface_fmt, 
            ideal_depth_fmt
        })
    }

    fn get_ideal_swapchain_depth_fmt(
        instance: &ash::Instance, 
        phys_dev: &vk::PhysicalDevice,
    ) -> vk::Format {
        let candidates: [vk::Format; 4] = [
            vk::Format::D32_SFLOAT,
            vk::Format::D32_SFLOAT_S8_UINT,
            vk::Format::D24_UNORM_S8_UINT,
            vk::Format::D16_UNORM,
        ];

        for candidate in candidates {
            let props = unsafe { 
                instance.get_physical_device_format_properties(
                    *phys_dev, candidate) 
            };

            if props.optimal_tiling_features.contains(
                vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT) {
                return candidate;
            }
        }

        vk::Format::UNDEFINED
    }

    fn get_depth_image_mask(
        fmt: vk::Format
    ) -> vk::ImageAspectFlags {
        match fmt {
            vk::Format::D32_SFLOAT_S8_UINT 
                | vk::Format::D24_UNORM_S8_UINT =>
                return vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL, 

            _ => return vk::ImageAspectFlags::DEPTH 
        }
    }

    fn create_swapchain(
        instance: &ash::Instance,
        swapchain: &mut Swapchain,
        swapchain_dev: &ash::khr::swapchain::Device,
        allocator: &vk_mem::Allocator,

        surface: &vk::SurfaceKHR,
        surface_inst: &ash::khr::surface::Instance,

        logical_dev: &ash::Device,
        phys_dev: &vk::PhysicalDevice,
        width: u32,
        height: u32,

        graphics_queue_family_idx: u32,
        present_queue_family_idx: u32
    ) -> anyhow::Result<()> {
        tracing::info!("Creating Vulkan swapchain...");

        for view in swapchain.img_views.drain(..) {
            unsafe {
                logical_dev.destroy_image_view(view, None);
            }
        }

        for view in swapchain.img_views_depth.drain(..) {
            unsafe {
                logical_dev.destroy_image_view(view, None);
            }
        }

        for (image, mut alloc) in swapchain.depth_images.drain(..)
            .zip(swapchain.depth_image_allocs.drain(..)) {
                unsafe {
                    allocator.destroy_image(image, &mut alloc);
                }
            }

        swapchain.info = Self::get_swapchain_info(instance, phys_dev, surface_inst, surface)?;

        if swapchain.info.ideal_depth_fmt == vk::Format::UNDEFINED {
            anyhow::bail!("No supported Vulkan depth format found for application window swapchain.");
        }
        if swapchain.info.ideal_surface_fmt.format == vk::Format::UNDEFINED {
            anyhow::bail!("No supported Vulkan surface format found.");
        }

        let extent = Self::get_swapchain_extent(&swapchain.info, 
            width, height);

        let mut img_count = swapchain.img_count.max(swapchain.info.capabilities.min_image_count);

        if swapchain.info.capabilities.max_image_count > 0 {
            img_count = img_count.min(swapchain.info.capabilities.max_image_count);
        }

        let old_swapchain = swapchain.handle; 

        let mut create_info = vk::SwapchainCreateInfoKHR {
            surface: *surface,
            min_image_count: img_count,
            image_format: swapchain.info.ideal_surface_fmt.format,
            image_color_space:   swapchain.info.ideal_surface_fmt.color_space,
            image_array_layers: 1,
            image_usage:  vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST,
            image_extent : extent, 
            pre_transform: swapchain.info.capabilities.current_transform,
            composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            present_mode: swapchain.info.ideal_present_mode,
            clipped: vk::TRUE, 
            old_swapchain, 
            ..Default::default()
        };

        let families = [graphics_queue_family_idx, present_queue_family_idx];

        if graphics_queue_family_idx != present_queue_family_idx {
            create_info.image_sharing_mode = vk::SharingMode::CONCURRENT;
            create_info.queue_family_index_count = 2;
            create_info.p_queue_family_indices = families.as_ptr();
        } else {
            create_info.image_sharing_mode = vk::SharingMode::EXCLUSIVE;
            create_info.queue_family_index_count = 0;
            create_info.p_queue_family_indices = std::ptr::null();
        }

        let swapchain_handle = unsafe { swapchain_dev.create_swapchain(
            &create_info, None)? };

        if old_swapchain != vk::SwapchainKHR::null() {
            unsafe {
                swapchain_dev.destroy_swapchain(old_swapchain, None);
            }
        }

        let images = unsafe { swapchain_dev.get_swapchain_images(swapchain_handle)? };

        swapchain.handle = swapchain_handle;

        swapchain.img_views.reserve(images.len());
        swapchain.img_views_depth.reserve(images.len());
        swapchain.depth_images.reserve(images.len());
        swapchain.depth_image_allocs.reserve(images.len());

        swapchain.imgs_in_flight.clear();
        swapchain.imgs_in_flight.resize(images.len(), vk::Fence::null());

        swapchain.extent = extent;

        swapchain.images = images;

        for &image in swapchain.images.iter() {
            let depth_image_info = vk::ImageCreateInfo {
                image_type: vk::ImageType::TYPE_2D,
                format: swapchain.info.ideal_depth_fmt,
                extent: vk::Extent3D { 
                    width: extent.width, 
                    height: extent.height, 
                    depth: 1 
                },
                mip_levels: 1,
                array_layers: 1,
                samples: vk::SampleCountFlags::TYPE_1,
                tiling: vk::ImageTiling::OPTIMAL,
                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                sharing_mode: vk::SharingMode::EXCLUSIVE,
                initial_layout: vk::ImageLayout::UNDEFINED,

                ..Default::default()
            };

            let depth_alloc_info = AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::AutoPreferDevice,
                preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                ..Default::default()
            };

            let (depth_image, depth_alloc) =  
                unsafe { allocator.create_image(&depth_image_info, &depth_alloc_info)? }; 

            let image_view_info = ImageViewCreateInfo {
                image, 
                view_type: vk::ImageViewType::TYPE_2D,
                format: swapchain.info.ideal_surface_fmt.format,
                components: vk::ComponentMapping { 
                    r: vk::ComponentSwizzle::IDENTITY,
                    g: vk::ComponentSwizzle::IDENTITY,
                    b: vk::ComponentSwizzle::IDENTITY,
                    a: vk::ComponentSwizzle::IDENTITY,
                }, 
                subresource_range: vk::ImageSubresourceRange { 
                    aspect_mask: vk::ImageAspectFlags::COLOR, 
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };
            let depth_image_view_info = ImageViewCreateInfo {
                image: depth_image, 
                view_type: vk::ImageViewType::TYPE_2D, 
                format: swapchain.info.ideal_depth_fmt,

                subresource_range: vk::ImageSubresourceRange { 
                    aspect_mask: Self::get_depth_image_mask(
                                     swapchain.info.ideal_depth_fmt), 
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };

            let img_view = match unsafe { logical_dev.create_image_view(&image_view_info, None) } {
                Ok(view) => view,
                Err(err) => {
                    unsafe {
                        let mut depth_alloc = depth_alloc;
                        allocator.destroy_image(depth_image, &mut depth_alloc);
                    }
                    return Err(err.into());
                }
            };

            let img_view_depth = match unsafe {
                logical_dev.create_image_view(&depth_image_view_info, None)
            } {
                Ok(view) => view,
                Err(err) => {
                    unsafe {
                        logical_dev.destroy_image_view(img_view, None);

                        let mut depth_alloc = depth_alloc;
                        allocator.destroy_image(depth_image, &mut depth_alloc);
                    }
                    return Err(err.into());
                }
            };

            swapchain.img_views.push(img_view);
            swapchain.img_views_depth.push(img_view_depth);
            swapchain.depth_images.push(depth_image);
            swapchain.depth_image_allocs.push(depth_alloc);
        }

        tracing::info!(
            "Initialized Vulkan swapchain (width: {}, height: {})",
            extent.width,
            extent.height
        );

        Ok(())

    }

    fn allocator(&self) -> anyhow::Result<&vk_mem::Allocator> {
        self.allocator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Vulkan allocator already destroyed"))
    }

    fn handle_resize(
        &mut self
    ) -> anyhow::Result<()> {
        if !self.pending_resize.pending {
            return Ok(());
        }

        unsafe {
            self.logical_device.device_wait_idle()?;
        }

        for fb in self.frameloop.fbs.drain(..) {
            unsafe {
                self.logical_device.destroy_framebuffer(fb, None);
            }
        }

        /* Absolute rust psychosis */
        let VulkanRenderer {
            instance,
            swapchain,
            swapchain_dev,
            allocator,
            surface,
            surface_inst,
            logical_device,
            phys_dev,
            pending_resize,
            graphics_queue_family_idx,
            present_queue_family_idx,
            ..
        } = self;

        let allocator = allocator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Vulkan allocator already destroyed"))?;

        Self::create_swapchain(
            instance,
            swapchain,
            swapchain_dev,
            allocator,
            surface,
            surface_inst,
            logical_device,
            phys_dev,
            pending_resize.width,
            pending_resize.height,
            *graphics_queue_family_idx,
            *present_queue_family_idx,
        )?;

        self.frameloop.fbs.reserve(self.swapchain.images.len());

        for (i, (&img_view, &depth_view)) in self.swapchain
            .img_views.iter().zip(self.swapchain.img_views_depth.iter()).enumerate() {
                let attachments = [
                    img_view,
                    depth_view,
                ];

                let fb_info = vk::FramebufferCreateInfo {
                    render_pass: self.frameloop.crnt_pass,
                    attachment_count: attachments.len() as u32,
                    p_attachments: attachments.as_ptr(),
                    width: self.swapchain.extent.width,
                    height: self.swapchain.extent.height,
                    layers: 1,
                    ..Default::default()
                };

                let framebuffer = unsafe {
                    self.logical_device.create_framebuffer(&fb_info, None)?
                };

                self.frameloop.fbs.push(framebuffer);

                tracing::info!(
                    "Initialized Vulkan frameloop framebuffer for swapchain image view {}",
                    i
                );
        }

        for frame in &mut self.frameloop.frames {
            unsafe {
                self.logical_device.reset_command_pool(
                    frame.cmd_pool,
                    vk::CommandPoolResetFlags::empty(),
                )?;
            }

            let sem_info = vk::SemaphoreCreateInfo {
                ..Default::default()
            };

            if frame.image_available != vk::Semaphore::null() {
                unsafe {
                    self.logical_device.destroy_semaphore(frame.image_available, None);
                }

                frame.image_available = vk::Semaphore::null();
            }

            for sem in frame.render_finished_per_image.drain(..) {
                unsafe {
                    self.logical_device.destroy_semaphore(sem, None);
                }
            }

            frame.image_available = unsafe {
                self.logical_device.create_semaphore(&sem_info, None)?
            };

            frame
                .render_finished_per_image
                .reserve(self.swapchain.images.len());

            for _ in 0..self.swapchain.images.len() {
                let sem = unsafe {
                    self.logical_device.create_semaphore(&sem_info, None)?
                };

                frame.render_finished_per_image.push(sem);
            }
        }

        self.width  = self.pending_resize.width;
        self.height = self.pending_resize.height;

        self.pending_resize.pending = false;
        self.frameloop.frame_idx    = 0;
        self.swapchain.image_idx    = 0;

        tracing::info!("Resized render viewport of application window: {}x{}px.", self.width, self.height);

        Ok(())
    }

    fn create_frameloop(
        frameloop: &mut Frameloop,
        swapchain: &Swapchain,
        logical_dev: &ash::Device,
        graphics_queue_family_idx: u32
    ) -> anyhow::Result<()> {
        tracing::info!("Creating Vulkan frameloop...");
        let pool_create_info = vk::CommandPoolCreateInfo {
            queue_family_index: graphics_queue_family_idx,
            flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            ..Default::default()
        };

        for (i, frame) in frameloop.frames.iter_mut().enumerate() {
            frame.cmd_pool = unsafe {
                logical_dev.create_command_pool(&pool_create_info, None)?
            };

            let buf_info = vk::CommandBufferAllocateInfo {
                command_pool: frame.cmd_pool,
                level: vk::CommandBufferLevel::PRIMARY,
                command_buffer_count: 1,
                ..Default::default()
            };

            let cmd_buffers = unsafe {
                logical_dev.allocate_command_buffers(&buf_info)?
            };

            frame.cmd_buf = *cmd_buffers
                .first()
                .ok_or_else(|| anyhow::anyhow!("vkAllocateCommandBuffers returned no command buffers"))?;

            let sem_info = vk::SemaphoreCreateInfo {
                ..Default::default()
            };

            frame.image_available = unsafe {
                logical_dev.create_semaphore(&sem_info, None)?
            };

            frame.render_finished_per_image.reserve(swapchain.images.len());

            for _ in 0..swapchain.images.len() {
                let sem = unsafe {
                    logical_dev.create_semaphore(&sem_info, None)?
                };

                frame.render_finished_per_image.push(sem);
            }

            let fence_info = vk::FenceCreateInfo {
                flags: vk::FenceCreateFlags::SIGNALED,
                ..Default::default()
            };

            frame.in_flight_fence = unsafe {
                logical_dev.create_fence(&fence_info, None)?
            };

            tracing::info!(
                "Initialized Vulkan frameloop frame data for frame {}",
                i
            );
        }

        frameloop.frame_idx = 0;

        let color_attachment = vk::AttachmentDescription {
            format: swapchain.info.ideal_surface_fmt.format,
            samples: vk::SampleCountFlags::TYPE_1,

            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::STORE,

            stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
            stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,

            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::PRESENT_SRC_KHR,

            ..Default::default()
        };

        let color_reference = vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        };

        let depth_attachment = vk::AttachmentDescription {
            format: swapchain.info.ideal_depth_fmt,
            samples: vk::SampleCountFlags::TYPE_1,

            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::DONT_CARE,

            stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
            stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,

            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,

            ..Default::default()
        };

        let depth_reference = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };

        let color_references = [color_reference];

        let subpass = vk::SubpassDescription {
            pipeline_bind_point: vk::PipelineBindPoint::GRAPHICS,
            color_attachment_count: color_references.len() as u32,
            p_color_attachments: color_references.as_ptr(),
            p_depth_stencil_attachment: &depth_reference,
            ..Default::default()
        };

        let dep = vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            dst_subpass: 0,

            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,

                dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,

                    src_access_mask: vk::AccessFlags::empty(),
                    dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,

                        ..Default::default()
        };

        let attachments = [
            color_attachment,
            depth_attachment,
        ];

        let subpasses = [subpass];
        let dependencies = [dep];

        let pass_info = vk::RenderPassCreateInfo {
            attachment_count: attachments.len() as u32,
            p_attachments: attachments.as_ptr(),

            subpass_count: subpasses.len() as u32,
            p_subpasses: subpasses.as_ptr(),

            dependency_count: dependencies.len() as u32,
            p_dependencies: dependencies.as_ptr(),

            ..Default::default()
        };

        frameloop.crnt_pass = unsafe {
            logical_dev.create_render_pass(&pass_info, None)?
        };

        frameloop.fbs.clear();
        frameloop.fbs.reserve(swapchain.images.len());

        for (i, (&img_view, &depth_view)) in swapchain
            .img_views
                .iter()
                .zip(swapchain.img_views_depth.iter())
                .enumerate()
                {
                    let fb_attachments = [
                        img_view,
                        depth_view,
                    ];

                    let fb_info = vk::FramebufferCreateInfo {
                        render_pass: frameloop.crnt_pass,

                        attachment_count: fb_attachments.len() as u32,
                        p_attachments: fb_attachments.as_ptr(),

                        width: swapchain.extent.width,
                        height: swapchain.extent.height,
                        layers: 1,

                        ..Default::default()
                    };

                    let framebuffer = unsafe {
                        logical_dev.create_framebuffer(&fb_info, None)?
                    };

                    frameloop.fbs.push(framebuffer);

                    tracing::info!(
                        "Initialized Vulkan frameloop framebuffer for swapchain image view {}",
                        i
                    );
                }

        tracing::info!("Initialized Vulkan frameloop.");

        Ok(())
    }

    fn create_upload_context(
        logical_dev: &ash::Device,
        graphics_queue_family_idx: u32,
    ) -> anyhow::Result<UploadContext> {
        let pool_info = vk::CommandPoolCreateInfo {
            queue_family_index: graphics_queue_family_idx,
            flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            ..Default::default()
        };

        let cmd_pool = unsafe {
            logical_dev.create_command_pool(&pool_info, None)?
        };

        let buf_info = vk::CommandBufferAllocateInfo {
            command_pool: cmd_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: 1,
            ..Default::default()
        };

        let cmd_buf = unsafe {
            let bufs = logical_dev.allocate_command_buffers(&buf_info)?;

            *bufs
                .first()
                .ok_or_else(|| anyhow::anyhow!("failed to allocate upload command buffer"))?
        };

        let fence_info = vk::FenceCreateInfo {
            flags: vk::FenceCreateFlags::SIGNALED,
            ..Default::default()
        };

        let fence = unsafe {
            logical_dev.create_fence(&fence_info, None)?
        };

        Ok(UploadContext {
            cmd_pool,
            cmd_buf,
            fence,
        })
    }

    fn destroy_upload_context(
        logical_dev: &ash::Device,
        upload: &mut UploadContext,
    ) {
        if upload.fence != vk::Fence::null() {
            unsafe { logical_dev.destroy_fence(upload.fence, None); } 
            upload.fence = vk::Fence::null();
        }

        if upload.cmd_pool != vk::CommandPool::null() {
            // No need to free upload.cmd_buf manually.
            // Destroying the command pool frees command buffers from it.
            unsafe { logical_dev.destroy_command_pool(upload.cmd_pool, None); }
            upload.cmd_pool = vk::CommandPool::null();
            upload.cmd_buf = vk::CommandBuffer::null();
        }
    }

    fn destroy_all_buffers(&mut self) {
        let Some(allocator) = self.allocator.as_ref() else {
            return;
        };

        for slot in &mut self.buffers {
            let Some(mut buffer) = slot.take() else {
                continue;
            };

            unsafe {
                allocator.destroy_buffer(buffer.raw, &mut buffer.alloc);
            }
        }
    } 

    fn destroy_swapchain_resources(&mut self) {
        unsafe {
            for fb in self.frameloop.fbs.drain(..) {
                self.logical_device.destroy_framebuffer(fb, None);
            }

            for view in self.swapchain.img_views.drain(..) {
                self.logical_device.destroy_image_view(view, None);
            }

            for view in self.swapchain.img_views_depth.drain(..) {
                self.logical_device.destroy_image_view(view, None);
            }

            for (image, mut alloc) in self
                .swapchain
                    .depth_images
                    .drain(..)
                    .zip(self.swapchain.depth_image_allocs.drain(..))
                    {
                        let Some(allocator) = self.allocator.as_ref() else {
                            return;
                        };
                        allocator.destroy_image(image, &mut alloc);
                    }

            if self.swapchain.handle != vk::SwapchainKHR::null() {
                self.swapchain_dev.destroy_swapchain(self.swapchain.handle, None);
                self.swapchain.handle = vk::SwapchainKHR::null();
            }
        }
    }

    fn destroy_frameloop(&mut self) {
        unsafe {
            if self.frameloop.crnt_pass != vk::RenderPass::null() {
                self.logical_device
                    .destroy_render_pass(self.frameloop.crnt_pass, None);
                self.frameloop.crnt_pass = vk::RenderPass::null();
            }

            for frame in &mut self.frameloop.frames {
                if frame.image_available != vk::Semaphore::null() {
                    self.logical_device
                        .destroy_semaphore(frame.image_available, None);
                    frame.image_available = vk::Semaphore::null();
                }

                for sem in frame.render_finished_per_image.drain(..) {
                    self.logical_device.destroy_semaphore(sem, None);
                }

                if frame.in_flight_fence != vk::Fence::null() {
                    self.logical_device
                        .destroy_fence(frame.in_flight_fence, None);
                    frame.in_flight_fence = vk::Fence::null();
                }

                if frame.cmd_pool != vk::CommandPool::null() {
                    self.logical_device
                        .destroy_command_pool(frame.cmd_pool, None);
                    frame.cmd_pool = vk::CommandPool::null();
                    frame.cmd_buf = vk::CommandBuffer::null();
                }
            }
        }
    }

    fn destroy_all_pipelines(&mut self) {
        unsafe {
            for slot in &mut self.pipelines {
                let Some(pipeline) = slot.take() else {
                    continue;
                };

                if pipeline.raw != vk::Pipeline::null() {
                    self.logical_device.destroy_pipeline(pipeline.raw, None);
                }

                if pipeline.layout != vk::PipelineLayout::null() {
                    self.logical_device
                        .destroy_pipeline_layout(pipeline.layout, None);
                }

                if pipeline.desc_pool != vk::DescriptorPool::null() {
                    self.logical_device
                        .destroy_descriptor_pool(pipeline.desc_pool, None);
                }

                if pipeline.desc_layout != vk::DescriptorSetLayout::null() {
                    self.logical_device
                        .destroy_descriptor_set_layout(pipeline.desc_layout, None);
                }
            }
        }
    }

    pub fn new(platform: &Platform) -> anyhow::Result<Self> { 
        tracing::info!("Using Vulkan rendering backend");

        let entry = Entry::linked();

        let (have_vulkan, 
            have_ext_vk_khr_surface,
            have_ext_vk_khr_xcb_surface,
            have_ext_vk_khr_xlib_surface,
            have_ext_vk_khr_wayland_surface) = Self::check_exts(&entry)?;

        if !have_vulkan {
            anyhow::bail!("Vulkan rendering is not supported on your device.");
        }

        tracing::info!("Required Vulkan extensions have been satisfied.");

        if !have_ext_vk_khr_surface {
            anyhow::bail!("VK_KHR_surface extension not supported, cannot create application window.");
        }

        let mut required_exts = Vec::new();
        required_exts.push(CString::new("VK_KHR_surface")?);

        match platform {
            Platform::X11(_)  => {
                if have_ext_vk_khr_xcb_surface {
                    required_exts.push(CString::new("VK_KHR_xcb_surface")?);
                } else if have_ext_vk_khr_xlib_surface {
                    required_exts.push(CString::new("VK_KHR_xlib_surface")?);
                } else {
                    anyhow::bail!("Neither VK_KHR_xcb_surface nor VK_KHR_xlib_surface is supported.");
                }
            }
            Platform::Wayland(_)  => {
                if !have_ext_vk_khr_wayland_surface {
                    anyhow::bail!("VK_KHR_wayland_surface is not supported.");
                }

                required_exts.push(CString::new("VK_KHR_wayland_surface")?);
            }
        }

        let app_info = vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };

        let pp_enabled_extension_names: Vec<*const i8> = required_exts 
            .iter()
            .map(|c_str| c_str.as_ptr() as *const i8)
            .collect();

        let layer_names: Vec<std::ffi::CString> =
            vec![std::ffi::CString::new("VK_LAYER_KHRONOS_validation").unwrap()];

        let layer_name_pointers: Vec<*const i8> = layer_names
            .iter()
            .map(|layer_name| layer_name.as_ptr())
            .collect();

        let create_info = vk::InstanceCreateInfo {
            enabled_extension_count: required_exts.len() as u32,
            pp_enabled_extension_names: 
                pp_enabled_extension_names.as_ptr(),
                pp_enabled_layer_names: layer_name_pointers.as_ptr(),
                enabled_layer_count: layer_name_pointers.len() as u32,
                p_application_info: &app_info,
                ..Default::default()
        };

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        tracing::info!("Created Vulkan instance successfully. (version 1.0).");

        let surface = platform.create_vulkan_surface(
            &entry,
            &instance,
            have_ext_vk_khr_xcb_surface,
            have_ext_vk_khr_wayland_surface,
        )?;

        let surface_inst = ash::khr::surface::Instance::new(&entry, &instance);

        let (graphics_queue_family_idx, present_queue_family_idx, phys_dev, phys_dev_limits) =
            Self::pick_phys_device_and_device_queues(&instance, &surface_inst, &surface)?;

        let (logical_device, graphics_queue, present_queue) = 
            Self::create_logical_device(
                &instance, &phys_dev, 
                present_queue_family_idx, 
                graphics_queue_family_idx)?;

        let allocator   = unsafe { vk_mem::Allocator::new(
            vk_mem::AllocatorCreateInfo::new(
                &instance, 
                &logical_device, phys_dev))? 
        };
        let upload_ctx  = Self::create_upload_context(&logical_device, 
            graphics_queue_family_idx)?;
        let staging_buf = Self::create_vk_buffer_raw(&allocator, 
            BufferDesc { 
                target: BufferTarget::Unspecified, 
                usage: BufferUsage::Staging,
                size: MAX_STAGING_RING_MEM as usize,
                data: None,
            })?;

        let mut initial_buffers = Vec::new();
        let staging_handle = BufferHandle(initial_buffers.len() as u32);
        initial_buffers.push(Some(staging_buf));

        let staging_ring = StagingRing {
            cap: MAX_STAGING_RING_MEM,
            head: 0,
            buf: staging_handle 
        };

        let mut swapchain  = Swapchain::default();
        let swapchain_dev = ash::khr::swapchain::Device::new(&instance, 
            &logical_device);

        let (width, height) = platform.size();

        Self::create_swapchain(&instance, 
            &mut swapchain, 
            &swapchain_dev, 
            &allocator, 
            &surface, 
            &surface_inst, 
            &logical_device, 
            &phys_dev, 
            width,
            height,
            graphics_queue_family_idx,
            present_queue_family_idx)?;

        let mut frameloop = Frameloop::default();

        Self::create_frameloop(&mut frameloop, &swapchain, 
            &logical_device, graphics_queue_family_idx)?;

        Ok(Self {
            instance,
            logical_device,

            surface,
            surface_inst,

            phys_dev,
            phys_dev_limits,

            graphics_queue,
            present_queue,

            graphics_queue_family_idx: graphics_queue_family_idx, 
            present_queue_family_idx: present_queue_family_idx, 

            swapchain,
            swapchain_dev,

            frameloop,
            allocator: Some(allocator),

            pending_resize: PendingResize::default(),
            width,
            height,

            skip_render: false,

            clear_color: [0.0, 0.0, 0.0, 1.0],

            buffers: initial_buffers, 
            pipelines: Vec::new(),

            staging_ring,

            upload_ctx
        })
    }

    fn vk_buffer_usage_from_desc(desc: &BufferDesc) -> vk::BufferUsageFlags {

        let mut usage = vk::BufferUsageFlags::empty();

        match desc.target {
            BufferTarget::Unspecified => {}

            BufferTarget::Vertex => {
                usage |= vk::BufferUsageFlags::VERTEX_BUFFER;
            }

            BufferTarget::Index => {
                usage |= vk::BufferUsageFlags::INDEX_BUFFER;
            }

            BufferTarget::Uniform => {
                usage |= vk::BufferUsageFlags::UNIFORM_BUFFER;
            }
        }

        match desc.usage {
            BufferUsage::Static => {
                usage |= vk::BufferUsageFlags::TRANSFER_DST;
            }

            BufferUsage::Dynamic | BufferUsage::Stream => {
                // Mapped CPU-visible buffer.
            }

            BufferUsage::Staging => {
                usage |= vk::BufferUsageFlags::TRANSFER_SRC;
            }
        }

        usage
    }

    fn create_vk_buffer_raw(
        allocator: &vk_mem::Allocator, desc: BufferDesc) -> anyhow::Result<VulkanBuffer> {
        let mut vk_usage = Self::vk_buffer_usage_from_desc(&desc);
        let mut mem_props = vk::MemoryPropertyFlags::empty();

        let mut want_mapping = false;

        let mut alloc_flags = vk_mem::AllocationCreateFlags::empty();

        match desc.usage {
            BufferUsage::Static => {
                // GPU-local buffer. Fast for rendering, not directly CPU writable.
                vk_usage |= vk::BufferUsageFlags::TRANSFER_DST;
                mem_props |= vk::MemoryPropertyFlags::DEVICE_LOCAL;
            }

            BufferUsage::Dynamic | BufferUsage::Stream => {
                // CPU-visible buffer. 
                mem_props |= vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT;

                want_mapping = true;

                alloc_flags |= vk_mem::AllocationCreateFlags::MAPPED;
                alloc_flags |= vk_mem::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE;
            }

            BufferUsage::Staging => {
                // CPU-written upload buffer.
                vk_usage |= vk::BufferUsageFlags::TRANSFER_SRC;

                mem_props |= vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT;

                want_mapping = true;

                alloc_flags |= vk_mem::AllocationCreateFlags::MAPPED;
                alloc_flags |= vk_mem::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE;
            }
        }

        let buffer_info = vk::BufferCreateInfo {
            size: desc.size as vk::DeviceSize,
            usage: vk_usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let alloc_info = vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::Auto,
            required_flags: mem_props,
            flags: alloc_flags,
            ..Default::default()
        };

        let (raw, alloc) = unsafe {
            allocator
                .create_buffer(&buffer_info, &alloc_info)
                .context("failed to create Vulkan buffer")?
        };

        let mapped = if want_mapping {
            let info = allocator.get_allocation_info(&alloc);
            Some(info.mapped_data as *mut c_void)
        } else {
            None
        };

        tracing::info!(
            "Created Vulkan buffer {:?} with size {} bytes, target {:?}, usage {:?}",
            raw,
            desc.size,
            desc.target,
            desc.usage,
        );

        Ok(VulkanBuffer {
            raw,
            alloc,
            size: desc.size,
            target: desc.target,
            usage: desc.usage,
            vk_usage,
            mem_props,
            mapped,
        })
    }

    fn create_vk_buffer_handle(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        let buffer = Self::create_vk_buffer_raw(self.allocator()?, desc)?;

        let handle = BufferHandle(self.buffers.len() as u32);
        self.buffers.push(Some(buffer));

        Ok(handle)
    }

    fn upload_device_local_buffer(
        &mut self,
        data: &[u8],
        target: BufferTarget,
    ) -> anyhow::Result<BufferHandle> {
        if data.is_empty() {
            anyhow::bail!("Cannot upload empty buffer");
        }

        if target == BufferTarget::Unspecified {
            anyhow::bail!("Static device-local upload needs a real buffer target");
        }

        let allocator = self.allocator()?;
        let dst_buffer = Self::create_vk_buffer_raw(
            allocator,
            BufferDesc {
                target,
                usage: BufferUsage::Static,
                size: data.len(),
                data: Some(data)
            },
        )?;

        let dst_handle = BufferHandle(self.buffers.len() as u32);
        self.buffers.push(Some(dst_buffer));

        let result = self.upload_into_device_local_buffer(dst_handle, data, target);

        if let Err(err) = result {
            self.destroy_buffer(dst_handle)?;
            return Err(err);
        }

        Ok(dst_handle)
    }

    fn destroy_buffer(&mut self, handle: BufferHandle) -> anyhow::Result<()> {
        let mut buffer = self
            .buffers
            .get_mut(handle.0 as usize)
            .and_then(|slot| slot.take())
            .ok_or_else(|| anyhow::anyhow!("invalid buffer handle {:?}", handle))?;

        let allocator = self
            .allocator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Vulkan allocator already destroyed"))?;

        unsafe {
            allocator.destroy_buffer(buffer.raw, &mut buffer.alloc);
        }

        Ok(())
    } 

    fn upload_into_device_local_buffer(
        &mut self,
        dst_handle: BufferHandle,
        data: &[u8],
        target: BufferTarget,
    ) -> anyhow::Result<()> {
        let size = data.len() as vk::DeviceSize;

        let alignment = self
            .phys_dev_limits
            .optimal_buffer_copy_offset_alignment
            .max(1);

        let staging_offset = self.staging_ring_alloc(size, alignment)?;

        let device = &self.logical_device;

        // Wait until the upload context is free.
        unsafe { device.wait_for_fences(
            &[self.upload_ctx.fence],
            true,
            u64::MAX,
        )?;
        }

        unsafe {
            device.reset_fences(&[self.upload_ctx.fence])?;
            device.reset_command_pool(
                self.upload_ctx.cmd_pool,
                vk::CommandPoolResetFlags::empty(),
            )?;
        }

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        unsafe {
            device.begin_command_buffer(self.upload_ctx.cmd_buf, &begin_info)?;
        }

        let staging_handle = self.staging_ring.buf;

        let (staging_raw, staging_mapped) = {
            let staging = self
                .buffers
                .get(staging_handle.0 as usize)
                .and_then(|b| b.as_ref())
                .ok_or_else(|| anyhow::anyhow!("invalid staging buffer handle"))?;

            let mapped = staging
                .mapped
                .ok_or_else(|| anyhow::anyhow!("staging buffer is not mapped"))?;

            (staging.raw, mapped)
        };

        let dst_raw = {
            let dst = self
                .buffers
                .get(dst_handle.0 as usize)
                .and_then(|b| b.as_ref())
                .ok_or_else(|| anyhow::anyhow!("invalid destination buffer handle"))?;

            dst.raw
        };

        unsafe { std::ptr::copy_nonoverlapping(
            data.as_ptr(),
            (staging_mapped as *mut u8).add(staging_offset as usize),
            data.len(),
        );
        }

        let copy = vk::BufferCopy {
            src_offset: staging_offset,
            dst_offset: 0,
            size,
        };

        unsafe { device.cmd_copy_buffer(
            self.upload_ctx.cmd_buf,
            staging_raw,
            dst_raw,
            &[copy],
        );
        }

        let (dst_stage_mask, dst_access_mask) = 
            Self::buffer_target_barrier_dst(target)?;

        let barrier = vk::BufferMemoryBarrier {
            src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            dst_access_mask,
            src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            buffer: dst_raw,
            offset: 0,
            size,
            ..Default::default()
        };

        unsafe { device.cmd_pipeline_barrier(
            self.upload_ctx.cmd_buf,
            vk::PipelineStageFlags::TRANSFER,
            dst_stage_mask,
            vk::DependencyFlags::empty(),
            &[],
            &[barrier],
            &[],
        );
        }

        unsafe { device.end_command_buffer(self.upload_ctx.cmd_buf)?; }

        let cmd_bufs = [self.upload_ctx.cmd_buf];

        let submit_info = vk::SubmitInfo {
            command_buffer_count: cmd_bufs.len() as u32,
            p_command_buffers: cmd_bufs.as_ptr(),
            ..Default::default()
        };

        unsafe { device.queue_submit(
            self.graphics_queue,
            &[submit_info],
            self.upload_ctx.fence,
        )?;
        }

        unsafe { device.wait_for_fences(
            &[self.upload_ctx.fence],
            true,
            u64::MAX,
        )?;
        }

        Ok(())
    }
    fn buffer_target_barrier_dst(
        target: BufferTarget,
    ) -> anyhow::Result<(vk::PipelineStageFlags, vk::AccessFlags)> {
        match target {
            BufferTarget::Vertex => Ok((
                    vk::PipelineStageFlags::VERTEX_INPUT,
                    vk::AccessFlags::VERTEX_ATTRIBUTE_READ,
            )),

            BufferTarget::Index => Ok((
                    vk::PipelineStageFlags::VERTEX_INPUT,
                    vk::AccessFlags::INDEX_READ,
            )),

            BufferTarget::Uniform => Ok((
                    vk::PipelineStageFlags::VERTEX_SHADER | vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::AccessFlags::UNIFORM_READ,
            )),

            BufferTarget::Unspecified => {
                anyhow::bail!("Cannot infer barrier for unspecified buffer target")
            }
        }
    }

    fn staging_ring_alloc(
        &mut self,
        size: vk::DeviceSize,
        alignment: vk::DeviceSize,
    ) -> anyhow::Result<vk::DeviceSize> {
        if size == 0 {
            anyhow::bail!("Cannot allocate zero bytes from staging ring");
        }

        if size > self.staging_ring.cap {
            anyhow::bail!(
                "Staging allocation too large: requested {} bytes, capacity {} bytes",
                size,
                self.staging_ring.cap,
            );
        }

        let aligned_head = Self::align_up(self.staging_ring.head, alignment);

        if aligned_head + size <= self.staging_ring.cap {
            self.staging_ring.head = aligned_head + size;
            return Ok(aligned_head);
        }

        let wrapped_head = 0;

        if wrapped_head + size <= self.staging_ring.cap {
            self.staging_ring.head = wrapped_head + size;
            return Ok(wrapped_head);
        }

        anyhow::bail!("Staging ring out of memory")
    }

    fn align_up(value: vk::DeviceSize, alignment: vk::DeviceSize) -> vk::DeviceSize {
        if alignment <= 1 {
            value
        } else {
            (value + alignment - 1) & !(alignment - 1)
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.pending_resize.width   = width;
        self.pending_resize.height  = height;
        self.pending_resize.pending = true;
    }

    pub fn begin_frame(&mut self) -> anyhow::Result<()> {
        if self.pending_resize.pending {
            self.handle_resize()?;
        }

        self.skip_render = false;
        self.swapchain.image_idx = 0;

        let frame = &mut self.frameloop.frames[self.frameloop.frame_idx];

        unsafe {
            self.logical_device.wait_for_fences(
                &[frame.in_flight_fence],
                true,
                u64::MAX,
            )?;
        }

        let acquire_res = unsafe {
            self.swapchain_dev.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
        };

        let (image_idx, suboptimal) = match acquire_res {
            Ok((idx, suboptimal)) => (idx as usize, suboptimal),

            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.pending_resize.pending = true;
                self.skip_render = true;
                return Ok(());
            }

            Err(err) => return Err(err.into()),
        };

        self.swapchain.image_idx = image_idx;
        if self.swapchain.imgs_in_flight[image_idx] != vk::Fence::null() {
            unsafe {
                self.logical_device.wait_for_fences(
                    &[self.swapchain.imgs_in_flight[image_idx]],
                    true,
                    u64::MAX,
                )?;
            }
        }

        self.swapchain.imgs_in_flight[image_idx] = frame.in_flight_fence;

        if suboptimal {
            self.pending_resize.pending = true;
        }

        unsafe {
            self.logical_device.reset_fences(&[frame.in_flight_fence])?;

            self.logical_device.reset_command_pool(
                frame.cmd_pool,
                vk::CommandPoolResetFlags::empty(),
            )?;
        }


        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        unsafe {
            self.logical_device.begin_command_buffer(frame.cmd_buf, &begin_info)?;
        }

        Ok(())
    }

    pub fn end_frame(&mut self) -> anyhow::Result<()> {
        if self.skip_render {
            return Ok(());
        }

        let frame_idx = self.frameloop.frame_idx;
        let image_idx = self.swapchain.image_idx;

        let frame = &mut self.frameloop.frames[frame_idx];

        let clear_values = [
            vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: self.clear_color,
                },
            },
            vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            },
        ];

        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: self.swapchain.extent,
        };

        let render_pass_begin = vk::RenderPassBeginInfo {
            render_pass: self.frameloop.crnt_pass,
            framebuffer: self.frameloop.fbs[image_idx],
            render_area,
            clear_value_count: clear_values.len() as u32,
            p_clear_values: clear_values.as_ptr(),
            ..Default::default()
        };

        unsafe {
            self.logical_device.cmd_begin_render_pass(
                frame.cmd_buf,
                &render_pass_begin,
                vk::SubpassContents::INLINE,
            );

            self.logical_device.cmd_end_render_pass(frame.cmd_buf);
        }

        unsafe {
            self.logical_device.end_command_buffer(frame.cmd_buf)?;
        }

        let wait_stage = vk::PipelineStageFlags::ALL_COMMANDS;

        let wait_semaphores = [
            frame.image_available,
        ];

        let wait_stages = [
            wait_stage,
        ];

        let command_buffers = [
            frame.cmd_buf,
        ];

        let signal_semaphores = [
            frame.render_finished_per_image[image_idx],
        ];

        let submit_info = vk::SubmitInfo {
            wait_semaphore_count: wait_semaphores.len() as u32,
            p_wait_semaphores: wait_semaphores.as_ptr(),

            p_wait_dst_stage_mask: wait_stages.as_ptr(),

            command_buffer_count: command_buffers.len() as u32,
            p_command_buffers: command_buffers.as_ptr(),

            signal_semaphore_count: signal_semaphores.len() as u32,
            p_signal_semaphores: signal_semaphores.as_ptr(),

            ..Default::default()
        };

        unsafe {
            self.logical_device.queue_submit(
                self.graphics_queue,
                &[submit_info],
                frame.in_flight_fence,
            )?;
        }

        let swapchains = [
            self.swapchain.handle,
        ];

        let image_indices = [
            image_idx as u32,
        ];

        let present_info = vk::PresentInfoKHR {
            wait_semaphore_count: signal_semaphores.len() as u32,
            p_wait_semaphores: signal_semaphores.as_ptr(),

            swapchain_count: swapchains.len() as u32,
            p_swapchains: swapchains.as_ptr(),

            p_image_indices: image_indices.as_ptr(),

            ..Default::default()
        };

        let present_res = unsafe {
            self.swapchain_dev.queue_present(self.present_queue, &present_info)
        };

        match present_res {
            Ok(suboptimal) => {
                if suboptimal {
                    self.pending_resize.pending = true;
                }
            }

            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.pending_resize.pending = true;
            }

            Err(err) => {
                return Err(err.into());
            }
        }

        self.frameloop.frame_idx =
            (self.frameloop.frame_idx + 1) % FRAME_COUNT;

        Ok(())
    }
    pub fn clear_color(&mut self, color: Color) {
        self.clear_color = [
            color.r,
            color.g,
            color.b,
            color.a,
        ];
    }


    fn create_shader_module(
        &self,
        filepath: &str,
    ) -> anyhow::Result<vk::ShaderModule> {
        let mut file = File::open(filepath)?;
        let code = ash::util::read_spv(&mut file)
            .context("failed to read SPIR-V shader")?;

        let create_info = vk::ShaderModuleCreateInfo {
            code_size: code.len() * std::mem::size_of::<u32>(),
            p_code: code.as_ptr(),
            ..Default::default()
        };

        let module = unsafe {
            self.logical_device.create_shader_module(&create_info, None)?
        };

        Ok(module)
    }

    fn vertex_fmt_to_vk_vertex_fmt(
        &self, 
        fmt: VertexFormat 
    ) -> vk::Format {
        match fmt {
            VertexFormat::Float32 =>  vk::Format::R32_SFLOAT,  
            VertexFormat::Float32x2 => vk::Format::R32G32_SFLOAT,
            VertexFormat::Float32x3 => vk::Format::R32G32B32_SFLOAT,
            VertexFormat::Float32x4 => vk::Format::R32G32B32A32_SFLOAT,

            VertexFormat::Uint32 => vk::Format::R32_UINT,
            VertexFormat::Uint32x2 => vk::Format::R32G32_UINT,
            VertexFormat::Uint32x3 => vk::Format::R32G32B32_UINT,
            VertexFormat::Uint32x4 => vk::Format::R32G32B32A32_UINT,

            VertexFormat::Unorm8x4 => vk::Format::R8G8B8A8_UNORM,
        }
    } 

    fn get_vertex_input_attribute_desc(
        &self, 
        attrib: VertexAttribute,
        binding: u32,
    ) -> anyhow::Result<vk::VertexInputAttributeDescription> {
        let location    = attrib.location;
        let binding     = binding;
        let format      = self.vertex_fmt_to_vk_vertex_fmt(attrib.format);
        let offset      = attrib.offset;

        let desc = vk::VertexInputAttributeDescription {
            location,
            binding,
            format,
            offset,
            ..Default::default()
        };

        Ok(desc)
    }

    fn get_vertex_input_rate_from_step_mode(
        &self, 
        step_mode: VertexStepMode
    ) -> vk::VertexInputRate {
        match step_mode {
            VertexStepMode::Vertex => vk::VertexInputRate::VERTEX, 
            VertexStepMode::Instance => vk::VertexInputRate::INSTANCE, 
        }
    }

    fn get_vertex_input_binding_desc(
        &self, 
        binding: &VertexBufferBindingLayout,
    ) -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription  {
            binding: binding.binding,
            stride: binding.stride,
            input_rate: self.get_vertex_input_rate_from_step_mode(binding.step_mode),
            ..Default::default()
        }
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        let vert_module = self.create_shader_module("compiled_shaders/vert.spv")?;
        let frag_module = self.create_shader_module("compiled_shaders/frag.spv")?;

        let shader_stages: [vk::PipelineShaderStageCreateInfo; 2] = [
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::VERTEX,
                p_name: b"main\0".as_ptr() as *const i8,
                module: vert_module,
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                stage: vk::ShaderStageFlags::FRAGMENT,
                p_name: b"main\0".as_ptr() as *const i8,
                module: frag_module,
                ..Default::default()
            },
        ];

        let assembly_state = vk::PipelineInputAssemblyStateCreateInfo {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            ..Default::default()
        };

        let raster_state = vk::PipelineRasterizationStateCreateInfo {
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: vk::CullModeFlags::NONE,
            front_face: vk::FrontFace::CLOCKWISE,
            line_width: 1.0,
            ..Default::default()
        };

        let msaa_state = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };

        let blend_attachments = [vk::PipelineColorBlendAttachmentState {
            blend_enable: vk::TRUE,
            src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::ONE,
            dst_alpha_blend_factor: vk::BlendFactor::ZERO,
            alpha_blend_op: vk::BlendOp::ADD,
            color_write_mask: ColorComponentFlags::from_raw(0xF),
        }];

        let blend_state = vk::PipelineColorBlendStateCreateInfo {
            p_attachments: blend_attachments.as_ptr(),
            attachment_count: blend_attachments.len() as u32,
            ..Default::default()
        };

        let dynamic_states = [
            vk::DynamicState::VIEWPORT,
            vk::DynamicState::SCISSOR,
        ];

        let dynamic_state = vk::PipelineDynamicStateCreateInfo {
            p_dynamic_states: dynamic_states.as_ptr(),
            dynamic_state_count: dynamic_states.len() as u32,
            ..Default::default()
        };

        let viewport_state = vk::PipelineViewportStateCreateInfo {
            viewport_count: 1,
            p_viewports: std::ptr::null(),
            scissor_count: 1,
            p_scissors: std::ptr::null(),
            ..Default::default()
        };

        let depth_state = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: vk::FALSE,
            depth_write_enable: vk::FALSE,
            depth_bounds_test_enable: vk::FALSE,
            stencil_test_enable: vk::FALSE,
            ..Default::default()
        };

        let bindings = desc
            .vert_bindings
            .iter()
            .map(|bind| self.get_vertex_input_binding_desc(bind))
            .collect::<Vec<_>>();

        let mut attribs = Vec::new();

        for binding in desc.vert_bindings {
            for attr in binding.attrs {
                attribs.push(self.get_vertex_input_attribute_desc(attr, binding.binding)?);
            }
        }
        let vertex_input_state = vk::PipelineVertexInputStateCreateInfo {
            vertex_binding_description_count: bindings.len() as u32,
            vertex_attribute_description_count: attribs.len() as u32,
            p_vertex_attribute_descriptions: attribs.as_ptr(),
            p_vertex_binding_descriptions: bindings.as_ptr(),
            ..Default::default()
        };

        let mut pc_size: u32 = 0; 
        let mut stage_flags = vk::ShaderStageFlags::empty();
        for pc in &desc.uniform_bindings {
            match pc.ty {
                UniformBindingType::Vec2 => {
                    pc_size += 
                        size_of::<f32>() as u32 * 2;
                    let vk_stage = match pc.stage {
                        UniformBindingShaderStage::Vertex => vk::ShaderStageFlags::VERTEX,
                        UniformBindingShaderStage::Fragment => vk::ShaderStageFlags::FRAGMENT
                    };
                    stage_flags |= vk_stage; 
                }
                _ => {},
            }
        }

        let push_ranges = if pc_size > 0 {
            vec![vk::PushConstantRange {
                offset: 0,
                stage_flags,
                size: pc_size,
            }]
        } else {
            Vec::new()
        };

        let mut desc_layout_bindings    = Vec::new();
        let mut desc_pool_sizes         = Vec::new();

        for binding in &desc.uniform_bindings {
            if binding.ty != UniformBindingType::Sampler2dArray {
                continue;
            }
            let vk_type = match binding.ty {
                UniformBindingType::Sampler2dArray => 
                    vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                _ => anyhow::bail!("This should not exist.")
            };

            desc_layout_bindings.push(vk::DescriptorSetLayoutBinding {
                binding: binding.binding as u32,
                descriptor_type: vk_type,
                descriptor_count: 1,
                stage_flags: match binding.stage {
                    UniformBindingShaderStage::Vertex => 
                        vk::ShaderStageFlags::VERTEX,
                    UniformBindingShaderStage::Fragment => 
                        vk::ShaderStageFlags::FRAGMENT,
                },
                ..Default::default()
            });

            desc_pool_sizes.push(
                vk::DescriptorPoolSize {
                    ty: vk_type, 
                    descriptor_count: self.swapchain.images.len() as u32,
            }

            );
        }

        let desc_layout_info = vk::DescriptorSetLayoutCreateInfo {
            binding_count: desc_layout_bindings.len() as u32,
            p_bindings: desc_layout_bindings.as_ptr(),
            ..Default::default()
        };

        let desc_layout = unsafe {
            self.logical_device
                .create_descriptor_set_layout(&desc_layout_info, None)?
        };

        let pipeline_set_layouts = [desc_layout];

        let layout_info = vk::PipelineLayoutCreateInfo {
            set_layout_count: pipeline_set_layouts.len() as u32,
            p_set_layouts: pipeline_set_layouts.as_ptr(),

            push_constant_range_count: push_ranges.len() as u32,
            p_push_constant_ranges: push_ranges.as_ptr(),
            ..Default::default()
        };

        let pipeline_layout = unsafe {
            self.logical_device
                .create_pipeline_layout(&layout_info, None)?
        };

        let pool_info = vk::DescriptorPoolCreateInfo {
            flags: DescriptorPoolCreateFlags::empty(),
            max_sets: self.swapchain.images.len() as u32,
            pool_size_count: desc_pool_sizes.len() as u32,
            p_pool_sizes: desc_pool_sizes.as_ptr(),
            ..Default::default()
        };

        let desc_pool = unsafe {
            self.logical_device
                .create_descriptor_pool(&pool_info, None)?
        };

        let set_layouts = vec![desc_layout; self.swapchain.images.len()];

        let alloc_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool: desc_pool,
            descriptor_set_count: set_layouts.len() as u32,
            p_set_layouts: set_layouts.as_ptr(),
            ..Default::default()
        };

        let desc_sets = unsafe {
            self.logical_device.allocate_descriptor_sets(&alloc_info)?
        };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo {
            stage_count: shader_stages.len() as u32,
            p_stages: shader_stages.as_ptr(),
            p_vertex_input_state: &vertex_input_state,
            p_input_assembly_state: &assembly_state,
            p_color_blend_state: &blend_state,
            p_multisample_state: &msaa_state,
            p_rasterization_state: &raster_state,
            p_dynamic_state: &dynamic_state,
            p_viewport_state: &viewport_state,
            p_depth_stencil_state: &depth_state,
            layout: pipeline_layout,
            render_pass: self.frameloop.crnt_pass,
            ..Default::default()
        }];

        let pipelines = unsafe {
            let result = self.logical_device
                .create_graphics_pipelines(
                    PipelineCache::null(),
                    pipeline_info.as_slice(),
                    None,
                );

            self.logical_device.destroy_shader_module(vert_module, None);
            self.logical_device.destroy_shader_module(frag_module, None);

            result.map_err(|(_, result)| result)?
        };

        if pipelines.is_empty() {
            anyhow::bail!("Failed to create graphics pipeline.")
        }

        let raw = pipelines[0];
        let handle = PipelineHandle(self.pipelines.len() as u32);

        self.pipelines.push(Some(VulkanPipeline { raw, layout: pipeline_layout, desc_layout,
            desc_pool, descriptor_sets: desc_sets}));

        tracing::info!(
            "Created Vulkan graphics pipeline (vertex bindings: {}, vertex attributes: {})",
            bindings.len(),
            attribs.len()
        );

        Ok(handle)
    }

    pub fn create_buffer(&mut self, desc: BufferDesc<'_>) -> anyhow::Result<BufferHandle> {
        match desc.data {
            Some(data) => {
                if data.len() > desc.size {
                    anyhow::bail!(
                        "buffer initial data is larger than buffer size: data={} size={}",
                        data.len(),
                        desc.size
                    );
                }

                match desc.usage {
                    BufferUsage::Static => {
                        self.upload_device_local_buffer(data, desc.target)
                    }

                    BufferUsage::Dynamic | BufferUsage::Stream | BufferUsage::Staging => {
                        let handle = self.create_vk_buffer_handle(BufferDesc {
                            data: None,
                            ..desc
                        })?;

                        self.write_buffer(handle, 0, 0, data)?;

                        Ok(handle)
                    }
                }
            }

            None => {
                self.create_vk_buffer_handle(desc)
            }
        }
    }


    pub fn write_buffer(
        &mut self,
        handle: BufferHandle,
        _binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let end = offset
            .checked_add(data.len())
            .ok_or_else(|| anyhow::anyhow!("buffer write offset overflow"))?;

        let buffer = self
            .buffers
            .get(handle.0 as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid buffer handle {:?}", handle))?;

        if end > buffer.size {
            anyhow::bail!(
                "buffer write out of bounds: offset={} size={} buffer_size={}",
                offset,
                data.len(),
                buffer.size,
            );
        }

        let is_mapped = buffer.mapped.is_some();

        if is_mapped {
            self.write_mapped_buffer(handle, offset, data)
        } else {
            self.transfer_to_device_local_buffer(handle, offset, data)
        }
    }

    fn write_mapped_buffer(
        &mut self,
        handle: BufferHandle,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let buffer = self
            .buffers
            .get(handle.0 as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or_else(|| anyhow::anyhow!("invalid buffer handle {:?}", handle))?;

        let mapped = buffer
            .mapped
            .ok_or_else(|| anyhow::anyhow!("buffer {:?} is not mapped", handle))?;

        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                (mapped as *mut u8).add(offset),
                data.len(),
            );
        }

        Ok(())
    }

    fn transfer_to_device_local_buffer(
        &mut self,
        handle: BufferHandle,
        dst_offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let size = data.len() as vk::DeviceSize;

        let alignment = self
            .phys_dev_limits
            .optimal_buffer_copy_offset_alignment
            .max(1);

        let staging_offset = self.staging_ring_alloc(size, alignment)?;

        let staging_handle = self.staging_ring.buf;

        let (staging_raw, staging_mapped) = {
            let staging = self
                .buffers
                .get(staging_handle.0 as usize)
                .and_then(|slot| slot.as_ref())
                .ok_or_else(|| anyhow::anyhow!("invalid staging buffer handle"))?;

            let mapped = staging
                .mapped
                .ok_or_else(|| anyhow::anyhow!("staging buffer is not mapped"))?;

            (staging.raw, mapped)
        };

        let (dst_raw, dst_target) = {
            let dst = self
                .buffers
                .get(handle.0 as usize)
                .and_then(|slot| slot.as_ref())
                .ok_or_else(|| anyhow::anyhow!("invalid destination buffer handle {:?}", handle))?;

            (dst.raw, dst.target)
        };

        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                (staging_mapped as *mut u8).add(staging_offset as usize),
                data.len(),
            );
        }

        let copy = vk::BufferCopy {
            src_offset: staging_offset,
            dst_offset: dst_offset as vk::DeviceSize,
            size,
        };

        let cmd_buf = self.frameloop.frames[self.frameloop.frame_idx].cmd_buf;

        unsafe {
            self.logical_device.cmd_copy_buffer(
                cmd_buf,
                staging_raw,
                dst_raw,
                &[copy],
            );
        }

        let (dst_stage, dst_access) = Self::buffer_target_barrier_dst(dst_target)?;

        let barrier = vk::BufferMemoryBarrier {
            src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            dst_access_mask: dst_access,

            src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,

            buffer: dst_raw,
            offset: dst_offset as vk::DeviceSize,
            size,

            ..Default::default()
        };

        unsafe {
            self.logical_device.cmd_pipeline_barrier(
                cmd_buf,
                vk::PipelineStageFlags::TRANSFER,
                dst_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier],
                &[],
            );
        }

        Ok(())
    }


}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            if let Err(err) = self.logical_device.device_wait_idle() {
                tracing::error!("vkDeviceWaitIdle failed during drop: {:?}", err);
            }
        }

        self.destroy_all_pipelines();

        self.destroy_all_buffers();

        self.destroy_swapchain_resources();

        self.destroy_frameloop();

        Self::destroy_upload_context(&self.logical_device, &mut self.upload_ctx);

        self.allocator.take();

        unsafe {
            self.logical_device.destroy_device(None);

            if self.surface != vk::SurfaceKHR::null() {
                self.surface_inst.destroy_surface(self.surface, None);
                self.surface = vk::SurfaceKHR::null();
            }

            self.instance.destroy_instance(None);
        }
    }
}

impl GraphicsDevice for VulkanRenderer {
    fn resize(&mut self, width: u32, height: u32) {
        VulkanRenderer::resize(self, width, height)
    }

    fn clear_color(&mut self, color: Color) {
        VulkanRenderer::clear_color(self, color)
    }

    fn begin_frame(&mut self) -> anyhow::Result<()> {
        VulkanRenderer::begin_frame(self)
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        VulkanRenderer::end_frame(self)
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> anyhow::Result<BufferHandle> {
        VulkanRenderer::create_buffer(self, desc)
    }

    fn write_buffer(
        &mut self,
        handle: BufferHandle,
        binding: u32,
        offset: usize,
        data: &[u8],
    ) -> anyhow::Result<()> {
        VulkanRenderer::write_buffer(self, handle, binding, offset, data)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc<'_>) -> anyhow::Result<PipelineHandle> {
        VulkanRenderer::create_pipeline(self, desc)
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

    fn create_texture(
        &mut self,
        _desc: TextureDesc,
        _data: Option<&[u8]>,
    ) -> anyhow::Result<TextureHandle> {
        anyhow::bail!("Vulkan create_texture not implemented yet")
    }

    fn write_texture(
        &mut self,
        _texture: TextureHandle,
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
        _data: &[u8]) -> anyhow::Result<()> {

        anyhow::bail!("Vulkan write_texture not implemented yet")
    }

    fn set_texture(&mut self,
        _slot: u32,
        _texture: TextureHandle)
        -> anyhow::Result<()> {
            anyhow::bail!("Vulkan set_texture not implemented yet")
    }

    fn set_uniform_1i(
        &mut self,
        _pipeline: PipelineHandle,
        _name: &str,
        _value: i32,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan set_uniform_1i not implemented yet")
    }

    fn texture_gen_mipmap(
        &mut self, 
        _texture: TextureHandle,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan texture_gen_mipmap not implemented yet")
    }

    fn create_texture_array(
        &mut self,
        _desc: TextureArrayDesc,
    ) -> anyhow::Result<TextureHandle> {
        anyhow::bail!("Vulkan create_texture_array not implemented yet")
    }

    fn write_texture_array_layer(
        &mut self,
        _texture: TextureHandle,
        _x: u32,
        _y: u32,
        _layer: u32,
        _width: u32,
        _height: u32,
        _pixels: &[u8],
    ) -> anyhow::Result<()> {
        anyhow::bail!("Vulkan write_texture_array_layer not implemented yet")
    }
    fn texture_get_kind(
        &mut self,
        _texture: TextureHandle,
    ) -> anyhow::Result<TextureKind> {
        anyhow::bail!("Vulkan texture_get_kind not implemented yet")
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

}
