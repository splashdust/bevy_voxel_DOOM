//! DOOM's sprites as billboards: every thing is a quad turned about y to
//! face the camera, showing the frame its [`Billboard`] names from the
//! rotation the camera sees it at.
//!
//! The rotation comes from r_things.c: the angle from the camera to the
//! thing, less the thing's facing, picks one of eight 45° sectors, rotation
//! 1 seen from the front. The quad sits where DOOM draws the patch: its left
//! edge `left_offset` pixels left of the thing's origin and its top edge
//! `top_offset` pixels above its feet, one pixel to a map unit. A mirrored
//! lump is the same rectangle with its columns flipped.
//!
//! Colours are worked out on the CPU, as DOOM does: each texel's palette
//! index goes through the light table of the sector the thing stands in, the
//! same table walls in that sector take at the distance the map is baked
//! for (`VoxelMap::sprite_table`), then the world's colour gain. A
//! full-bright frame skips the table. Each (lump, table, mirrored, shadow)
//! is one image and one unlit, alpha-masked material, made the first time
//! it is drawn. A shadow thing, the spectre, is drawn in a dark table and
//! see-through instead of DOOM's fuzz.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use doom_wad::{Patch, SpriteSet, Wad};

use crate::camera::FlyCamera;
use crate::world::GameMap;

/// What a thing looks like now. Game logic sets it; [`draw_billboards`]
/// turns it into a quad.
#[derive(Component, Clone, Debug)]
pub struct Billboard {
    pub sprite: &'static str,
    pub letter: u8,
    pub bright: bool,
    pub shadow: bool,
    /// The thing's facing, radians counter-clockwise from DOOM's east, for
    /// sprites drawn from eight angles.
    pub facing: f32,
    /// Its origin's position, in voxels, this tic and the one before, for
    /// drawing between tics.
    pub feet: Vec3,
    pub prev: Vec3,
    pub visible: bool,
}

impl Billboard {
    pub fn new(sprite: &'static str, letter: u8, feet: Vec3) -> Self {
        Billboard {
            sprite,
            letter,
            bright: false,
            shadow: false,
            facing: 0.0,
            feet,
            prev: feet,
            visible: true,
        }
    }

    /// Moves it to `feet` for this tic.
    pub fn place(&mut self, feet: Vec3) {
        self.prev = self.feet;
        self.feet = feet;
    }
}

/// The material a billboard shows now, to skip setting it when unchanged.
#[derive(Component, Default)]
pub struct Shown(Option<Look>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Look {
    lump: u16,
    table: u8,
    mirrored: bool,
    shadow: bool,
}

/// The light table a shadow thing is drawn in, and how opaque it is.
const SHADOW_TABLE: u8 = 1 + 26;
const SHADOW_ALPHA: f32 = 0.55;

/// A sprite, frame letter and rotation.
type FrameKey = (&'static str, u8, u8);

/// The sprites of the WAD, decoded, and the images and materials made from
/// them so far.
#[derive(Resource)]
pub struct SpriteBank {
    set: SpriteSet,
    lumps: Vec<Arc<Patch>>,
    index: HashMap<String, u16>,
    /// (sprite, letter, rotation) to (lump, mirrored).
    frames: HashMap<FrameKey, Option<(u16, bool)>>,
    materials: HashMap<Look, Handle<StandardMaterial>>,
    ui_images: HashMap<(u16, u8), Handle<Image>>,
    quad: Handle<Mesh>,
}

impl SpriteBank {
    /// Decodes every sprite in `wad`. Lumps that don't decode are left out.
    pub fn load(wad: &Wad) -> Self {
        let set = wad.sprites();
        let mut lumps = Vec::new();
        let mut index = HashMap::new();
        for name in set.lump_names() {
            match wad.patch(name) {
                Ok(patch) => {
                    index.insert(name.to_string(), lumps.len() as u16);
                    lumps.push(Arc::new(patch));
                }
                Err(e) => warn!("sprite {name}: {e}"),
            }
        }
        SpriteBank {
            set,
            lumps,
            index,
            frames: HashMap::new(),
            materials: HashMap::new(),
            ui_images: HashMap::new(),
            quad: Handle::default(),
        }
    }

    /// The lump for frame `letter` of `sprite` from `rotation`, 1 to 8.
    fn frame(
        &mut self,
        sprite: &'static str,
        letter: u8,
        rotation: u8,
    ) -> Option<(u16, bool)> {
        if let Some(found) = self.frames.get(&(sprite, letter, rotation)) {
            return *found;
        }
        let found = self
            .set
            .get(sprite, letter, rotation)
            .or_else(|| self.set.get(sprite, letter, 1))
            .and_then(|l| Some((*self.index.get(&l.name)?, l.mirrored)));
        if found.is_none() {
            warn!("no sprite {sprite} {}{rotation}", letter as char);
        }
        self.frames.insert((sprite, letter, rotation), found);
        found
    }

    /// The patch of frame `letter` of `sprite` as seen from the front.
    pub fn patch(&mut self, sprite: &'static str, letter: u8) -> Option<Arc<Patch>> {
        let (lump, _) = self.frame(sprite, letter, 1)?;
        Some(Arc::clone(&self.lumps[lump as usize]))
    }

    /// An image of frame `letter` of `sprite` in light `table`, for the UI.
    pub fn ui_image(
        &mut self,
        sprite: &'static str,
        letter: u8,
        table: u8,
        map: &GameMap,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Arc<Patch>)> {
        let (lump, _) = self.frame(sprite, letter, 1)?;
        let patch = Arc::clone(&self.lumps[lump as usize]);
        let handle = self
            .ui_images
            .entry((lump, table))
            .or_insert_with(|| images.add(image(&patch, table, false, map)))
            .clone();
        Some((handle, patch))
    }
}

/// The RGBA image of `patch` in light `table`, mirrored or not.
fn image(patch: &Patch, table: u8, mirrored: bool, map: &GameMap) -> Image {
    let pic = &patch.picture;
    let (w, h) = (pic.width as usize, pic.height as usize);
    let palette = map.map.palette();
    let gain = map.map.colour_gain();
    let mut data = vec![0u8; w.max(1) * h.max(1) * 4];
    for y in 0..h {
        for x in 0..w {
            let sx = if mirrored { w - 1 - x } else { x };
            if let Some(index) = pic.pixels[y * w + sx] {
                let rgb = palette[map.map.lit(index, table) as usize];
                let o = (y * w + x) * 4;
                for c in 0..3 {
                    data[o + c] = (rgb[c] as f32 * gain).round().min(255.0) as u8;
                }
                data[o + 3] = 255;
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w.max(1) as u32,
            height: h.max(1) as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Makes the shared quad.
pub fn setup_sprites(mut bank: ResMut<SpriteBank>, mut meshes: ResMut<Assets<Mesh>>) {
    bank.quad = meshes.add(Rectangle::new(1.0, 1.0));
}

/// The components a new billboard needs besides its [`Billboard`].
pub fn billboard_bundle(bank: &SpriteBank) -> impl Bundle {
    (
        Mesh3d(bank.quad.clone()),
        MeshMaterial3d::<StandardMaterial>(Handle::default()),
        Transform::default(),
        Visibility::Hidden,
        Shown::default(),
        bevy::light::NotShadowCaster,
    )
}

/// DOOM's rotation, 1 to 8, of a thing at `at` facing `facing` (radians
/// counter-clockwise from DOOM's east) seen from `eye`.
pub fn rotation(eye: Vec3, at: Vec3, facing: f32) -> u8 {
    // Voxel x is DOOM's x and voxel −z DOOM's y.
    let to_thing = (eye.z - at.z).atan2(at.x - eye.x);
    let turn =
        (to_thing - facing + 202.5f32.to_radians()).rem_euclid(std::f32::consts::TAU);
    (turn / 45f32.to_radians()) as u8 % 8 + 1
}

/// Places every billboard's quad and picks its material.
#[allow(clippy::too_many_arguments)]
pub fn draw_billboards(
    fixed: Res<Time<Fixed>>,
    camera: Single<&Transform, (With<FlyCamera>, Without<Billboard>)>,
    map: Res<GameMap>,
    mut bank: ResMut<SpriteBank>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut boards: Query<(
        &Billboard,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
        &mut Shown,
    )>,
) {
    let alpha = fixed.overstep_fraction();
    let eye = camera.translation;
    let s = map.units;
    for (board, mut transform, mut material, mut visibility, mut shown) in &mut boards {
        if !board.visible {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let at = board.prev.lerp(board.feet, alpha);
        let rot = rotation(eye, at, board.facing);
        let Some((lump, mirrored)) = bank.frame(board.sprite, board.letter, rot) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let table = if board.shadow {
            SHADOW_TABLE
        } else if board.bright || !map.sector_light {
            0
        } else {
            map.map
                .sprite_table(at.x.floor() as i32, at.z.floor() as i32)
        };
        let look = Look {
            lump,
            table,
            mirrored,
            shadow: board.shadow,
        };
        if shown.0 != Some(look) {
            shown.0 = Some(look);
            let bank = &mut *bank;
            let patch = &bank.lumps[lump as usize];
            let handle = bank.materials.entry(look).or_insert_with(|| {
                let texture = images.add(image(patch, table, mirrored, &map));
                materials.add(StandardMaterial {
                    base_color: Color::srgba(
                        1.0,
                        1.0,
                        1.0,
                        if look.shadow { SHADOW_ALPHA } else { 1.0 },
                    ),
                    base_color_texture: Some(texture),
                    unlit: true,
                    cull_mode: None,
                    fog_enabled: false,
                    alpha_mode: if look.shadow {
                        AlphaMode::Blend
                    } else {
                        AlphaMode::Mask(0.5)
                    },
                    ..default()
                })
            });
            material.0 = handle.clone();
        }
        let patch = &bank.lumps[lump as usize];
        let (w, h) = (patch.picture.width as f32, patch.picture.height as f32);
        let yaw = (eye.x - at.x).atan2(eye.z - at.z);
        let rotation = Quat::from_rotation_y(yaw);
        let right = rotation * Vec3::X;
        let centre = at
            + right * ((w / 2.0 - patch.left_offset as f32) / s)
            + Vec3::Y * ((patch.top_offset as f32 - h / 2.0) / s);
        *transform = Transform {
            translation: centre,
            rotation,
            scale: Vec3::new(w / s, h / s, 1.0),
        };
        visibility.set_if_neq(Visibility::Inherited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_follow_r_things() {
        let at = Vec3::new(0.0, 0.0, 0.0);
        // A thing facing DOOM's east, seen from the east: its front.
        let east = 0.0;
        assert_eq!(rotation(Vec3::new(10.0, 0.0, 0.0), at, east), 1);
        // Seen from the west: its back.
        assert_eq!(rotation(Vec3::new(-10.0, 0.0, 0.0), at, east), 5);
        // Seen from DOOM's north (voxel −z): its left side, rotation 3,
        // since rotations go counter-clockwise round the thing.
        assert_eq!(rotation(Vec3::new(0.0, 0.0, -10.0), at, east), 3);
        assert_eq!(rotation(Vec3::new(0.0, 0.0, 10.0), at, east), 7);
        // Seen from the north-east: between.
        assert_eq!(rotation(Vec3::new(10.0, 0.0, -10.0), at, east), 2);
        // Facing north, seen from the north: its front.
        let north = std::f32::consts::FRAC_PI_2;
        assert_eq!(rotation(Vec3::new(0.0, 0.0, -10.0), at, north), 1);
    }
}
