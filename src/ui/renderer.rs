
use std::cmp::max;
use std::collections::HashMap;

use crate::Color;

use crate::graphics::font::{Glyph, GlyphKey};
use crate::graphics::{
    BufferDesc, BufferHandle, BufferTarget, BufferUsage, FontHandle, FontManager, GraphicsDevice, Image, ImageData, PipelineDesc, PipelineHandle, TextureDesc, TextureFormat, TextureHandle, VertexStepMode
};

use crate::graphics::device::{
    DrawIndexedInstanced,
    VertexAttribute,
    VertexBufferLayout,
    VertexFormat,
};

use crate::ui::{
    QuadInstance,
    QuadVertex,
    QUAD_INDICES,
    QUAD_VERTICES,
};

use crate::ui::quad::{
    BatchKey, BatchKind, QuadBatch, UI_QUAD_FRAGMENT_SHADER, UI_QUAD_VERTEX_SHADER
};

pub struct UIRenderer {
    screen_width: u32,
    screen_height: u32,

    pipeline: PipelineHandle,

    quad_vbo: BufferHandle,
    quad_ibo: BufferHandle,
    instance_vbo: BufferHandle,

    instances: Vec<QuadInstance>,
    batches: Vec<QuadBatch>,
    max_instances: usize,
    batch_count: usize,

    white_texture: TextureHandle,

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

impl UIRenderer {
    pub fn new<G: GraphicsDevice>(
        gpu: &mut G,
        screen_width: u32,
        screen_height: u32
    ) -> anyhow::Result<Self> {
        const MAX_INSTANCES: usize = 65_536;
        const MAX_INSTANCES_PER_FRAME: usize = 65_536 * 8;

        let quad_vbo = gpu.create_buffer(BufferDesc{
            target: BufferTarget::Vertex,
            usage: BufferUsage::Static,
            size: std::mem::size_of_val(&QUAD_VERTICES)
        })?;

        gpu.write_buffer(quad_vbo, 0, 0, bytemuck::cast_slice(&QUAD_VERTICES))?;

        let quad_ibo = gpu.create_buffer(BufferDesc{
            target: BufferTarget::Index,
            usage: BufferUsage::Static,
            size: std::mem::size_of_val(&QUAD_INDICES)
        })?;

        gpu.write_buffer(quad_ibo, 0, 0, bytemuck::cast_slice(&QUAD_INDICES))?;

        let instance_vbo = gpu.create_buffer(BufferDesc{
            target: BufferTarget::Vertex,
            usage: BufferUsage::Dynamic,
            size: MAX_INSTANCES_PER_FRAME * std::mem::size_of::<QuadInstance>()
        })?;

        let white_pixel = [255u8, 255, 255, 255];

        let white_texture = gpu.create_texture(
            TextureDesc {
                width: 1,
                height: 1,
                format: TextureFormat::Rgba8,
            },
            Some(&white_pixel),
        )?;

        let pipeline = gpu.create_pipeline(PipelineDesc{
            vertex_source: UI_QUAD_VERTEX_SHADER,
            fragment_source: UI_QUAD_FRAGMENT_SHADER,
            vert_layouts: vec![
                VertexBufferLayout {
                    binding: 0,
                    stride: std::mem::size_of::<QuadVertex>() as u32,
                    step_mode: VertexStepMode::Vertex,
                    attrs: vec![
                        // pos
                        VertexAttribute {
                            location: 0,
                            offset: 0,
                            format: VertexFormat::Float32x2,
                        },
                        // uv 
                        VertexAttribute {
                            location: 1,
                            offset: 8,
                            format: VertexFormat::Float32x2,
                        }
                    ]
                },
                VertexBufferLayout {
                    binding: 1,
                    stride: std::mem::size_of::<QuadInstance>() as u32,
                    step_mode: VertexStepMode::Instance,
                    attrs: vec![
                        // rect 
                        VertexAttribute {
                            location: 2,
                            offset: 0,
                            format: VertexFormat::Float32x4,
                        },
                        // color 
                        VertexAttribute {
                            location: 3,
                            offset: 16,
                            format: VertexFormat::Float32x4,
                        },
                        // uv 
                        VertexAttribute {
                            location: 4,
                            offset: 32,
                            format: VertexFormat::Float32x4,
                        },
                        // params
                        VertexAttribute {
                            location: 5,
                            offset: 48,
                            format: VertexFormat::Float32x4,
                        }
                    ]
                }

            ]
        })?;

        gpu.set_uniform_1i(pipeline, "u_texture", 0)?;

        let font_manager = FontManager::new()?;

        Ok(Self { 
            screen_width: screen_width, 
            screen_height: screen_height,
            pipeline,
            quad_vbo,
            quad_ibo,
            instance_vbo,
            instances: Vec::with_capacity(4096),
            batches: Vec::with_capacity(256),
            max_instances: MAX_INSTANCES, 
            batch_count: 0,
            white_texture,
            glyph_atlases: Vec::new(),
            glyph_locations: HashMap::new(),
            font_manager
        })
    }

    pub fn begin(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;

        self.instances.clear();
        self.batches.clear();
        self.batch_count = 0;
    }

    pub fn end<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        if self.instances.is_empty() {
            return Ok(());
        }

        let instance_bytes = bytemuck::cast_slice(&self.instances);

        gpu.write_buffer(self.instance_vbo, 1, 0, instance_bytes)?;

        gpu.set_pipeline(self.pipeline)?;
        gpu.set_uniform_2f(self.pipeline, "u_screen_size", 
            self.screen_width as f32, self.screen_height as f32)?;

        gpu.set_vertex_buffer(self.quad_vbo,    0)?;
        gpu.set_vertex_buffer(self.instance_vbo, 1)?;
        gpu.set_index_buffer(self.quad_ibo)?;

        for batch in &self.batches {
            if let Some(texture) = batch.key.texture {
                gpu.set_texture(0, texture)?;
            }

            gpu.draw_indexed_instanced(DrawIndexedInstanced {
                index_count: 6, 
                index_offset: 0, 
                vertex_offset: 0, 
                inst_count: batch.inst_count, 
                inst_offset: batch.inst_start, 
            })?;
        }

        println!("{} drawcalls.", self.batches.len());

        Ok(())
    }
    
    pub fn textured_quad(
        &mut self, 
        rect: [f32; 4], 
        texture: TextureHandle,
        uv: [f32; 4],
        color: Color,
        kind: f32
        ) -> anyhow::Result<()> {
        if self.instances.len() >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }

        let instance_start = self.instances.len() as u32;

        let batch_key = match kind {
            0.0 => BatchKey {
                kind: BatchKind::Solid,
                texture: None,
            },

            // image or glyph
            _ => BatchKey {
                kind: BatchKind::Textured,
                texture: Some(texture),
            },
        };

        match self.batches.last_mut() {
            Some(batch) if 
                batch.key.texture == batch_key.texture || 
                batch.key.kind == BatchKind::Solid || 
                batch_key.kind == BatchKind::Solid => {
                    // can merge 
                    batch.inst_count += 1;
            }
            _ => {
                // spill instance to new batch
                self.batches.push(
                    QuadBatch { 
                        inst_start: instance_start, 
                        inst_count: 1, 
                        key: batch_key 
                    });
            }
        } 


        self.instances.push(QuadInstance::textured(rect, uv, color, kind));

        Ok(())
    }

    pub fn quad(&mut self, rect: [f32; 4], color: Color) -> anyhow::Result<()> {
        self.textured_quad(
            rect,
            self.white_texture,
            [0.0, 0.0, 1.0, 1.0],
            color,
            0.0 
        )
    }

    pub fn image(
        &mut self,
        x: f32,
        y: f32,
        image: Image
    ) -> anyhow::Result<()> {
        self.textured_quad(
            [x, y, image.width as f32, image.height as f32],
            image.texture,
            [0.0, 0.0, 1.0, 1.0],
            Color::rgba(1.0, 1.0, 1.0, 1.0),
            1.0 
        )
    }

    pub fn text<G: GraphicsDevice>(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font_handle: FontHandle,
        gpu: &mut G,
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

            self.textured_quad(
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
                Color::rgba(0.0, 0.0, 0.0, 1.0),
                2.0,
            )?;

            cursor_x += (advance_x >> 6) as f32;
        }

        Ok(())
    }

    pub fn image_tined(
        &mut self,
        x: f32,
        y: f32,
        image: Image,
        color: Color
    ) -> anyhow::Result<()> {
        self.textured_quad(
            [x, y, image.width as f32, image.height as f32],
            image.texture,
            [0.0, 0.0, 1.0, 1.0],
            color,
            1.0 
        )
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

    pub fn load_image<G: GraphicsDevice>(
        &self,
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

}
