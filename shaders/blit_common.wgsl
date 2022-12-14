struct BlitVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv_coords: vec2<f32>,
}

struct BlitVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv_coords: vec2<f32>,
}

@group(0) @binding(0)
var blit_texture: texture_2d_array<f32>;
@group(0) @binding(1)
var blit_sampler: sampler;

@vertex
fn blit_vs_main(model: BlitVertexInput) -> BlitVertexOutput {
    var out: BlitVertexOutput;
    out.position = vec4<f32>(model.position, 1.0);
    out.uv_coords = model.uv_coords;
    return out;
}