//! The player: DOOM's walking body, health, death and respawn.
//!
//! The player is a box 32 map units across and 56 tall with the eye at 41,
//! stepping up ledges of up to 24 units, as in DOOM, moved through
//! `collide::move_among` against the voxels and the things that block. It
//! accelerates toward the speed it wants and slows when it stops at the rate
//! DOOM's friction gives, 0.90625 a tic, with DOOM's walking and running
//! speeds as the limits; in the air it barely steers. Gravity is DOOM's, 1
//! unit per tic per tic. DOOM has no jump; Space jumps about 32 units, so
//! the player can climb out of craters. After stepping up the eye catches up
//! over a few frames, as DOOM's view height does.
//!
//! `V` switches to the noclip fly camera and back. At 0 health the view
//! sinks to the floor and turns red; R or a click respawns at the start.

use bevy::prelude::*;

use crate::camera::FlyCamera;
use crate::collide::{move_among, Blocker, Body};
use crate::input::Intent;
use crate::world::{EditedChunks, GameMap, WorldSolid};
use crate::{sound::PlaySound, world::DoomWorld};
use bevy_voxel_world::prelude::*;

/// DOOM's player, in map units.
pub const RADIUS: f32 = 16.0;
pub const HEIGHT: f32 = 56.0;
pub const EYE: f32 = 41.0;
pub const STEP: f32 = 24.0;
/// The eye of a dead player.
const DEAD_EYE: f32 = 6.0;
/// The speed DOOM's thrust and friction settle at, units a second: forward
/// move 25 or 50 × 2048 / 65536 a tic over 1 − 0.90625.
const WALK_SPEED: f32 = 25.0 * 2048.0 / 65536.0 / (1.0 - FRICTION) * 35.0;
const RUN_SPEED: f32 = 2.0 * WALK_SPEED;
/// DOOM's friction, kept each tic.
const FRICTION: f32 = 0.90625;
/// The share of ground control left in the air.
const AIR_CONTROL: f32 = 0.15;
/// Units per second per second: 1 unit a tic a tic.
pub const GRAVITY: f32 = 35.0 * 35.0;
/// How high a jump lifts the feet, in units.
const JUMP_HEIGHT: f32 = 32.0;
/// How fast the eye catches up after a step, per second.
const VIEW_CATCH_UP: f32 = 14.0;
/// Seconds dead before a click respawns, so the click that was firing
/// doesn't.
const RESPAWN_DELAY: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Walk,
    Fly,
}

/// Where the player starts, in voxels.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlayerStart {
    pub feet: Vec3,
    pub yaw: f32,
    pub mode: Mode,
    pub god: bool,
    /// Map units per voxel.
    pub units: f32,
}

#[derive(Component, Debug)]
pub struct Player {
    pub feet: Vec3,
    /// Voxels per second.
    pub vel: Vec3,
    pub grounded: bool,
    pub health: i32,
    pub mode: Mode,
    pub god: bool,
    /// How far the eye lags below its height after stepping up, in voxels.
    view_lag: f32,
    /// The red of the screen after taking damage, 0 to 1.
    pub hurt: f32,
    /// Seconds since dying.
    pub dead_for: f32,
    start: PlayerStart,
    body: Body,
    eye: f32,
    /// How fast it moves across the ground, voxels a second, for the
    /// weapon's bob.
    pub ground_speed: f32,
}

impl Player {
    pub fn new(start: &PlayerStart) -> Self {
        let s = start.units;
        Player {
            feet: start.feet,
            vel: Vec3::ZERO,
            grounded: false,
            health: 100,
            mode: start.mode,
            god: start.god,
            view_lag: 0.0,
            hurt: 0.0,
            dead_for: 0.0,
            start: *start,
            body: Body {
                radius: RADIUS / s,
                height: HEIGHT / s,
                step: STEP / s,
            },
            eye: EYE / s,
            ground_speed: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.health > 0
    }

    pub fn body(&self) -> &Body {
        &self.body
    }

    /// The middle of its body, where monsters aim.
    pub fn centre(&self) -> Vec3 {
        self.feet + Vec3::Y * self.body.height / 2.0
    }

    /// Where it sees from: the camera's position in walk mode.
    pub fn eye(&self) -> Vec3 {
        self.feet + Vec3::Y * self.eye
    }

    pub fn blocker(&self) -> Blocker {
        Blocker {
            feet: self.feet,
            radius: self.body.radius,
            height: self.body.height,
            id: PLAYER_ID,
        }
    }

    /// Takes `amount` damage, pushed along `push` in voxels a second.
    /// Returns whether it died of it.
    pub fn damage(&mut self, amount: i32, push: Vec3) -> bool {
        if !self.alive() || self.god || amount <= 0 {
            return false;
        }
        self.health -= amount;
        self.hurt = (self.hurt + amount as f32 / 40.0).min(0.8);
        self.vel += push;
        if self.health <= 0 {
            self.health = 0;
            self.dead_for = 0.0;
            return true;
        }
        false
    }

    /// Gives `amount` health up to `max`; false when it has that already.
    pub fn heal(&mut self, amount: i32, max: i32) -> bool {
        if !self.alive() || self.health >= max {
            return false;
        }
        self.health = (self.health + amount).min(max);
        true
    }

    pub fn describe(&self) -> String {
        format!(
            "feet {:.1} {:.1} {:.1}{} health {}",
            self.feet.x,
            self.feet.y,
            self.feet.z,
            if self.grounded {
                " on ground"
            } else {
                " in air"
            },
            self.health
        )
    }

    fn respawn(&mut self, fly: &mut FlyCamera) {
        self.feet = self.start.feet;
        self.vel = Vec3::ZERO;
        self.health = 100;
        self.hurt = 0.0;
        self.view_lag = 0.0;
        fly.yaw = self.start.yaw;
        fly.pitch = 0.0;
    }
}

/// The [`Blocker::id`] of the player; monsters' are their entity bits.
pub const PLAYER_ID: u64 = u64::MAX;

/// The things that block bodies this frame: living monsters, solid
/// decorations and the player.
#[derive(Resource, Default)]
pub struct Blockers(pub Vec<Blocker>);

/// Walks the player, or follows the fly camera in fly mode.
#[allow(clippy::too_many_arguments)]
pub fn walk(
    time: Res<Time>,
    intent: Res<Intent>,
    map: Res<GameMap>,
    edited: Res<EditedChunks>,
    voxel_world: VoxelWorld<DoomWorld>,
    blockers: Res<Blockers>,
    camera: Single<(&mut FlyCamera, &mut Transform, &mut Player)>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let (mut fly, mut transform, mut player) = camera.into_inner();
    let dt = time.delta_secs().min(0.1);
    let s = map.units;
    player.hurt = (player.hurt - dt * 0.8).max(0.0);
    if intent.toggle_fly {
        player.mode = match player.mode {
            Mode::Walk => Mode::Fly,
            Mode::Fly => {
                player.vel = Vec3::ZERO;
                Mode::Walk
            }
        };
        println!("{:?} mode", player.mode);
    }
    if player.mode == Mode::Fly {
        player.feet = transform.translation - Vec3::Y * player.eye;
        player.grounded = false;
        player.ground_speed = 0.0;
        return;
    }
    let world = WorldSolid::new(&voxel_world, &map, &edited);
    let alive = player.alive();
    if !alive {
        player.dead_for += dt;
        if intent.respawn && player.dead_for > RESPAWN_DELAY {
            player.respawn(&mut fly);
            transform.rotation = fly.rotation();
            sounds.write(PlaySound::global("TELEPT"));
        }
    }

    // Accelerate toward the wished velocity at DOOM's friction rate.
    let rate = -FRICTION.ln() * 35.0;
    let (yaw_sin, yaw_cos) = fly.yaw.sin_cos();
    let forward = Vec3::new(-yaw_sin, 0.0, -yaw_cos);
    let right = Vec3::new(yaw_cos, 0.0, -yaw_sin);
    let mut wish = forward * intent.forward + right * intent.strafe;
    if !alive {
        wish = Vec3::ZERO;
    }
    let speed = if intent.run { RUN_SPEED } else { WALK_SPEED } / s;
    let target = wish.normalize_or_zero() * speed;
    let rate = if player.grounded {
        rate
    } else {
        rate * AIR_CONTROL
    };
    let keep = (-rate * dt).exp();
    let horizontal = target + (player.vel.with_y(0.0) - target) * keep;
    player.vel.x = horizontal.x;
    player.vel.z = horizontal.z;
    player.vel.y -= GRAVITY / s * dt;
    if alive && player.grounded && intent.jump {
        player.vel.y = (2.0 * GRAVITY * JUMP_HEIGHT).sqrt() / s;
        player.grounded = false;
    }

    let others: Vec<Blocker> = blockers.0.clone();
    let body = player.body;
    let moved = move_among(
        &world,
        &body,
        player.feet,
        player.vel * dt,
        &others,
        PLAYER_ID,
    );
    player.feet = moved.feet;
    if moved.blocked[0] {
        player.vel.x = 0.0;
    }
    if moved.blocked[2] {
        player.vel.z = 0.0;
    }
    if moved.blocked[1] && player.vel.y > 0.0 {
        player.vel.y = 0.0;
    }
    let landed_hard = moved.grounded && !player.grounded && player.vel.y < -300.0 / s;
    player.grounded = moved.grounded;
    if player.grounded && player.vel.y < 0.0 {
        player.vel.y = 0.0;
        if landed_hard && alive {
            sounds.write(PlaySound::global("OOF"));
        }
    }
    player.ground_speed = player.vel.with_y(0.0).length();
    player.view_lag = (player.view_lag + moved.stepped).min(STEP / s);
    player.view_lag *= (-VIEW_CATCH_UP * dt).exp();
    let eye = if alive { player.eye } else { DEAD_EYE / s };
    transform.translation = player.feet + Vec3::Y * (eye - player.view_lag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_match_doom() {
        // 8.33 units a tic walking and 16.7 running.
        assert!((WALK_SPEED / 35.0 - 8.33).abs() < 0.01, "{WALK_SPEED}");
        assert!((RUN_SPEED / 35.0 - 16.67).abs() < 0.01);
    }

    #[test]
    fn damage_heal_and_god() {
        let start = PlayerStart {
            feet: Vec3::ZERO,
            yaw: 0.0,
            mode: Mode::Walk,
            god: false,
            units: 2.0,
        };
        let mut p = Player::new(&start);
        assert!(!p.heal(10, 100));
        assert!(!p.damage(30, Vec3::X));
        assert_eq!(p.health, 70);
        assert!(p.heal(25, 100));
        assert_eq!(p.health, 95);
        assert!(p.heal(25, 100));
        assert_eq!(p.health, 100);
        assert!(p.heal(1, 200));
        assert!(p.damage(500, Vec3::ZERO));
        assert!(!p.alive() && p.health == 0);
        assert!(!p.damage(5, Vec3::ZERO));
        let mut god = Player::new(&PlayerStart { god: true, ..start });
        assert!(!god.damage(500, Vec3::ZERO));
        assert_eq!(god.health, 100);
        assert_eq!(p.body().height, 28.0);
        assert_eq!(p.eye().y, 20.5);
    }
}
