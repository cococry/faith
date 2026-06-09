use bytemuck::{Pod, Zeroable};

use crate::graphics::Color;

/// Represents one vertex of the shared UI quad.
///
/// The local position and UV are expanded by
/// per-instance data in the UI vertex shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadVertex {
    pub local_pos: [f32; 2],
    pub local_uv: [f32; 2],
}

/// Represents one renderable UI quad instance.
///
/// Stores the screen-space rectangle, color,
/// UV coordinates and shader parameters used
/// to render one quad.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadInstance {
    pub rect: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 4],

    // Extra shader parameters.
    //
    // Currently used for atlas layer and
    // QuadType information.
    pub params: [f32; 4],
}

impl QuadInstance {
    /// Creates a textured quad instance using
    /// the given rectangle, UV coordinates,
    /// tint color and shader parameters.
    pub fn textured(rect: [f32; 4], uv: [f32; 4], tint: Color, params: [f32; 4]) -> Self {
        Self {
            rect,
            color: [tint.r, tint.g, tint.b, tint.a],
            uv,
            params,
        }
    }
}

/// Shared unit quad vertices used by all UI
/// quad instances.
pub const QUAD_VERTICES: [QuadVertex; 4] = [
    QuadVertex {
        local_pos: [0.0, 0.0],
        local_uv: [0.0, 0.0],
    },
    QuadVertex {
        local_pos: [1.0, 0.0],
        local_uv: [1.0, 0.0],
    },
    QuadVertex {
        local_pos: [1.0, 1.0],
        local_uv: [1.0, 1.0],
    },
    QuadVertex {
        local_pos: [0.0, 1.0],
        local_uv: [0.0, 1.0],
    },
];

/// Indices for rendering the shared unit quad
/// as two triangles.
pub const QUAD_INDICES: [u32; 6] = [0, 1, 2, 0, 2, 3];
