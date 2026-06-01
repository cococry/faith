use crate::platform::window::WindowHandleInfo;

use super::event::WindowEvent;
use super::window::WindowConfig;

pub struct WaylandPlatform;

impl WaylandPlatform {
    pub fn new(_config: &WindowConfig) -> anyhow::Result<Self> {
        anyhow::bail!("Wayland backend not implemented yet");
    }

    pub fn poll_events(&mut self) -> anyhow::Result<Vec<WindowEvent>> {
        Ok(Vec::new())
    }

    pub fn size(&self) -> (u32, u32) {
        (0, 0)
    }

    pub fn native_handle(&self) -> WindowHandleInfo {
        WindowHandleInfo::Wayland {
            display: std::ptr::null_mut(),
            surface: std::ptr::null_mut()
        }
    }

}
