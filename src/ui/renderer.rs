use crate::Color;

use crate::graphics::{
    BufferDesc, BufferHandle, BufferTarget, BufferUsage, GraphicsDevice, Image, ImageData, PipelineDesc, PipelineHandle, TextureFormat, TextureHandle, VertexStepMode
};

use crate::graphics::device::{
    DrawIndexedInstanced, TextureArrayDesc, TextureKind, VertexAttribute, VertexBufferLayout, VertexFormat
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


pub struct ImageAtlasLayer {
    width: u32,
    height: u32,
    layer: u32,
    row_h: u32,
    cursor_x: u32,
    cursor_y: u32,
}

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
    
    ui_texture_array: TextureHandle,
    image_atlas_layers: Vec<ImageAtlasLayer>,
}

const UI_ATLAS_WIDTH: u32 = 2048;
const UI_ATLAS_HEIGHT: u32 = 2048;
const UI_ATLAS_LAYERS: u32 = 32;

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

        gpu.set_uniform_1i(pipeline, "u_texture_array", 0)?;
        gpu.set_uniform_1i(pipeline, "u_texture", 1)?;

        let ui_texture_array = gpu.create_texture_array(TextureArrayDesc {
            width: UI_ATLAS_WIDTH,
            height: UI_ATLAS_HEIGHT,
            layers: UI_ATLAS_LAYERS,
            format: TextureFormat::Rgba8,
        })?;

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
            ui_texture_array,
            image_atlas_layers: Vec::new(),
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
                let slot = if gpu.texture_get_kind(texture)? == TextureKind::TextureArray2d {
                    0
                } else { 1 };
                gpu.set_texture(slot, texture)?;
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
        params: [f32; 4],
        ) -> anyhow::Result<()> {
        if self.instances.len() >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }

        let instance_start = self.instances.len() as u32;

        let kind = params[3];
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


        self.instances.push(QuadInstance::textured(rect, uv, color, params));

        Ok(())
    }

    pub fn quad(&mut self, rect: [f32; 4], color: Color) -> anyhow::Result<()> {
        self.textured_quad(
            rect,
            TextureHandle(0),
            [0.0, 0.0, 1.0, 1.0],
            color,
            [0.0, 0.0, 0.0, 0.0] 
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
            self.ui_texture_array,
            [0.0, 0.0, 1.0, 1.0],
            Color::rgba(1.0, 1.0, 1.0, 1.0),
            [0.0, 0.0, image.layer as f32, 3.0] 
        )
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
            self.ui_texture_array,
            [0.0, 0.0, 1.0, 1.0],
            color,
            [0.0, 0.0, image.layer as f32, 3.0] 
        )
    }

    fn allocate_image_rect(
    &mut self,
    width: u32,
    height: u32,
    padding: u32,
) -> anyhow::Result<(u32, u32, u32, u32, u32)> {
    if width + padding > UI_ATLAS_WIDTH || height + padding > UI_ATLAS_HEIGHT {
        anyhow::bail!("image is larger than UI atlas layer");
    }

    if self.image_atlas_layers.is_empty() {
        self.create_new_image_layer()?;
    }

    let mut layer_idx = self.image_atlas_layers.len() - 1;

    {
        let layer = &mut self.image_atlas_layers[layer_idx];

        if layer.cursor_x + width + padding > layer.width {
            layer.cursor_x = 0;
            layer.cursor_y += layer.row_h + padding;
            layer.row_h = 0;
        }

        if layer.cursor_y + height + padding > layer.height {
            self.create_new_image_layer()?;
            layer_idx = self.image_atlas_layers.len() - 1;
        }
    }

    let layer = &mut self.image_atlas_layers[layer_idx];

    let x = layer.cursor_x;
    let y = layer.cursor_y;
    let layer_id = layer.layer;

    layer.cursor_x += width + padding;
    layer.row_h = layer.row_h.max(height);

    Ok((layer_id, x, y, layer.width, layer.height))
    }

    fn create_new_image_layer(&mut self) -> anyhow::Result<u32> {
        let layer = self.image_atlas_layers.len() as u32;

        if layer >= UI_ATLAS_LAYERS {
            anyhow::bail!("UI image atlas texture array is full");
        }

        self.image_atlas_layers.push(ImageAtlasLayer {
            width: UI_ATLAS_WIDTH,
            height: UI_ATLAS_HEIGHT,
            layer,
            row_h: 0,
            cursor_x: 0,
            cursor_y: 0,
        });

        Ok(layer)
    }

    pub fn load_image<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<Image> {
        let data = ImageData::load_rgba8(path)?;

        let (layer, x, y, atlas_w, atlas_h) =
            self.allocate_image_rect(data.width, data.height, 1)?;

        gpu.write_texture_array_layer(
            self.ui_texture_array,
            x,
            y,
            layer,
            data.width,
            data.height,
            &data.pixels,
        )?;

        gpu.texture_gen_mipmap(self.ui_texture_array)?;

        Ok(Image {
            layer,
            width: data.width,
            height: data.height,
            uv_min: [
                x as f32 / atlas_w as f32,
                y as f32 / atlas_h as f32,
            ],
            uv_max: [
                (x + data.width) as f32 / atlas_w as f32,
                (y + data.height) as f32 / atlas_h as f32,
            ],
        })
    }


}
