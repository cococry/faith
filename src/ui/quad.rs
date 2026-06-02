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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Solid,
    Textured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchKey {
    pub texture: Option<TextureHandle>,
    pub kind: BatchKind
}


#[derive(Debug, Clone, Copy)]
pub struct QuadBatch {
    pub inst_start: u32,
    pub inst_count: u32,
    pub key: BatchKey,
}

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

    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

pub const UI_QUAD_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec2 v_local_pos;
in vec2 v_uv;
in vec4 v_color;
in vec4 v_rect;
in vec4 v_params;

uniform sampler2DArray u_glpyh_texture_array;
uniform sampler2DArray u_ui_texture_array;

flat in int v_layer;

out vec4 out_color;

void main() {
    float kind = v_params.w;

    if (kind > 2.5) {
        // image
        vec4 tex_color = texture(u_ui_texture_array, vec3(v_uv, float(v_layer)));
        out_color = tex_color * v_color;
    } else if (kind > 1.5) {
        // text glyph 
        vec4 tex_color = texture(u_glpyh_texture_array, vec3(v_uv, float(v_layer)));
        float alpha = tex_color.a;
        out_color = vec4(v_color.rgb, v_color.a * alpha);
    } else if (kind > 0.5) {
        // color emoji
        vec4 tex_color = texture(u_glpyh_texture_array, vec3(v_uv, float(v_layer)));
        out_color = tex_color * v_color;
    } else {
        out_color = v_color;
    }
}
"#;
