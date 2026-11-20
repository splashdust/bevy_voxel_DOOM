// Each voxel face in its palette colour, times the mesher's ambient
// occlusion, unlit. The texture index mapper passes the palette index.
//
// In a lit world the crate adds the `VOXEL_LIGHT` shader def, and the colour
// is scaled by a brightness made of the face's sky light, the lamp light in
// front of it, and its glow if its own voxel is a lamp at `glow_from` or
// more, capped at 1 so a fully lit face shows the palette colour itself. With
// `in_srgb` the brightness scales the colour's sRGB value, as COLORMAP does,
// instead of its linear value. The
// look's `minimum_brightness` floors the sky part, so unlit places needn't be
// pure black. Last, `gain` scales the sRGB value for the map's contrast.
// There is no PBR: the crate's sun gate, its voxel brightness and
// Bevy's ambient light play no part.

import bevy_pbr::render::forward_io::FragmentOutput;
import bevy_voxel_world::voxel_io::VoxelVertexOutput;

@if(VOXEL_LIGHT)
import bevy_voxel_world::light::{sample_face_light, sky_brightness, voxel_falloff};

struct ShaderLight {
    lamp_gain: f32,
    glow_from: f32,
    in_srgb: f32,
    gain: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0)
var<uniform> palette: array<vec4<f32>, 256>;

@group(constants::MATERIAL_BIND_GROUP) @binding(1)
var<uniform> light: ShaderLight;

// How many times lamp light at level 15 a lamp's own face shows, as the
// crate's `emissive_glow`: capped at 1, so any lamp face shows full-bright.
const LAMP_GLOW: f32 = 6.0;

fn to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}

fn to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: VoxelVertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    var colour = palette[in.tex_idx[0]].rgb * in.color.rgb;
    @if(VOXEL_LIGHT) {
        // A sub-grid's face (a door, a lift) also reads the world's light
        // where it is now.
        @if(SUB_GRIDS)
        let grid_position = in.world_grid_position;
        @if(!SUB_GRIDS)
        let grid_position = in.world_position.xyz;
        let levels = sample_face_light(
            in.light_tag,
            in.local_position,
            in.local_normal,
            grid_position,
            in.world_normal,
        );
        var lamp = voxel_falloff(levels.voxel);
        if levels.own_voxel.x >= light.glow_from {
            lamp += LAMP_GLOW * voxel_falloff(levels.own_voxel);
        }
        let brightness = vec3<f32>(sky_brightness(levels.sky)) + lamp * light.lamp_gain;
        let scale = min(brightness, vec3<f32>(1.0));
        if light.in_srgb > 0.5 {
            colour = to_linear(to_srgb(colour) * scale);
        } else {
            colour = colour * scale;
        }
    }
    if light.gain != 1.0 {
        colour = to_linear(to_srgb(colour) * light.gain);
    }
    out.color = vec4<f32>(colour, 1.0);
    return out;
}
