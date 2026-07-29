#version 450

layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec4 v_color;

layout(set = 0, binding = 0) uniform sampler2D u_texture;

layout(location = 0) out vec4 out_color;

void main() {
    vec4 tex_color = texture(u_texture, v_uv);
    out_color = tex_color * v_color;
}
