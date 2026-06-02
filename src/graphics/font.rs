use freetype as ft;
use std::{collections::HashMap, path::{Path, PathBuf}};

use crate::graphics::FontHandle;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_handle: FontHandle,
    pub glyph_idx: u32,
}

pub struct FontManager {
    lib_handle: ft::Library,
    fonts: Vec<Font>,
    glyph_cache: HashMap<GlyphKey, Glyph>
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance_x: i64,

    pub pixels: Vec<u8>
}

pub struct Font {
    size: u32,
    path: PathBuf,
    face: ft::Face,
}

impl FontManager {
    pub fn new() -> anyhow::Result<Self> {
        let lib_handle = ft::Library::init()?;

        Ok(Self {
            lib_handle,
            fonts: Vec::new(), 
            glyph_cache: HashMap::new() 
        })
    }

    pub fn glyph_idx_for_char(&self, font_handle: FontHandle, ch: char) ->
    anyhow::Result<Option<u32>> {
        let font = self.get_font(font_handle)?; 

        let idx = font.face.get_char_index(ch as usize) as Option<u32>;

        if idx.is_some() {
            Ok(idx)
        } else {
            Ok(None)
        } 
    }

    fn rasterize_glyph(
        &mut self, 
        font_handle: FontHandle,
        glyph_idx: u32
    ) -> anyhow::Result<Glyph> {
        let font = self.get_font(font_handle)?; 

        font.face.load_glyph(
            glyph_idx,
            ft::face::LoadFlag::RENDER
        )?;

        let glyph = font.face.glyph();
        let bitmap = glyph.bitmap();

        Ok(Glyph{
            width: bitmap.width() as u32,
            height: bitmap.rows() as u32,
            bearing_x: glyph.bitmap_left(),
            bearing_y: glyph.bitmap_top(),
            advance_x: glyph.advance().x,
            pixels: bitmap.buffer().to_vec(),
        })
    }

    pub fn get_or_load_glyph(
        &mut self,
        font_handle: FontHandle,
        glyph_idx: u32
    ) -> anyhow::Result<&Glyph> {
        let key = GlyphKey {
            font_handle,
            glyph_idx
        };

        if !self.glyph_cache.contains_key(&key) {
            let glyph = self.rasterize_glyph(font_handle, glyph_idx)?;
            self.glyph_cache.insert(key, glyph);
        }

        Ok(self.glyph_cache.get(&key).unwrap())
    }

    pub fn load_font(
        &mut self,
        path: impl AsRef<Path>,
        size: u32,
        face_idx: isize 
    ) -> anyhow::Result<FontHandle> {
        let path = path.as_ref();

        let face = self.lib_handle.new_face(path, face_idx)?;
        face.set_pixel_sizes(0, size)?;

        let handle = FontHandle(self.fonts.len() as u32);
        self.fonts.push(Font { size, path: path.to_path_buf(), face });

        Ok(handle)
    }
    
    fn get_font(&self, handle: FontHandle) -> anyhow::Result<&Font> {
        self
            .fonts
            .get(handle.0 as usize)
            .ok_or_else(|| anyhow::anyhow!("invalid texture handle: {:?}", handle))
    }

    pub fn ascender(&self, font_handle: FontHandle) -> anyhow::Result<i32> {
        let font = self.get_font(font_handle)?;
        Ok(font.face.size_metrics().unwrap().ascender as i32 >> 6)
    }

    pub fn line_height(&self, font_handle: FontHandle) -> anyhow::Result<i32>  {
        let font = self.get_font(font_handle)?;
        Ok(font.face.size_metrics().unwrap().height as i32 >> 6)
    }

}
