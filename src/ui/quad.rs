use bytemuck::{Pod, Zeroable};

use crate::graphics::{
    Color, TextureHandle,
};



#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadVertex {
    pub local_pos: [f32; 2],
    pub local_uv: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadInstance {
    /// x, y, w, h in screen pixels.
    pub rect: [f32; 4],

    /// r, g, b, a.
    pub color: [f32; 4],

    /// uv_min.x, uv_min.y, uv_max.x, uv_max.y.
    pub uv: [f32; 4],

    /// radius, softness, border_width, kind.
    ///
    /// kind:
    /// 0.0 = normal quad
    /// 1.0 = rounded rect
    /// 2.0 = glyph/text
    pub params: [f32; 4],
}

impl QuadInstance {
    pub fn colored(rect: [f32; 4], color: Color) -> Self {
        Self {
            rect: rect, 
            color: [color.r, color.g, color.b, color.a],
            uv: [0.0, 0.0, 1.0, 1.0],
            params: [0.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn textured(
        rect: [f32; 4],
        uv: [f32; 4],
        tint: Color,
        params: [f32; 4],
    ) -> Self {
        Self {
            rect: rect, 
            color: [tint.r, tint.g, tint.b, tint.a],
            uv,
            params, 
        }
    }

    pub fn rounded(rect: [f32; 4], radius: f32, color: Color) -> Self {
        Self {
            rect: rect, 
            color: [color.r, color.g, color.b, color.a],
            uv: [0.0, 0.0, 1.0, 1.0],
            params: [radius, 0.0, 0.0, 1.0],
        }
    }
}

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

pub const QUAD_INDICES: [u32; 6] = [
    0, 1, 2,
    0, 2, 3,
];

pub const UI_QUAD_VERTEX_SHADER: &str = r#"
#version 330 core

layout(location = 0) in vec2 a_local_pos;
layout(location = 1) in vec2 a_local_uv;

layout(location = 2) in vec4 i_rect;
layout(location = 3) in vec4 i_color;
layout(location = 4) in vec4 i_uv;
layout(location = 5) in vec4 i_params;

uniform vec2 u_screen_size;

out vec2 v_local_pos;
out vec2 v_uv;
out vec4 v_color;
out vec4 v_rect;
out vec4 v_params;
flat out int v_layer;
flat out int v_kind;

void main() {
    vec2 pixel_pos = i_rect.xy + a_local_pos * i_rect.zw;

    vec2 ndc = vec2(
        (pixel_pos.x / u_screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / u_screen_size.y) * 2.0
    );

    vec2 uv_min = i_uv.xy;
    vec2 uv_max = i_uv.zw;

    v_local_pos = a_local_pos;
    v_uv = mix(uv_min, uv_max, a_local_uv);
    v_color = i_color;
    v_rect = i_rect;
    v_params = i_params;
    
    v_layer = int(i_params.z + 0.5);
    v_kind = int(v_params.w + 0.5);

    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

pub const UI_QUAD_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec2 v_uv;
in vec4 v_color;

flat in int v_layer;
flat in int v_kind;

uniform sampler2DArray u_texture_array;

out vec4 out_color;

void main() {
    // 0 = solid rect
    // 1 = color emoji or atlas image
    // 2 = text glyph alpha mask
    // 3 = regular atlas image

    if (v_kind == 0) {
        out_color = v_color;
        return;
    }

    vec4 tex_color = texture(u_texture_array, vec3(v_uv, float(v_layer)));

    if (v_kind == 2) {
        // text glyph: use texture alpha only
        out_color = vec4(v_color.rgb, v_color.a * tex_color.a);
    } else {
        // emoji/image: use full RGBA texture
        out_color = tex_color * v_color;
    }
}
"#;

pub const UI_QUAD_FRAGMENT_SHADER_DEDICATED: &str = r#"
#version 330 core

in vec2 v_uv;
in vec4 v_color;

uniform sampler2D u_texture;

out vec4 out_color;

void main() {
    vec4 tex_color = texture(u_texture, v_uv);
    out_color = tex_color * v_color;
}
"#;
