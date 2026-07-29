use std::path::Path;

/// Raw RGBA image data loaded from an image file.
///
/// Pixels are stored as 8-bit RGBA values in row-major order, with four bytes
/// per pixel: red, green, blue, and alpha.
pub struct ImageData {
    pub width: u32,
    pub height: u32,

    pub pixels: Vec<u8>,
}

impl ImageData {
    /// Loads an image from disk and converts it to RGBA8 format.
    ///
    /// The returned image data always contains four 8-bit channels per pixel:
    /// red, green, blue, and alpha.
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
