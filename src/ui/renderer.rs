use crate::Color;

use crate::graphics::{
    BufferDesc, BufferHandle, BufferTarget, BufferUsage, GraphicsDevice, ImageData, PipelineDesc, PipelineHandle, TextureDesc, TextureFormat, TextureHandle, VertexStepMode
};

use crate::graphics::device::{
    DrawIndexedInstanced, TextureArrayDesc, VertexAttribute, VertexBufferLayout, VertexFormat
};

use crate::ui::quad::{UI_QUAD_FRAGMENT_SHADER, UI_QUAD_FRAGMENT_SHADER_DEDICATED, UI_QUAD_VERTEX_SHADER};
use crate::ui::{
    QuadInstance,
    QuadVertex,
    QUAD_INDICES,
    QUAD_VERTICES,
};

#[derive(Debug, Clone, Copy)]
pub enum ImageStorage {
    Atlas {
        layer: u32,
        uv_min: [f32; 2],
        uv_max: [f32; 2],
    },
    Dedicated {
        texture_handle: TextureHandle,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Image {
    storage: ImageStorage,
    size: [u32; 2],
}

pub struct ImageAtlasLayer {
    width: u32,
    height: u32,
    layer: u32,
    row_h: u32,
    cursor_x: u32,
    cursor_y: u32,
}

pub struct DedicatedDraw {
    texture_handle: TextureHandle,
    start: u32,
    count: u32
}

pub struct UIRenderer {
    screen_width: u32,
    screen_height: u32,

    pipeline: PipelineHandle,
    pipeline_dedicated: PipelineHandle,

    quad_vbo: BufferHandle,
    quad_ibo: BufferHandle,
    instance_vbo: BufferHandle,

    instances: Vec<QuadInstance>,

    atlas_instances: Vec<QuadInstance>,
    dedicated_instances: Vec<QuadInstance>,

    max_instances: usize,

    ui_texture_array: TextureHandle,
    image_atlas_layers: Vec<ImageAtlasLayer>,

    dedicated_draws: Vec<DedicatedDraw>,
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

        let quad_layouts =  vec![
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

        ];


        let pipeline = gpu.create_pipeline(PipelineDesc{
            vertex_source: UI_QUAD_VERTEX_SHADER,
            fragment_source: UI_QUAD_FRAGMENT_SHADER,
            vert_layouts: quad_layouts.clone()
        })?;

        let pipeline_dedicated = gpu.create_pipeline(PipelineDesc{
            vertex_source: UI_QUAD_VERTEX_SHADER,
            fragment_source: UI_QUAD_FRAGMENT_SHADER_DEDICATED,
            vert_layouts: quad_layouts,
        })?;

        gpu.set_uniform_1i(pipeline, "u_texture_array", 0)?;

        let ui_texture_array = gpu.create_texture_array(TextureArrayDesc {
            width: UI_ATLAS_WIDTH,
            height: UI_ATLAS_HEIGHT,
            layers: UI_ATLAS_LAYERS,
            format: TextureFormat::Rgba8,
        })?;

        gpu.set_uniform_1i(pipeline_dedicated, "u_texture", 0)?;

        Ok(Self { 
            screen_width: screen_width, 
            screen_height: screen_height,
            pipeline,
            pipeline_dedicated,
            quad_vbo,
            quad_ibo,
            instance_vbo,
            instances: Vec::with_capacity(4096),
            dedicated_instances: Vec::new(),
            atlas_instances: Vec::with_capacity(4096),
            max_instances: MAX_INSTANCES, 
            ui_texture_array,
            image_atlas_layers: Vec::new(),
            dedicated_draws: Vec::new(),
        })
    }

    pub fn begin(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;

        self.instances.clear();
        self.atlas_instances.clear();
        self.dedicated_instances.clear();
        self.dedicated_draws.clear();
    }

    pub fn end<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        if self.atlas_instances.is_empty() && self.dedicated_instances.is_empty() {
            return Ok(());
        }

        self.instances.clear();

        let (instances_to_upload, atlas_start) = if self.dedicated_instances.is_empty() {
            (self.atlas_instances.as_slice(), 0)
        } else {
            self.instances
                .extend_from_slice(&self.dedicated_instances);

            let atlas_start = self.instances.len() as u32;

            self.instances
                .extend_from_slice(&self.atlas_instances);

            (self.instances.as_slice(), atlas_start)
        };

        let instance_bytes = bytemuck::cast_slice(instances_to_upload);

        gpu.write_buffer(self.instance_vbo, 1, 0, instance_bytes)?;

        gpu.set_vertex_buffer(self.quad_vbo,    0)?;
        gpu.set_vertex_buffer(self.instance_vbo, 1)?;
        gpu.set_index_buffer(self.quad_ibo)?;

        if !self.dedicated_instances.is_empty() {
            gpu.set_pipeline(self.pipeline_dedicated)?;
            gpu.set_uniform_2f(self.pipeline_dedicated, "u_screen_size", 
                self.screen_width as f32, self.screen_height as f32)?;

            for draw in &self.dedicated_draws {
                gpu.set_texture(0, draw.texture_handle)?;

                gpu.draw_indexed_instanced(DrawIndexedInstanced {
                    index_count: 6, 
                    index_offset: 0, 
                    vertex_offset: 0, 
                    inst_count: draw.count, 
                    inst_offset: draw.start,
                })?;
            }
        }


        if !self.atlas_instances.is_empty() {
            gpu.set_pipeline(self.pipeline)?;
            gpu.set_uniform_2f(self.pipeline, "u_screen_size", 
                self.screen_width as f32, self.screen_height as f32)?;


            gpu.set_texture(0, self.ui_texture_array)?;

            gpu.draw_indexed_instanced(DrawIndexedInstanced {
                index_count: 6, 
                index_offset: 0, 
                vertex_offset: 0, 
                inst_count: self.atlas_instances.len() as u32, 
                inst_offset: atlas_start 
            })?;
        }


        Ok(())
    }

    pub fn atlas_array_texture(&self) -> anyhow::Result<TextureHandle> {
        Ok(self.ui_texture_array)
    }

    pub fn raw_quad_atlas(
        &mut self, 
        rect: [f32; 4], 
        uv: [f32; 4],
        color: Color,
        params: [f32; 4],
    ) -> anyhow::Result<()> {

        let total_instances =
            self.atlas_instances.len() + self.dedicated_instances.len();

        if total_instances >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }


        self.atlas_instances.push(QuadInstance::textured(rect, uv, color, params));

        Ok(())
    }
    pub fn raw_quad_dedicated(
        &mut self, 
        texture_handle: TextureHandle,
        rect: [f32; 4], 
        uv: [f32; 4],
        color: Color,
        params: [f32; 4],
    ) -> anyhow::Result<()> {
        if self.instances.len() >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }

        let start = self.dedicated_instances.len() as u32;

        self.dedicated_instances
            .push(QuadInstance::textured(rect, uv, color, params));

        match self.dedicated_draws.last_mut() {
            Some(draw) if 
                draw.texture_handle == texture_handle => {
                    draw.count += 1;
            }
            _ => {
                self.dedicated_draws.push(DedicatedDraw { 
                    start,
                    texture_handle, 
                    count: 1});
            }
        }


        Ok(())
    }

    pub fn quad(&mut self, rect: [f32; 4], color: Color) -> anyhow::Result<()> {
        self.raw_quad_atlas(
            rect,
            [0.0, 0.0, 1.0, 1.0],
            color,
            [0.0, 0.0, 0.0, 0.0],
        )
    }

    pub fn image(
        &mut self,
        x: f32,
        y: f32,
        image: Image
    ) -> anyhow::Result<()> {
        self.image_tined(x, y, image, Color::rgba(1.0, 1.0, 1.0, 1.0))
    }

    pub fn image_tined(
        &mut self,
        x: f32,
        y: f32,
        image: Image,
        color: Color
    ) -> anyhow::Result<()> {
        match image.storage {
            ImageStorage::Dedicated { texture_handle } => { 
                self.raw_quad_dedicated(
                    texture_handle,
                    [x, y, image.size[0] as f32, image.size[1] as f32],
                    [0.0, 0.0, 1.0, 1.0],
                    color,
                    [0.0, 0.0, 0.0, 3.0],
                )?;

                Ok(())
            }
            ImageStorage::Atlas { layer, uv_min, uv_max } => { 
                self.raw_quad_atlas(
                    [x, y, image.size[0] as f32, image.size[1] as f32],
                    [
                    uv_min[0],
                    uv_min[1],
                    uv_max[0],
                    uv_max[1],
                    ],
                    color,
                    [0.0, 0.0, layer as f32, 3.0],
                )?;

                Ok(())
            }
        }
    }

    pub fn allocate_image_rect(
        &mut self,
        width: u32,
        height: u32,
        padding: u32,
    ) -> anyhow::Result<(u32, u32, u32, u32, u32)> {
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


    pub fn upload_pixels_to_atlas<G: GraphicsDevice>(&self, 
        x: u32, 
        y: u32, 
        width: u32, 
        height: u32, 
        layer: u32,
        pixels: &[u8],
        gpu: &mut G) -> anyhow::Result<()> {
        gpu.write_texture_array_layer(
            self.ui_texture_array, 
            x, y, layer, 
            width, height, 
            pixels)
    }

    pub fn load_image<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<Image> {
        let data = ImageData::load_rgba8(path)?;

        let padding = 1;
        if data.width + padding > UI_ATLAS_WIDTH || 
            data.height + padding > UI_ATLAS_HEIGHT {

                let texture_handle = gpu.create_texture(
                    TextureDesc {
                        width: data.width,
                        height: data.height,
                        format: TextureFormat::Rgba8,
                    },
                    Some(&data.pixels),
                )?;
                gpu.texture_gen_mipmap(texture_handle)?;
                Ok(Image { storage: ImageStorage::Dedicated {
                    texture_handle
                }, size: [data.width, data.height] })

            } else {

                let (layer, x, y, atlas_w, atlas_h) =
                    self.allocate_image_rect(data.width, data.height, padding)?;

                self.upload_pixels_to_atlas(x, y, data.width, data.height, layer, &data.pixels, gpu)?;

                gpu.texture_gen_mipmap(self.ui_texture_array)?;

                Ok(Image {
                    storage: ImageStorage::Atlas {
                        layer,
                        uv_min: [
                            x as f32 / atlas_w as f32,
                            y as f32 / atlas_h as f32,
                        ],
                        uv_max: [
                            (x + data.width) as f32 / atlas_w as f32,
                            (y + data.height) as f32 / atlas_h as f32,
                        ],
                    },
                    size: [data.width, data.height]
                })
        }

    }
}
