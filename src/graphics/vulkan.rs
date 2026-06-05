use std::ffi::{CStr, CString, c_char};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind};
use crate::platform::window::WindowHandleInfo;
use crate::platform::{self, Platform};
use crate::graphics::{BufferDesc, BufferHandle, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc, TextureHandle};

use ash::vk::{QueueFlags, SurfaceFormatKHR};
use ash::{Entry, khr, vk};
use x11::keysym::XK_Pointer_EnableKeys;

pub struct VulkanRenderer {
    instance: ash::Instance,
    logical_device: ash::Device,

    surface: vk::SurfaceKHR,
    phys_dev: vk::PhysicalDevice,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,

    have_vulkan: bool,
    have_ext_vk_khr_surface: bool,
    have_ext_vk_khr_xcb_surface: bool,
    have_ext_vk_khr_xlib_surface: bool,
    have_ext_vk_khr_wayland_surface: bool,
    
    required_exts: Vec<CString>,
    pp_enabled_extension_names: Vec<*const i8>,

    graphics_queue_family_idx: usize,
    present_queue_family_idx: usize,

    swapchain_info: SwapchainInfo,
}

struct SwapchainInfo {
    present_modes: Vec<vk::PresentModeKHR>,
    surface_fmts: Vec<vk::SurfaceFormatKHR>,
    capabilities: vk::SurfaceCapabilitiesKHR,

    ideal_present_mode: vk::PresentModeKHR,
    ideal_surface_fmt: vk::SurfaceFormatKHR,
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
        present_queue_family_idx: isize, 
        graphics_queue_family_idx: isize, 
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
    ) -> anyhow::Result<(isize, isize, vk::PhysicalDevice)> {
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
                Self::pick_phys_device(instance, phys_dev, present_queue_family_idx, graphics_queue_family_idx)?;
                return Ok((graphics_queue_family_idx, present_queue_family_idx, phys_dev));
            }
        } 

        anyhow::bail!(
            "None of your GPUs support all Vulkan features needed for 
            rendering in the application window. Either a physical device (GPU) 
            queue for graphics or presentation is missing.");
    }

    fn string_vec_to_raw_bytes(
        vec: &Vec<String> 
    ) -> Vec<*const c_char> {
       vec 
            .iter()
            .map(|c_str| c_str.as_ptr() as *const i8)
            .collect::< Vec<*const i8>>()
    }

    fn create_logical_device(
        instance: &ash::Instance,
        phys_dev: &vk::PhysicalDevice,
        present_queue_family_idx: isize,
        graphics_queue_family_idx: isize
    ) -> anyhow::Result<(ash::Device, vk::Queue, vk::Queue)> {
        let mut queues = Vec::new();

        queues.push(
            vk::DeviceQueueCreateInfo {
                queue_family_index:  graphics_queue_family_idx as u32,
                ..Default::default()
            }
        );
        if graphics_queue_family_idx != present_queue_family_idx {
            queues.push(
                vk::DeviceQueueCreateInfo {
                    queue_family_index:  present_queue_family_idx as u32,
                    ..Default::default()
                }
            );
        }

        let enabled_exts_vec: Vec<String> = vec!["VK_KHR_swapchain".to_string()];
       
        let enabled_exts = Self::string_vec_to_raw_bytes(&enabled_exts_vec);

        let device_info = vk::DeviceCreateInfo {
            enabled_extension_count: 1, 
            pp_enabled_extension_names: enabled_exts.as_ptr(),
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
    ) -> vk::SurfaceFormatKHR {
        for fmt in surface_fmts {
            if fmt.format == vk::Format::B8G8R8_SRGB && 
                fmt.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR {
                    return *fmt;
            }
        }
        surface_fmts[0]
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

    fn get_swapchain_info(
        phys_dev: vk::PhysicalDevice,
        surface_inst: &ash::khr::surface::Instance,
        surface: &vk::SurfaceKHR,
    ) -> anyhow::Result<SwapchainInfo> {
        let capabilities    = unsafe { surface_inst.get_physical_device_surface_capabilities(phys_dev, *surface)? };
        let surface_fmts    = unsafe { surface_inst.get_physical_device_surface_formats(phys_dev, *surface)? };
        let present_modes   = unsafe { surface_inst.get_physical_device_surface_present_modes(phys_dev, *surface)? };
      
        let ideal_present_mode = Self::get_ideal_swapchain_present_mode(&present_modes);
        let ideal_surface_fmt  = Self::get_ideal_swapchain_surface_fmt(&surface_fmts);

        Ok(SwapchainInfo { present_modes, surface_fmts, capabilities,
            ideal_present_mode, ideal_surface_fmt})
    }

    fn create_swapchain(
        phys_dev: vk::PhysicalDevice,
        swap_info: &SwapchainInfo,
    ) -> anyhow::Result<vk::SwapchainKHR> {
        todo!()
    }

    pub fn new(platform: &Platform) -> anyhow::Result<Self> { 
        tracing::info!("Using Vulkan rendering backend");

        let entry = Entry::linked();

        let (have_vulkan, 
            have_ext_vk_khr_surface,
            have_ext_vk_khr_xcb_surface,
            have_ext_vk_khr_xlib_surface,
            have_ext_vk_khr_wayland_surface) = Self::check_exts(&entry)?;
        
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
                } else {
                    required_exts.push(CString::new("VK_KHR_xlib_surface")?);
                }
            }
            Platform::Wayland(_)  => {
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
            &entry, &instance, 
            have_ext_vk_khr_xcb_surface, 
            have_ext_vk_khr_wayland_surface).expect("Failed to create Vulkan surface for appplication window");
        let surface_inst = ash::khr::surface::Instance::new(&entry, &instance);

        let (graphics_queue_family_idx, present_queue_family_idx, phys_dev) =
            Self::pick_phys_device_and_device_queues(&instance, &surface_inst, &surface)?;

        let (logical_device, graphics_queue, present_queue) = 
            Self::create_logical_device(&instance, &phys_dev, present_queue_family_idx, graphics_queue_family_idx)?;

        let swapchain_info = Self::get_swapchain_info(phys_dev, &surface_inst, &surface)?;
        
        Ok(Self {
            instance,
            logical_device,

            surface,
            phys_dev,
            graphics_queue,
            present_queue,

            have_vulkan,
            have_ext_vk_khr_surface,
            have_ext_vk_khr_xcb_surface,
            have_ext_vk_khr_xlib_surface,
            have_ext_vk_khr_wayland_surface,

            pp_enabled_extension_names,
            required_exts
                ,
            graphics_queue_family_idx: graphics_queue_family_idx as usize,
            present_queue_family_idx: present_queue_family_idx as usize,

            swapchain_info,
            
        })
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
        (0, 0)
    }

}
