use std::{cmp::max, collections::HashMap, num::NonZeroUsize};

use anyhow::Ok;
use lru::LruCache;

use crate::{graphics::{FontHandle, FontManager, GraphicsDevice, TextureDesc, TextureFormat, TextureHandle, font::{Glyph, GlyphKey, ShapedGlyph}}, ui::UIRenderer};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapedTextKey {
    font_handle: FontHandle,
    text: String,
}

pub struct TextRenderer {
    glyph_atlases: Vec<GlyphAtlas>,
    glyph_locations: HashMap<GlyphKey, AtlasGlyph>,
    font_manager: FontManager,
    shaped_cache: LruCache<ShapedTextKey, Vec<ShapedGlyph>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GlyphAtlasKind {
    Alpha,
    Color,
}

pub struct GlyphAtlas {
    width: u32,
    height: u32,
    texture: TextureHandle,
    row_h: u32,
    cursor_x: u32,
    cursor_y: u32,
    kind: GlyphAtlasKind
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
            font_manager,
            shaped_cache: LruCache::new(NonZeroUsize::new(4096).unwrap()),
        })

    }

    fn shape_cached(&mut self, font_handle: FontHandle, text: &str) ->
        anyhow::Result<Vec<ShapedGlyph>> {
            let key = ShapedTextKey {
                font_handle,
                text: text.to_owned()
            };

            if let Some(shaped) = self.shaped_cache.get(&key) {
                return Ok(shaped.clone());
            }

            let shaped = self.font_manager.shape_text(font_handle, text)?;
            self.shaped_cache.put(key, shaped.clone());

            Ok(shaped)
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

    fn latest_atlas_id_of_kind(&self, kind: &GlyphAtlasKind) -> Option<usize> {
        self.glyph_atlases
            .iter()
            .rposition(|atlas| atlas.kind == *kind)
    }
    fn upload_glyph<G: GraphicsDevice>(&mut self, key: GlyphKey, glyph: Glyph, gpu: &mut G, colored: bool) -> anyhow::Result<()> {
        let padding = 1;
        let atlas_kind = if colored { GlyphAtlasKind::Color } else { GlyphAtlasKind::Alpha };

        if !self.latest_atlas_id_of_kind(&atlas_kind).is_some() {
            self.create_new_atlas(gpu, atlas_kind.clone())?;
        }

        let mut atlas_id = self.latest_atlas_id_of_kind(&atlas_kind).expect(
            "Atlas should exist after creation"
        );

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
                let atlas_kind = if colored { GlyphAtlasKind::Color } else { GlyphAtlasKind::Alpha };
                self.create_new_atlas(gpu, atlas_kind)?;
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

        // y is top of the text box.
        let scale = self.font_manager.scale(font_handle)?;
        let mut baseline_y = y + self.font_manager.ascender(font_handle)? as f32 * scale;
        let line_height = self.font_manager.line_height(font_handle)? as f32 * scale;

        let mut seen_atlases: HashMap<u32, TextureHandle> = HashMap::with_capacity(self.glyph_atlases.len());

        for line in text.split('\n') {
            let shaped_glyphs = self.shape_cached(font_handle, line)?;

            let mut cursor_x = x;

            for shaped in shaped_glyphs {
                let key = GlyphKey {
                    font_handle,
                    glyph_idx: shaped.glyph_idx,
                };
                let mut have_uploaded_glyph = false;
                if !self.glyph_locations.contains_key(&key) {
                    let glyph = {
                        let glyph_ref = self.font_manager.get_or_load_glyph(font_handle, shaped.glyph_idx)?;
                        glyph_ref.clone()
                    };

                    let is_colored = self.font_manager.is_colored(font_handle)?;
                    self.upload_glyph(key, glyph, gpu, is_colored)?;

                    have_uploaded_glyph = true;
                }

                let atlas_glyph = &self.glyph_locations[&key];

                let uv_min = atlas_glyph.uv_min;
                let uv_max = atlas_glyph.uv_max;

                let atlas_id = atlas_glyph.atlas_id;
                let texture = self.glyph_atlases[atlas_id as usize].texture;

                if have_uploaded_glyph && !seen_atlases.contains_key(&atlas_id) {
                    seen_atlases.insert(atlas_id, texture);
                }


                // emojis are handled as if they were images
                let kind = if self.font_manager.is_colored(font_handle)? { 1.0 } else { 2.0 };

                let font_scale = self.font_manager.scale(font_handle)?;

                let render_x =
                    cursor_x
                    + shaped.x_off * font_scale
                    + atlas_glyph.bearing[0] as f32 * font_scale;

                let render_y =
                    baseline_y
                    - shaped.y_off * font_scale
                    - atlas_glyph.bearing[1] as f32 * font_scale;

                let w = atlas_glyph.size[0] as f32 * font_scale;
                let h = atlas_glyph.size[1] as f32 * font_scale;

                ui.textured_quad(
                    [
                    render_x,
                    render_y,
                    w,
                    h
                    ],
                    texture,
                    [
                    uv_min[0],
                    uv_min[1],
                    uv_max[0],
                    uv_max[1],
                    ],
                    crate::graphics::Color::rgba(0.0, 0.0, 0.0, 1.0),
                    kind
                )?;

                cursor_x += shaped.x_adv * font_scale;
                baseline_y += shaped.y_adv * font_scale;


            }
            baseline_y += line_height;
        }

        for (_, atlas) in seen_atlases {
            gpu.texture_gen_mipmap(atlas)?;
        }
     
        Ok(())
    }

    fn create_new_atlas<G: GraphicsDevice>(&mut self, gpu: &mut G, kind: GlyphAtlasKind) -> anyhow::Result<()> {
        const ATLAS_WIDTH: u32  = 1024; 
        const ATLAS_HEIGHT: u32 = 1024; 
        let format = if kind == GlyphAtlasKind::Alpha {
            TextureFormat::Alpha8
        } else {
            TextureFormat::Rgba8
        };
        let empty = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize];
        let handle = gpu.create_texture(
            TextureDesc { 
                width: ATLAS_WIDTH, 
                height: ATLAS_HEIGHT, 
                format 
            },
            Some(&empty))?;

        self.glyph_atlases.push(
            GlyphAtlas { 
                width:  ATLAS_WIDTH, 
                height: ATLAS_HEIGHT, 
                texture: handle, 
                row_h: 0, 
                cursor_x: 0, 
                cursor_y: 0,
                kind 
            });

        Ok(())
    } 


}
