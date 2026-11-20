//! The voxel world: a `VoxelWorldConfig` over a `VoxelMap`, and the palette
//! material that shows each voxel's palette colour flat and unlit, times
//! propagated light in the light modes that use it.

use std::sync::Arc;

use bevy::{
    asset::uuid_handle,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    platform::collections::HashSet,
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::{ShaderDefVal, ShaderRef},
};
use bevy_voxel_world::{
    custom_meshing::CHUNK_SIZE_I,
    prelude::*,
    rendering::{vertex_layout, VOXEL_TEXTURE_SHADER_HANDLE},
};
use doom_voxelize::{Voxel, VoxelMap};

use crate::args::{LightKnobs, LightMode};
use crate::collide::Solid;
use crate::movers::MoverSolids;

/// How far chunks spawn, in DOOM map units: a long DOOM sightline.
const VIEW_DISTANCE_UNITS: i32 = 2048;

/// The voxel world of one DOOM map. Its material index holds the palette
/// index in its low byte and, for an emissive voxel, the level it gives off
/// light at in bits 8 to 11: see [`material`].
///
/// `Default` is only there because the crate's config trait needs it: the
/// world is always built with a map.
#[derive(Resource, Clone, Default)]
pub struct DoomWorld {
    map: Option<Arc<VoxelMap>>,
    spawning_distance: u32,
    /// `Some` in the light modes that propagate light.
    light: Option<LightPropagation>,
    /// The map's emissive materials and the shot flash's, with light on.
    emissive: Vec<(u16, Emission)>,
}

/// The palette colour of the shot flash: a hot yellow white.
const FLASH_RGB: [u8; 3] = [255, 236, 160];

/// The material of a shot's flash in `map`'s palette, glowing at level 15.
pub fn flash_material(map: &VoxelMap) -> u16 {
    material(map.nearest_colour(FLASH_RGB), 15)
}

/// The material index of a voxel of palette colour `index`, giving off light
/// at `level`, 0 for none.
pub fn material(index: u8, level: u8) -> u16 {
    index as u16 | ((level as u16 & 15) << 8)
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
    pub fn new(
        map: Arc<VoxelMap>,
        units_per_voxel: i32,
        mode: LightMode,
        knobs: &LightKnobs,
    ) -> Self {
        let flash = flash_material(&map);
        let light = mode.propagated().then(|| light_settings(&map, mode, knobs));
        let emissive = if light.is_some() {
            map.emissive_materials()
                .iter()
                .map(|&(index, level)| {
                    (material(index, level), Emission::level(level as f32))
                })
                .chain([(flash, Emission::level(15.0))])
                .collect()
        } else {
            Vec::new()
        };
        DoomWorld {
            map: Some(map),
            spawning_distance: spawning_distance(units_per_voxel),
            light,
            emissive,
        }
    }
}

/// The light settings of `map` in `mode` under `knobs`.
///
/// Sky light enters a column that holds no solid voxel up to the map's top,
/// above which everything is air: the sky sectors' air runs up to it, and
/// every other column is solid above its ceiling up to it, so indoor rooms
/// get no direct sky.
///
/// The palette shader is unlit and reads the look's curves, not its voxel
/// brightness: see [`ShaderLight`]. In sector glow, voxel light follows the
/// brightness the voxelizer measured from COLORMAP for each level, as a share
/// of sRGB value, so a face lit by its own surface shows its sector's light.
fn light_settings(
    map: &VoxelMap,
    mode: LightMode,
    knobs: &LightKnobs,
) -> LightPropagation {
    let (_, max) = map.bounds();
    let curve = falloff_curve(knobs.falloff);
    let voxel_curve = if mode.sector_glow() {
        LevelCurve::from_table(map.glow_brightness())
    } else {
        curve.clone()
    };
    LightPropagation {
        sky: if knobs.sky(mode) {
            Sky::Ceiling { y: max[1] }
        } else {
            Sky::Off
        },
        sky_reach: knobs.sky_reach,
        voxel_reach: knobs.voxel_reach(mode),
        sky_fill: knobs.sky_fill.then(SkyFill::default),
        precision: if knobs.steps_256(mode) {
            LightPrecision::Steps256
        } else {
            LightPrecision::Steps16
        },
        slot_capacity: SLOT_CAPACITY,
        look: LightLook {
            smoothing: if knobs.stepped {
                LightSmoothing::Stepped
            } else {
                LightSmoothing::Smoothed
            },
            sky_falloff: curve,
            voxel_falloff: voxel_curve,
            minimum_brightness: knobs.min_light(mode),
            ..default()
        },
        ..default()
    }
}

/// Each level `base` times as bright as the one above, shifted and scaled so
/// level 15 is 1 and level 0 is 0.
///
/// The crate shows level 0 black whatever the curve says, so a plain
/// `LevelCurve::exponential` with a base near 1 ends light's reach in a hard
/// step from level 1's brightness to black, which reads as a sharp-edged
/// pool. This one fades into it.
fn falloff_curve(base: f32) -> LevelCurve {
    if base >= 0.999 {
        return LevelCurve::linear();
    }
    let floor = base.powi(15);
    LevelCurve::from_fn(|level| (base.powi(15 - level as i32) - floor) / (1.0 - floor))
}

/// The map the world is built from, shared by everything that reads it.
#[derive(Resource, Clone)]
pub struct GameMap {
    pub map: Arc<VoxelMap>,
    /// The material of a shot's flash, in the light modes that have one.
    pub flash: Option<u16>,
    /// DOOM map units per voxel.
    pub units: f32,
    /// Things are drawn in their sector's light, as the world is.
    pub sector_light: bool,
}

/// The level-0 chunks with voxel edits, as of the last commit; see
/// [`WorldSolid`].
#[derive(Resource, Default, Clone)]
pub struct EditedChunks(pub Arc<HashSet<IVec3>>);

/// Refreshes [`EditedChunks`] when the edits have changed.
pub fn track_edited_chunks(
    voxel_world: VoxelWorld<DoomWorld>,
    mut edited: ResMut<EditedChunks>,
    mut last: Local<(usize, u64)>,
) {
    let chunks = voxel_world.edited_chunks();
    let stamp = (chunks.len(), chunks.iter().map(|(_, seq)| seq).sum());
    if stamp != *last {
        *last = stamp;
        edited.0 = Arc::new(chunks.into_iter().map(|(p, _)| p).collect());
    }
}

/// What bodies, sight lines and shots collide with: the voxel world's
/// voxels, edits included, with flashes counting as air, and the doors,
/// lifts and floors on sub-grids (see `movers`).
///
/// A chunk without edits holds just what the map generates, so its voxels
/// come from [`VoxelMap::is_solid`], which is much cheaper than
/// `VoxelWorld::get_voxel`. Only voxels in edited chunks go through the
/// voxel world, falling back to the map where it has nothing.
#[derive(Clone)]
pub struct WorldSolid {
    map: Arc<VoxelMap>,
    edited: Arc<HashSet<IVec3>>,
    get: Arc<dyn Fn(IVec3) -> WorldVoxel<u16> + Send + Sync>,
    flash: Option<u16>,
    movers: MoverSolids,
}

impl WorldSolid {
    pub fn new(
        voxel_world: &VoxelWorld<DoomWorld>,
        map: &GameMap,
        edited: &EditedChunks,
        movers: &MoverSolids,
    ) -> Self {
        WorldSolid {
            map: Arc::clone(&map.map),
            edited: Arc::clone(&edited.0),
            get: voxel_world.get_voxel_fn(),
            flash: map.flash,
            movers: movers.clone(),
        }
    }
}

impl Solid for WorldSolid {
    fn solid(&self, p: IVec3) -> bool {
        if !self.movers.is_empty() {
            if let Some(sector) = self.map.column_sector(p.x, p.z) {
                if self.movers.solid(sector, p.y) {
                    return true;
                }
            }
        }
        if !self
            .edited
            .contains(&p.div_euclid(IVec3::splat(CHUNK_SIZE_I)))
        {
            return self.map.is_solid(p.to_array());
        }
        match (self.get)(p) {
            WorldVoxel::Unset => self.map.is_solid(p.to_array()),
            voxel => voxel.is_solid() && Some(voxel) != self.flash.map(WorldVoxel::Solid),
        }
    }
}

/// Light texture slots: a long DOOM sightline loads thousands of mixed
/// chunks, far more than the crate's default of 1,024.
const SLOT_CAPACITY: u32 = 4096;

pub(crate) fn world_voxel(voxel: Voxel) -> WorldVoxel<u16> {
    match voxel {
        Voxel::Air => WorldVoxel::Air,
        Voxel::Solid(index) => WorldVoxel::Solid(material(index, 0)),
        Voxel::Emissive { index, level } => WorldVoxel::Solid(material(index, level)),
    }
}

impl VoxelWorldConfig for DoomWorld {
    type MaterialIndex = u16;
    type ChunkUserBundle = ();

    fn spawning_distance(&self) -> u32 {
        self.spawning_distance
    }

    // Turning around shouldn't drop and regenerate the room behind the camera.
    fn chunk_despawn_strategy(&self) -> ChunkDespawnStrategy {
        ChunkDespawnStrategy::FarAway
    }

    fn chunk_fill_hint(&self, key: ChunkKey) -> FillHint<u16> {
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

    fn voxel_lookup_delegate(&self) -> VoxelLookupDelegate<u16> {
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

    fn texture_index_mapper(&self) -> Arc<dyn Fn(u16) -> [u32; 3] + Send + Sync> {
        Arc::new(|material| [(material & 0xff) as u32; 3])
    }

    fn light_propagation(&self) -> Option<LightPropagation> {
        self.light.clone()
    }

    fn emissive_materials(&self) -> Vec<(u16, Emission)> {
        self.emissive.clone()
    }
}

const PALETTE_SHADER: Handle<Shader> =
    uuid_handle!("6d1f0a52-3c47-4e8b-9a61-0f2b7d94c3e5");

/// Shows each voxel in its palette colour, times the mesher's ambient
/// occlusion, with no PBR lighting. In a lit world the crate wraps it in
/// `VoxelWorldMaterial<PaletteMaterial>`, whose extension calls this
/// `specialize` first and adds the `VOXEL_LIGHT` shader def, under which the
/// shader scales the colour by the face's propagated light.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PaletteMaterial {
    /// The palette in linear RGB, so the sRGB output is the palette's bytes.
    #[uniform(0)]
    palette: [Vec4; 256],
    /// How the shader turns light into brightness, in a lit world.
    #[uniform(1)]
    light: ShaderLight,
}

/// What the palette shader needs beyond the crate's look: lamp light at
/// level 15 shows as brightness `lamp_gain`, a face whose own voxel gives off
/// light at `glow_from` or more glows as a lamp's, and with `in_srgb` at 1 the
/// brightness scales the colour's sRGB value rather than its linear one.
/// `gain` scales every colour's sRGB value last, for the map's contrast
/// (`VoxelMap::colour_gain`).
///
/// Sector glow scales in sRGB because COLORMAP does: a table leaves dark and
/// bright colours at about the same share of their sRGB value, where the same
/// linear factor darkens a dark colour's sRGB value far more than a bright
/// one's, and DOOM's textures are mostly dark.
///
/// In sector glow every surface gives off light, so only real lamps, at
/// level 15, may glow: a sector surface glowing would show its full-bright
/// colour instead of the light in front of it. Elsewhere any lamp glows.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ShaderLight {
    pub lamp_gain: f32,
    pub glow_from: f32,
    pub in_srgb: f32,
    pub gain: f32,
}

impl ShaderLight {
    pub fn new(mode: LightMode, knobs: &LightKnobs, gain: f32) -> Self {
        ShaderLight {
            gain,
            lamp_gain: knobs.lamp_gain(mode),
            glow_from: if mode.sector_glow() { 14.5 } else { 0.5 },
            in_srgb: if mode.sector_glow() { 1.0 } else { 0.0 },
        }
    }
}

impl PaletteMaterial {
    pub fn new(palette: &[[u8; 3]; 256], light: ShaderLight) -> Self {
        PaletteMaterial {
            light,
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
    pub light: LightMode,
    pub knobs: LightKnobs,
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
                    self.light,
                    &self.knobs,
                ))
                .with_material(PaletteMaterial::new(
                    self.map.palette(),
                    ShaderLight::new(self.light, &self.knobs, self.map.colour_gain()),
                )),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_falloff_runs_from_black_to_full_without_a_step() {
        for base in [0.7, 0.9, 1.0] {
            let curve = falloff_curve(base);
            assert!(curve.at(0.01) < 0.01, "{base}");
            assert!((curve.at(15.0) - 1.0).abs() < 1e-6, "{base}");
            for level in 1..15 {
                let (a, b) = (curve.at(level as f32), curve.at(level as f32 + 1.0));
                assert!(a < b, "{base} at {level}");
            }
        }
    }

    #[test]
    fn materials_hold_the_palette_index_and_the_level() {
        assert_eq!(material(200, 0), 200);
        assert_eq!(material(200, 15), 200 | 15 << 8);
        assert_eq!(material(7, 15) & 0xff, 7);
    }
}
