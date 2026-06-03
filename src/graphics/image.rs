use std::path::Path;

use crate::graphics::{
    GraphicsDevice,
    TextureDesc,
    TextureFormat,
    TextureHandle,
};

pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl ImageData {
    pub fn load_rgba8(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();

        let image = image::open(path)
            .map_err(|err| anyhow::anyhow!("Failed to load image {:?}: {}", path, err))?;

        let rgba = image.to_rgba8();

        Ok(Self {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        })
    }
}
