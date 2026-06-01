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

#[derive(Debug, Clone, Copy)]
pub struct Image {
    pub texture: TextureHandle,
    pub width: u32,
    pub height: u32,
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

    pub fn upload<G: GraphicsDevice>(
        &self,
        gpu: &mut G,
    ) -> anyhow::Result<TextureHandle> {
        gpu.create_texture(
            TextureDesc {
                width: self.width,
                height: self.height,
                format: TextureFormat::Rgba8,
            },
            Some(&self.pixels),
        )
    }
}

pub fn load_image<G: GraphicsDevice>(
    gpu: &mut G,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<Image> {
    let data = ImageData::load_rgba8(path)?;
    let texture = data.upload(gpu)?;

    Ok(Image {
        texture,
        width: data.width,
        height: data.height,
    })
}
