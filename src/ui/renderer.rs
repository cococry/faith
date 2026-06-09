use crate::Color;

use crate::graphics::{
    BufferDesc, BufferHandle, BufferTarget, BufferUsage, BuiltinShaderPipeline, GraphicsDevice,
    ImageData, PipelineDesc, PipelineHandle, TextureArrayWrite, TextureDesc, TextureFormat,
    TextureHandle, VertexStepMode,
};

use crate::graphics::device::{
    DrawIndexedInstanced, TextureArrayDesc, UniformBinding, UniformBindingShaderStage,
    UniformBindingType, VertexAttribute, VertexBufferBindingLayout, VertexFormat,
};

use crate::ui::{QUAD_INDICES, QUAD_VERTICES, QuadInstance, QuadVertex};

/// Represents where an image is stored
/// for rendering by UIRenderer.
///
/// Atlas images are stored in
/// UIRenderer.ui_texture_array.
///
/// Dedicated images are stored in their
/// own texture and rendered through the
/// dedicated image pipeline.
///
/// Dedicated images exist to avoid creating an
/// extremely large atlas texture array.
/// They are used when a loaded image
/// (through UIRenderer.image()) does not fit
/// into the atlas dimensions (
/// UI_ATLAS_WIDTH x UI_ATLAS_HEIGHT).
#[derive(Debug, Clone, Copy)]
pub enum ImageStorage {
    Atlas {
        layer: u32,
        uv_min: [f32; 2],
        uv_max: [f32; 2],
    },
    Dedicated {
        texture_handle: TextureHandle,
    },
}

/// Represents an image that can be rendered
/// by UIRenderer.
///
/// The texture for the image can either be
/// stored inside the shared UI atlas texture
/// array or in a dedicated GPU texture.
#[derive(Debug, Clone, Copy)]
pub struct Image {
    pub storage: ImageStorage,
    pub size: [u32; 2],
}

/// Represents one layer in the image atlas
/// texture array.
///
/// Uses a simple row-based allocator to place
/// uploaded images into the layer.
pub struct ImageAtlasLayer {
    width: u32,
    height: u32,
    layer: u32,

    // Current row height used by the atlas
    // allocator when advancing to the next row.
    row_h: u32,

    // Current insertion cursor used by the
    // atlas allocator.
    cursor_x: u32,
    cursor_y: u32,
}

/// Represents one instanced draw call for a
/// sequence of quads that all use the same
/// dedicated texture.
pub struct DedicatedDraw {
    texture_handle: TextureHandle,
    start: u32,
    count: u32,
}

/// UI rendering engine.
///
/// Manages UI quad pipelines, vertex/index/
/// instance buffers, image atlas storage,
/// dedicated image draws and per-frame
/// quad instance batching.
pub struct UIRenderer {
    screen_width: u32,
    screen_height: u32,

    pipeline: PipelineHandle,
    pipeline_dedicated: PipelineHandle,

    quad_vbo: BufferHandle,
    quad_ibo: BufferHandle,
    instance_vbo: BufferHandle,

    // Temporary combined instance buffer used
    // when both atlas and dedicated quads need
    // to be uploaded in one frame.
    instances: Vec<QuadInstance>,

    atlas_instances: Vec<QuadInstance>,
    dedicated_instances: Vec<QuadInstance>,

    max_instances: usize,

    // Shared texture array used for UI images,
    // glyphs and atlas-backed quads.
    ui_texture_array: TextureHandle,

    image_atlas_layers: Vec<ImageAtlasLayer>,

    // Batched draw ranges for dedicated
    // texture-backed images.
    dedicated_draws: Vec<DedicatedDraw>,
}

/// Type of quad rendered by the UI shader.
///
/// Passed to the shader as a float through
/// QuadInstance.params.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadType {
    Solid = 0,
    TextGlyph = 1,
    ColoredImage = 2,
}

impl QuadType {
    pub fn as_f32(self) -> f32 {
        self as u32 as f32
    }
}

const UI_ATLAS_WIDTH: u32 = 2048;
const UI_ATLAS_HEIGHT: u32 = 2048;
const UI_ATLAS_LAYERS: u32 = 32;
const MAX_INSTANCES: usize = 65_536;
const MAX_INSTANCES_PER_FRAME: usize = 65_536 * 8;

impl UIRenderer {
    /// Creates a new UI renderer and initializes
    /// the GPU resources needed for rendering
    /// UI quads.
    ///
    /// Creates the shared quad vertex/index
    /// buffers, the dynamic instance buffer,
    /// atlas- and dedicated-image-pipelines and
    /// the UI atlas texture array.
    pub fn new<G: GraphicsDevice>(
        gpu: &mut G,
        screen_width: u32,
        screen_height: u32,
    ) -> anyhow::Result<Self> {
        let uniform_bindings = vec![
            UniformBinding {
                name: "u_texture_array".to_string(),
                ty: UniformBindingType::Sampler2dArray,
                binding: 0,
                stage: UniformBindingShaderStage::Fragment,
                ..Default::default()
            },
            UniformBinding {
                name: "u_screen_size".to_string(),
                ty: UniformBindingType::Vec2,
                f_data: [screen_width as f32, screen_height as f32, 0.0, 0.0],
                ..Default::default()
            },
        ];

        let quad_layouts = vec![
            VertexBufferBindingLayout {
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
                    },
                ],
            },
            VertexBufferBindingLayout {
                binding: 1,
                stride: std::mem::size_of::<QuadInstance>() as u32,
                step_mode: VertexStepMode::Instance,
                attrs: vec![
                    // Rect position
                    VertexAttribute {
                        location: 2,
                        offset: 0,
                        format: VertexFormat::Float32x4,
                    },
                    // Color
                    VertexAttribute {
                        location: 3,
                        offset: 16,
                        format: VertexFormat::Float32x4,
                    },
                    // UV
                    VertexAttribute {
                        location: 4,
                        offset: 32,
                        format: VertexFormat::Float32x4,
                    },
                    // Params
                    // Currently, only params.w (params[3])
                    // is used by the shader to determine
                    // quad render type in fragment shader.
                    VertexAttribute {
                        location: 5,
                        offset: 48,
                        format: VertexFormat::Float32x4,
                    },
                ],
            },
        ];

        // Create base pipeline which is using the
        // texture array to sample.
        let pipeline = gpu.create_pipeline(PipelineDesc {
            shader: BuiltinShaderPipeline::UiQuadAtlas,
            vert_bindings: quad_layouts.clone(),
            uniform_bindings: uniform_bindings.clone(),
        })?;

        // Create the static vertex buffer for one
        // quad instance
        let quad_vbo = gpu.create_buffer(BufferDesc {
            target: BufferTarget::Vertex,
            usage: BufferUsage::Static,
            size: std::mem::size_of_val(&QUAD_VERTICES),
            data: Some(bytemuck::cast_slice(&QUAD_VERTICES)),
        })?;

        // Create the static index buffer for one
        // quad instance
        let quad_ibo = gpu.create_buffer(BufferDesc {
            target: BufferTarget::Index,
            usage: BufferUsage::Static,
            size: std::mem::size_of_val(&QUAD_INDICES),
            data: Some(bytemuck::cast_slice(&QUAD_INDICES)),
        })?;

        // Create the dynamic instance buffer holding all
        // instanes to be rendered in one frame.
        let instance_vbo = gpu.create_buffer(BufferDesc {
            target: BufferTarget::Vertex,
            usage: BufferUsage::Dynamic,
            size: MAX_INSTANCES_PER_FRAME * std::mem::size_of::<QuadInstance>(),
            data: None,
        })?;

        // Create dedicated pipeline which is using
        // individual textures in batched draw calls
        // to sample.
        let pipeline_dedicated = gpu.create_pipeline(PipelineDesc {
            shader: BuiltinShaderPipeline::UiQuadDedicated,
            vert_bindings: quad_layouts,
            uniform_bindings,
        })?;

        // Create the texture array for the base pipeline
        let ui_texture_array = gpu.create_texture_array(TextureArrayDesc {
            width: UI_ATLAS_WIDTH,
            height: UI_ATLAS_HEIGHT,
            layers: UI_ATLAS_LAYERS,
            format: TextureFormat::Rgba8,
        })?;

        Ok(Self {
            screen_width,
            screen_height,
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

    /// Begins a new UI frame.
    ///
    /// Updates the current screen size and clears
    /// all per-frame quad instance batches.
    pub fn begin(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;

        self.instances.clear();
        self.atlas_instances.clear();
        self.dedicated_instances.clear();
        self.dedicated_draws.clear();
    }

    /// Ends the current UI frame and submits all
    /// submitted quad instances to the GPU.
    ///
    /// Uploads the instance data, binds the
    /// required buffers and renders dedicated
    /// atlas texture array-backed quads,
    /// then optionally the dedicated texture quads.
    pub fn end<G: GraphicsDevice>(&mut self, gpu: &mut G) -> anyhow::Result<()> {
        if self.atlas_instances.is_empty() && self.dedicated_instances.is_empty() {
            return Ok(());
        }

        // If there are no dedicated-texture instances to
        // be rendered, do not copy self.atlas_instances
        // to self.instances to render.
        // Instead, render with self.atlas_instances directly.
        let (instances_to_upload, atlas_start) = if self.dedicated_instances.is_empty() {
            (self.atlas_instances.as_slice(), 0)
        } else {
            self.instances.extend_from_slice(&self.dedicated_instances);

            // Atlas-based instances start after
            // dedicated-texture instances.
            let atlas_start = self.instances.len() as u32;

            self.instances.extend_from_slice(&self.atlas_instances);

            (self.instances.as_slice(), atlas_start)
        };

        // Upload merged instances
        let instance_bytes = bytemuck::cast_slice(instances_to_upload);
        gpu.write_buffer(self.instance_vbo, 1, 0, instance_bytes)?;

        gpu.set_vertex_buffer(self.quad_vbo, 0)?;
        gpu.set_vertex_buffer(self.instance_vbo, 1)?;
        gpu.set_index_buffer(self.quad_ibo)?;

        // Render dedicated batches
        if !self.dedicated_instances.is_empty() {
            gpu.set_pipeline(self.pipeline_dedicated)?;
            gpu.set_uniform_2f(
                self.pipeline_dedicated,
                "u_screen_size",
                self.screen_width as f32,
                self.screen_height as f32,
            )?;

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

        // Render atlas-based instances (in one drawcall)
        if !self.atlas_instances.is_empty() {
            gpu.set_pipeline(self.pipeline)?;
            gpu.set_uniform_2f(
                self.pipeline,
                "u_screen_size",
                self.screen_width as f32,
                self.screen_height as f32,
            )?;

            gpu.set_texture(0, self.ui_texture_array)?;

            gpu.draw_indexed_instanced(DrawIndexedInstanced {
                index_count: 6,
                index_offset: 0,
                vertex_offset: 0,
                inst_count: self.atlas_instances.len() as u32,
                inst_offset: atlas_start,
            })?;
        }

        Ok(())
    }

    /// Submits an atlas-based quad to be
    /// rendered this frame.
    ///
    /// Used for solid quads, text glyphs (including emojis)
    /// and texture-based images.
    pub fn raw_quad_atlas(
        &mut self,
        rect: [f32; 4],
        uv: [f32; 4],
        color: Color,
        params: [f32; 4],
    ) -> anyhow::Result<()> {
        let total_instances = self.atlas_instances.len() + self.dedicated_instances.len();

        if total_instances >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }

        self.atlas_instances
            .push(QuadInstance::textured(rect, uv, color, params));

        Ok(())
    }

    /// Submits a dedicated texture-based quad
    /// to be rendered this frame.
    ///
    /// Consecutive quads using the same dedicated
    /// texture are batched into one draw range.
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
            Some(draw) if draw.texture_handle == texture_handle => {
                // If the last draw used the same
                // dedicated texture, append quad
                // to the last draw.
                draw.count += 1;
            }
            _ => {
                // If the last draw's and this quad's
                // textures don't match, push a new
                // dedicated texture batch draw.
                self.dedicated_draws.push(DedicatedDraw {
                    start,
                    texture_handle,
                    count: 1,
                });
            }
        }

        Ok(())
    }

    /// Submits a solid-colored quad to be rendered
    /// this frame.
    #[allow(dead_code)]
    pub fn quad(&mut self, rect: [f32; 4], color: Color) -> anyhow::Result<()> {
        self.raw_quad_atlas(
            rect,
            [0.0, 0.0, 1.0, 1.0],
            color,
            [0.0, 0.0, 0.0, QuadType::Solid.as_f32()],
        )
    }

    /// Submits an image to be rendered this frame
    /// at the given position.
    ///
    /// The image is rendered using its original
    /// loaded size and full white tint.
    pub fn image(&mut self, x: f32, y: f32, w: f32, h: f32, image: Image) -> anyhow::Result<()> {
        self.image_tined(x, y, w, h, image, Color::rgba(1.0, 1.0, 1.0, 1.0))
    }

    /// Submits a tinted image to be rendered
    /// this frame at the given position.
    ///
    /// The image is rendered using its original
    /// loaded size.
    pub fn image_tined(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image: Image,
        color: Color,
    ) -> anyhow::Result<()> {
        match image.storage {
            ImageStorage::Dedicated { texture_handle } => {
                self.raw_quad_dedicated(
                    texture_handle,
                    [x, y, w, h],
                    [0.0, 0.0, 1.0, 1.0],
                    color,
                    [0.0, 0.0, 0.0, 0.0],
                )?;

                Ok(())
            }
            ImageStorage::Atlas {
                layer,
                uv_min,
                uv_max,
            } => {
                self.raw_quad_atlas(
                    [x, y, w, h],
                    [uv_min[0], uv_min[1], uv_max[0], uv_max[1]],
                    color,
                    [0.0, 0.0, layer as f32, QuadType::ColoredImage.as_f32()],
                )?;

                Ok(())
            }
        }
    }

    /// Allocates a rectangle inside the UI image
    /// atlas texture array.
    ///
    /// Uses a simple row-based allocator and creates
    /// a new atlas layer when the current layer is
    /// full.
    pub fn allocate_image_rect(
        &mut self,
        width: u32,
        height: u32,
        padding: u32,
    ) -> anyhow::Result<(u32, u32, u32, u32, u32)> {
        // If no atlas layer exists yet, create the
        // first layer.
        if self.image_atlas_layers.is_empty() {
            self.create_new_image_layer()?;
        }

        // Use the most recent layer to allocate the
        // rectangle into.
        let mut layer_idx = self.image_atlas_layers.len() - 1;

        {
            let layer = &mut self.image_atlas_layers[layer_idx];

            // Current row is full -> go to next row
            if layer.cursor_x + width + padding > layer.width {
                layer.cursor_x = 0;
                layer.cursor_y += layer.row_h + padding;
                layer.row_h = 0;
            }

            // Current atlas layer is full -> allocate new layer
            if layer.cursor_y + height + padding > layer.height {
                self.create_new_image_layer()?;
                layer_idx = self.image_atlas_layers.len() - 1;
            }
        }

        let layer = &mut self.image_atlas_layers[layer_idx];

        let x = layer.cursor_x;
        let y = layer.cursor_y;
        let layer_id = layer.layer;

        // Update current layer's allocation cursors
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

    /// Uploads pixel data into the UI atlas
    /// texture array at the given layer and
    /// position.
    pub fn upload_pixels_to_atlas<G: GraphicsDevice>(
        &self,
        pos_size: [u32; 4],
        layer: u32,
        pixels: &[u8],
        gpu: &mut G,
    ) -> anyhow::Result<()> {
        // Write to the GPU texture array at the
        // given layer
        gpu.write_texture_array_layer(
            TextureArrayWrite {
                texture: self.ui_texture_array,
                x: pos_size[0],
                y: pos_size[1],
                layer,
                width: pos_size[2],
                height: pos_size[3],
            },
            pixels,
        )
    }

    /// Loads an image from disk and uploads it
    /// to the GPU.
    ///
    /// Images that fit inside the UI atlas are
    /// uploaded into the shared atlas texture
    /// array. Larger images are stored in a
    /// dedicated texture.
    pub fn load_image<G: GraphicsDevice>(
        &mut self,
        gpu: &mut G,
        path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<Image> {
        let data = ImageData::load_rgba8(path)?;

        // Padding is needed to avoid bleeding and
        // sample issues with glyphs and emojis.
        let padding = 1;

        if data.width + padding > UI_ATLAS_WIDTH || data.height + padding > UI_ATLAS_HEIGHT {
            // If image does not fit into atlas,
            // create dedicated texture for image.

            let texture_handle = gpu.create_texture(
                TextureDesc {
                    width: data.width,
                    height: data.height,
                    format: TextureFormat::Rgba8,
                },
                Some(&data.pixels),
            )?;
            Ok(Image {
                storage: ImageStorage::Dedicated { texture_handle },
                size: [data.width, data.height],
            })
        } else {
            // If image fits into an atlas page,
            // allocate rectangle for the image.
            let (layer, x, y, atlas_w, atlas_h) =
                self.allocate_image_rect(data.width, data.height, padding)?;

            self.upload_pixels_to_atlas([x, y, data.width, data.height], layer, &data.pixels, gpu)?;

            Ok(Image {
                storage: ImageStorage::Atlas {
                    layer,
                    uv_min: [x as f32 / atlas_w as f32, y as f32 / atlas_h as f32],
                    uv_max: [
                        (x + data.width) as f32 / atlas_w as f32,
                        (y + data.height) as f32 / atlas_h as f32,
                    ],
                },
                size: [data.width, data.height],
            })
        }
    }
}
