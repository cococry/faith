use std::ffi::{CStr, CString};

use crate::graphics::device::{DrawIndexedInstanced, TextureArrayDesc, TextureKind};
use crate::platform::window::WindowHandleInfo;
use crate::platform::{self, Platform};
use crate::graphics::{BufferDesc, BufferHandle, Color, DrawIndexed, GraphicsDevice, PipelineDesc, PipelineHandle, TextureDesc, TextureHandle};

use ash::{vk, Entry};

pub struct VulkanRenderer {
    instance: ash::Instance,
    surface: ash::vk::SurfaceKHR,

    have_vulkan: bool,
    have_ext_vk_khr_surface: bool,
    have_ext_vk_khr_xcb_surface: bool,
    have_ext_vk_khr_xlib_surface: bool,
    have_ext_vk_khr_wayland_surface: bool,
    
    required_exts: Vec<CString>,
    pp_enabled_extension_names: Vec<*const i8>,
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
            have_ext_vk_khr_wayland_surface)?;

        Ok(Self {
            instance,
            surface,
            have_vulkan,
            have_ext_vk_khr_surface,
            have_ext_vk_khr_xcb_surface,
            have_ext_vk_khr_xlib_surface,
            have_ext_vk_khr_wayland_surface,
            pp_enabled_extension_names,
            required_exts,
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
