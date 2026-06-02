use std::{cmp::max, collections::HashMap};

use anyhow::Ok;

use crate::{graphics::{FontHandle, FontManager, GraphicsDevice, TextureDesc, TextureFormat, TextureHandle, font::{Glyph, GlyphKey}}, ui::UIRenderer};


pub struct TextRenderer {
    glyph_atlases: Vec<GlyphAtlas>,
    glyph_locations: HashMap<GlyphKey, AtlasGlyph>,
    font_manager: FontManager
}

pub struct GlyphAtlas {
    width: u32,
    height: u32,
    texture: TextureHandle,
    row_h: u32,
    cursor_x: u32,
    cursor_y: u32,
}

pub struct AtlasGlyph {
    pub atlas_id: u32,
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [u32; 2],
    pub bearing: [i32; 2],
    pub advance_x: i64, 
}


impl TextRenderer {
    pub fn new() -> anyhow::Result<Self> {
        let font_manager = FontManager::new()?;

        Ok(Self {
            glyph_atlases: Vec::new(),
            glyph_locations: HashMap::new(),
            font_manager
        })

    }

    fn upload_pixels_to_atlas<G: GraphicsDevice>(&self, 
        atlas_id: usize, 
        x: u32, 
        y: u32, 
        width: u32, 
        height: u32, 
        pixels: &[u8],
        gpu: &mut G) -> anyhow::Result<()> {
        let texture_handle = self.glyph_atlases[atlas_id].texture;
        gpu.write_texture(texture_handle, x, y, width, height, pixels)
    }

    fn upload_glyph<G: GraphicsDevice>(&mut self, key: GlyphKey, glyph: Glyph, gpu: &mut G) -> anyhow::Result<()> {
        let padding = 1;

        if self.glyph_atlases.is_empty() {
            self.create_new_atlas(gpu)?;
        }
        let mut atlas_id = self.glyph_atlases.len() - 1;
        {
            let atlas = &mut self.glyph_atlases[atlas_id]; 

            if glyph.width + padding > atlas.width || glyph.height + padding > atlas.height {
                anyhow::bail!("glyph is larger than atlas");
            }

            if atlas.cursor_x + glyph.width + padding > atlas.width {
                atlas.cursor_x = 0;
                atlas.cursor_y += atlas.row_h + padding;
                atlas.row_h = 0;
            }

            if atlas.cursor_y + glyph.height + padding > atlas.height {
                self.create_new_atlas(gpu)?;
                atlas_id = self.glyph_atlases.len() - 1;
            }

        }

        let atlas = &self.glyph_atlases[atlas_id]; 

        let x = atlas.cursor_x;
        let y = atlas.cursor_y;
        let atlas_width = atlas.width;
        let atlas_height = atlas.height;

        self.upload_pixels_to_atlas(
            atlas_id,
            x,
            y,
            glyph.width,
            glyph.height,
            &glyph.pixels,
            gpu
        )?;


        self.glyph_locations.insert(key, AtlasGlyph{
            size: [glyph.width, glyph.height],
            atlas_id: atlas_id as u32,
            advance_x: glyph.advance_x,
            bearing: [glyph.bearing_x, glyph.bearing_y],
            uv_min: [
                x as f32 /  atlas_width as f32, 
                y as f32 /  atlas_height as f32
            ],
            uv_max: [
                (x + glyph.width) as f32 /  atlas_width as f32, 
                (y + glyph.height) as f32 /  atlas_height as f32
            ],
        });

        let atlas = &mut self.glyph_atlases[atlas_id]; 

        atlas.cursor_x += glyph.width + padding;
        atlas.row_h = max(glyph.height, atlas.row_h); 

        Ok(())
    }

    pub fn load_font(
        &mut self,
        path: impl AsRef<std::path::Path>,
        size: u32,
    ) -> anyhow::Result<FontHandle> {
        self.font_manager.load_font(path, size, 0)
    }


    pub fn render<G: GraphicsDevice>(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_handle: FontHandle,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let start_x = x;
        let mut cursor_x = x;

        // y is top of the text box.
        let mut baseline_y = y + self.font_manager.ascender(font_handle)? as f32;
        let line_height = self.font_manager.line_height(font_handle)? as f32;

        for ch in text.chars() {
            if ch == '\n' {
                cursor_x = start_x;
                baseline_y += line_height;
                continue;
            }

            let Some(glyph_idx) = self.font_manager.glyph_idx_for_char(font_handle, ch)? else {
                continue;
            };

            let key = GlyphKey {
                font_handle,
                glyph_idx,
            };

            if !self.glyph_locations.contains_key(&key) {
                let glyph = {
                    let glyph_ref = self.font_manager.get_or_load_glyph(font_handle, glyph_idx)?;
                    glyph_ref.clone()
                };

                self.upload_glyph(key, glyph, gpu)?;
            }

            let atlas_glyph = &self.glyph_locations[&key];

            let size = atlas_glyph.size;
            let bearing = atlas_glyph.bearing;
            let advance_x = atlas_glyph.advance_x;
            let uv_min = atlas_glyph.uv_min;
            let uv_max = atlas_glyph.uv_max;
            let atlas_id = atlas_glyph.atlas_id;

            let render_x = cursor_x + bearing[0] as f32;
            let render_y = baseline_y - bearing[1] as f32;

            let texture = self.glyph_atlases[atlas_id as usize].texture;

            ui.textured_quad(
                [
                render_x,
                render_y,
                size[0] as f32,
                size[1] as f32,
                ],
                texture,
                [
                uv_min[0],
                uv_min[1],
                uv_max[0],
                uv_max[1],
                ],
                crate::graphics::Color::rgba(0.0, 0.0, 0.0, 1.0),
                2.0,
            )?;

            cursor_x += (advance_x >> 6) as f32;
        }

        Ok(())
    }

    fn create_new_atlas<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        const ATLAS_WIDTH: u32  = 1024; 
        const ATLAS_HEIGHT: u32 = 1024; 
        let handle = gpu.create_texture(
            TextureDesc { 
                width: ATLAS_WIDTH, 
                height: ATLAS_HEIGHT, 
                format: TextureFormat::Alpha8
            },
            None)?;

        self.glyph_atlases.push(
            GlyphAtlas { 
                width:  ATLAS_WIDTH, 
                height: ATLAS_HEIGHT, 
                texture: handle, 
                row_h: 0, 
                cursor_x: 0, 
                cursor_y: 0 
            });

        Ok(())
    } 


}
