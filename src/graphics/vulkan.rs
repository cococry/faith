use std::ffi::{CString, c_char};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind};
use crate::platform::{Platform};
use crate::graphics::{BufferDesc, BufferHandle, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc, TextureHandle};

use ash::vk::{Extent2D, ImageViewCreateInfo, QueueFlags };
use ash::{Entry, vk};
use vk_mem::{Alloc, AllocationCreateInfo};

const FRAME_COUNT: usize = 2;

#[derive(Default)]
struct PendingResize {
    width: u32,
    height: u32,
    pending: bool,
}

pub struct VulkanRenderer {
    instance: ash::Instance,
    logical_device: ash::Device,

    surface: vk::SurfaceKHR,
    surface_inst: ash::khr::surface::Instance,

    phys_dev: vk::PhysicalDevice,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    graphics_queue_family_idx: u32,
    present_queue_family_idx: u32,

    swapchain: Swapchain,
    swapchain_dev:  ash::khr::swapchain::Device,

    frameloop: Frameloop,
    allocator: vk_mem::Allocator,

    pending_resize: PendingResize,

    width: u32,
    height: u32,
    
    skip_render: bool,
    clear_color: [f32; 4],
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
    pub timestamp_pool: vk::QueryPool,

    /// Value of ring.head at frame begin.
    pub staging_begin: usize,

    /// Max head reached in this frame.
    pub staging_end: usize,
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
    ) -> anyhow::Result<()> {
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

        Ok(())
    }

    fn pick_phys_device_and_device_queues(
        instance: &ash::Instance,
        surface_inst: &ash::khr::surface::Instance,
        surface: &vk::SurfaceKHR,
    ) -> anyhow::Result<(u32, u32, vk::PhysicalDevice)> {
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
                Self::pick_phys_device(instance, 
                    phys_dev, 
                    present_queue_family_idx as u32, 
                    graphics_queue_family_idx as u32)?;
                return Ok((
                        graphics_queue_family_idx as u32, 
                        present_queue_family_idx as u32, 
                        phys_dev
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

        tracing::info!("Initialized Vulkan logical device (graphics queue index: %i, present queue index; %i)");

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
        extent.height   = std::cmp::min(width, 
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
        
        Self::create_swapchain(
            &self.instance, &mut self.swapchain, 
            &self.swapchain_dev, &self.allocator, 
            &self.surface, &self.surface_inst, 
            &self.logical_device, &self.phys_dev,
            self.pending_resize.width, self.pending_resize.height, 
            self.graphics_queue_family_idx, 
            self.present_queue_family_idx)?;

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

        let create_info = vk::InstanceCreateInfo {
            enabled_extension_count: required_exts.len() as u32,
            pp_enabled_extension_names: pp_enabled_extension_names.as_ptr(),
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

        let (graphics_queue_family_idx, present_queue_family_idx, phys_dev) =
            Self::pick_phys_device_and_device_queues(&instance, &surface_inst, &surface)?;

        let (logical_device, graphics_queue, present_queue) = 
            Self::create_logical_device(&instance, &phys_dev, present_queue_family_idx, graphics_queue_family_idx)?;

        let allocator = unsafe { vk_mem::Allocator::new(vk_mem::AllocatorCreateInfo::new(&instance, &logical_device, phys_dev))? };

        let mut swapchain  = Swapchain::default();
        let swapchain_dev = ash::khr::swapchain::Device::new(&instance, &logical_device);
        
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
        
        Self::create_frameloop(&mut frameloop, &swapchain, &logical_device, graphics_queue_family_idx)?;
        

        Ok(Self {
            instance,
            logical_device,

            surface,
            surface_inst,

            phys_dev,
            graphics_queue,
            present_queue,

            graphics_queue_family_idx: graphics_queue_family_idx, 
            present_queue_family_idx: present_queue_family_idx, 

            swapchain,
            swapchain_dev,

            frameloop,
            allocator,

            pending_resize: PendingResize::default(),
            width,
            height,

            skip_render: false,

            clear_color: [0.0, 0.0, 0.0, 1.0]
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.pending_resize.width   = width;
        self.pending_resize.height  = height;
        self.pending_resize.pending = true;
    }
    
    fn begin_frame(&mut self) -> anyhow::Result<()> {
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

        Ok(())
    }

    fn end_frame(&mut self) -> anyhow::Result<()> {
        if self.skip_render {
            return Ok(());
        }

        let frame_idx = self.frameloop.frame_idx;
        let image_idx = self.swapchain.image_idx;

        let frame = &mut self.frameloop.frames[frame_idx];

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        unsafe {
            self.logical_device.begin_command_buffer(frame.cmd_buf, &begin_info)?;
        }

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
    fn clear_color(&mut self, color: Color) {
    self.clear_color = [
        color.r,
        color.g,
        color.b,
        color.a,
    ];
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
