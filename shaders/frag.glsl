#version 450

layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec4 v_color;

layout(location = 5) flat in int v_layer;
layout(location = 6) flat in int v_kind;

layout(set = 0, binding = 0) uniform sampler2DArray u_texture_array;

layout(location = 0) out vec4 out_color;

void main() {
    // 0 = solid rect
    // 1 = text glyph alpha mask
    // 2 = emoji/atlas image

    if (v_kind == 0) {
        out_color = v_color;
        return;
    }

    vec4 tex_color = texture(u_texture_array, vec3(v_uv, float(v_layer)));

    if (v_kind == 1) {
        // text glyph: use texture alpha only
        out_color = vec4(v_color.rgb, v_color.a * tex_color.a);
    } else {
        // emoji/image: use full RGBA texture
        out_color = tex_color * v_color;
    }
}
