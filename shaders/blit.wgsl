#include blit_common.wgsl

var<push_constant> view_index: u32;
@fragment
fn blit_fs_main(in: BlitVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(blit_texture, blit_sampler, in.uv_coords, i32(view_index));
}