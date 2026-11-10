//! The voxel world: a `VoxelWorldConfig` over a `VoxelMap`, and the palette
//! material that shows each voxel's palette colour flat and unlit.

use std::sync::Arc;

use bevy::{
    asset::uuid_handle,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::{ShaderDefVal, ShaderRef},
};
use bevy_voxel_world::{
    custom_meshing::CHUNK_SIZE_I,
    prelude::*,
    rendering::{vertex_layout, VOXEL_TEXTURE_SHADER_HANDLE},
};
use doom_voxelize::{Voxel, VoxelMap};

/// How far chunks spawn, in DOOM map units: a long DOOM sightline.
const VIEW_DISTANCE_UNITS: i32 = 2048;

/// The voxel world of one DOOM map. Its material index is the palette index.
///
/// `Default` is only there because the crate's config trait needs it: the
/// world is always built with a map.
#[derive(Resource, Clone, Default)]
pub struct DoomWorld {
    map: Option<Arc<VoxelMap>>,
    spawning_distance: u32,
}

/// The spawning distance in chunks: `VIEW_DISTANCE_UNITS`, and at least 8.
fn spawning_distance(units_per_voxel: i32) -> u32 {
    let chunk_units = CHUNK_SIZE_I * units_per_voxel;
    ((VIEW_DISTANCE_UNITS + chunk_units - 1) / chunk_units).max(8) as u32
}

/// How far chunks spawn, in voxels.
pub fn view_distance_voxels(units_per_voxel: i32) -> f32 {
    (spawning_distance(units_per_voxel) as i32 * CHUNK_SIZE_I) as f32
}

impl DoomWorld {
    pub fn new(map: Arc<VoxelMap>, units_per_voxel: i32) -> Self {
        DoomWorld {
            map: Some(map),
            spawning_distance: spawning_distance(units_per_voxel),
        }
    }
}

fn world_voxel(voxel: Voxel) -> WorldVoxel<u8> {
    match voxel {
        Voxel::Air => WorldVoxel::Air,
        Voxel::Solid(index) => WorldVoxel::Solid(index),
    }
}

impl VoxelWorldConfig for DoomWorld {
    type MaterialIndex = u8;
    type ChunkUserBundle = ();

    fn spawning_distance(&self) -> u32 {
        self.spawning_distance
    }

    // Turning around shouldn't drop and regenerate the room behind the camera.
    fn chunk_despawn_strategy(&self) -> ChunkDespawnStrategy {
        ChunkDespawnStrategy::FarAway
    }

    fn chunk_fill_hint(&self, key: ChunkKey) -> FillHint<u8> {
        let Some(map) = &self.map else {
            return FillHint::Unknown;
        };
        let min = key.min_voxel();
        let max = min + IVec3::splat(key.voxel_extent());
        match map.uniform_in(min.to_array(), max.to_array()) {
            Some(voxel) => FillHint::Uniform(world_voxel(voxel)),
            None => FillHint::Unknown,
        }
    }

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<u8> {
        let map = self.map.clone().expect("the world is built with a map");
        Box::new(move |chunk_position, lod_level, _previous| {
            // The chunk's voxels and the one-voxel padding the mesher samples.
            let min = chunk_position * CHUNK_SIZE_I - IVec3::ONE;
            let max = min + IVec3::splat((CHUNK_SIZE_I << lod_level) + 2);
            if let Some(voxel) = map.uniform_in(min.to_array(), max.to_array()) {
                let voxel = world_voxel(voxel);
                return Box::new(move |_, _| voxel);
            }
            let map = Arc::clone(&map);
            Box::new(move |position: IVec3, _previous| {
                world_voxel(map.voxel(position.to_array()))
            })
        })
    }

    fn texture_index_mapper(&self) -> Arc<dyn Fn(u8) -> [u32; 3] + Send + Sync> {
        Arc::new(|index| [index as u32; 3])
    }
}

const PALETTE_SHADER: Handle<Shader> =
    uuid_handle!("6d1f0a52-3c47-4e8b-9a61-0f2b7d94c3e5");

/// Shows each voxel in its palette colour, times the mesher's ambient
/// occlusion, with no lighting.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PaletteMaterial {
    /// The palette in linear RGB, so the sRGB output is the palette's bytes.
    #[uniform(0)]
    palette: [Vec4; 256],
}

impl PaletteMaterial {
    pub fn new(palette: &[[u8; 3]; 256]) -> Self {
        PaletteMaterial {
            palette: palette
                .map(|[r, g, b]| Color::srgb_u8(r, g, b).to_linear().to_vec4()),
        }
    }
}

impl Material for PaletteMaterial {
    fn vertex_shader() -> ShaderRef {
        VOXEL_TEXTURE_SHADER_HANDLE.into()
    }

    fn fragment_shader() -> ShaderRef {
        PALETTE_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if descriptor
            .vertex
            .shader_defs
            .contains(&ShaderDefVal::Bool("PREPASS_PIPELINE".into(), true))
        {
            return Ok(());
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&vertex_layout())?];
        Ok(())
    }
}

/// Adds the voxel world of `map`, shown in its palette.
pub struct DoomWorldPlugin {
    pub map: Arc<VoxelMap>,
    pub units_per_voxel: i32,
}

impl Plugin for DoomWorldPlugin {
    fn build(&self, app: &mut App) {
        let shader = Shader::from_wesl(
            include_str!("palette.wesl"),
            "embedded://doom_voxels/palette.wesl",
        );
        app.world_mut()
            .resource_mut::<Assets<Shader>>()
            .insert(PALETTE_SHADER.id(), shader)
            .expect("the palette shader's handle is free");
        app.add_plugins(MaterialPlugin::<PaletteMaterial>::default())
            .add_plugins(
                VoxelWorldPlugin::with_config(DoomWorld::new(
                    Arc::clone(&self.map),
                    self.units_per_voxel,
                ))
                .with_material(PaletteMaterial::new(self.map.palette())),
            );
    }
}
