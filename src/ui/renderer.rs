
use crate::Color;

use crate::graphics::{
    BufferDesc,
    BufferHandle,
    BufferTarget,
    BufferUsage,
    GraphicsDevice,
    PipelineDesc,
    PipelineHandle,
    VertexStepMode,
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
    QuadBatch,
    UI_QUAD_FRAGMENT_SHADER,
    UI_QUAD_VERTEX_SHADER,
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

        gpu.write_buffer(self.instance_vbo, 0, 0, instance_bytes)?;

        gpu.set_pipeline(self.pipeline)?;
        gpu.set_uniform_2f(self.pipeline, "u_screen_size", 
            self.screen_width as f32, self.screen_height as f32)?;

        gpu.set_vertex_buffer(self.quad_vbo,    0)?;
        gpu.set_vertex_buffer(self.instance_vbo, 1)?;
        gpu.set_index_buffer(self.quad_ibo)?;

        if self.batch_count != 0 {
            let off = self.batches.len() as u32 * self.max_instances as u32; 

            self.batches.push(
                QuadBatch { 
                    inst_start: off, 
                    inst_count: self.batch_count as u32
                });
        }

        for batch in &self.batches {
            gpu.draw_indexed_instanced(DrawIndexedInstanced {
                index_count: 6, 
                index_offset: 0, 
                vertex_offset: 0, 
                inst_count: batch.inst_count, 
                inst_offset: batch.inst_start, 
            })?;
        }

        Ok(())
    }

    pub fn quad(&mut self, rect: [f32; 4], color: Color) -> anyhow::Result<()> {
        /*(if self.batch_count >= self.max_instances {
          let off = self.batches.len() as u32 * self.max_instances as u32;
          self.batches.push(
          QuadBatch { 
          inst_start: off, 
          inst_count: self.max_instances as u32
          });
          self.batch_count = 0;
          }*/
        if self.instances.len() >= self.max_instances {
            anyhow::bail!("UI instance buffer overflow");
        }
        self.instances.push(QuadInstance::colored(rect, color));

        self.batch_count += 1;

        Ok(())

    }
}
