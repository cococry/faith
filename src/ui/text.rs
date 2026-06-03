use std::{collections::HashMap, num::NonZeroUsize};

use lru::LruCache;
use unicode_segmentation::UnicodeSegmentation;

use crate::{graphics::{FontHandle, FontManager, GraphicsDevice, font::{Glyph, GlyphKey, ShapedGlyph}}, ui::UIRenderer};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapedTextKey {
    font_handle: FontHandle,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClusterFontKey {
    original: FontHandle,
    cluster: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontSupportKey{
    cluster: String,
    font_handle: FontHandle
}

#[derive(Debug, Clone)]
struct TextRun {
    font_handle: FontHandle,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallbackRunKey {
    original_font: FontHandle,
    text: String,
}

pub struct TextRenderer {
    glyph_locations: HashMap<GlyphKey, AtlasGlyph>,
    font_manager: FontManager,
    shaped_cache: LruCache<ShapedTextKey, Vec<ShapedGlyph>>,

    fallback_cache: HashMap<ClusterFontKey, FontHandle>,
    font_run_cache: LruCache<FallbackRunKey, Vec<TextRun>>,
    font_support_cache: HashMap<FontSupportKey, bool>,
    fallback_fonts: Vec<FontHandle>,
}


pub struct AtlasGlyph {
    pub atlas_layer: u32,
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [u32; 2],
    pub bearing: [i32; 2],
}

impl TextRenderer {
    pub fn new() -> anyhow::Result<Self> {
        let font_manager = FontManager::new()?;
        
        Ok(Self {
            glyph_locations: HashMap::new(),
            font_manager,
            shaped_cache: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            fallback_cache: HashMap::new(),
            font_run_cache: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            font_support_cache: HashMap::new(),
            fallback_fonts: Vec::new(),
        })

    }

    fn font_supports_cluster_cached(&mut self, font_handle: FontHandle, 
        cluster: &str) -> anyhow::Result<bool> {
        let key = FontSupportKey {
            font_handle,
            cluster: cluster.to_owned(),
        };

        if let Some(&supported) = self.font_support_cache.get(&key) {
            return Ok(supported)
        }

        let shaped = self.font_manager.shape_text(font_handle, cluster)?;

        let supported = !shaped.is_empty()
            && shaped.iter().all(|g| g.glyph_idx != 0);

        self.font_support_cache.insert(key, supported);

        Ok(supported)

    }

    fn build_fallback_runs(
        &mut self,
        original_font: FontHandle,
        text: &str,
    ) -> anyhow::Result<Vec<TextRun>> {

        let key = FallbackRunKey {
            original_font,
            text: text.to_owned(),
        };

        if let Some(runs) = self.font_run_cache.get(&key) {
            return Ok(runs.clone())
        }

        let mut runs = Vec::new();

        let mut current_font: Option<FontHandle> = None;
        let mut current_text = String::new();

        for cluster in text.graphemes(true) {
            let font = self.font_for_cluster(original_font, cluster)?;

            if current_font != Some(font) {
                if let Some(font_handle) = current_font {
                    runs.push(TextRun {
                        font_handle,
                        text: std::mem::take(&mut current_text),
                    });
                }
                current_font = Some(font); 
            }

            current_text.push_str(cluster);
        }
                
        if let Some(font_handle) = current_font {
            runs.push(TextRun {
                font_handle,
                text: current_text,
            });
        }
        
        self.font_run_cache.put(key, runs.clone());

        Ok(runs)
    }

    fn font_for_cluster(&mut self, original_font: FontHandle, 
        cluster: &str) -> anyhow::Result<FontHandle>{
        let key = ClusterFontKey {
            original: original_font,
            cluster: cluster.to_owned(),
        };
        
        if let Some(font) = self.fallback_cache.get(&key) {
            return Ok(*font)
        }

        if self.font_supports_cluster_cached(original_font, cluster)? {
            self.fallback_cache.insert(key, original_font);
            return Ok(original_font)
        }


        for i in 0..self.fallback_fonts.len() {
            let static_fallback = self.fallback_fonts[i];

            if static_fallback == original_font {
                continue;
            }

            if self.font_supports_cluster_cached(static_fallback, cluster)? {
                // found fallback
                self.fallback_cache.insert(key, static_fallback);
                return Ok(static_fallback);
            }
        }

        self.fallback_cache.insert(key, original_font);
        Ok(original_font)
    }

    fn shape_cached(&mut self, font_handle: FontHandle, text: &str) ->
        anyhow::Result<Vec<ShapedGlyph>> {
            let key = ShapedTextKey {
                font_handle,
                text: text.to_owned()
            };

            if text.len() <= 4096 {
                if let Some(shaped) = self.shaped_cache.get(&key) {
                    return Ok(shaped.clone());
                }
            }

            let shaped = self.font_manager.shape_text(font_handle, text)?;

            if text.len() <= 4096 {
                self.shaped_cache.put(key, shaped.clone());
            }

            Ok(shaped)
        }


    fn upload_glyph<G: GraphicsDevice>(
        &mut self, key: GlyphKey, glyph: 
        Glyph, gpu: &mut G, 
        ui: &mut UIRenderer) -> anyhow::Result<()> {

        let (layer, x, y, atlas_w, atlas_h) =
            ui.allocate_image_rect(glyph.width, glyph.height, 1)?;


        ui.upload_pixels_to_atlas(
            x,
            y,
            glyph.width,
            glyph.height,
            layer,
            &glyph.pixels,
            gpu
        )?;

        self.glyph_locations.insert(key, AtlasGlyph{
            size: [glyph.width, glyph.height],
            atlas_layer: layer,
            bearing: [glyph.bearing_x, glyph.bearing_y],
            uv_min: [
                x as f32 /  atlas_w as f32, 
                y as f32 /  atlas_h as f32
            ],
            uv_max: [
                (x + glyph.width) as f32 /  atlas_w as f32, 
                (y + glyph.height) as f32 /  atlas_h as f32
            ],
        }
        );

        Ok(())


    }

    pub fn load_font(
        &mut self,
        path: impl AsRef<std::path::Path>,
        size: u32,
    ) -> anyhow::Result<FontHandle> {
        let font = self.font_manager.load_font(path, size, 0)?;

        self.fallback_fonts.push(font);
        
        self.font_run_cache.clear();
        self.fallback_cache.clear();
        self.font_support_cache.clear();

        Ok(font)
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

        for line in text.split('\n') {
            let runs = self.build_fallback_runs(font_handle, line)?;

            let mut cursor_x = x;

            for run in runs {
                let run_font = run.font_handle;
                let shaped_glyphs = self.shape_cached(run_font, &run.text)?;

                let is_colored = self.font_manager.is_colored(run_font)?;

                // emojis are handled as if they were images
                let kind = if is_colored { 1.0 } else { 2.0 };

                let font_scale = self.font_manager.scale(run_font)?;
                let font_render_scale = self.font_manager.render_scale(run_font)?;

                let color = if is_colored {
                    crate::graphics::Color::rgba(1.0, 1.0, 1.0, 1.0)
                } else {
                    crate::graphics::Color::rgba(0.0, 0.0, 0.0, 1.0)
                };


                for shaped in shaped_glyphs {
                    let key = GlyphKey {
                        font_handle: run_font,
                        glyph_idx: shaped.glyph_idx,
                    };
                    if !self.glyph_locations.contains_key(&key) {
                        let glyph = {
                            let glyph_ref = self.font_manager.get_or_load_glyph(run_font, shaped.glyph_idx)?;
                            glyph_ref.clone()
                        };

                        self.upload_glyph(key, glyph, gpu, ui)?;
                    }

                    let atlas_glyph = &self.glyph_locations[&key];

                    let uv_min = atlas_glyph.uv_min;
                    let uv_max = atlas_glyph.uv_max;

                    let atlas_layer = atlas_glyph.atlas_layer;

                    let render_x =
                        cursor_x
                        + shaped.x_off * font_scale
                        + atlas_glyph.bearing[0] as f32 * font_render_scale;

                    let render_y =
                        baseline_y
                        - shaped.y_off * font_scale
                        - atlas_glyph.bearing[1] as f32 * font_render_scale;

                    let w = atlas_glyph.size[0] as f32 * font_render_scale;
                    let h = atlas_glyph.size[1] as f32 * font_render_scale;

                    ui.raw_quad_atlas(
                        [
                        render_x,
                        render_y,
                        w,
                        h
                        ],
                        [
                        uv_min[0],
                        uv_min[1],
                        uv_max[0],
                        uv_max[1],
                        ],
                        color,
                        [0.0, 0.0, atlas_layer as f32, kind]
                    )?;

                    cursor_x += shaped.x_adv * font_scale;
                    baseline_y += shaped.y_adv * font_scale;


                }
            }
            baseline_y += line_height;
        }
     
        Ok(())
    }

}
