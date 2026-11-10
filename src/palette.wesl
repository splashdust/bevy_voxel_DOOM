// Each voxel face in its palette colour, times the mesher's ambient
// occlusion, unlit. The texture index mapper passes the palette index.
//
// In a lit world the crate adds the `VOXEL_LIGHT` shader def, and the colour
// is scaled by a brightness made of the face's sky light, the voxel light
// around it, and its glow if it is a lamp's face, capped at 1 so a fully lit
// face shows the palette colour itself. The look's `minimum_brightness`
// floors the sky part, so unlit places aren't pure black. There is no PBR:
// the crate's sun gate and Bevy's ambient light play no part.

import bevy_pbr::render::forward_io::FragmentOutput;
import bevy_voxel_world::voxel_io::VoxelVertexOutput;

@if(VOXEL_LIGHT)
import bevy_voxel_world::light::{sample_light, sky_brightness, voxel_brightness};

@group(constants::MATERIAL_BIND_GROUP) @binding(0)
var<uniform> palette: array<vec4<f32>, 256>;

@fragment
fn fragment(in: VoxelVertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    var colour = palette[in.tex_idx[0]].rgb * in.color.rgb;
    @if(VOXEL_LIGHT) {
        let levels = sample_light(in.light_tag, in.local_position, in.local_normal);
        // `voxel_brightness` holds the light around the face and the face's
        // own glow, times the look's voxel brightness and the exposure.
        let light = vec3<f32>(sky_brightness(levels.sky)) + voxel_brightness(levels);
        colour = colour * min(light, vec3<f32>(1.0));
    }
    out.color = vec4<f32>(colour, 1.0);
    return out;
}
