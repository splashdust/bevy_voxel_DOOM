//! The player's chaingun, and the one place the world is destroyed.
//!
//! The trigger (left mouse, or a script) fires at a DOOM chaingun's rate.
//! A shot is a ray from the camera along its forward vector, spread a
//! little after the first shot of a burst as DOOM's refire is. It hits the
//! first monster or barrel whose body it crosses, for DOOM's 5 × (1 to 3)
//! damage, or else carves the first solid voxel within the view distance.
//! Every shot wakes monsters within 1024 units.
//!
//! [`Carver::carve`] does every carve: the player's shots, monsters'
//! bullets and projectiles, and barrels. It blows a rough ball out of the
//! world, its edge jittered per voxel so it reads as a chunk blown out, and
//! scorches a thin shell of solid voxels around it. Scorched voxels are
//! darkened from the colour the map generates there, so carving the same
//! spot again doesn't darken it further.
//!
//! `--fire N` fires N shots by itself once the world has settled, spread in
//! a fixed pattern around the view centre.
//!
//! In the light modes that propagate light, a player's shot also flashes:
//! the voxel just in front of the face it hits, along the face's normal,
//! glows for `--flash-time` seconds and goes back to air. The ray came
//! through that voxel, so it is air, and the carve leaves it alone, since it
//! only empties solid voxels. Rays and bodies pass through flashes.

use std::sync::Arc;
use std::time::Instant;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_voxel_world::prelude::*;
use doom_voxelize::Voxel;

use crate::actors::{spawn_effect, DamageQueue, Hit, Noise, Rng, Shot, ShotEnd, Who};
use crate::camera::FlyCamera;
use crate::capture::{CaptureSet, WorldSettled};
use crate::debris::SprayDebris;
use crate::info;
use crate::input::Intent;
use crate::player::Player;
use crate::sound::PlaySound;
use crate::sprites::SpriteBank;
use crate::world::{material, DoomWorld, EditedChunks, GameMap, WorldSolid};

/// DOOM's chaingun: a shot every 4 tics at 35 tics a second.
pub const SHOT_INTERVAL: f32 = 4.0 / 35.0;
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
    /// The carved ball's radius in voxels.
    pub radius: i32,
    /// How far a shot reaches, in voxels.
    pub range: f32,
    /// Shots to fire once the world has settled.
    pub scripted_shots: u32,
    /// Seconds a flash glows.
    pub flash_time: f32,
}

/// The gun's state, which the HUD draws.
#[derive(Resource)]
pub struct Weapon {
    radius: i32,
    /// How far a shot reaches, in voxels.
    range: f32,
    flash_time: f32,
    /// Flashes glowing now: where, and seconds left.
    flashes: Vec<(IVec3, f32)>,
    /// Since the last shot whose light hasn't landed: when it fired, frames
    /// since, and the longest frame since, in seconds.
    landing: Option<(Instant, u32, f32)>,
    /// Seconds until the next shot may fire.
    cooldown: f32,
    /// Shots fired, and seconds since the last.
    pub shots: u32,
    pub since_shot: f32,
    /// Shots in the current burst.
    burst: u32,
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
            radius: self.radius,
            range: self.range,
            flash_time: self.flash_time,
            flashes: Vec::new(),
            landing: None,
            cooldown: 0.0,
            shots: 0,
            since_shot: f32::INFINITY,
            burst: 0,
        })
        .add_systems(Update, fire_trigger.after(crate::player::walk))
        .add_systems(Update, put_out_flashes)
        .add_systems(Last, report_landing);
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

/// Destroys the world: see [`Carver::carve`]. Also reads it, for sight,
/// bodies and shots.
#[derive(SystemParam)]
pub struct Carver<'w, 's> {
    pub voxel_world: VoxelWorld<'w, 's, DoomWorld>,
    map: Res<'w, GameMap>,
    edited: Res<'w, EditedChunks>,
    spray: MessageWriter<'w, SprayDebris>,
}

impl Carver<'_, '_> {
    /// What bodies and shots collide with now.
    pub fn solid(&self) -> WorldSolid {
        WorldSolid::new(&self.voxel_world, &self.map, &self.edited)
    }

    /// Blows a ball of `radius` voxels out of the world around `hit`, the
    /// solid voxel something struck (from the side `normal` points to, when
    /// known), and scorches a shell around it. Returns the solid voxels it
    /// emptied, with the materials they had, and sends them as
    /// [`SprayDebris`], thrown along `normal`, or every way without one.
    ///
    /// Every source of destruction calls this, so anything that reacts to
    /// destruction hooks in here.
    pub fn carve(
        &mut self,
        hit: IVec3,
        normal: Option<IVec3>,
        radius: i32,
    ) -> Vec<(IVec3, u16)> {
        let map = Arc::clone(&self.map.map);
        let flash = self.map.flash.map(WorldVoxel::Solid);
        // What's there now: an edit, a published voxel, or what the map makes.
        let current = self.voxel_world.get_voxel_fn();
        let reach = radius + SCORCH_DEPTH.ceil() as i32 + 1;
        let mut carved = Vec::new();
        self.voxel_world.set_voxels_in(
            hit - IVec3::splat(reach),
            hit + IVec3::splat(reach + 1),
            |p| {
                let effect = blast(p - hit, p, radius)?;
                let now = match current(p) {
                    WorldVoxel::Unset => match map.voxel(p.to_array()) {
                        Voxel::Air => WorldVoxel::Air,
                        Voxel::Solid(index) => WorldVoxel::Solid(material(index, 0)),
                        Voxel::Emissive { index, level } => {
                            WorldVoxel::Solid(material(index, level))
                        }
                    },
                    voxel => voxel,
                };
                let WorldVoxel::Solid(was) = now else {
                    return None;
                };
                if Some(now) == flash {
                    return None;
                }
                match effect {
                    Blast::Carve => {
                        carved.push((p, was));
                        Some(WorldVoxel::Air)
                    }
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
        if !carved.is_empty() {
            let normal = normal.map_or(Vec3::ZERO, |n| n.as_vec3());
            self.spray.write(SprayDebris {
                hit: hit.as_vec3() + 0.5 + normal * 0.5,
                normal,
                carved: carved.clone(),
            });
        }
        carved
    }
}

/// Fires while the trigger is held, at the chaingun's rate.
#[allow(clippy::too_many_arguments)]
fn fire_trigger(
    mut commands: Commands,
    time: Res<Time>,
    intent: Res<Intent>,
    camera: Single<(&Transform, &Player), With<FlyCamera>>,
    mut weapon: ResMut<Weapon>,
    mut carver: Carver,
    mut gun: Gun,
) {
    let (transform, player) = camera.into_inner();
    weapon.since_shot += time.delta_secs();
    if weapon.cooldown > 0.0 {
        weapon.cooldown -= time.delta_secs();
    }
    if !intent.fire || !player.alive() {
        weapon.cooldown = weapon.cooldown.max(0.0);
        weapon.burst = 0;
        return;
    }
    if weapon.cooldown <= 0.0 {
        weapon.cooldown += SHOT_INTERVAL;
        let mut forward = *transform.forward();
        if weapon.burst > 0 {
            // DOOM's refire: (P_Random() − P_Random()) << 18 off the aim.
            let off = gun.rng.spread() as f32 * (1u64 << 18) as f32 / 4_294_967_296.0
                * std::f32::consts::TAU;
            forward = Quat::from_rotation_y(off) * forward;
        }
        weapon.burst += 1;
        let ray = Ray3d::new(
            transform.translation,
            Dir3::new(forward).unwrap_or(Dir3::NEG_Z),
        );
        player_shot(&mut commands, &mut carver, &mut weapon, &mut gun, ray);
    }
}

/// What a player's shot touches besides the world.
#[derive(SystemParam)]
pub struct Gun<'w, 's> {
    rng: ResMut<'w, Rng>,
    queue: ResMut<'w, DamageQueue>,
    noise: ResMut<'w, Noise>,
    sounds: MessageWriter<'w, PlaySound>,
    bank: Res<'w, SpriteBank>,
    map: Res<'w, GameMap>,
    player: Single<'w, 's, &'static Player>,
    actors: Query<'w, 's, (Entity, &'static crate::actors::Actor)>,
}

/// Fires one shot along `ray`: hurts the first thing it crosses, or carves
/// the world. Returns the voxel it carved around.
fn player_shot(
    commands: &mut Commands,
    carver: &mut Carver,
    weapon: &mut Weapon,
    gun: &mut Gun,
    ray: Ray3d,
) -> Option<IVec3> {
    weapon.shots += 1;
    weapon.since_shot = 0.0;
    gun.sounds.write(PlaySound::global("PISTOL"));
    gun.noise.0 = Some(ray.origin);
    let targets: Vec<_> = gun
        .actors
        .iter()
        .filter(|(_, a)| a.shootable)
        .map(|(e, a)| (e, a.feet, a.radius, a.height, a.info))
        .collect();
    let world = carver.solid();
    let shot = Shot {
        origin: ray.origin,
        dir: *ray.direction,
        range: weapon.range,
        source: Who::Player,
    };
    match shot.fire(&world, &gun.player, &targets) {
        ShotEnd::Thing(target, at) => {
            let amount = 5 * (gun.rng.next() % 3 + 1);
            gun.queue.0.push(Hit {
                target,
                amount,
                source: Who::Player,
                from: ray.origin,
            });
            let barrel = matches!(target, Who::Actor(e) if gun.actors.get(e).is_ok_and(|(_, a)| a.info.inert));
            let (sprite, frames) = if barrel {
                ("PUFF", info::PUFF)
            } else {
                ("BLUD", info::BLOOD)
            };
            spawn_effect(commands, &gun.bank, sprite, frames, at, 0.0);
            None
        }
        ShotEnd::World(hit, at) => {
            weapon.landing = Some((Instant::now(), 0, 0.0));
            let centre = hit.voxel;
            carver.carve(centre, Some(hit.normal), weapon.radius);
            spawn_effect(
                commands,
                &gun.bank,
                "PUFF",
                info::PUFF,
                at,
                0.5 / gun.map.units,
            );
            light_flash(carver, weapon, &gun.map, centre + hit.normal);
            Some(centre)
        }
        ShotEnd::Nothing => None,
    }
}

/// Lights a flash at `p` if it is air, in the light modes that have them.
fn light_flash(carver: &mut Carver, weapon: &mut Weapon, map: &GameMap, p: IVec3) {
    let Some(flash) = map.flash else {
        return;
    };
    let air = match carver.voxel_world.get_voxel(p) {
        WorldVoxel::Unset => !map.map.voxel(p.to_array()).is_solid(),
        voxel => voxel == WorldVoxel::Air,
    };
    if air {
        carver.voxel_world.set_voxel(p, WorldVoxel::Solid(flash));
        let time = weapon.flash_time;
        weapon.flashes.push((p, time));
    }
}

/// Sets each flash whose time is up back to air, unless something else has
/// replaced it.
fn put_out_flashes(
    time: Res<Time>,
    map: Res<GameMap>,
    mut weapon: ResMut<Weapon>,
    mut voxel_world: VoxelWorld<DoomWorld>,
) {
    if weapon.flashes.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let flash = map.flash.map(WorldVoxel::Solid);
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
    mut commands: Commands,
    mut shots: ResMut<ScriptedShots>,
    mut settled: ResMut<WorldSettled>,
    camera: Single<&Transform, With<FlyCamera>>,
    mut weapon: ResMut<Weapon>,
    mut carver: Carver,
    mut gun: Gun,
) {
    if shots.fired >= shots.total || (shots.fired == 0 && !settled.is_settled()) {
        return;
    }
    if shots.fired == 0 {
        let world = carver.solid();
        let distance = crate::collide::trace(
            &world,
            camera.translation,
            *camera.forward(),
            weapon.range,
        )
        .map_or(SPREAD_FALLBACK_DISTANCE, |hit| hit.distance.max(1.0));
        shots.spread = (SPREAD * weapon.radius as f32 / distance).atan();
    }
    let i = shots.fired;
    let off = shots.spread * (i as f32).sqrt();
    let turn = GOLDEN_ANGLE * i as f32;
    let aim = Quat::from_euler(EulerRot::YXZ, off * turn.cos(), off * turn.sin(), 0.0);
    let direction = Dir3::new(camera.rotation * aim * Vec3::NEG_Z).unwrap_or(Dir3::NEG_Z);
    let ray = Ray3d::new(camera.translation, direction);
    match player_shot(&mut commands, &mut carver, &mut weapon, &mut gun, ray) {
        Some(hit) => println!("shot {}: hit voxel {hit}", i + 1),
        None => println!("shot {}: hit a thing or nothing", i + 1),
    }
    shots.fired += 1;
    settled.unsettle();
}

/// Prints how long the light of the last shot took to land, in a world that
/// propagates light: from the shot until `light_is_settled`, which waits for
/// the remeshed chunks' light too. Another shot before then restarts it.
fn report_landing(
    time: Res<Time<Real>>,
    map: Res<GameMap>,
    mut weapon: ResMut<Weapon>,
    voxel_world: VoxelWorld<DoomWorld>,
) {
    let lit = map.flash.is_some();
    let Some((fired, frames, longest)) = weapon.landing.as_mut() else {
        return;
    };
    *frames += 1;
    *longest = longest.max(time.delta_secs());
    if !lit || !voxel_world.light_is_settled() {
        return;
    }
    println!(
        "shot light landed after {:.1} ms, {frames} frames; longest frame {:.1} ms",
        fired.elapsed().as_secs_f64() * 1000.0,
        *longest * 1000.0
    );
    weapon.landing = None;
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
