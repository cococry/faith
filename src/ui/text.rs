use std::{collections::HashMap, num::NonZeroUsize};

use anyhow::{Ok, anyhow};
use lru::LruCache;
use unicode_segmentation::UnicodeSegmentation;

use crate::{graphics::{FontHandle, FontManager, GraphicsDevice, font::{Glyph, GlyphKey, ShapedGlyph}}, ui::{UIRenderer, renderer::QuadType}};
use unicode_bidi::{BidiInfo, Level};
use unicode_script::{Script, UnicodeScript};


/// Used as the key to get cached shaped 
/// glyphs for a given font/string combination
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ShapedTextKey {
    font_handle: FontHandle,
    text: String,
    bidi_lvl: u8,
    script: Script,
}


/// A sub-region of a text string that 
/// has been shaped by the font in 
/// `font_handle`.
/// Stores shaping & unicode/bidi information 
/// for a text run.
/// `font_handle` is the (fallback) font that 
/// supports all grapheme clusters within the 
/// `text` string. 
#[derive(Clone, Debug)]
struct ShapedTextRun {
    font_handle: FontHandle,
    text: String,

    shaped_glyphs: Vec<ShapedGlyph>,

    byte_start: usize,
    bidi_lvl: Level,
    script: Script,

    x_adv: f32,
    y_adv: f32,
}


/// A sub-region of a text string that 
/// has been itemized by splitting into 
/// bidi visual runs & scripts. 
#[derive(Debug, Clone)]
struct ItemizedRun {
    font_handle: FontHandle,
    text: String,
    byte_start: usize,

    bidi_lvl: unicode_bidi::Level,
    script: unicode_script::Script,
}

/// Used as the key to get cached shaped 
/// text runs for a given font/string combination
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ShapedRunKey {
    original_font: FontHandle, 
    text: String,
}

/// Used as the key to get cached font fallback 
/// choices for a given rendered string 
/// with a given original font
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FallbackChoiceKey {
    original_font: FontHandle,
    text: String,
    bidi_lvl: u8,
    script: Script,
}

/// Represents a glyph's UV coordinate- 
/// and layer-information within the  
/// UIRenderer.ui_texture_array
struct AtlasGlyph {
    pub atlas_layer: u32,
    pub uv_min:     [f32; 2],
    pub uv_max:     [f32; 2],
    pub size:       [u32; 2],
    pub bearing:    [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextLayoutKey {
    font_handle: FontHandle,
    text: String,

    // Store quantized width so float hashing is not needed.
    max_width_px: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    lines: Vec<TextLine>,
    width: f32,
    height: f32,
    line_height: f32,
    ascender: f32
}

#[derive(Debug, Clone)]
struct TextLine {
    runs: Vec<ShapedTextRun>,
    width: f32,
    is_rtl: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TextLayoutOptions {
    pub max_width: Option<f32>,
}

impl Default for TextLayoutOptions {
    fn default() -> Self {
        Self {
            max_width: None,
        }
    }
}

/// Text rendering engine. 
/// Manages glyph- and font caches 
/// as well as loaded fallback fonts.
///
/// Uses FontManager to shape texts/glyphs 
/// with HarfBuzz and load glyphs/fonts with 
/// FreeType.
pub struct TextRenderer {
    font_manager: FontManager,

    /// Provided possible fallback fonts 
    /// for glyphs. We do not do dynamic
    /// fallback font discovery such as 
    /// DirectWrite or fontconfig.
    fallback_fonts: Vec<FontHandle>,

    /// Cached atlas locations (uv, atlas layer) of glyphs
    glyph_locations: HashMap<GlyphKey, AtlasGlyph>,

    shaped_cache: LruCache<ShapedTextKey, Vec<ShapedGlyph>>,
    font_run_cache: LruCache<ShapedRunKey, Vec<ShapedTextRun>>,
    fallback_choice_cache: LruCache<FallbackChoiceKey, Option<FontHandle>>,

    layout_cache: LruCache<TextLayoutKey, TextLayout>,
}

impl TextRenderer {
    /// Creates a new text renderer with empty 
    /// glyph-, fallback-font-, and text-shaping caches.
    ///
    /// Initializes FreeType internally by 
    /// calling FontManager::new() 
    pub fn new() -> anyhow::Result<Self> {
        let font_manager = FontManager::new()?;

        Ok(Self {
            font_manager,
            fallback_fonts: Vec::new(),

            glyph_locations: HashMap::new(),

            shaped_cache: LruCache::new(NonZeroUsize::new(8192).unwrap()),
            font_run_cache: LruCache::new(NonZeroUsize::new(2048).unwrap()),
            fallback_choice_cache: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            layout_cache: LruCache::new(NonZeroUsize::new(1024).unwrap()),
        })

    }

    fn is_neutral_script(
        &self, 
        script: Script
    ) -> bool {
        matches!(script, Script::Common
            | Script::Unknown
            | Script::Inherited)
    }

    fn char_script(
        &self,
        ch: char, 
        crnt_script: Option<Script>
    ) -> Script {
        let script = ch.script(); 

        if self.is_neutral_script(script) {
            crnt_script.unwrap_or(script)
        } else {
            script
        }
    }

    fn split_by_script(
        &self,
        text: &str,
        byte_off: usize,
        bidi_lvl: Level,
        font_handle: FontHandle
    ) -> anyhow::Result<Vec<ItemizedRun>> {
        let mut runs = Vec::new();

        let mut run_off = 0;
        let mut crnt_script: Option<Script> = None;

        for (idx, ch) in text.char_indices() {
            let script = self.char_script(ch, crnt_script);

            match crnt_script {
                None => {
                    crnt_script = Some(script);
                }
                Some(old_script) if script != old_script && 
                    !self.is_neutral_script(script) => {
                        if run_off < idx {
                            runs.push(
                                ItemizedRun { 
                                    font_handle, 
                                    text: text[run_off..idx].to_owned(),
                                    byte_start: byte_off + run_off,
                                    bidi_lvl,
                                    script: old_script 
                                }
                            );
                        }
                        run_off = idx;
                        crnt_script = Some(script);
                    }

                _ => {}
            }
        }

        if run_off < text.len() {

            runs.push(
                ItemizedRun { 
                    font_handle, 
                    text: text[run_off..].to_owned(),
                    byte_start: byte_off + run_off,
                    bidi_lvl: bidi_lvl,
                    script: crnt_script.unwrap_or(Script::Unknown)
                }
            );
        }

        Ok(runs)
    }

    fn itemize_runs(
        &mut self, 
        original_font: FontHandle,
        text: &str
    ) -> anyhow::Result<Vec<ItemizedRun>> {

        let bidi = BidiInfo::new(text, None);
        let mut runs = Vec::new();

        for paragraph in &bidi.paragraphs {
            let range = paragraph.range.clone(); 

            let (_, visual_runs) = 
                bidi.visual_runs(paragraph, range.clone());

            for run in visual_runs {
                let run_text = &text[run.clone()];
                let lvl = bidi.levels[run.start];

                let mut split = self.split_by_script(
                    run_text, 
                    run.start,
                    lvl, 
                    original_font)?; 

                // split_by_script walks the bidi run in logical 
                // string order.
                //
                // The outer renderer places returned runs from 
                // left to right as visual boxes. For RTL bidi 
                // runs, the script runs therefore need to be 
                // reversed so they remain in visual order.
                if lvl.is_rtl() {
                    split.reverse();
                }
                runs.extend(split);
            }
        }

        Ok(runs)
    }

    fn cluster_to_grapheme_range(&self, text: &str, cluster: u32) -> std::ops::Range<usize> {
        let cluster = cluster as usize;

        for (start, grapheme) in text.grapheme_indices(true) {
            let end = start + grapheme.len();

            if cluster >= start && cluster < end {
                return start..end;
            }
        }
        0..0
    }

    fn merge_ranges(
        &self,
        mut ranges: Vec<std::ops::Range<usize>>,
    ) -> Vec<std::ops::Range<usize>> {
        ranges.sort_by_key(|range| range.start);

        let mut merged: Vec<std::ops::Range<usize>> = Vec::new();

        for range in ranges {
            if range.start >= range.end {
                continue;
            }

            if let Some(last) = merged.last_mut() {
                // This merges duplicate grapheme 
                // ranges and also adjacent ranges.
                // Adjacent merging is done because 
                // for most scripts like Arabic, 
                // fallback-glyphs should be shaped 
                // as a connected word.
                if range.start <= last.end {
                    last.end = last.end.max(range.end);
                    continue;
                }
            }

            merged.push(range);
        }

        merged
    }

    fn fallback_for_text_cached(
        &mut self,
        original_font: FontHandle,
        text: &str,
        bidi_lvl: Level,
        script: Script,
    ) -> anyhow::Result<Option<FontHandle>> {
        let key = FallbackChoiceKey {
            original_font,
            text: text.to_owned(),
            bidi_lvl: bidi_lvl.number(),
            script,
        };

        if let Some(&cached) = self.fallback_choice_cache.get(&key) {
            return Ok(cached);
        }

        for i in 0..self.fallback_fonts.len() {
            let fallback = self.fallback_fonts[i];

            if fallback == original_font {
                continue;
            }

            let glyphs = self.shape_cached(
                text,
                bidi_lvl,
                script,
                fallback,
            )?;

            if glyphs.iter().all(|g| g.glyph_idx != 0) {
                self.fallback_choice_cache.push(key, Some(fallback));
                return Ok(Some(fallback));
            }
        }

        self.fallback_choice_cache.push(key, None);
        Ok(None)
    }

    fn repair_missing_glyphs_in_run(
        &mut self,
        run: ItemizedRun,
        glyphs: Vec<ShapedGlyph>
    ) -> anyhow::Result<Vec<ShapedTextRun>> {
        let mut repaired = Vec::new();


        // acquire missing grapheme ranges within the run
        let mut missing_ranges = Vec::new();

        for glyph in &glyphs {
            if glyph.glyph_idx == 0 {
                // Get the grapheme range for this 
                // cluster. This works because glyph.cluster 
                // is a byte offset within the run's text.
                let range = self.cluster_to_grapheme_range(&run.text, glyph.cluster);

                if range.start < range.end {
                    missing_ranges.push(range);
                }
            }
        }
        // Merge duplicate and adjacent ranges.
        // Duplicate ranges are merged to avoid 
        // shaping & rendering duplicate glyphs 
        // incorrectly, while adjacent ranges 
        // are merged to maintain shaping context.
        //
        // HarfBuzz needs to be able to see whole 
        // arabic words at once in order to generate 
        // correct glyphs, thus we merge adjacent 
        // fallback ranges.
        let missing_ranges = self.merge_ranges(missing_ranges);

        let mut cursor = 0;

        for missing in missing_ranges {
            // Not in a missing range -> shape with 
            // run's font for that section. 
            if cursor < missing.start {
                let text = run.text[cursor..missing.start].to_owned();

                let shaped_glyphs = self.shape_cached(
                    &text,
                    run.bidi_lvl,
                    run.script,
                    run.font_handle,
                )?;

                let (x_adv, y_adv) = shaped_glyphs
                    .iter()
                    .fold((0.0, 0.0), |(x, y), g| {
                        (x + g.x_adv, y + g.y_adv)
                    });

                repaired.push(ShapedTextRun {
                    font_handle: run.font_handle,
                    text,
                    shaped_glyphs,
                    byte_start: run.byte_start + cursor,
                    bidi_lvl: run.bidi_lvl,
                    script: run.script,
                    x_adv,
                    y_adv
                });
            }

            let missing_text = run.text[missing.clone()].to_owned();


            // Check loaded fallback fonts 
            // wheter one of them supports the 
            // missing  text range.
            let fallback = self.fallback_for_text_cached(
                run.font_handle,
                &missing_text,
                run.bidi_lvl,
                run.script,
            )?;

            // If we successfully found a fallback 
            // font for the missing range, push the new
            // fallen-back run.
            if let Some(fallback) = fallback {
                let fallback_glyphs = self.shape_cached(
                    &missing_text,
                    run.bidi_lvl,
                    run.script,
                    fallback,
                )?;
                let (x_adv, y_adv) = fallback_glyphs
                    .iter()
                    .fold((0.0, 0.0), |(x, y), g| {
                        (x + g.x_adv, y + g.y_adv)
                    });


                repaired.push(ShapedTextRun {
                    font_handle: fallback,
                    text: missing_text,
                    shaped_glyphs: fallback_glyphs,
                    byte_start: run.byte_start + missing.start,
                    bidi_lvl: run.bidi_lvl,
                    script: run.script,
                    x_adv,
                    y_adv,
                });
            } else {
                // If no fallback font supports this range, keep the 
                // original shaped result so the missing glyphs still 
                // contribute advance width.
                let shaped_glyphs = self.shape_cached(
                    &missing_text,
                    run.bidi_lvl,
                    run.script,
                    run.font_handle,
                )?;

                let (x_adv, y_adv) = shaped_glyphs
                    .iter()
                    .fold((0.0, 0.0), |(x, y), g| {
                        (x + g.x_adv, y + g.y_adv)
                    });

                repaired.push(ShapedTextRun {
                    font_handle: run.font_handle,
                    text: missing_text,
                    shaped_glyphs,
                    byte_start: run.byte_start + missing.start,
                    bidi_lvl: run.bidi_lvl,
                    script: run.script,
                    x_adv,
                    y_adv
                });

            }


            // Update cursor to range end
            cursor = missing.end;
        }

        // Push the last range if there is still one
        if cursor < run.text.len() {
            let text = run.text[cursor..].to_owned();

            let shaped_glyphs = self.shape_cached(
                &text,
                run.bidi_lvl,
                run.script,
                run.font_handle,
            )?;

            let (x_adv, y_adv) = shaped_glyphs
                .iter()
                .fold((0.0, 0.0), |(x, y), g| {
                    (x + g.x_adv, y + g.y_adv)
                });

            repaired.push(ShapedTextRun {
                font_handle: run.font_handle,
                text,
                shaped_glyphs,
                byte_start: run.byte_start + cursor,
                bidi_lvl: run.bidi_lvl,
                script: run.script,
                x_adv,
                y_adv
            });
        }

        // Repaired glyphs need to be reversed 
        // if run is right-to-left.
        if run.bidi_lvl.is_rtl() {
            repaired.reverse();
        }

        Ok(repaired)
    }

    fn shape_cached(
        &mut self,
        text: &str,
        bidi_lvl: Level,
        script: Script,
        font_handle: FontHandle,
    ) -> anyhow::Result<Vec<ShapedGlyph>> {
        let key = ShapedTextKey {
            font_handle,
            text: text.to_owned(),
            bidi_lvl: bidi_lvl.number(),
            script,
        };

        if let Some(glyphs) = self.shaped_cache.get(&key) {
            return Ok(glyphs.clone());
        }

        let glyphs = self.font_manager.shape_text_with_props(
            text,
            bidi_lvl,
            script,
            font_handle,
        )?;

        self.shaped_cache.put(key, glyphs.clone());

        Ok(glyphs)
    }

    fn shape_run(
        &mut self, 
        run: ItemizedRun 
    ) -> anyhow::Result<Vec<ShapedTextRun>> {
        let glyphs = self.shape_cached(
            &run.text,
            run.bidi_lvl,
            run.script,
            run.font_handle,
        )?;


        let mut all_glyphs_support = true; 
        let mut x_adv = 0.0;
        let mut y_adv = 0.0;

        for glyph in &glyphs {
            if all_glyphs_support && glyph.glyph_idx == 0 {
                all_glyphs_support = false
            }
            x_adv += glyph.x_adv;
            y_adv += glyph.y_adv;
        }

        // If all shaped glyphs are supported by 
        // the run's font, return the shaped run.
        if all_glyphs_support {
            return Ok(vec![ShapedTextRun {
                font_handle: run.font_handle,
                text: run.text,
                byte_start: run.byte_start,
                bidi_lvl: run.bidi_lvl,
                script: run.script,
                x_adv,
                y_adv,
                shaped_glyphs: glyphs,
            }]);
        }

        // If not all shaped glyphs are supported 
        // by the run's font, repair the missing glyphs
        // by generating fallback runs and returning them. 
        self.repair_missing_glyphs_in_run(run, glyphs)
    }

    fn build_shaped_runs_cached(
        &mut self,
        original_font: FontHandle,
        text: &str,
    ) -> anyhow::Result<Vec<ShapedTextRun>> {
        let key = ShapedRunKey {
            original_font,
            text: text.to_owned(),
        };

        if let Some(runs) = self.font_run_cache.get(&key) {
            return Ok(runs.clone());
        }

        let itemized = self.itemize_runs(original_font, text)?;

        let mut shaped_runs = Vec::new();

        for run in itemized {
            shaped_runs.extend(self.shape_run(run)?);
        }

        self.font_run_cache.put(key, shaped_runs.clone());

        Ok(shaped_runs)
    } 


    fn upload_glyph<G: GraphicsDevice>(
        &mut self, key: GlyphKey, glyph: 
        Glyph, gpu: &mut G, 
        ui: &mut UIRenderer) -> anyhow::Result<()> {

        // Use UIRenderer to allocate a region 
        // for the glyph in the texture array
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

        // Avoid inserting zero-sized glyphs 
        if glyph.width == 0 || glyph.height == 0 {
            self.glyph_locations.insert(key, AtlasGlyph {
                atlas_layer: 0,
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size: [0, 0],
                bearing: [glyph.bearing_x, glyph.bearing_y],
            });

            return Ok(());
        }

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

        if glyph.width == 0 || glyph.height == 0 {
            self.glyph_locations.insert(key, AtlasGlyph {
                atlas_layer: 0,
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size: [0, 0],
                bearing: [glyph.bearing_x, glyph.bearing_y],
            });

            return Ok(());
        }

        Ok(())


    }

    /// Loads a given font path and returns the FontHandle 
    /// for it. 
    /// Clears the text engine's font-run-, fallback-,
    /// and font-support-caches.
    pub fn load_font(
        &mut self,
        path: impl AsRef<std::path::Path>,
        size: u32,
    ) -> anyhow::Result<FontHandle> {
        let font = self.font_manager.load_font(path, size, 0)?;

        self.fallback_fonts.push(font);
        self.fallback_choice_cache.clear();
        self.layout_cache.clear();
        self.font_run_cache.clear();

        Ok(font)
    }

    fn render_shaped_run<G: GraphicsDevice>(
        &mut self,
        run_x: f32,
        baseline_y: f32,
        run: &ShapedTextRun,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let run_font = run.font_handle;

        let is_colored = self.font_manager.is_colored(run_font)?;

        let kind = if is_colored { 
            QuadType::ColoredImage 
        } else { 
            QuadType::TextGlyph 
        };

        let font_scale = self.font_manager.scale(run_font)?;
        let font_render_scale = self.font_manager.render_scale(run_font)?;

        let color = if is_colored {
            crate::graphics::Color::rgba(1.0, 1.0, 1.0, 1.0)
        } else {
            crate::graphics::Color::rgba(0.0, 0.0, 0.0, 1.0)
        };

        let mut pen_x = run_x;

        for shaped in &run.shaped_glyphs {
            let key = GlyphKey {
                font_handle: run_font,
                glyph_idx: shaped.glyph_idx,
            };

            if !self.glyph_locations.contains_key(&key) {
                let glyph = {
                    let glyph_ref = self.font_manager.get_or_load_glyph(
                        run_font,
                        shaped.glyph_idx,
                    )?;
                    glyph_ref.clone()
                };

                self.upload_glyph(key, glyph, gpu, ui)?;
            }

            let atlas_glyph = &self.glyph_locations[&key];

            if atlas_glyph.size[0] != 0 && atlas_glyph.size[1] != 0 {
                let render_x =
                    pen_x
                    + shaped.x_off * font_scale
                    + atlas_glyph.bearing[0] as f32 * font_render_scale;

                let render_y =
                    baseline_y
                    - shaped.y_off * font_scale
                    - atlas_glyph.bearing[1] as f32 * font_render_scale;

                let w = atlas_glyph.size[0] as f32 * font_render_scale;
                let h = atlas_glyph.size[1] as f32 * font_render_scale;

                ui.raw_quad_atlas(
                    [render_x, render_y, w, h],
                    [
                    atlas_glyph.uv_min[0],
                    atlas_glyph.uv_min[1],
                    atlas_glyph.uv_max[0],
                    atlas_glyph.uv_max[1],
                    ],
                    color,
                    [0.0, 0.0, atlas_glyph.atlas_layer as f32, kind.as_f32()],
                )?;
            }

            pen_x += shaped.x_adv * font_scale;
        }

        Ok(())
    }

    /// Renders a given text with a given font. 
    ///
    /// The given font is the preferred font to 
    /// render glyphs with. If a grapheme cluster 
    /// is not supported by the preferred font, 
    /// it is rendered using a fallback font within 
    /// TextRenderer.fallback_fonts. 
    /// If no font supports a glyph, it is not rendered. 
    ///
    /// The rendered text is split into horizontal lines 
    /// by `\n` characters in the provided string.  
    pub fn render<G: GraphicsDevice>(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_handle: FontHandle,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {

        // Use baseline-y and line height of the 
        // base, preferred font.
        let base_scale = self.font_manager.scale(font_handle)?;
        let mut baseline_y = y + self.font_manager.ascender(font_handle)? as f32 * base_scale;
        let line_height = self.font_manager.line_height(font_handle)? as f32 * base_scale;

        for line in text.split('\n') {
            let runs = self.build_shaped_runs_cached(font_handle, line)?;

            let mut cursor_x = x;

            for run in runs {

                self.render_shaped_run(cursor_x, baseline_y, &run, gpu, ui)?;

                let font_scale = self.font_manager.scale(run.font_handle)?;
                cursor_x += run.x_adv * font_scale;
            }
            baseline_y += line_height;
        }

        Ok(())
    }

    fn layout_cached(
        &mut self,
        text: &str,
        font_handle: FontHandle,
        options: TextLayoutOptions,
    ) -> anyhow::Result<TextLayout> {

        let key = TextLayoutKey {
            font_handle,
            text: text.to_owned(),
            max_width_px: options.max_width.map(|w| w.ceil() as u32),
        };

        if let Some(layout) = self.layout_cache.get(&key) {
            return Ok(layout.clone());
        }

        let base_scale = self.font_manager.scale(font_handle)?;
        let line_height = self.font_manager.line_height(font_handle)? as f32 * base_scale;
        let ascender = self.font_manager.ascender(font_handle)? as f32 * base_scale;

        let mut lines = Vec::new();

        // TODO: Wrap lines by '\n' before layout 
        // based wrapping.
            self.layout_paragraph(
                &text.replace('\n', ""),
                font_handle,
                options.max_width,
                &mut lines,
            )?;

        let width = lines.iter()
            .map(|line| line.width)
            .fold(0.0, f32::max);

        let height = lines.len() as f32 * line_height;

        let layout = TextLayout {
            lines,
            width,
            height,
            line_height, 
            ascender
        };

        self.layout_cache.put(key, layout.clone());

        Ok(layout)
    }

    fn runs_width(&mut self, runs: &[ShapedTextRun]) -> anyhow::Result<f32> {
        // Gets the combined width of all 
        // text runs in a given slice of runs,
        // adjusted for the run font's 
        // scale factor.
        let mut width = 0.0;

        for run in runs {
            let scale = self.font_manager.scale(run.font_handle)?;
            width += run.x_adv * scale;
        }

        Ok(width)
    }

    fn measure_text(
        &mut self,
        text: &str,
        font_handle: FontHandle,
    ) -> anyhow::Result<f32> {
        let runs = self.build_shaped_runs_cached(font_handle, text)?;
        self.runs_width(&runs)
    }

    fn is_leading_forbidden_token(&self, token: &str) -> bool {
    token.chars().all(|ch| {
        matches!(
            ch,
            '.' | ',' | ';' | ':' | '!' | '?' |
            ')' | ']' | '}' |
            '»' | '”' | '’' |
            '،' | '؛' | '؟'
        )
    })
    }

    fn word_wrap_tokens(&self, paragraph: &str) -> Vec<String> {
        let mut tokens: Vec<String> = Vec::new();

        for token in paragraph.split_word_bounds() {
            if self.is_leading_forbidden_token(token) {
                if let Some(prev) = tokens.last_mut() {
                    prev.push_str(token);
                    continue;
                }
            }

            tokens.push(token.to_owned());
        }

        tokens
    }

    fn shape_and_push_line(
        &mut self,
        paragraph: &str,
        font_handle: FontHandle,
        lines: &mut Vec<TextLine>,
    ) -> anyhow::Result<()> {

        let runs = self.build_shaped_runs_cached(font_handle, paragraph)?;
        let width = self.runs_width(&runs)?;
        let mut is_rtl = false;
        for run in &runs {
            is_rtl = run.bidi_lvl.is_rtl();
            if is_rtl {
                break;
            }
        }

        lines.push(TextLine { runs, width, is_rtl });
        Ok(())
    }

    fn layout_paragraph(
        &mut self,
        paragraph: &str,
        font_handle: FontHandle,
        max_width: Option<f32>,
        lines: &mut Vec<TextLine>,
    ) -> anyhow::Result<()> {
        // If there is no maximum width, do not 
        // care to wrap paragraph.
        let Some(max_width) = max_width else {
            return self.shape_and_push_line(paragraph, font_handle, lines);
        };

        let mut current_text = String::new();
        let mut current_width = 0.0;

        for token in self.word_wrap_tokens(paragraph) {
            // Only build the word token's runs for 
            // the current word.
            let token_width = self.measure_text(&token, font_handle)?;

            if !current_text.is_empty() && current_width + token_width > max_width {
                // If there has been a line wrap, push a new 
                // text line into the layout's lines and start 
                // over from the next line.
                let line_text = current_text.trim_end().to_owned();

                // Build the entire line's runs once at the end 
                self.shape_and_push_line(&line_text, font_handle, lines)?;

                current_text.clear();
                current_width = 0.0;

                let token = token.trim_start();
                if token.is_empty() {
                    continue;
                }

                // Consume the first token if not empty  
                let token_width = self.measure_text(token, font_handle)?;
                current_text.push_str(token);
                current_width += token_width;
            } else {
                // Advance by the measured token width if there 
                // has not been a line break caused by wrapping.
                current_text.push_str(&token);
                current_width += token_width;
            }
        }

        // Push the remaining text line 
        if !current_text.is_empty() {
            let line_text = current_text.trim_end().to_owned();

            self.shape_and_push_line(&line_text, font_handle, lines)?;
        }


        Ok(())
    }

    /// Renders text with word wrapping.
    ///
    /// Lays out the text into wrapped lines using 
    /// `max_width` and renders the resulting layout 
    /// at the given position.
    pub fn render_wrapped<G: GraphicsDevice>(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_handle: FontHandle,
        max_width: f32,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let layout = self.layout_cached(
            text,
            font_handle,
            TextLayoutOptions {
                max_width: Some(max_width),
            },
        )?;

        let mut baseline_y = y + layout.ascender;

        for line in &layout.lines {
            let mut cursor_x = if line.is_rtl {
                x + (max_width - line.width)
            } else {
                x
            };

            for run in &line.runs {
                self.render_shaped_run(cursor_x, baseline_y, run, gpu, ui)?;

                let scale = self.font_manager.scale(run.font_handle)?;
                cursor_x += run.x_adv * scale;
            }

            baseline_y += layout.line_height; 
        }

        Ok(())

    }
}
