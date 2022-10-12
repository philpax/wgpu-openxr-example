#include blit_common.wgsl

@fragment
fn blit_fs_main(
    in: BlitVertexOutput,
    @builtin(view_index) view_index: i32
) -> @location(0) vec4<f32> {
    return textureSample(blit_texture, blit_sampler, in.uv_coords, i32(view_index));
}