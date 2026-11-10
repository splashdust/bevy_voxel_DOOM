// Each voxel face in its palette colour, times the mesher's ambient
// occlusion, unlit. The texture index mapper passes the palette index.

import bevy_pbr::render::forward_io::FragmentOutput;
import bevy_voxel_world::voxel_io::VoxelVertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0)
var<uniform> palette: array<vec4<f32>, 256>;

@fragment
fn fragment(in: VoxelVertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    let colour = palette[in.tex_idx[0]];
    out.color = vec4<f32>(colour.rgb * in.color.rgb, 1.0);
    return out;
}
