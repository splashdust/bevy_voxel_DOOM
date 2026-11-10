//! A hitscan weapon that blows chunks out of the map.
//!
//! The left mouse button fires while the cursor is captured, and holding it
//! fires at a DOOM chaingun's rate. A shot is a ray from the camera along
//! its forward vector; where it meets a solid voxel within the view distance
//! it carves a rough ball, whose edge is jittered per voxel so it reads as a
//! chunk blown out, and scorches a thin shell of solid voxels around it.
//! Scorched voxels are darkened from the colour the map generates there, so
//! shooting the same spot again doesn't darken it further.
//!
//! `--fire N` fires N shots by itself once the world has settled, spread in
//! a fixed pattern around the view centre.
//!
//! In the light modes that propagate light, a shot also flashes: the voxel
//! just in front of the face it hits, along the face's normal, glows for
//! `--flash-time` seconds and goes back to air. The ray came through that
//! voxel, so it is air, and the carve leaves it alone, since it only empties
//! solid voxels. Rays pass through flashes.

use std::sync::Arc;

use bevy::window::PrimaryWindow;
use bevy::{prelude::*, window::CursorGrabMode, window::CursorOptions};
use bevy_voxel_world::prelude::*;
use doom_voxelize::{Voxel, VoxelMap};

use crate::camera::FlyCamera;
use crate::capture::{CaptureSet, WorldSettled};
use crate::world::{material, DoomWorld};

/// DOOM's chaingun: a shot every 4 tics at 35 tics a second.
const SHOT_INTERVAL: f32 = 4.0 / 35.0;
/// Scorched voxels lie this far past the carved radius, in voxels.
const SCORCH_DEPTH: f32 = 1.5;
/// COLORMAP tables darker at the scorch's inner and outer edge.
const SCORCH_INNER: f32 = 6.0;
const SCORCH_OUTER: f32 = 2.0;
/// The scripted shots' spread: at the distance the centre ray hits, shot
/// `i` lands `SPREAD·radius·√i` voxels off centre, turned a golden angle
/// from the one before, so the craters sit side by side.
const SPREAD: f32 = 2.2;
const GOLDEN_ANGLE: f32 = 2.399_963;
/// The spread is measured this far away when the centre ray hits nothing.
const SPREAD_FALLBACK_DISTANCE: f32 = 100.0;

pub struct WeaponPlugin {
    /// The map the world is built from: scorched voxels are darkened from
    /// its colours.
    pub map: Arc<VoxelMap>,
    /// The carved ball's radius in voxels.
    pub radius: i32,
    /// How far a shot reaches, in voxels.
    pub range: f32,
    /// Shots to fire once the world has settled.
    pub scripted_shots: u32,
    /// The material of a shot's flash, in the light modes that propagate
    /// light.
    pub flash: Option<u16>,
    /// Seconds a flash glows.
    pub flash_time: f32,
}

#[derive(Resource)]
struct Weapon {
    map: Arc<VoxelMap>,
    radius: i32,
    /// How far a shot reaches, in voxels.
    range: f32,
    flash: Option<u16>,
    flash_time: f32,
    /// Flashes glowing now: where, and seconds left.
    flashes: Vec<(IVec3, f32)>,
    /// Seconds until the next shot may fire.
    cooldown: f32,
    /// The button went down with the cursor captured and is still down.
    trigger_held: bool,
}

#[derive(Resource)]
struct ScriptedShots {
    total: u32,
    fired: u32,
    /// Radians off centre per √shot, fixed when the first shot fires.
    spread: f32,
}

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Weapon {
            map: Arc::clone(&self.map),
            radius: self.radius,
            range: self.range,
            flash: self.flash,
            flash_time: self.flash_time,
            flashes: Vec::new(),
            cooldown: 0.0,
            trigger_held: false,
        })
        // Before the click that captures the cursor is seen by `grab_cursor`.
        .add_systems(Update, fire_from_mouse.before(crate::camera::grab_cursor))
        .add_systems(Update, put_out_flashes);
        if self.scripted_shots > 0 {
            app.insert_resource(ScriptedShots {
                total: self.scripted_shots,
                fired: 0,
                spread: 0.0,
            })
            .add_systems(
                Update,
                fire_scripted
                    .after(CaptureSet::Track)
                    .before(CaptureSet::Shoot),
            );
        }
    }
}

fn fire_from_mouse(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<&Transform, With<FlyCamera>>,
    mut weapon: ResMut<Weapon>,
    mut voxel_world: VoxelWorld<DoomWorld>,
) {
    let captured = cursor.grab_mode != CursorGrabMode::None;
    if !captured
        || keys.just_pressed(KeyCode::Escape)
        || !buttons.pressed(MouseButton::Left)
    {
        weapon.trigger_held = false;
    } else if buttons.just_pressed(MouseButton::Left) {
        weapon.trigger_held = true;
    }
    if weapon.cooldown > 0.0 {
        weapon.cooldown -= time.delta_secs();
    }
    if !weapon.trigger_held {
        weapon.cooldown = weapon.cooldown.max(0.0);
        return;
    }
    if weapon.cooldown <= 0.0 {
        weapon.cooldown += SHOT_INTERVAL;
        let ray = Ray3d::new(camera.translation, camera.forward());
        shoot(&mut voxel_world, &mut weapon, ray);
    }
}

/// Sets each flash whose time is up back to air, unless something else has
/// replaced it.
fn put_out_flashes(
    time: Res<Time>,
    mut weapon: ResMut<Weapon>,
    mut voxel_world: VoxelWorld<DoomWorld>,
) {
    if weapon.flashes.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let flash = weapon.flash.map(WorldVoxel::Solid);
    weapon.flashes.retain_mut(|(p, left)| {
        *left -= dt;
        if *left > 0.0 {
            return true;
        }
        if Some(voxel_world.get_voxel(*p)) == flash {
            voxel_world.set_voxel(*p, WorldVoxel::Air);
        }
        false
    });
}

/// Fires the next scripted shot once the world has settled, one a frame,
/// and holds the screenshot back until the carved chunks have remeshed.
fn fire_scripted(
    mut shots: ResMut<ScriptedShots>,
    mut settled: ResMut<WorldSettled>,
    camera: Single<&Transform, With<FlyCamera>>,
    mut weapon: ResMut<Weapon>,
    mut voxel_world: VoxelWorld<DoomWorld>,
) {
    if shots.fired >= shots.total || (shots.fired == 0 && !settled.is_settled()) {
        return;
    }
    if shots.fired == 0 {
        let ray = Ray3d::new(camera.translation, camera.forward());
        let flash = weapon.flash;
        let distance = voxel_world
            .raycast(ray, &|(_, voxel)| stops_a_shot(voxel, flash))
            .map_or(SPREAD_FALLBACK_DISTANCE, |hit| {
                hit.position.distance(camera.translation).max(1.0)
            });
        shots.spread = (SPREAD * weapon.radius as f32 / distance).atan();
    }
    let i = shots.fired;
    let off = shots.spread * (i as f32).sqrt();
    let turn = GOLDEN_ANGLE * i as f32;
    let aim = Quat::from_euler(EulerRot::YXZ, off * turn.cos(), off * turn.sin(), 0.0);
    let direction = Dir3::new(camera.rotation * aim * Vec3::NEG_Z).unwrap_or(Dir3::NEG_Z);
    let ray = Ray3d::new(camera.translation, direction);
    match shoot(&mut voxel_world, &mut weapon, ray) {
        Some(hit) => println!("shot {}: hit voxel {hit}", i + 1),
        None => println!("shot {}: missed", i + 1),
    }
    shots.fired += 1;
    settled.unsettle();
}

/// Whether a shot stops at `voxel`: any solid but a flash.
fn stops_a_shot(voxel: WorldVoxel<u16>, flash: Option<u16>) -> bool {
    voxel.is_solid() && voxel != WorldVoxel::Solid(flash.unwrap_or(u16::MAX))
}

/// Fires along `ray`: carves and scorches around the first solid voxel
/// within the weapon's range, lights a flash in front of it, and returns it.
fn shoot(
    voxel_world: &mut VoxelWorld<DoomWorld>,
    weapon: &mut Weapon,
    ray: Ray3d,
) -> Option<IVec3> {
    let (map, radius, flash) = (Arc::clone(&weapon.map), weapon.radius, weapon.flash);
    let hit = voxel_world.raycast(ray, &|(_, voxel)| stops_a_shot(voxel, flash))?;
    let centre = hit.voxel_pos();
    if (centre.as_vec3() + 0.5).distance(ray.origin) > weapon.range {
        return None;
    }
    // What's there now: an edit, a published voxel, or what the map makes.
    let current = voxel_world.get_voxel_fn();
    let reach = radius + SCORCH_DEPTH.ceil() as i32 + 1;
    voxel_world.set_voxels_in(
        centre - IVec3::splat(reach),
        centre + IVec3::splat(reach + 1),
        |p| {
            let effect = blast(p - centre, p, radius)?;
            let solid = match current(p) {
                WorldVoxel::Unset => map.voxel(p.to_array()).is_solid(),
                voxel => voxel.is_solid(),
            };
            if !solid {
                return None;
            }
            match effect {
                Blast::Carve => Some(WorldVoxel::Air),
                // A scorched lamp goes out.
                Blast::Scorch(steps) => match map.voxel(p.to_array()) {
                    Voxel::Solid(index) | Voxel::Emissive { index, .. } => {
                        Some(WorldVoxel::Solid(material(map.shade(index, steps), 0)))
                    }
                    Voxel::Air => None,
                },
            }
        },
    );
    if let (Some(flash), Some(normal)) = (flash, hit.voxel_normal()) {
        let p = centre + normal;
        let air = match current(p) {
            WorldVoxel::Unset => !map.voxel(p.to_array()).is_solid(),
            voxel => voxel == WorldVoxel::Air,
        };
        if air {
            voxel_world.set_voxel(p, WorldVoxel::Solid(flash));
            let time = weapon.flash_time;
            weapon.flashes.push((p, time));
        }
    }
    Some(centre)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blast {
    Carve,
    /// Darken by this many COLORMAP tables.
    Scorch(u8),
}

/// What a shot does to voxel `p`, `offset` from the voxel hit.
fn blast(offset: IVec3, p: IVec3, radius: i32) -> Option<Blast> {
    let radius = radius as f32;
    let amplitude = (0.2 * radius).max(0.6);
    let jitter = (hash(p) as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude;
    let distance = offset.as_vec3().length() + jitter;
    if distance <= radius {
        Some(Blast::Carve)
    } else if distance <= radius + SCORCH_DEPTH {
        let t = (distance - radius) / SCORCH_DEPTH;
        let steps = SCORCH_INNER + (SCORCH_OUTER - SCORCH_INNER) * t;
        Some(Blast::Scorch(steps.round() as u8))
    } else {
        None
    }
}

/// A cheap, well-mixed hash of a voxel position.
fn hash(p: IVec3) -> u32 {
    let mut h = (p.x as u32).wrapping_mul(0x8da6_b343)
        ^ (p.y as u32).wrapping_mul(0xd816_3841)
        ^ (p.z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(radius: i32, want: impl Fn(Option<Blast>) -> bool) -> usize {
        let r = radius + 4;
        let mut n = 0;
        for z in -r..=r {
            for y in -r..=r {
                for x in -r..=r {
                    let o = IVec3::new(x, y, z);
                    n += want(blast(o, o + IVec3::new(100, 7, -40), radius)) as usize;
                }
            }
        }
        n
    }

    #[test]
    fn the_ball_is_about_the_radius_with_a_ragged_edge() {
        let carved = count(4, |b| b == Some(Blast::Carve));
        let ball = 4.0 / 3.0 * std::f32::consts::PI * 64.0;
        assert!((carved as f32 - ball).abs() < ball * 0.2, "{carved} carved");
        // The centre always goes; nothing past radius + scorch + jitter is touched.
        assert_eq!(blast(IVec3::ZERO, IVec3::ZERO, 4), Some(Blast::Carve));
        assert_eq!(blast(IVec3::new(7, 0, 0), IVec3::ZERO, 4), None);
        // Some voxels at exactly the radius go and some stay: a ragged edge.
        let at_edge: Vec<_> = (0..40)
            .map(|i| blast(IVec3::new(4, 0, 0), IVec3::new(i, 3, 9), 4))
            .collect();
        assert!(at_edge.contains(&Some(Blast::Carve)));
        assert!(at_edge.iter().any(|b| matches!(b, Some(Blast::Scorch(_)))));
    }

    #[test]
    fn scorch_is_a_thin_shell_darker_inside() {
        let shell = count(4, |b| matches!(b, Some(Blast::Scorch(_))));
        assert!(shell > 100, "{shell} scorched");
        for i in 0..200 {
            let p = IVec3::new(i, -i, 3 * i);
            for (offset, max) in [(IVec3::new(5, 0, 0), 6), (IVec3::new(6, 0, 0), 4)] {
                if let Some(Blast::Scorch(steps)) = blast(offset, p, 4) {
                    assert!((2..=max).contains(&steps), "{steps} at {offset}");
                }
            }
        }
    }
}
