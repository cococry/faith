use std::{collections::HashMap, num::NonZeroUsize};

use anyhow::Ok;
use lru::LruCache;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    graphics::{
        FontHandle, FontManager, GraphicsDevice,
        font::{Glyph, GlyphKey, ShapedGlyph},
    },
    ui::{UIRenderer, renderer::QuadType},
};

use unicode_bidi::{BidiInfo, Level};
use unicode_script::{Script, UnicodeScript};

use std::sync::Arc;

/// Used as the key to get all cached
/// shaped glyphs (ShapedGlyph) generated
/// for a given font/string combination
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ShapedTextKey {
    font_handle: FontHandle,
    text: String,
    bidi_lvl: u8,
    script: Script,
}

/// A sub-region of a text string that
/// has been successfully shaped by the
/// font in `font_handle`.
/// Stores shaping & unicode/bidi information
/// for a sub-region of text.
/// `font_handle` is the (fallback) font that
/// supports all grapheme clusters within the
/// `text` string.
#[derive(Debug)]
struct ShapedTextRun {
    font_handle: FontHandle,
    text: String,

    shaped_glyphs: Arc<[ShapedGlyph]>,

    cluster_groups: Arc<[ClusterGroup]>,

    byte_start: usize,
    bidi_lvl: Level,

    x_adv: f32,
    #[allow(dead_code)]
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
/// text runs (ShapedTextRun) for a given
/// font/string combination
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ShapedRunKey {
    original_font: FontHandle,
    text: String,
}

/// Used as the key to get cached font fallback
/// choices (FontHandle) for a given rendered string
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
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [u32; 2],
    pub bearing: [i32; 2],
}

/// Used as the key to get cached
/// layouting data (TextLayout) for a given
/// combination of font & string +
/// layouting properties.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextLayoutKey {
    font_handle: FontHandle,
    text: String,

    max_width_bits: u32,
}

/// Represents a paragraphs's layout
/// information after layout computation.
/// Used to render layouted text (render_wrapped)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TextLayout {
    lines: Vec<TextLine>,
    paragraph_runs: Arc<[ShapedTextRun]>,
    pub width: f32,
    pub height: f32,
    line_height: f32,
    ascender: f32,
}

pub struct PreservedLayout {
    pub lines: Vec<PreservedLine>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    ascent: f32,
}

pub struct PreservedLine {
    pub source: std::ops::Range<usize>,
    pub display: String,
    tabs: Vec<usize>,
    width: f32,
    runs: Arc<[ShapedTextRun]>,
}

impl PreservedLine {
    pub fn display_offset(&self, source: usize) -> usize {
        let offset = source - self.source.start;
        offset + 3 * self.tabs.partition_point(|&tab| tab < offset)
    }

    pub fn source_offset(&self, mut display: usize) -> usize {
        for &tab in &self.tabs {
            if display <= tab {
                break;
            }

            if display < tab + 4 {
                display = tab + usize::from(display >= tab + 2);
                break;
            }

            display -= 3;
        }

        self.source.start + display
    }
}

impl PreservedLayout {
    pub fn row_at(&self, byte: usize) -> usize {
        self.lines
            .partition_point(|line| line.source.start <= byte)
            .saturating_sub(1)
    }
}

/// A byte range within the original paragraph string
/// representing one word-wrap token.
///
/// Tokens are used only as candidate wrapping units,
/// not as independently shaped text. `width` stores the
/// pixel width of this byte range measured from the
/// already-shaped paragraph runs.
#[derive(Debug, Clone)]
struct TokenRange {
    byte_start: usize,
    byte_end: usize,
    width: f32,
}

/// A byte range within the original paragraph string
/// representing one laid-out visual line.
///
/// This range is produced by the wrapping algorithm
/// from TokenRange boundaries. It is later converted
/// into RunSlice values so the line can render slices
/// of the already-shaped paragraph runs.
#[derive(Debug, Clone)]
struct LineRange {
    byte_start: usize,
    byte_end: usize,
}

/// A group of one or more shaped glyphs that belong
/// to the same HarfBuzz cluster.
///
/// Cluster groups are used to safely map byte ranges
/// in the original paragraph to glyph ranges in a
/// shaped run. This avoids splitting inside a shaped
/// cluster when wrapping or rendering text.
///
/// `byte_start_abs` and `byte_end_abs` are absolute
/// byte offsets within the original paragraph string.
/// `glyph_range` indexes into ShapedTextRun.shaped_glyphs.
/// `x_advance` stores the scaled pixel advance of all
/// glyphs in this cluster group.
#[derive(Debug, Clone)]
struct ClusterGroup {
    byte_start_abs: usize,
    byte_end_abs: usize,
    glyph_range: std::ops::Range<usize>,
    x_advance: f32,
}

fn cluster_caret(text: &str, group: &ClusterGroup, byte: usize, rtl: bool) -> f32 {
    let cluster = &text[group.byte_start_abs..group.byte_end_abs];
    if byte == group.byte_start_abs {
        return if rtl {
            group.x_advance
        } else {
            0.0
        };
    }

    if byte == group.byte_end_abs {
        return if rtl {
            0.0
        } else {
            group.x_advance
        };
    }

    // a ligature can contain multiple editable graphemes
    let count = cluster.graphemes(true).count().max(1);
    let before = cluster
        .grapheme_indices(true)
        .take_while(|(i, _)| group.byte_start_abs + i < byte)
        .count();
    let step = if rtl {
        count - before
    } else {
        before
    };
    group.x_advance * step as f32 / count as f32
}

/// A renderable slice of one ShapedTextRun.
///
/// TextLine does not own newly-shaped runs. Instead,
/// it stores RunSlice values that reference glyph ranges
/// within TextLayout.paragraph_runs. This allows wrapped
/// lines to reuse paragraph-shaped glyphs without
/// reshaping each line independently.
///
/// `run_index` indexes into TextLayout.paragraph_runs.
/// `glyph_range` indexes into that run's shaped_glyphs.
/// `x_advance` stores the scaled pixel advance of this
/// slice.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct RunSlice {
    run_index: usize,
    glyph_range: std::ops::Range<usize>,
    x_advance: f32,
}

/// A single laid-out visual line of text.
///
/// `width` is the total pixel width of all slices.
/// `is_rtl` is true if the line contains at least one
/// right-to-left shaped run and is used for horizontal
/// placement/alignment.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TextLine {
    slices: Vec<RunSlice>,
    width: f32,
    is_rtl: bool,
}

/// Options used when computing wrapped text layout.
#[derive(Debug, Clone, Copy)]
pub struct TextLayoutOptions {
    pub max_width: f32,
}

/// Text rendering engine.
/// Manages glyph- and font caches
/// as well as loaded fallback fonts.
///
/// Uses FontManager to shape texts/glyphs
/// with HarfBuzz and load glyphs/fonts with
/// FreeType.
pub struct TextRenderer {
    scale: f32,
    pub color: crate::graphics::Color,
    font_manager: FontManager,

    /// Provided possible fallback fonts
    /// for glyphs. We do not do dynamic
    /// fallback font discovery such as
    /// DirectWrite or fontconfig.
    fallback_fonts: Vec<FontHandle>,
    fallback_sources: Vec<std::path::PathBuf>,
    preferred_emoji: Option<std::path::PathBuf>,

    /// Cached atlas locations (uv, atlas layer) of glyphs
    glyph_locations: HashMap<GlyphKey, AtlasGlyph>,

    shaped_cache: LruCache<ShapedTextKey, Arc<[ShapedGlyph]>>,
    font_run_cache: LruCache<ShapedRunKey, Arc<[ShapedTextRun]>>,
    token_range_cache: LruCache<ShapedRunKey, Arc<[TokenRange]>>,
    layout_cache: LruCache<TextLayoutKey, Arc<TextLayout>>,
    preserved_cache: LruCache<TextLayoutKey, Arc<PreservedLayout>>,

    fallback_choice_cache: LruCache<FallbackChoiceKey, Option<FontHandle>>,
}

fn emoji_hint(c: char) -> bool {
    matches!(c, '\u{1f000}'..='\u{1faff}' | '\u{fe0f}' | '\u{20e3}')
}

impl TextRenderer {
    pub fn set_scale(&mut self, scale: f32) {
        if self.scale != scale {
            self.scale = scale;
            self.shaped_cache.clear();
            self.font_run_cache.clear();
            self.token_range_cache.clear();
            self.layout_cache.clear();
            self.preserved_cache.clear();
        }
    }

    /// Creates a new text renderer with empty
    /// glyph-, fallback-font-, and text-shaping caches.
    ///
    /// Initializes FreeType internally by
    /// calling FontManager::new()
    pub fn new() -> anyhow::Result<Self> {
        let font_manager = FontManager::new()?;

        Ok(Self {
            color: crate::graphics::Color::rgba(0.0, 0.0, 0.0, 1.0),
            font_manager,
            fallback_fonts: Vec::new(),
            fallback_sources: Vec::new(),
            preferred_emoji: None,

            glyph_locations: HashMap::new(),

            scale: 1.0,
            shaped_cache: LruCache::new(NonZeroUsize::new(8192).unwrap()),
            font_run_cache: LruCache::new(NonZeroUsize::new(2048).unwrap()),
            token_range_cache: LruCache::new(NonZeroUsize::new(2048).unwrap()),
            fallback_choice_cache: LruCache::new(NonZeroUsize::new(4096).unwrap()),
            layout_cache: LruCache::new(NonZeroUsize::new(1024).unwrap()),
            preserved_cache: LruCache::new(NonZeroUsize::new(256).unwrap()),
        })
    }

    fn is_neutral_script(&self, script: Script) -> bool {
        matches!(script, Script::Common | Script::Unknown | Script::Inherited)
    }

    fn char_script(&self, ch: char, crnt_script: Option<Script>) -> Script {
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
        font_handle: FontHandle,
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

                Some(old_script) if script != old_script && !self.is_neutral_script(script) => {
                    if run_off < idx {
                        // If the script changed and is not a neutral
                        // script, insert a new itemized run for the
                        // current sub-section of text
                        runs.push(ItemizedRun {
                            font_handle,
                            text: text[run_off..idx].to_owned(),
                            byte_start: byte_off + run_off,
                            bidi_lvl,
                            script: old_script,
                        });
                    }

                    run_off = idx;
                    crnt_script = Some(script);
                }

                _ => {}
            }
        }

        if run_off < text.len() {
            runs.push(ItemizedRun {
                font_handle,
                text: text[run_off..].to_owned(),
                byte_start: byte_off + run_off,
                bidi_lvl,
                script: crnt_script.unwrap_or(Script::Unknown),
            });
        }

        Ok(runs)
    }

    fn itemize_runs(
        &mut self,
        original_font: FontHandle,
        text: &str,
    ) -> anyhow::Result<Vec<ItemizedRun>> {
        // Bidi info for text string, containing
        // paragraphs which contain visual text
        // runs. This function splits visual text
        // runs by script to itemize the text
        // into runs.
        let emoji = if let Some(path) = &self.preferred_emoji
            && text.chars().any(emoji_hint)
        {
            let size = self.font_manager.requested_size(original_font)?;
            Some(self.font_manager.load_font(path, size, 0)?)
        } else {
            None
        };

        let bidi = BidiInfo::new(text, None);

        let mut runs = Vec::new();

        for paragraph in &bidi.paragraphs {
            let range = paragraph.range.clone();

            let (_, visual_runs) = bidi.visual_runs(paragraph, range.clone());

            for run in visual_runs {
                if run.is_empty() {
                    continue;
                }

                let run_text = &text[run.clone()];
                let lvl = bidi.levels[run.start];

                // run.start is the byte offset into the original string
                let mut split = if let Some(emoji) = emoji {
                    self.split_emoji(run_text, run.start, lvl, original_font, emoji)?
                } else {
                    self.split_by_script(run_text, run.start, lvl, original_font)?
                };

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

    fn split_emoji(
        &mut self,
        text: &str,
        offset: usize,
        level: Level,
        original: FontHandle,
        emoji: FontHandle,
    ) -> anyhow::Result<Vec<ItemizedRun>> {
        let mut runs = Vec::new();
        let mut start = 0;
        for (index, grapheme) in text.grapheme_indices(true) {
            if !grapheme.chars().any(emoji_hint) || grapheme.contains('\u{fe0e}') {
                continue;
            }

            let glyphs = self.shape_cached(grapheme, level, Script::Common, emoji)?;
            if glyphs.is_empty() || glyphs.iter().any(|glyph| glyph.glyph_idx == 0) {
                continue;
            }

            if start < index {
                runs.extend(self.split_by_script(
                    &text[start..index],
                    offset + start,
                    level,
                    original,
                )?);
            }

            runs.push(ItemizedRun {
                font_handle: emoji,
                text: grapheme.to_owned(),
                byte_start: offset + index,
                bidi_lvl: level,
                script: Script::Common,
            });
            start = index + grapheme.len();
        }

        if start < text.len() {
            runs.extend(self.split_by_script(&text[start..], offset + start, level, original)?);
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

    fn merge_ranges(&self, mut ranges: Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
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

        let size = self.font_manager.requested_size(original_font)?;

        for i in 0..self.fallback_fonts.len() + self.fallback_sources.len() {
            let fallback = if i < self.fallback_fonts.len() {
                self.font_manager.at_size(self.fallback_fonts[i], size)?
            } else {
                self.font_manager.load_font(
                    &self.fallback_sources[i - self.fallback_fonts.len()],
                    size,
                    0,
                )?
            };

            if fallback == original_font
                || !self
                    .font_manager
                    .supports_text(fallback, text, bidi_lvl, script)?
            {
                continue;
            }

            let glyphs = self.shape_cached(text, bidi_lvl, script, fallback)?;

            if glyphs.iter().all(|g| g.glyph_idx != 0) {
                self.fallback_choice_cache.push(key, Some(fallback));
                return Ok(Some(fallback));
            }
        }

        self.fallback_choice_cache.push(key, None);
        Ok(None)
    }

    fn reshape_text_run_with_font(
        &mut self,
        text: &str,
        run: &ItemizedRun,
        font: FontHandle,
        run_byte_start: usize,
    ) -> anyhow::Result<ShapedTextRun> {
        let reshaped_glyphs = self.shape_cached(text, run.bidi_lvl, run.script, font)?;

        // x and y advance of all glyphs in the
        // run summed up to get run advances.
        let (x_adv, y_adv) = reshaped_glyphs
            .iter()
            .fold((0.0, 0.0), |(x, y), g| (x + g.x_adv, y + g.y_adv));

        let font_scale = self.font_manager.scale(font)?;

        let new_cluster_groups_vec = self.build_cluster_groups_from_glyphs(
            run_byte_start,
            text.len(),
            &reshaped_glyphs,
            font_scale,
        );

        let new_cluster_groups: Arc<[ClusterGroup]> = Arc::from(new_cluster_groups_vec);

        Ok(ShapedTextRun {
            font_handle: font,
            text: text.to_owned(),
            shaped_glyphs: reshaped_glyphs,
            byte_start: run_byte_start,
            bidi_lvl: run.bidi_lvl,
            x_adv,
            y_adv,
            cluster_groups: new_cluster_groups,
        })
    }

    fn repair_missing_glyphs_in_run(
        &mut self,
        run: ItemizedRun,
        glyphs: Arc<[ShapedGlyph]>,
    ) -> anyhow::Result<Vec<ShapedTextRun>> {
        let mut repaired = Vec::new();

        // acquire missing grapheme ranges within the run
        let mut missing_ranges = Vec::new();

        for glyph in glyphs.iter() {
            if glyph.glyph_idx == 0 {
                // Get the grapheme range for this
                // cluster. This works because glyph.cluster
                // is a byte offset within the run's text.
                let range = self.cluster_to_grapheme_range(&run.text, glyph.cluster);

                // if the cluster has a valid grapheme range
                // but has a .notdef/0 glyph index in the
                // shaped glyphs, the glyph and it's
                // grapheme range is missing.
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
        // For example, HarfBuzz needs to be able
        // to see whole arabic words at once
        // to generate correct glyphs. Thus
        // we merge adjacent fallback ranges, which
        // are then shaped at once.
        let missing_ranges = self.merge_ranges(missing_ranges);

        let mut cursor = 0;

        for missing in missing_ranges {
            // Not in a missing range -> shape with
            // run's font for that section.
            if cursor < missing.start {
                let non_missing_text = run.text[cursor..missing.start].to_owned();

                // we reshape the non-missing text with
                // the run's font. NOTE: We could potentially
                // try to get the non-missing glyph range
                // from the alrady shaped glyphs.
                repaired.push(self.reshape_text_run_with_font(
                    &non_missing_text,
                    &run,
                    run.font_handle,
                    run.byte_start + cursor,
                )?);
            }

            let missing_text = run.text[missing.clone()].to_owned();

            // Check loaded fallback fonts
            // wheter one of them supports the
            // missing text range.
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
                repaired.push(self.reshape_text_run_with_font(
                    &missing_text,
                    &run,
                    fallback,
                    run.byte_start + missing.start,
                )?);
            } else {
                // If no fallback font supports this range, keep the
                // original shaped result so the missing glyphs still
                // contribute advance width.
                repaired.push(self.reshape_text_run_with_font(
                    &missing_text,
                    &run,
                    run.font_handle,
                    run.byte_start + missing.start,
                )?);
            }

            // Update cursor to range end
            cursor = missing.end;
        }

        // Push the last range if there is still one
        if cursor < run.text.len() {
            let text = run.text[cursor..].to_owned();
            repaired.push(self.reshape_text_run_with_font(
                &text,
                &run,
                run.font_handle,
                run.byte_start + cursor,
            )?);
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
    ) -> anyhow::Result<Arc<[ShapedGlyph]>> {
        let key = ShapedTextKey {
            font_handle,
            text: text.to_owned(),
            bidi_lvl: bidi_lvl.number(),
            script,
        };

        if let Some(glyphs) = self.shaped_cache.get(&key) {
            return Ok(Arc::clone(glyphs));
        }

        // Shape at the same rounded device size used by FreeType, then express
        // positions in logical units for wrapping, carets, and run placement.
        let size = self.font_manager.requested_size(font_handle)?;
        let raster = self.font_manager.at_size(
            font_handle,
            (size as f32 * self.scale).round().max(1.0) as u32,
        )?;
        let factor = self.font_manager.scale(raster)?
            / (self.scale * self.font_manager.scale(font_handle)?);
        let mut glyphs_vec = self
            .font_manager
            .shape_text_with_props(text, bidi_lvl, script, raster)?;
        for glyph in &mut glyphs_vec {
            glyph.x_adv *= factor;
            glyph.y_adv *= factor;
            glyph.x_off *= factor;
            glyph.y_off *= factor;
        }

        let glyphs: Arc<[ShapedGlyph]> = Arc::from(glyphs_vec);

        self.shaped_cache.put(key, Arc::clone(&glyphs));

        Ok(glyphs)
    }

    fn shape_run(&mut self, run: ItemizedRun) -> anyhow::Result<Vec<ShapedTextRun>> {
        let glyphs = self.shape_cached(&run.text, run.bidi_lvl, run.script, run.font_handle)?;

        let font_scale = self.font_manager.scale(run.font_handle)?;

        let cluster_groups_vec = self.build_cluster_groups_from_glyphs(
            run.byte_start,
            run.text.len(),
            &glyphs,
            font_scale,
        );

        let cluster_groups: Arc<[ClusterGroup]> = Arc::from(cluster_groups_vec);

        let mut all_glyphs_support = true;
        let mut x_adv = 0.0;
        let mut y_adv = 0.0;

        for glyph in glyphs.iter() {
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
                x_adv,
                y_adv,
                shaped_glyphs: glyphs,
                cluster_groups,
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
    ) -> anyhow::Result<Arc<[ShapedTextRun]>> {
        let key = ShapedRunKey {
            original_font,
            text: text.to_owned(),
        };

        if let Some(runs) = self.font_run_cache.get(&key) {
            return Ok(Arc::clone(runs));
        }

        let itemized = self.itemize_runs(original_font, text)?;

        let mut shaped_runs = Vec::new();

        for run in itemized {
            shaped_runs.extend(self.shape_run(run)?);
        }

        let shaped_runs: Arc<[ShapedTextRun]> = Arc::from(shaped_runs);

        self.font_run_cache.put(key, Arc::clone(&shaped_runs));

        Ok(shaped_runs)
    }

    fn upload_glyph<G: GraphicsDevice>(
        &mut self,
        key: GlyphKey,
        glyph: Glyph,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        if glyph.width == 0 || glyph.height == 0 {
            self.glyph_locations.insert(
                key,
                AtlasGlyph {
                    atlas_layer: 0,
                    uv_min: [0.0; 2],
                    uv_max: [0.0; 2],
                    size: [0; 2],
                    bearing: [glyph.bearing_x, glyph.bearing_y],
                },
            );
            return Ok(());
        }

        let width = glyph
            .width
            .checked_add(2)
            .ok_or_else(|| anyhow::anyhow!("glyph width overflow"))?;
        let height = glyph
            .height
            .checked_add(2)
            .ok_or_else(|| anyhow::anyhow!("glyph height overflow"))?;
        let (layer, x, y, atlas_w, atlas_h) = ui.allocate_image_rect(width, height, 0)?;
        let pixels = bordered_glyph(&glyph)?;
        ui.upload_pixels_to_atlas([x, y, width, height], layer, &pixels, gpu)?;
        self.glyph_locations.insert(
            key,
            AtlasGlyph {
                size: [glyph.width, glyph.height],
                atlas_layer: layer,
                bearing: [glyph.bearing_x, glyph.bearing_y],
                uv_min: [
                    (x + 1) as f32 / atlas_w as f32,
                    (y + 1) as f32 / atlas_h as f32,
                ],
                uv_max: [
                    (x + width - 1) as f32 / atlas_w as f32,
                    (y + height - 1) as f32 / atlas_h as f32,
                ],
            },
        );
        Ok(())
    }

    pub fn set_fallback_fonts(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        preferred_emoji: Option<std::path::PathBuf>,
    ) {
        self.fallback_sources = paths;
        self.preferred_emoji = preferred_emoji;
        self.fallback_choice_cache.clear();
        self.font_run_cache.clear();
        self.token_range_cache.clear();
        self.layout_cache.clear();
        self.preserved_cache.clear();
    }

    pub fn load_primary_font(
        &mut self,
        path: &std::path::Path,
        size: u32,
        index: u32,
    ) -> anyhow::Result<FontHandle> {
        self.font_manager
            .load_font(path, size, isize::try_from(index)?)
    }

    pub fn validate_primary_font(
        &mut self,
        face: &crate::system_fonts::Face,
    ) -> anyhow::Result<()> {
        let handle = self.load_primary_font(&face.path, 16, face.index)?;
        self.font_manager.validate_ui_font(handle)
    }

    /// Registers a lazy fallback face.
    #[allow(dead_code)]
    pub fn load_font(
        &mut self,
        path: impl AsRef<std::path::Path>,
        size: u32,
    ) -> anyhow::Result<FontHandle> {
        let font = self.font_manager.load_font(path, size, 0)?;

        if !self.fallback_fonts.contains(&font) {
            self.fallback_fonts.push(font);
            self.fallback_choice_cache.clear();
            self.layout_cache.clear();
            self.preserved_cache.clear();
            self.font_run_cache.clear();
            self.token_range_cache.clear();
        }

        Ok(font)
    }

    #[allow(dead_code)]
    fn render_run_slice<G: GraphicsDevice>(
        &mut self,
        run_x: f32,
        baseline_y: f32,
        run: &ShapedTextRun,
        slice: &RunSlice,
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
        let size = self.font_manager.requested_size(run_font)?;
        let raster_size = (size as f32 * self.scale).round().max(1.0) as u32;
        let raster_font = self.font_manager.at_size(run_font, raster_size)?;
        // The bitmap is already rasterized at the target device size. Never
        // stretch it back to the unrounded requested size.
        let margin = self.font_manager.line_height(run_font)? as f32 * font_scale * 2.0;
        let clip = ui.clip();

        let color = if is_colored {
            crate::graphics::Color::rgba(1.0, 1.0, 1.0, 1.0)
        } else {
            self.color
        };

        let mut pen_x = run_x;

        for glyph_idx in slice.glyph_range.clone() {
            let shaped = &run.shaped_glyphs[glyph_idx];
            let glyph_x = pen_x + shaped.x_off * font_scale;

            if glyph_x + shaped.x_adv.abs() * font_scale + margin < clip[0]
                || glyph_x - margin > clip[2]
            {
                pen_x += shaped.x_adv * font_scale;
                continue;
            }

            let key = GlyphKey {
                font_handle: raster_font,
                glyph_idx: shaped.glyph_idx,
            };

            if !self.glyph_locations.contains_key(&key) {
                let glyph = {
                    let glyph_ref = self
                        .font_manager
                        .get_or_load_glyph(raster_font, shaped.glyph_idx)?;
                    glyph_ref.clone()
                };

                self.upload_glyph(key, glyph, gpu, ui)?;
            }

            let atlas_glyph = &self.glyph_locations[&key];

            if atlas_glyph.size[0] != 0 && atlas_glyph.size[1] != 0 {
                let rect = glyph_rect(
                    [pen_x + shaped.x_off * font_scale, baseline_y],
                    shaped.y_off * font_scale,
                    atlas_glyph.bearing,
                    atlas_glyph.size,
                    self.scale,
                );

                ui.raw_quad_atlas(
                    rect,
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

    #[allow(dead_code)]
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
        let size = self.font_manager.requested_size(run_font)?;
        let raster_size = (size as f32 * self.scale).round().max(1.0) as u32;
        let raster_font = self.font_manager.at_size(run_font, raster_size)?;
        // The bitmap is already rasterized at the target device size. Never
        // stretch it back to the unrounded requested size.
        let margin = self.font_manager.line_height(run_font)? as f32 * font_scale * 2.0;
        let clip = ui.clip();

        let color = if is_colored {
            crate::graphics::Color::rgba(1.0, 1.0, 1.0, 1.0)
        } else {
            self.color
        };

        let mut pen_x = run_x;

        for shaped in run.shaped_glyphs.iter() {
            let glyph_x = pen_x + shaped.x_off * font_scale;

            if glyph_x + shaped.x_adv.abs() * font_scale + margin < clip[0]
                || glyph_x - margin > clip[2]
            {
                pen_x += shaped.x_adv * font_scale;
                continue;
            }

            let key = GlyphKey {
                font_handle: raster_font,
                glyph_idx: shaped.glyph_idx,
            };

            if !self.glyph_locations.contains_key(&key) {
                let glyph = {
                    let glyph_ref = self
                        .font_manager
                        .get_or_load_glyph(raster_font, shaped.glyph_idx)?;
                    glyph_ref.clone()
                };

                self.upload_glyph(key, glyph, gpu, ui)?;
            }

            let atlas_glyph = &self.glyph_locations[&key];

            if atlas_glyph.size[0] != 0 && atlas_glyph.size[1] != 0 {
                let rect = glyph_rect(
                    [pen_x + shaped.x_off * font_scale, baseline_y],
                    shaped.y_off * font_scale,
                    atlas_glyph.bearing,
                    atlas_glyph.size,
                    self.scale,
                );

                ui.raw_quad_atlas(
                    rect,
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
    #[allow(dead_code)]
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
        let layout = self.measure(text, font_handle)?;
        let mut baseline_y = y + layout.ascender;
        let line_height = layout.line_height;

        for line in text.split('\n') {
            let clip = ui.clip();

            if baseline_y + line_height < clip[1] || baseline_y - line_height > clip[3] {
                baseline_y += line_height;
                continue;
            }

            let runs = self.build_shaped_runs_cached(font_handle, line)?;

            let mut cursor_x = x;

            for run in runs.iter() {
                self.render_shaped_run(cursor_x, baseline_y, run, gpu, ui)?;

                let font_scale = self.font_manager.scale(run.font_handle)?;
                cursor_x += run.x_adv * font_scale;
            }

            baseline_y += line_height;
        }

        Ok(())
    }

    /// Visual caret coordinate in the same unwrapped shaping used by render().
    /// Unlike paragraph measurement, this preserves leading/trailing spaces and
    /// never reshapes a prefix independently of its following characters.
    pub fn caret_x(&mut self, text: &str, font: FontHandle, byte: usize) -> anyhow::Result<f32> {
        anyhow::ensure!(
            byte <= text.len() && text.is_char_boundary(byte),
            "invalid caret byte offset"
        );
        let runs = self.build_shaped_runs_cached(font, text)?;
        let mut x = 0.0;

        for run in runs.iter() {
            for group in run.cluster_groups.iter() {
                if byte >= group.byte_start_abs && byte <= group.byte_end_abs {
                    return Ok(x + cluster_caret(text, group, byte, run.bidi_lvl.is_rtl()));
                }

                x += group.x_advance;
            }
        }

        Ok(x)
    }

    pub fn selection_spans(
        &mut self,
        text: &str,
        font: FontHandle,
        selected: std::ops::Range<usize>,
    ) -> anyhow::Result<Vec<[f32; 2]>> {
        let runs = self.build_shaped_runs_cached(font, text)?;
        let mut spans = Vec::<[f32; 2]>::new();
        let mut x = 0.0;

        for run in runs.iter() {
            for group in run.cluster_groups.iter() {
                let start = selected.start.max(group.byte_start_abs);
                let end = selected.end.min(group.byte_end_abs);
                if start < end {
                    let rtl = run.bidi_lvl.is_rtl();
                    let a = x + cluster_caret(text, group, start, rtl);
                    let b = x + cluster_caret(text, group, end, rtl);
                    let left = a.min(b);
                    let right = a.max(b);
                    if let Some(span) = spans.last_mut().filter(|span| span[1] >= left) {
                        span[1] = span[1].max(right);
                    } else {
                        spans.push([left, right]);
                    }
                }

                x += group.x_advance;
            }
        }

        Ok(spans)
    }

    pub fn caret_at_x(
        &mut self,
        text: &str,
        font: FontHandle,
        target: f32,
        nearest: bool,
    ) -> anyhow::Result<usize> {
        let runs = self.build_shaped_runs_cached(font, text)?;
        let mut x = 0.0;
        let mut end = 0;
        for run in runs.iter() {
            let rtl = run.bidi_lvl.is_rtl();
            for group in run.cluster_groups.iter() {
                end = if rtl {
                    group.byte_start_abs
                } else {
                    group.byte_end_abs
                };
                if group.x_advance > 0.0 && target < x + group.x_advance {
                    let cluster = &text[group.byte_start_abs..group.byte_end_abs];
                    let count = cluster.graphemes(true).count();
                    let fraction = ((target - x) / group.x_advance).clamp(0.0, 1.0);
                    let visual = fraction * count as f32;
                    let index = if nearest {
                        visual.round() as usize
                    } else {
                        (visual as usize).min(count.saturating_sub(1))
                    };
                    let logical = if rtl {
                        count.saturating_sub(index + usize::from(!nearest))
                    } else {
                        index
                    };
                    let offset = cluster
                        .grapheme_indices(true)
                        .nth(logical)
                        .map_or(cluster.len(), |(offset, _)| offset);
                    return Ok(group.byte_start_abs + offset);
                }

                x += group.x_advance;
            }
        }

        Ok(end)
    }

    fn measure_byte_range_from_runs(
        &mut self,
        paragraph_runs: &[ShapedTextRun],
        byte_range: std::ops::Range<usize>,
    ) -> anyhow::Result<f32> {
        let mut width = 0.0;

        for (run_index, run) in paragraph_runs.iter().enumerate() {
            let run_start = run.byte_start;
            let run_end = run.byte_start + run.text.len();

            let slice_start = byte_range.start.max(run_start);
            let slice_end = byte_range.end.min(run_end);

            if slice_start >= slice_end {
                continue;
            }

            let slices = self.run_slices_for_byte_range(run_index, run, slice_start..slice_end)?;

            for slice in slices {
                width += slice.x_advance;
            }
        }

        Ok(width)
    }

    fn word_wrap_token_ranges(&mut self, paragraph: &str) -> anyhow::Result<Vec<TokenRange>> {
        let mut tokens: Vec<TokenRange> = Vec::new();

        let mut cursor = 0;

        for token in paragraph.split_word_bounds() {
            let byte_start = cursor;
            let byte_end = cursor + token.len();
            cursor = byte_end;

            if self.is_leading_forbidden_token(token)
                && let Some(prev) = tokens.last_mut()
            {
                prev.byte_end = byte_end;
                continue;
            }

            tokens.push(TokenRange {
                byte_start,
                byte_end,
                width: 0.0,
            });
        }

        Ok(tokens)
    }

    fn trim_line_start_whitespace(
        &self,
        text: &str,
        mut byte_start: usize,
        byte_end: usize,
    ) -> usize {
        while byte_start < byte_end {
            let Some(ch) = text[byte_start..byte_end].chars().next() else {
                break;
            };

            if !ch.is_whitespace() {
                break;
            }

            byte_start += ch.len_utf8();
        }

        byte_start
    }

    fn build_line_ranges(
        &mut self,
        paragraph: &str,
        paragraph_runs: &[ShapedTextRun],
        token_ranges: &[TokenRange],
        max_width: f32,
    ) -> anyhow::Result<Vec<LineRange>> {
        let mut crnt_width = 0.0;
        let mut crnt_start: Option<usize> = None;
        let mut crnt_end: usize = 0;

        let mut lines = Vec::new();

        for range in token_ranges {
            if matches!(
                &paragraph[range.byte_start..range.byte_end],
                "\n" | "\r\n" | "\r"
            ) {
                lines.push(LineRange {
                    byte_start: crnt_start.unwrap_or(range.byte_start),
                    byte_end: range.byte_start,
                });
                crnt_start = None;
                crnt_end = range.byte_end;
                crnt_width = 0.0;
                continue;
            }
            if crnt_start.is_none() {
                // Trim whitespace
                crnt_start = Some(self.trim_line_start_whitespace(
                    paragraph,
                    range.byte_start,
                    range.byte_end,
                ));

                crnt_end = range.byte_end;
                crnt_width = if let Some(start) = crnt_start {
                    if start < crnt_end {
                        self.measure_byte_range_from_runs(paragraph_runs, start..crnt_end)?
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                continue;
            }

            let would_wrap = crnt_width != 0.0 && crnt_width + range.width >= max_width;

            if would_wrap {
                lines.push(LineRange {
                    byte_start: crnt_start.unwrap(),
                    byte_end: crnt_end,
                });

                crnt_start = Some(self.trim_line_start_whitespace(
                    paragraph,
                    range.byte_start,
                    range.byte_end,
                ));

                crnt_end = range.byte_end;
                crnt_width = if let Some(start) = crnt_start {
                    if start < crnt_end {
                        self.measure_byte_range_from_runs(paragraph_runs, start..crnt_end)?
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
            } else {
                crnt_width += range.width;
                crnt_end = range.byte_end;
            }
        }

        if let Some(byte_start) = crnt_start
            && byte_start < crnt_end
        {
            lines.push(LineRange {
                byte_start,
                byte_end: crnt_end,
            });
        }
        if paragraph.ends_with(['\n', '\r']) {
            lines.push(LineRange {
                byte_start: paragraph.len(),
                byte_end: paragraph.len(),
            });
        }

        Ok(lines)
    }

    fn build_cluster_groups_from_glyphs(
        &self,
        run_byte_start: usize,
        run_text_len: usize,
        glyphs: &[ShapedGlyph],
        font_scale: f32,
    ) -> Vec<ClusterGroup> {
        let mut groups = Vec::new();

        let mut i = 0;

        while i < glyphs.len() {
            let cluster = glyphs[i].cluster as usize;
            let glyph_start = i;
            let mut glyph_end = i + 1;

            while glyph_end < glyphs.len() && glyphs[glyph_end].cluster as usize == cluster {
                glyph_end += 1;
            }

            let x_advance = glyphs[glyph_start..glyph_end]
                .iter()
                .map(|g| g.x_adv * font_scale)
                .sum();

            groups.push(ClusterGroup {
                byte_start_abs: run_byte_start + cluster,
                byte_end_abs: 0,
                glyph_range: glyph_start..glyph_end,
                x_advance,
            });

            i = glyph_end;
        }

        // HarfBuzz glyph order may not be byte-monotonic,
        // especially for RTL text. So compute byte ends
        // from sorted logical cluster starts.
        let mut starts: Vec<usize> = groups.iter().map(|g| g.byte_start_abs).collect();

        starts.sort_unstable();
        starts.dedup();

        let run_byte_end = run_byte_start + run_text_len;

        for group in &mut groups {
            let pos = starts
                .binary_search(&group.byte_start_abs)
                .expect("cluster start should exist in starts");

            // Get range ends from starts
            group.byte_end_abs = starts.get(pos + 1).copied().unwrap_or(run_byte_end);
        }

        groups
    }

    fn run_slices_for_byte_range(
        &mut self,
        run_index: usize,
        run: &ShapedTextRun,
        byte_range_abs: std::ops::Range<usize>,
    ) -> anyhow::Result<Vec<RunSlice>> {
        let groups = &run.cluster_groups;

        let mut slices = Vec::new();
        let mut current: Option<RunSlice> = None;

        for group in groups.iter() {
            let intersects = group.byte_end_abs > byte_range_abs.start
                && group.byte_start_abs < byte_range_abs.end;

            if !intersects {
                if let Some(slice) = current.take() {
                    slices.push(slice);
                }

                continue;
            }

            match &mut current {
                Some(slice) if slice.glyph_range.end == group.glyph_range.start => {
                    slice.glyph_range.end = group.glyph_range.end;
                    slice.x_advance += group.x_advance;
                }

                Some(_) => {
                    slices.push(current.take().unwrap());

                    current = Some(RunSlice {
                        run_index,
                        glyph_range: group.glyph_range.clone(),
                        x_advance: group.x_advance,
                    });
                }

                None => {
                    current = Some(RunSlice {
                        run_index,
                        glyph_range: group.glyph_range.clone(),
                        x_advance: group.x_advance,
                    });
                }
            }
        }

        if let Some(slice) = current {
            slices.push(slice);
        }

        Ok(slices)
    }

    fn build_run_slices_for_line(
        &mut self,
        paragraph_runs: &[ShapedTextRun],
        line: LineRange,
    ) -> anyhow::Result<TextLine> {
        let mut slices = Vec::new();
        let mut width = 0.0;
        let mut is_rtl = false;

        for (idx, run) in paragraph_runs.iter().enumerate() {
            let run_start = run.byte_start;
            let run_end = run.byte_start + run.text.len();

            let slice_start = line.byte_start.max(run_start);
            let slice_end = line.byte_end.min(run_end);

            if slice_start >= slice_end {
                continue;
            }

            let run_slices = self.run_slices_for_byte_range(idx, run, slice_start..slice_end)?;

            if run.bidi_lvl.is_rtl() {
                is_rtl = true;
            }

            for slice in run_slices {
                width += slice.x_advance;
                slices.push(slice);
            }
        }

        Ok(TextLine {
            slices,
            width,
            is_rtl,
        })
    }

    fn token_ranges_cached(
        &mut self,
        font_handle: FontHandle,
        text: &str,
        paragraph_runs: &[ShapedTextRun],
    ) -> anyhow::Result<Arc<[TokenRange]>> {
        let key = ShapedRunKey {
            original_font: font_handle,
            text: text.to_owned(),
        };

        if let Some(tokens) = self.token_range_cache.get(&key) {
            return Ok(Arc::clone(tokens));
        }

        let mut tokens = self.word_wrap_token_ranges(text)?;

        for token in &mut tokens {
            token.width = self
                .measure_byte_range_from_runs(paragraph_runs, token.byte_start..token.byte_end)?;
        }

        let tokens: Arc<[TokenRange]> = Arc::from(tokens);

        self.token_range_cache.put(key, Arc::clone(&tokens));

        Ok(tokens)
    }

    fn line_metrics(
        &mut self,
        base: FontHandle,
        runs: &[ShapedTextRun],
    ) -> anyhow::Result<(f32, f32)> {
        let size = self.font_manager.requested_size(base)?;
        let raster = self
            .font_manager
            .at_size(base, (size as f32 * self.scale).round().max(1.0) as u32)?;
        let mut ascent = self.font_manager.ascender(raster)? as f32;
        let mut descent = self.font_manager.descender(raster)? as f32;
        let line_height = self.font_manager.line_height(raster)? as f32;
        for run in runs {
            let size = self.font_manager.requested_size(run.font_handle)?;
            let raster = self.font_manager.at_size(
                run.font_handle,
                (size as f32 * self.scale).round().max(1.0) as u32,
            )?;
            let offset_scale = self.font_manager.scale(run.font_handle)? * self.scale;
            for shaped in run.shaped_glyphs.iter() {
                let glyph = self
                    .font_manager
                    .get_or_load_glyph(raster, shaped.glyph_idx)?;
                let top = glyph.bearing_y as f32 + (shaped.y_off * offset_scale).round();
                ascent = ascent.max(top);
                descent = descent.max(glyph.height as f32 - top);
            }
        }
        // A device pixel of leading on each side absorbs baseline snapping
        // and fractional widget/clip origins without cutting off edge rows.
        Ok((
            (ascent + 1.0) / self.scale,
            line_height.max(ascent + descent + 2.0) / self.scale,
        ))
    }

    fn layout_cached(
        &mut self,
        text: &str,
        font_handle: FontHandle,
        options: TextLayoutOptions,
    ) -> anyhow::Result<Arc<TextLayout>> {
        let key = TextLayoutKey {
            font_handle,
            text: text.to_owned(),
            max_width_bits: options.max_width.to_bits(),
        };

        if let Some(layout) = self.layout_cache.get(&key) {
            return Ok(layout.clone());
        }

        // First shape the entire paragraph once
        // (cached per paragraph)
        let paragraph_runs = self.build_shaped_runs_cached(font_handle, text)?;
        let (ascender, line_height) = self.line_metrics(font_handle, &paragraph_runs)?;
        // Second, firgure out the byte ranges of the
        // tokens in the paragraph string and get their
        // displayed pixel size by mapping the
        // string byte ranges back to the shaped glyphs
        // of the entire paragraph. (cached per paragraph)
        let token_ranges = self.token_ranges_cached(font_handle, text, &paragraph_runs)?;

        // Build the string byte ranges of where line breaks
        // are happening (not cached)
        let line_ranges =
            self.build_line_ranges(text, &paragraph_runs, &token_ranges, options.max_width)?;

        // Build run slices for the line byte ranges.
        // This is done to map string token byte ranges
        // back to the slices of glyphs they are affecting
        // in their run of the line.
        let lines = line_ranges
            .into_iter()
            .map(|line| self.build_run_slices_for_line(&paragraph_runs, line))
            .collect::<anyhow::Result<Vec<TextLine>>>()?;

        // Maximum with of all lines is
        // width of the text layout
        let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
        let height = lines.len() as f32 * line_height;

        let layout = Arc::new(TextLayout {
            lines,
            paragraph_runs,
            width,
            height,
            line_height,
            ascender,
        });

        self.layout_cache.put(key, Arc::clone(&layout));

        Ok(layout)
    }

    pub fn preserved_layout(
        &mut self,
        value: &str,
        font: FontHandle,
        width: f32,
    ) -> anyhow::Result<Arc<PreservedLayout>> {
        let key = TextLayoutKey {
            font_handle: font,
            text: value.to_owned(),
            max_width_bits: width.to_bits(),
        };
        if let Some(layout) = self.preserved_cache.get(&key) {
            return Ok(Arc::clone(layout));
        }

        let mut lines = Vec::new();
        let (mut ascent, base_height) = self.line_metrics(font, &[])?;
        let mut below_baseline = base_height - ascent;
        let mut start = 0;
        let breaks = value
            .match_indices(['\r', '\n'])
            .chain(std::iter::once((value.len(), "")));
        for (end, newline) in breaks {
            if end < start {
                continue;
            }

            let paragraph = &value[start..end];
            let display = paragraph.replace('\t', "    ");
            let runs = self.build_shaped_runs_cached(font, &display)?;
            let (row_ascent, row_height) = self.line_metrics(font, &runs)?;
            ascent = ascent.max(row_ascent);
            below_baseline = below_baseline.max(row_height - row_ascent);

            let mut advances = vec![0.0; display.len() + 1];
            for run in runs.iter() {
                for group in run.cluster_groups.iter() {
                    let cluster = &display[group.byte_start_abs..group.byte_end_abs];
                    let step = group.x_advance / cluster.graphemes(true).count().max(1) as f32;
                    for (offset, _) in cluster.grapheme_indices(true) {
                        advances[group.byte_start_abs + offset] += step;
                    }
                }
            }

            let mut stops = vec![(0, 0.0)];
            let mut offset = 0;
            let mut x = 0.0;
            for (byte, grapheme) in paragraph.grapheme_indices(true) {
                let len = if grapheme == "\t" {
                    4
                } else {
                    grapheme.len()
                };
                x += advances[offset..offset + len].iter().sum::<f32>();
                offset += len;
                stops.push((byte + grapheme.len(), x));
            }

            let breaks: Vec<_> = paragraph
                .split_word_bound_indices()
                .map(|(byte, _)| byte)
                .collect();
            let mut first = 0;
            loop {
                let mut last = first;
                let mut word_break = first;
                while last + 1 < stops.len() {
                    if last > first && stops[last + 1].1 - stops[first].1 > width {
                        if word_break > first {
                            last = word_break;
                        }

                        break;
                    }

                    last += 1;
                    if breaks.binary_search(&stops[last].0).is_ok() {
                        word_break = last;
                    }
                }

                loop {
                    let range = stops[first].0..stops[last].0;
                    let source = &paragraph[range.clone()];
                    let display = source.replace('\t', "    ");
                    let runs = self.build_shaped_runs_cached(font, &display)?;
                    let advance = runs
                        .iter()
                        .flat_map(|run| run.cluster_groups.iter())
                        .map(|group| group.x_advance)
                        .sum::<f32>();

                    // shaping the line on its own can change kerning at the wrap
                    if advance > width && last > first + 1 {
                        last -= 1;
                        continue;
                    }

                    lines.push(PreservedLine {
                        source: start + range.start..start + range.end,
                        tabs: source.match_indices('\t').map(|(byte, _)| byte).collect(),
                        width: advance,
                        display,
                        runs,
                    });
                    break;
                }

                if last + 1 == stops.len() {
                    break;
                }

                first = last;
            }

            start = end + newline.len();
            if newline == "\r" && value.as_bytes().get(start) == Some(&b'\n') {
                start += 1;
            }
        }

        let line_height = ascent + below_baseline;
        let layout = Arc::new(PreservedLayout {
            width: lines.iter().map(|line| line.width).fold(0.0, f32::max),
            height: lines.len() as f32 * line_height,
            lines,
            line_height,
            ascent,
        });
        self.preserved_cache.put(key, Arc::clone(&layout));
        Ok(layout)
    }

    pub fn preserved_caret_at(
        &mut self,
        layout: &PreservedLayout,
        font: FontHandle,
        [x, y]: [f32; 2],
        nearest: bool,
    ) -> anyhow::Result<(usize, usize)> {
        let row = ((y / layout.line_height).floor().max(0.0) as usize).min(layout.lines.len() - 1);
        let line = &layout.lines[row];
        let offset = self.caret_at_x(&line.display, font, x, nearest)?;
        Ok((line.source_offset(offset), row))
    }

    pub fn render_preserved_selection(
        &mut self,
        [x, top]: [f32; 2],
        layout: &PreservedLayout,
        font: FontHandle,
        selection: std::ops::Range<usize>,
        color: crate::graphics::Color,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let clip = ui.clip();
        let first = ((clip[1] - top) / layout.line_height).floor().max(0.0) as usize;
        let last = ((clip[3] - top) / layout.line_height).ceil().max(0.0) as usize;
        for (index, line) in layout.lines.iter().enumerate().take(last).skip(first) {
            let start = selection.start.max(line.source.start);
            let end = selection.end.min(line.source.end);
            let y = top + index as f32 * layout.line_height;
            if start < end {
                let range = line.display_offset(start)..line.display_offset(end);
                for [left, right] in self.selection_spans(&line.display, font, range)? {
                    ui.quad([x + left, y, right - left, layout.line_height], color)?;
                }
            }

            if selection.start <= line.source.end
                && selection.end > line.source.end
                && layout
                    .lines
                    .get(index + 1)
                    .is_some_and(|next| next.source.start > line.source.end)
            {
                let eol = self.caret_x(&line.display, font, line.display.len())?;
                ui.quad([x + eol, y, 3.0, layout.line_height], color)?;
            }
        }

        Ok(())
    }

    pub fn render_preserved<G: GraphicsDevice>(
        &mut self,
        origin: [f32; 2],
        layout: &PreservedLayout,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let clip = ui.clip();
        for (index, line) in layout.lines.iter().enumerate() {
            let top = origin[1] + index as f32 * layout.line_height;
            if top + layout.line_height <= clip[1] {
                continue;
            }

            if top >= clip[3] {
                break;
            }

            let mut x = origin[0];
            for run in line.runs.iter() {
                self.render_shaped_run(x, top + layout.ascent, run, gpu, ui)?;
                x += run.x_adv * self.font_manager.scale(run.font_handle)?;
            }
        }

        Ok(())
    }

    pub fn measure_with_options(
        &mut self,
        text: &str,
        font_handle: FontHandle,
        options: TextLayoutOptions,
    ) -> anyhow::Result<Arc<TextLayout>> {
        self.layout_cached(text, font_handle, options)
    }

    pub fn measure(
        &mut self,
        text: &str,
        font_handle: FontHandle,
    ) -> anyhow::Result<Arc<TextLayout>> {
        self.measure_with_options(
            text,
            font_handle,
            TextLayoutOptions {
                max_width: f32::MAX,
            },
        )
    }

    fn is_leading_forbidden_token(&self, token: &str) -> bool {
        token.chars().all(|ch| {
            matches!(
                ch,
                '.' | ','
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | ')'
                    | ']'
                    | '}'
                    | '»'
                    | '”'
                    | '’'
                    | '،'
                    | '؛'
                    | '؟'
            )
        })
    }

    /// Draw wrapped text only on complete lines inside the vertical viewport.
    /// The layout origin may lie above the viewport when showing recent history.
    #[allow(dead_code)]
    pub fn render_viewport<G: GraphicsDevice>(
        &mut self,
        viewport: ([f32; 2], [f32; 2], f32),
        content: &str,
        font: FontHandle,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let (origin, clip_y, width) = viewport;
        let layout = self.layout_cached(content, font, TextLayoutOptions { max_width: width })?;
        let mut y = origin[1];

        for line in &layout.lines {
            if y >= clip_y[0] && y + layout.line_height <= clip_y[1] {
                let mut x = if line.is_rtl {
                    origin[0] + width - line.width
                } else {
                    origin[0]
                };

                for slice in &line.slices {
                    let run = &layout.paragraph_runs[slice.run_index];
                    self.render_run_slice(x, y + layout.ascender, run, slice, gpu, ui)?;
                    x += slice.x_advance;
                }
            }

            y += layout.line_height;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn render_centered<G: GraphicsDevice>(
        &mut self,
        xy: [f32; 2],
        value: &str,
        font: FontHandle,
        width: f32,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let layout = self.layout_cached(value, font, TextLayoutOptions { max_width: width })?;
        self.render_layout(xy, &layout, width, true, gpu, ui)
    }

    pub fn render_layout<G: GraphicsDevice>(
        &mut self,
        xy: [f32; 2],
        layout: &TextLayout,
        width: f32,
        centered: bool,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<()> {
        let clip = ui.clip();
        let height = layout.line_height.max(1.0);
        let first =
            (((clip[1] - xy[1]) / height).floor().max(0.0) as usize).min(layout.lines.len());
        let end = (((clip[3] - xy[1]) / height).ceil().max(0.0) as usize).min(layout.lines.len());
        let mut baseline = xy[1] + layout.ascender + first as f32 * layout.line_height;

        for line in layout.lines.iter().take(end).skip(first) {
            let offset = if centered {
                (width - line.width) * 0.5
            } else if line.is_rtl {
                width - line.width
            } else {
                0.0
            };

            let mut x = xy[0] + offset;

            for slice in &line.slices {
                self.render_run_slice(
                    x,
                    baseline,
                    &layout.paragraph_runs[slice.run_index],
                    slice,
                    gpu,
                    ui,
                )?;
                x += slice.x_advance;
            }

            baseline += layout.line_height;
        }

        Ok(())
    }

    /// Renders text with word wrapping.
    ///
    /// Lays out the text into wrapped lines using
    /// `max_width` and renders the resulting layout
    /// at the given position.
    #[allow(dead_code)]
    pub fn render_wrapped<G: GraphicsDevice>(
        &mut self,
        xy: [f32; 2],
        text: &str,
        font_handle: FontHandle,
        max_width: f32,
        gpu: &mut G,
        ui: &mut UIRenderer,
    ) -> anyhow::Result<Arc<TextLayout>> {
        let layout = self.layout_cached(text, font_handle, TextLayoutOptions { max_width })?;

        self.render_layout(xy, &layout, max_width, false, gpu, ui)?;

        Ok(layout)
    }
}

/// Place an unscaled raster bitmap on the framebuffer pixel grid. Snap the
/// shared baseline before subtracting integer bearings, never each logical y.
fn glyph_rect(
    origin: [f32; 2],
    y_offset: f32,
    bearing: [i32; 2],
    size: [u32; 2],
    scale: f32,
) -> [f32; 4] {
    [
        ((origin[0] * scale).round() + bearing[0] as f32) / scale,
        ((origin[1] * scale).round() - (y_offset * scale).round() - bearing[1] as f32) / scale,
        size[0] as f32 / scale,
        size[1] as f32 / scale,
    ]
}

fn bordered_glyph(glyph: &Glyph) -> anyhow::Result<Vec<u8>> {
    let w = glyph.width as usize;
    let h = glyph.height as usize;
    anyhow::ensure!(
        w > 0 && h > 0 && glyph.pixels.len() == w * h * 4,
        "invalid RGBA glyph bitmap"
    );
    let stride = (w + 2) * 4;
    let mut pixels = vec![0; stride * (h + 2)];

    for row in 0..h {
        let start = (row + 1) * stride + 4;
        pixels[start..start + w * 4].copy_from_slice(&glyph.pixels[row * w * 4..(row + 1) * w * 4]);

        for (column, source) in [(0, row * w * 4), (w + 1, ((row + 1) * w - 1) * 4)] {
            let target = (row + 1) * stride + column * 4;
            pixels[target..target + 3].copy_from_slice(&glyph.pixels[source..source + 3]);
        }
    }

    for column in 0..w + 2 {
        let source_x = column.saturating_sub(1).min(w - 1);

        for (row, source_y) in [(0, 0), (h + 1, h - 1)] {
            let source = (source_y * w + source_x) * 4;
            let target = row * stride + column * 4;
            pixels[target..target + 3].copy_from_slice(&glyph.pixels[source..source + 3]);
        }
    }

    Ok(pixels)
}

#[cfg(test)]
mod system_font_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn preserved_layout_keeps_blank_lines_indentation_and_grapheme_boundaries() -> anyhow::Result<()>
    {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut renderer = TextRenderer::new()?;
        let font = renderer.load_primary_font(&assets.join("NotoSans-Bold.ttf"), 16, 0)?;
        let value = "  first\r\n\r\n\tsecond\rthird\n";
        let layout = renderer.preserved_layout(value, font, 500.0)?;
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| line.display.as_str())
                .collect::<Vec<_>>(),
            ["  first", "", "    second", "third", ""],
        );
        let indented = &layout.lines[2];
        assert_eq!(&value[indented.source.clone()], "\tsecond");
        assert_eq!(indented.display_offset(indented.source.start + 1), 4);
        assert_eq!(indented.source_offset(1), indented.source.start);
        assert_eq!(indented.source_offset(3), indented.source.start + 1);
        assert_eq!(layout.height, 5.0 * layout.line_height);

        let unbroken = "e\u{301}e\u{301}e\u{301}";
        let layout = renderer.preserved_layout(unbroken, font, 1.0)?;
        assert_eq!(layout.lines.len(), 3);
        assert!(layout.lines.iter().all(|line| line.display == "e\u{301}"));
        Ok(())
    }

    #[test]
    fn alternate_text_face_keeps_emoji_and_script_fallbacks() -> anyhow::Result<()> {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let emoji = assets.join("NotoColorEmoji.ttf");
        let arabic = assets.join("NotoSansArabic-Regular.ttf");
        let hebrew = assets.join("NotoSansHebrew-Regular.ttf");
        let mut renderer = TextRenderer::new()?;
        let primary = renderer.load_primary_font(&assets.join("NotoSans-Bold.ttf"), 16, 0)?;
        let expected = [
            ("😀", renderer.load_primary_font(&emoji, 16, 0)?),
            ("العربية", renderer.load_primary_font(&arabic, 16, 0)?),
            ("עברית", renderer.load_primary_font(&hebrew, 16, 0)?),
        ];
        renderer.set_fallback_fonts(vec![emoji.clone(), arabic, hebrew], Some(emoji));

        for (text, font) in expected {
            let runs = renderer.build_shaped_runs_cached(primary, text)?;
            assert!(!runs.is_empty());
            assert!(
                runs.iter().all(|run| {
                    run.font_handle == font
                        && run.shaped_glyphs.iter().all(|glyph| glyph.glyph_idx != 0)
                }),
                "{text}"
            );
        }

        Ok(())
    }
}
