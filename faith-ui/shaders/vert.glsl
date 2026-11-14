#version 450

layout(location = 0) in vec2 a_local_pos;
layout(location = 1) in vec2 a_local_uv;

layout(location = 2) in vec4 i_rect;
layout(location = 3) in vec4 i_color;
layout(location = 4) in vec4 i_uv;
layout(location = 5) in vec4 i_params;
layout(location = 6) in vec4 i_clip;

layout(push_constant) uniform PushConstants {
    vec2 u_screen_size;
} pc;

layout(location = 0) out vec2 v_local_pos;
layout(location = 1) out vec2 v_uv;
layout(location = 2) out vec4 v_color;
layout(location = 3) out vec4 v_rect;
layout(location = 4) out vec4 v_params;
layout(location = 5) flat out int v_layer;
layout(location = 6) flat out int v_kind;

void main() {
    // Trim geometry before rasterization. UVs remain relative to the original
    // quad, so clipping never stretches glyphs or images. Empty quads degenerate.
    vec2 clip_min = max(i_rect.xy, i_clip.xy);
    vec2 clip_max = max(clip_min, min(i_rect.xy + i_rect.zw, i_clip.zw));
    vec2 pixel_pos = mix(clip_min, clip_max, a_local_pos);
    vec2 clipped_local = (pixel_pos - i_rect.xy) / max(i_rect.zw, vec2(0.000001));

    vec2 ndc = vec2(
        (pixel_pos.x / pc.u_screen_size.x) * 2.0 - 1.0,
        (pixel_pos.y / pc.u_screen_size.y) * 2.0 - 1.0
        );

    vec2 uv_min = i_uv.xy;
    vec2 uv_max = i_uv.zw;

    v_local_pos = clipped_local;
    v_uv = mix(uv_min, uv_max, clipped_local);
    v_color = i_color;
    v_rect = i_rect;
    v_params = i_params;

    v_layer = int(i_params.z + 0.5);
    v_kind = int(i_params.w + 0.5);

    gl_Position = vec4(ndc, 0.0, 1.0);
}
