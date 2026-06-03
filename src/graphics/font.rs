use freetype::{self as ft, bitmap};
use unicode_bidi::Level;
use unicode_script::Script;
use std::{collections::HashMap, path::{Path, PathBuf}};
use harfbuzz_rs::{Script as HbScript, Tag};

use crate::graphics::FontHandle;

use harfbuzz_rs::{shape, UnicodeBuffer};

/// Used as the key to get cached glyphs for 
/// a given font/glyph index combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_handle: FontHandle,
    pub glyph_idx: u32,
}

/// Font loading, shaping and glyph 
/// rasterization manager.
///
/// Manages the FreeType library handle, 
/// loaded fonts and rasterized glyph cache.
pub struct FontManager {
    lib_handle: ft::Library,
    fonts: Vec<Font>,
    glyph_cache: HashMap<GlyphKey, Glyph>
}

/// Represents one rasterized glyph bitmap 
/// and its layout metrics.
#[derive(Debug, Clone)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    pub bearing_x: i32,
    pub bearing_y: i32,
    pub advance_x: i64,

    pub pixels: Vec<u8>
}

/// Represents one glyph produced by text 
/// shaping.
#[derive(Debug, Clone)]
pub struct ShapedGlyph {
    pub glyph_idx: u32,
    pub cluster: u32,
    pub x_adv: f32,
    pub y_adv: f32,
    pub x_off: f32,
    pub y_off: f32,
}

/// Represents one loaded font face.
///
/// Stores both the FreeType face used for 
/// rasterization and the HarfBuzz font used 
/// for shaping.
pub struct Font {
    pub size: u32,

    // Selected strike size for bitmap-strike 
    // fonts. This is equal to the size field 
    // for almost all fonts except Emoji fonts.
    pub raster_size: u32,

    // Scale factor from selected raster 
    // size to the requested font size.
    // This is 1.0 for almost all fonts 
    // except Emoji fonts.
    pub scale: f32, 

    pub bitmap_strike: bool,

    pub path: PathBuf,
    pub face: freetype::Face,
    pub hb_font: harfbuzz_rs::Owned<harfbuzz_rs::Font<'static>>,
}

impl FontManager {
    /// Creates a new font manager and initializes 
    /// the FreeType library.
    pub fn new() -> anyhow::Result<Self> {
        let lib_handle = ft::Library::init()?;

        Ok(Self {
            lib_handle,
            fonts: Vec::new(), 
            glyph_cache: HashMap::new() 
        })
    }

    fn resize_rgba(
        &self,
        pixels: &[u8],
        width: u32,
        height: u32,
        new_width: u32,
        new_height: u32,
    ) -> anyhow::Result<Vec<u8>> {
        use image::{ImageBuffer, Rgba};

        // Resize the RGBa pixel buffer with 
        // 'image' and FilterType::Triangle. 
        // This is done to drastically improve 
        // rendering quality of glyphs that need 
        // scaling (meaning their font's raster size 
        // is not equal to their actual preferred size).
        //
        // This is almost exclusively done for emojis.
        //
        // We have found that FilterType::Triangle is 
        // the bets trade off between performance and 
        // quality.
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width, height, pixels.to_vec())
            .ok_or_else(|| anyhow::anyhow!("invalid RGBA bitmap buffer"))?;

        let resized = image::imageops::resize(
            &img,
            new_width.max(1),
            new_height.max(1),
            image::imageops::FilterType::Triangle,
        );

        Ok(resized.into_raw())
    }

    fn alpha_to_rgba(
        &self,
        bitmap: &freetype::Bitmap,
    ) -> Vec<u8> {
        // Convert an alpha-only bitmap to a RGBA 
        // bitmap by extending the given u8 Vec
        // with 1 btye per pixel to a newly |allocate|d 
        // u8 Vec with 4 bytes per pixel with 
        // R,G,B=255 A=alpha and returning the result 
        let width = bitmap.width() as usize;
        let height = bitmap.rows() as usize;
        let pitch = bitmap.pitch().abs() as usize;
        let src = bitmap.buffer();

        let mut dst = Vec::with_capacity(width * height * 4);

        for row in 0..height {
            let row_start = row * pitch;
            let row_data = &src[row_start..row_start + width];

            for &a in row_data {
                dst.extend_from_slice(&[255, 255, 255, a]);
            }
        }

        dst
    }

    fn bgra_to_rgba(
        &self,
        bitmap: &freetype::Bitmap,
    ) -> Vec<u8> {
        // Convert a BGRA bitmap to a RGBA 
        // bitmap by reversing the given u8 Vec's
        // B,G,R bytes -> R,G,B bytes and storing 
        // the result + the A byte in a newly 
        // |allocate|d u8 Vec with 4 bytes per pixel with 
        // R,G,B,A and returning the result 
        let width = bitmap.width() as usize;
        let height = bitmap.rows() as usize;
        let pitch = bitmap.pitch().abs() as usize;
        let src = bitmap.buffer();

        let mut dst = Vec::with_capacity(width * height * 4);

        for row in 0..height {
            let row_start = row * pitch;
            let row_data = &src[row_start..row_start + width * 4];

            for px in row_data.chunks_exact(4) {
                let b = px[0] as u32;
                let g = px[1] as u32;
                let r = px[2] as u32;
                let a = px[3] as u32;

                if a == 0 {
                    dst.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    let r = ((r * 255 + a / 2) / a).min(255) as u8;
                    let g = ((g * 255 + a / 2) / a).min(255) as u8;
                    let b = ((b * 255 + a / 2) / a).min(255) as u8;

                    dst.extend_from_slice(&[r, g, b, a as u8]);
                }
            }
        }

        dst
    }

    fn rasterize_glyph(
        &mut self, 
        font_handle: FontHandle,
        glyph_idx: u32
    ) -> anyhow::Result<Glyph> {
        // Get the font from the opaque handle 
        let font = self.get_font(font_handle)?; 

        // If the font has bitmap_strike set to 
        // true, it means the font has fixed 
        // sizes, e.g 32px, 64px, 128px. This is 
        // true for all Emoji fonts, making it a 
        // reasonable way to determine if a glyph
        // needs color loaded.
        let flags = if font.bitmap_strike {
            ft::face::LoadFlag::RENDER | 
                ft::face::LoadFlag::COLOR 
        } else {
            ft::face::LoadFlag::RENDER 
        };

        // Load/rasterize the glyph with freetype
        font.face.load_glyph(
            glyph_idx,
            flags
        )?;

        let glyph           = font.face.glyph();
        let bitmap          = glyph.bitmap();
        let mut width       = bitmap.width() as u32;
        let mut height      = bitmap.rows() as u32;

        // Convert the pixels to RGBA
        let mut pixels_rgba = match bitmap.pixel_mode()? {
            bitmap::PixelMode::Bgra => self.bgra_to_rgba(&bitmap),
            bitmap::PixelMode::Gray => self.alpha_to_rgba(&bitmap),
            _ => bitmap.buffer().to_vec(),
        };

        let mut bearing_x   = glyph.bitmap_left();
        let mut bearing_y   = glyph.bitmap_top();
        let mut advance_x   = glyph.advance().x;

        // If the font is an emoji font and it's
        // preferred font size is not equal to 
        // selected strike size, resize the glyph 
        // bitmap to include the font's scale factor. 
        if font.bitmap_strike && font.scale != 1.0 {
            let new_width = ((width as f32 * font.scale).round() as u32).max(1);
            let new_height = ((height as f32 * font.scale).round() as u32).max(1);

            pixels_rgba = self.resize_rgba(
                &pixels_rgba,
                width,
                height,
                new_width,
                new_height,
            )?;

            width = new_width;
            height = new_height;

            bearing_x = (bearing_x as f32 * font.scale).round() as i32;
            bearing_y = (bearing_y as f32 * font.scale).round() as i32;
            advance_x = (advance_x as f32 * font.scale) as i64;
        }

        Ok(Glyph {
            width,
            height,
            bearing_x,
            bearing_y,
            advance_x,
            pixels: pixels_rgba,
        })
    }

    /// Gets a cached glyph or rasterizes and 
    /// caches it if it has not been loaded yet.
    pub fn get_or_load_glyph(
        &mut self,
        font_handle: FontHandle,
        glyph_idx: u32
    ) -> anyhow::Result<&Glyph> {
        let key = GlyphKey {
            font_handle,
            glyph_idx
        };

        // Cached rasterization of glyphs
        if !self.glyph_cache.contains_key(&key) {
            let glyph = self.rasterize_glyph(font_handle, glyph_idx)?;
            self.glyph_cache.insert(key, glyph);
        }

        Ok(self.glyph_cache.get(&key).unwrap())
    }

    fn select_best_bitmap_strike(
        &self,
        face: &freetype::Face,
        requested_size: u32,
    ) -> anyhow::Result<Option<u32>> {
        let raw = face.raw();

        let num_fixed_sizes = raw.num_fixed_sizes;

        // No available sizes -> Return None to 
        // indicate.
        if num_fixed_sizes <= 0 || raw.available_sizes.is_null() {
            return Ok(None);
        }

        // Get available strikes (no safe function available)
        let strikes = unsafe {
            std::slice::from_raw_parts(
                raw.available_sizes,
                num_fixed_sizes as usize,
            )
        };

        // Get closest strike match  
        let mut best_larger_or_equal: Option<(usize, u32)> = None;
        let mut best_smaller: Option<(usize, u32)> = None;

        for (idx, strike) in strikes.iter().enumerate() {
            let strike_px = if strike.height > 0 {
                strike.height as u32
            } else {
                (strike.y_ppem >> 6) as u32
            };

            if strike_px >= requested_size {
                match best_larger_or_equal {
                    Some((_, best_px)) if best_px <= strike_px => {}
                    _ => best_larger_or_equal = Some((idx, strike_px)),
                }
            } else {
                match best_smaller {
                    Some((_, best_px)) if best_px >= strike_px => {}
                    _ => best_smaller = Some((idx, strike_px)),
                }
            }
        }

        let (best_idx, raster_size) =
            if let Some(best) = best_larger_or_equal {
                best
            } else if let Some(best) = best_smaller {
                best
            } else {
                return Ok(None);
            };

        face.select_size(best_idx as i32)?;

        Ok(Some(raster_size))
    }

    /// Loads a font from disk and stores it in 
    /// the font manager.
    ///
    /// Selects the best bitmap strike when the 
    /// font has fixed-size strikes, otherwise 
    /// configures the face for scalable rendering.
    pub fn load_font(
        &mut self,
        path: impl AsRef<Path>,
        size: u32,
        face_idx: isize,
    ) -> anyhow::Result<FontHandle> {
        let path = path.as_ref();

        if size == 0 {
            anyhow::bail!("font size must be greater than zero");
        }

        let face = self.lib_handle.new_face(path, face_idx)?;

        let mut raster_size = size;
        let mut bitmap_strike = false;

        // Select best fixed size if the font is a 
        // fixed-size font
        if let Some(selected_raster_size) = self.select_best_bitmap_strike(&face, size)? {
            raster_size = selected_raster_size;
            bitmap_strike = true;
        } else {
            // If the font supports it, set exact preferred 
            // pixel size
            face.set_pixel_sizes(0, size)?;
        }

        let hb_face = harfbuzz_rs::Face::from_file(path, face_idx as u32)?;
        let mut hb_font = harfbuzz_rs::Font::new(hb_face);

        // HarfBuzz needs * 64
        hb_font.set_scale(
            (raster_size as i32) * 64,
            (raster_size as i32) * 64,
        );

        let handle = FontHandle(self.fonts.len() as u32);

        self.fonts.push(Font {
            size,
            raster_size,
            scale: size as f32 / raster_size as f32,
            bitmap_strike,
            path: path.to_path_buf(),
            face,
            hb_font,
        });

        Ok(handle)
    }

    fn get_font(&self, handle: FontHandle) -> anyhow::Result<&Font> {
        self
            .fonts
            .get(handle.0 as usize)
            .ok_or_else(|| anyhow::anyhow!("invalid texture handle: {:?}", handle))
    }

    /// Returns the font ascender in pixels.
    pub fn ascender(&self, font_handle: FontHandle) -> anyhow::Result<i32> {
        let font = self.get_font(font_handle)?;
        Ok(font.face.size_metrics().unwrap().ascender as i32 >> 6)
    }

    /// Returns the font line height in pixels.
    pub fn line_height(&self, font_handle: FontHandle) -> anyhow::Result<i32>  {
        let font = self.get_font(font_handle)?;
        Ok(font.face.size_metrics().unwrap().height as i32 >> 6)
    }

    /// Returns the font scale from selected raster 
    /// size to requested font size.
    pub fn scale(&self, font_handle: FontHandle) -> anyhow::Result<f32>  {
        let font = self.get_font(font_handle)?;
        Ok(font.scale)
    }

    /// Returns the scale that should be applied 
    /// during rendering.
    ///
    /// Bitmap-strike fonts are already rasterized 
    /// at their target size and use a render scale 
    /// of 1.0.
    pub fn render_scale(&self, font_handle: FontHandle) -> anyhow::Result<f32>  {
        let font = self.get_font(font_handle)?;

        if font.bitmap_strike {
            Ok(1.0)
        } else {
            Ok(font.scale)
        }
    }

    /// Returns whether the font uses colored 
    /// bitmap-strike glyphs.
    pub fn is_colored(&self, font_handle: FontHandle) -> anyhow::Result<bool>  {
        let font = self.get_font(font_handle)?;
        Ok(font.bitmap_strike)
    }

    /// Shapes text into positioned glyphs using 
    /// HarfBuzz.
    pub fn shape_text(&self, font_handle: FontHandle, text: &str) -> anyhow::Result<Vec<ShapedGlyph>> {
        let font = self.get_font(font_handle)?;

        let buffer = UnicodeBuffer::new().add_str(text);

        // Use HarfBuzz to Shape the text
        let output = shape(&font.hb_font, buffer, &[]);

        let infos = output.get_glyph_infos();
        let positions = output.get_glyph_positions();

        // Iterate HarfBuzz positions and infos to 
        // collect shaped glyph information
        let shaped = infos.iter().zip(positions.iter())
            .map(|(info, pos)| ShapedGlyph {
                glyph_idx: info.codepoint,
                cluster: info.cluster,

                // convert from fixed point vals
                x_adv: pos.x_advance as f32 / 64.0,
                y_adv: pos.y_advance as f32 / 64.0,
                x_off: pos.x_offset as f32 / 64.0,
                y_off: pos.y_offset as f32 / 64.0,
            }).collect();

        Ok(shaped)
    }

    fn hb_tag(&self, tag: &str) -> harfbuzz_rs::Tag {
        let mut chars = tag.chars();

        let a = chars.next().unwrap_or('Z');
        let b = chars.next().unwrap_or('z');
        let c = chars.next().unwrap_or('z');
        let d = chars.next().unwrap_or('z');

            harfbuzz_rs::Tag::new(a, b, c, d)
    }

    fn hb_script_tag_from_unicode_script(&self, script: Script) -> harfbuzz_rs::Tag {
        let tag = match script {
            Script::Arabic => "Arab",
            Script::Armenian => "Armn",
            Script::Bengali => "Beng",
            Script::Bopomofo => "Bopo",
            Script::Braille => "Brai",
            Script::Canadian_Aboriginal => "Cans",
            Script::Cherokee => "Cher",
            Script::Common => "Zyyy",
            Script::Coptic => "Copt",
            Script::Cyrillic => "Cyrl",
            Script::Devanagari => "Deva",
            Script::Ethiopic => "Ethi",
            Script::Georgian => "Geor",
            Script::Greek => "Grek",
            Script::Gujarati => "Gujr",
            Script::Gurmukhi => "Guru",
            Script::Han => "Hani",
            Script::Hangul => "Hang",
            Script::Hanunoo => "Hano",
            Script::Hebrew => "Hebr",
            Script::Hiragana => "Hira",
            Script::Inherited => "Zinh",
            Script::Kannada => "Knda",
            Script::Katakana => "Kana",
            Script::Khmer => "Khmr",
            Script::Lao => "Laoo",
            Script::Latin => "Latn",
            Script::Malayalam => "Mlym",
            Script::Mongolian => "Mong",
            Script::Myanmar => "Mymr",
            Script::Ogham => "Ogam",
            Script::Oriya => "Orya",
            Script::Runic => "Runr",
            Script::Sinhala => "Sinh",
            Script::Syriac => "Syrc",
            Script::Tagalog => "Tglg",
            Script::Tagbanwa => "Tagb",
            Script::Tamil => "Taml",
            Script::Telugu => "Telu",
            Script::Thaana => "Thaa",
            Script::Thai => "Thai",
            Script::Tibetan => "Tibt",
            Script::Yi => "Yiii",

            _ => "Zzzz",
        };


        self.hb_tag(tag)
    }

    pub fn shape_text_with_props(
        &self, 
        text: &str,
        bidi_lvl: Level,
        script: Script,
        font_handle: FontHandle) 
        -> anyhow::Result<Vec<ShapedGlyph>>  {
            let font = self.get_font(font_handle)?;


            let buffer = UnicodeBuffer::new()
                .add_str(text)
                .set_direction(if bidi_lvl.is_rtl() {
                    harfbuzz_rs::Direction::Rtl
                } else {
                    harfbuzz_rs::Direction::Ltr
                })
            .set_script(self.hb_script_tag_from_unicode_script(script));


            // Use HarfBuzz to Shape the text
            let output = shape(&font.hb_font, buffer, &[]);

            let infos = output.get_glyph_infos();
            let positions = output.get_glyph_positions();

            // Iterate HarfBuzz positions and infos to 
            // collect shaped glyph information
            let shaped = infos.iter().zip(positions.iter())
                .map(|(info, pos)| ShapedGlyph {
                    glyph_idx: info.codepoint,
                    cluster: info.cluster,

                    // convert from fixed point vals
                x_adv: pos.x_advance as f32 / 64.0,
                y_adv: pos.y_advance as f32 / 64.0,
                x_off: pos.x_offset as f32 / 64.0,
                y_off: pos.y_offset as f32 / 64.0,
            }).collect();

        Ok(shaped)
    }

}
