//! The player's weapons, and the one place the world is destroyed.
//!
//! The weapon in hand runs DOOM's player sprite states (p_pspr.c) at 35
//! tics a second: it raises from the bottom of the screen, waits, fires on
//! the trigger through its attack frames at DOOM's rates, refires while the
//! trigger is held, and lowers to switch. The number keys pick a weapon as
//! DOOM's do (1 the fist or chainsaw, 3 the shotgun or super shotgun), and
//! the mouse wheel steps through the weapons with ammo. Each shot takes its
//! ammo; out of ammo, the weapon lowers and the best one left comes up.
//!
//! Hitscans are rays from the camera along its forward vector, so they go
//! where the crosshair is, spread as DOOM spreads them: the pistol and
//! chaingun after the first shot of a burst, every pellet of the shotguns,
//! and the fist and chainsaw within DOOM's 64 units. A ray hits the first
//! monster or barrel whose body it crosses for DOOM's damage, or else
//! carves the first solid voxel it reaches. The rocket launcher, plasma
//! rifle and BFG fire projectiles (see `actors`). Every attack wakes
//! monsters within 1024 units.
//!
//! How much each weapon carves grows with `--blast-radius` R (4 voxels by
//! default): a bullet R, a pellet or chainsaw bite R/2, a punch 1 (R/2 with
//! berserk), a plasma ball 3R/4, a rocket 2R and the BFG's ball 4R.
//!
//! [`Carver::carve`] does every carve: the player's shots, monsters'
//! bullets and projectiles, and barrels. It blows a rough ball out of the
//! world, its edge jittered per voxel so it reads as a chunk blown out, and
//! scorches a thin shell of solid voxels around it. Scorched voxels are
//! darkened from the colour the map generates there, so carving the same
//! spot again doesn't darken it further.
//!
//! `--fire N` fires N chaingun bullets by itself once the world has
//! settled, spread in a fixed pattern around the view centre, without ammo.
//!
//! In the light modes that propagate light, a hitscan also flashes: the
//! voxel just in front of the face it hits, along the face's normal, glows
//! for `--flash-time` seconds and goes back to air. The ray came through
//! that voxel, so it is air, and the carve leaves it alone, since it only
//! empties solid voxels. Rays and bodies pass through flashes.

use std::sync::Arc;
use std::time::Instant;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_voxel_world::prelude::*;
use doom_voxelize::Voxel;

use crate::actors::{
    spawn_effect, spawn_missile, Actor, DamageQueue, Hit, Noise, Rng, Shot, ShotEnd, Who,
};
use crate::camera::FlyCamera;
use crate::capture::{CaptureSet, WorldSettled};
use crate::debris::SprayDebris;
use crate::info::{self, weapon_info, MissileInfo, WAction, WFrame};
use crate::input::Intent;
use crate::inventory::{Inventory, WeaponKind};
use crate::movers::MoverSolids;
use crate::player::Player;
use crate::sound::PlaySound;
use crate::sprites::SpriteBank;
use crate::world::{material, DoomWorld, EditedChunks, GameMap, WorldSolid};

/// DOOM's chaingun: a shot every 4 tics at 35 tics a second.
pub const SHOT_INTERVAL: f32 = 4.0 / 35.0;
/// p_pspr.c: the weapon's height on DOOM's 320×200 screen in hand and out
/// of sight, and how far it moves a tic raising and lowering.
pub const WEAPON_TOP: f32 = 32.0;
const WEAPON_BOTTOM: f32 = 128.0;
const RAISE_SPEED: f32 = 6.0;
/// p_local.h `MELEERANGE` and `MISSILERANGE`, in units.
const MELEE_RANGE: f32 = 64.0;
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
    /// A bullet's carved ball radius in voxels; other weapons scale it.
    pub radius: i32,
    /// How far a shot reaches, in voxels.
    pub range: f32,
    /// Shots to fire once the world has settled.
    pub scripted_shots: u32,
    /// Seconds a flash glows.
    pub flash_time: f32,
}

/// Where the weapon sprite is in its states: p_pspr.c's `ps_weapon`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Psp {
    /// A_Raise, each tic until it is up.
    Raising,
    /// A_Lower, each tic until it is down.
    Lowering,
    /// Frame `frame` of the weapon's ready or attack frames, with tics left.
    Ready {
        frame: usize,
        tics: i32,
    },
    Attack {
        frame: usize,
        tics: i32,
    },
}

/// A weapon switch asked for: a number key, or wheel steps.
#[derive(Clone, Copy, Debug)]
enum Change {
    Slot(u8),
    Step(i32),
}

/// The weapon in hand's state, which the HUD draws.
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
    psp: Psp,
    /// The weapon's height on DOOM's 320×200 screen: `WEAPON_TOP` in hand.
    pub sy: f32,
    /// The muzzle flash showing: its frames, the frame, and tics left.
    flash: Option<(&'static [WFrame], usize, i32)>,
    /// Shots fired holding the trigger since the first: the pistol and
    /// chaingun spread from the second on.
    refire: u32,
    /// The trigger was held at the last ready frame: the rocket launcher
    /// and BFG don't fire on a trigger held from before they came up.
    attack_down: bool,
    /// A switch asked for since the last tic.
    wanted: Option<Change>,
}

impl Weapon {
    /// The sprite, frame letter and full-brightness of the weapon in hand.
    pub fn frame(&self, inventory: &Inventory) -> (&'static str, u8, bool) {
        let info = weapon_info(inventory.ready);
        let frame = match self.psp {
            Psp::Raising | Psp::Lowering => info.ready[0],
            Psp::Ready { frame, .. } => info.ready[frame],
            Psp::Attack { frame, .. } => info.attack[frame],
        };
        (info.sprite, frame.letter, frame.bright)
    }

    /// The muzzle flash's sprite and frame letter, while it shows.
    pub fn flash_frame(&self, inventory: &Inventory) -> Option<(&'static str, u8)> {
        let (frames, i, _) = self.flash?;
        Some((weapon_info(inventory.ready).flash_sprite, frames[i].letter))
    }

    /// Whether it waits to fire, so it bobs as the player walks.
    pub fn bobbing(&self) -> bool {
        matches!(self.psp, Psp::Ready { .. })
    }
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
            // P_SetupPsprites: the weapon comes up as the map starts.
            psp: Psp::Raising,
            sy: WEAPON_BOTTOM,
            flash: None,
            refire: 0,
            attack_down: false,
            wanted: None,
        })
        .add_systems(
            Startup,
            drop_missing_weapons
                .after(crate::camera::spawn_camera)
                .after(crate::sprites::setup_sprites),
        )
        .add_systems(Update, ask_for_switch.after(crate::player::walk))
        .add_systems(FixedUpdate, tic_weapon)
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
    movers: Res<'w, MoverSolids>,
    spray: MessageWriter<'w, SprayDebris>,
}

impl Carver<'_, '_> {
    /// What bodies and shots collide with now.
    pub fn solid(&self) -> WorldSolid {
        WorldSolid::new(&self.voxel_world, &self.map, &self.edited, &self.movers)
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

/// Takes away the weapons the WAD has no sprites for, such as the super
/// shotgun in DOOM, so `--weapons all` gives what the WAD can show.
fn drop_missing_weapons(bank: Res<SpriteBank>, mut player: Single<&mut Player>) {
    for w in WeaponKind::ALL {
        if player.inventory.has(w) && !bank.has(weapon_info(w).sprite, b'A') {
            println!(
                "this WAD has no {} sprites: no {}",
                weapon_info(w).sprite,
                w.name()
            );
            player.inventory.remove(w);
        }
    }
}

/// Keeps the number key or wheel step of this frame for the next tic.
fn ask_for_switch(intent: Res<Intent>, mut weapon: ResMut<Weapon>) {
    if let Some(slot) = intent.weapon_slot {
        weapon.wanted = Some(Change::Slot(slot));
    } else if intent.weapon_step != 0 {
        let steps = match weapon.wanted {
            Some(Change::Step(n)) => n + intent.weapon_step,
            _ => intent.weapon_step,
        };
        weapon.wanted = Some(Change::Step(steps));
    }
}

/// Runs the weapon one tic: p_user.c `P_PlayerThink`'s switch, then
/// p_pspr.c `P_MovePsprites`.
fn tic_weapon(
    intent: Res<Intent>,
    mut weapon: ResMut<Weapon>,
    camera: Single<(&Transform, &mut Player), With<FlyCamera>>,
    mut arms: Arms,
) {
    let (transform, mut player) = camera.into_inner();
    if let Some(change) = weapon.wanted.take() {
        let inventory = &mut player.inventory;
        let to = match change {
            Change::Slot(slot) => inventory.for_slot(slot),
            Change::Step(steps) => inventory.stepped(steps),
        };
        if to.is_some() {
            inventory.pending = to;
        }
    }
    let mut hand = Hand {
        weapon: &mut weapon,
        arms: &mut arms,
        player: &mut player,
        eye: transform.translation,
        rotation: transform.rotation,
        fire: intent.fire,
    };
    hand.tic();
}

/// What the weapon touches besides the world.
#[derive(SystemParam)]
pub struct Arms<'w, 's> {
    commands: Commands<'w, 's>,
    carver: Carver<'w, 's>,
    rng: ResMut<'w, Rng>,
    queue: ResMut<'w, DamageQueue>,
    noise: ResMut<'w, Noise>,
    sounds: MessageWriter<'w, PlaySound>,
    bank: Res<'w, SpriteBank>,
    map: Res<'w, GameMap>,
    actors: Query<'w, 's, (Entity, &'static Actor)>,
}

/// The weapon in the player's hand for one tic.
struct Hand<'a, 'w, 's> {
    weapon: &'a mut Weapon,
    arms: &'a mut Arms<'w, 's>,
    player: &'a mut Player,
    /// The camera: shots start at its position and go along its view.
    eye: Vec3,
    rotation: Quat,
    /// The trigger is held.
    fire: bool,
}

/// What a hitscan struck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Struck {
    Thing,
    World,
    Nothing,
}

impl Hand<'_, '_, '_> {
    fn tic(&mut self) {
        // P_DropWeapon: down it goes on death, and comes back up on respawn.
        if !self.player.alive() && self.weapon.psp != Psp::Lowering {
            self.weapon.flash = None;
            self.set(Psp::Lowering);
            return;
        }
        match self.weapon.psp {
            psp @ (Psp::Raising | Psp::Lowering) => self.set(psp),
            Psp::Ready { frame, tics } | Psp::Attack { frame, tics } if tics > 0 => {
                let left = tics - 1;
                let psp = match self.weapon.psp {
                    Psp::Ready { .. } => Psp::Ready { frame, tics: left },
                    _ => Psp::Attack { frame, tics: left },
                };
                self.weapon.psp = psp;
                if left == 0 {
                    let next = self.next(psp);
                    self.set(next);
                }
            }
            _ => {}
        }
        if let Some((frames, i, tics)) = &mut self.weapon.flash {
            *tics -= 1;
            if *tics <= 0 {
                *i += 1;
                match frames.get(*i) {
                    Some(f) => *tics = f.tics,
                    None => self.weapon.flash = None,
                }
            }
        }
    }

    fn ready_weapon(&self) -> WeaponKind {
        self.player.inventory.ready
    }

    /// The frame after `psp`: ready frames loop, attack frames end in ready.
    fn next(&self, psp: Psp) -> Psp {
        let info = weapon_info(self.ready_weapon());
        match psp {
            Psp::Ready { frame, .. } => Psp::Ready {
                frame: (frame + 1) % info.ready.len(),
                tics: 0,
            },
            Psp::Attack { frame, .. } if frame + 1 < info.attack.len() => Psp::Attack {
                frame: frame + 1,
                tics: 0,
            },
            _ => Psp::Ready { frame: 0, tics: 0 },
        }
    }

    /// p_pspr.c `P_SetPsprite`: enters `to` and runs actions, on through
    /// frames of no tics and the jumps actions make.
    fn set(&mut self, mut to: Psp) {
        for _ in 0..16 {
            let info = weapon_info(self.ready_weapon());
            let jump = match to {
                Psp::Raising => {
                    self.weapon.psp = to;
                    self.raise()
                }
                Psp::Lowering => {
                    self.weapon.psp = to;
                    self.lower()
                }
                Psp::Ready { frame, .. } => {
                    let f = info.ready[frame];
                    self.weapon.psp = Psp::Ready {
                        frame,
                        tics: f.tics,
                    };
                    self.act(f.action, frame)
                }
                Psp::Attack { frame, .. } => {
                    let f = info.attack[frame];
                    self.weapon.psp = Psp::Attack {
                        frame,
                        tics: f.tics,
                    };
                    self.act(f.action, frame)
                }
            };
            to = match (jump, self.weapon.psp) {
                (Some(next), _) => next,
                (
                    None,
                    psp @ (Psp::Ready { tics: 0, .. } | Psp::Attack { tics: 0, .. }),
                ) => self.next(psp),
                _ => return,
            };
        }
    }

    /// A_Raise.
    fn raise(&mut self) -> Option<Psp> {
        self.weapon.sy -= RAISE_SPEED;
        if self.weapon.sy > WEAPON_TOP {
            return None;
        }
        self.weapon.sy = WEAPON_TOP;
        Some(Psp::Ready { frame: 0, tics: 0 })
    }

    /// A_Lower: once down, the pending weapon comes up, unless the player
    /// is dead (P_BringUpWeapon).
    fn lower(&mut self) -> Option<Psp> {
        self.weapon.sy += RAISE_SPEED;
        if self.weapon.sy < WEAPON_BOTTOM {
            return None;
        }
        self.weapon.sy = WEAPON_BOTTOM;
        if !self.player.alive() {
            return None;
        }
        let inventory = &mut self.player.inventory;
        inventory.ready = inventory.pending.take().unwrap_or(inventory.ready);
        if inventory.ready == WeaponKind::Chainsaw {
            self.sound("SAWUP");
        }
        Some(Psp::Raising)
    }

    /// p_pspr.c `P_CheckAmmo`: whether the weapon in hand can fire; if not,
    /// the best one left is pending and this one lowers.
    fn check_ammo(&mut self) -> bool {
        let inventory = &mut self.player.inventory;
        if inventory.can_fire(inventory.ready) {
            return true;
        }
        inventory.pending = Some(inventory.best_with_ammo());
        false
    }

    /// p_pspr.c `P_FireWeapon`.
    fn fire_weapon(&mut self) -> Option<Psp> {
        if !self.check_ammo() {
            return Some(Psp::Lowering);
        }
        self.arms.noise.0 = Some(self.eye);
        Some(Psp::Attack { frame: 0, tics: 0 })
    }

    /// A_ReFire.
    fn refire(&mut self) -> Option<Psp> {
        if self.fire && self.player.inventory.pending.is_none() && self.player.alive() {
            self.weapon.refire += 1;
            return self.fire_weapon();
        }
        self.weapon.refire = 0;
        (!self.check_ammo()).then_some(Psp::Lowering)
    }

    /// The action of a frame, `frame` of its sequence: where it jumps.
    fn act(&mut self, action: WAction, frame: usize) -> Option<Psp> {
        let weapon = self.ready_weapon();
        match action {
            WAction::None => {}
            WAction::Ready => {
                if self.player.inventory.pending.is_some() || !self.player.alive() {
                    return Some(Psp::Lowering);
                }
                if weapon == WeaponKind::Chainsaw && frame == 0 {
                    self.sound("SAWIDL");
                }
                if self.fire {
                    let held = matches!(weapon, WeaponKind::Missile | WeaponKind::Bfg);
                    if !self.weapon.attack_down || !held {
                        self.weapon.attack_down = true;
                        return self.fire_weapon();
                    }
                } else {
                    self.weapon.attack_down = false;
                }
            }
            WAction::ReFire => return self.refire(),
            WAction::GunFlash => self.start_flash(0),
            WAction::Punch => {
                let mut damage = (self.arms.rng.next() % 10 + 1) * 2;
                let berserk = self.player.inventory.berserk;
                if berserk {
                    damage *= 10;
                }
                let yaw = self.spread(18);
                let dir = self.turned(yaw, 0.0);
                let carve = if berserk { self.half() } else { 1 };
                let reach = MELEE_RANGE / self.arms.map.units;
                if self.line_attack(dir, reach, damage, carve) == Struck::Thing {
                    self.sound("PUNCH");
                }
            }
            WAction::Saw => {
                let damage = 2 * (self.arms.rng.next() % 10 + 1);
                let yaw = self.spread(18);
                let dir = self.turned(yaw, 0.0);
                let reach = (MELEE_RANGE + 1.0) / self.arms.map.units;
                let carve = self.half();
                let struck = self.line_attack(dir, reach, damage, carve);
                self.sound(if struck == Struck::Thing {
                    "SAWHIT"
                } else {
                    "SAWFUL"
                });
            }
            WAction::Pistol => {
                self.sound("PISTOL");
                self.player.inventory.use_ammo();
                self.start_flash(0);
                self.gun_shot(self.weapon.refire == 0, self.weapon.radius);
            }
            WAction::Shotgun => {
                self.sound("SHOTGN");
                self.player.inventory.use_ammo();
                self.start_flash(0);
                for _ in 0..7 {
                    self.gun_shot(false, self.half());
                }
            }
            WAction::SuperShotgun => {
                self.sound("DSHTGN");
                self.player.inventory.use_ammo();
                self.start_flash(0);
                for _ in 0..20 {
                    let damage = 5 * (self.arms.rng.next() % 3 + 1);
                    let yaw = self.spread(19);
                    // (P_Random() − P_Random()) << 5 on the slope.
                    let slope = self.arms.rng.spread() as f32 * 32.0 / 65536.0;
                    let dir = self.turned(yaw, slope.atan());
                    self.line_attack(dir, self.weapon.range, damage, self.half());
                }
            }
            WAction::CheckReload => {
                if !self.check_ammo() {
                    return Some(Psp::Lowering);
                }
            }
            WAction::OpenShotgun2 => self.sound("DBOPN"),
            WAction::LoadShotgun2 => self.sound("DBLOAD"),
            WAction::CloseShotgun2 => {
                self.sound("DBCLS");
                return self.refire();
            }
            WAction::ChainGun => {
                if !self.player.inventory.can_fire(weapon) {
                    return None;
                }
                self.sound("PISTOL");
                self.player.inventory.use_ammo();
                self.start_flash(frame);
                self.gun_shot(self.weapon.refire == 0, self.weapon.radius);
            }
            WAction::Missile => {
                self.player.inventory.use_ammo();
                self.launch(&info::ROCKET, 2 * self.weapon.radius);
            }
            WAction::Plasma => {
                self.player.inventory.use_ammo();
                let which = (self.arms.rng.next() & 1) as usize;
                self.start_flash(which);
                let crater = (self.weapon.radius * 3 / 4).max(1);
                self.launch(&info::PLASMA_BALL, crater);
            }
            WAction::BfgSound => self.sound("BFG"),
            WAction::Bfg => {
                self.player.inventory.use_ammo();
                self.launch(&info::BFG_BALL, 4 * self.weapon.radius);
            }
        }
        None
    }

    fn sound(&mut self, name: &'static str) {
        self.arms.sounds.write(PlaySound::global(name));
    }

    /// Half the bullet's carve: a pellet's, the chainsaw's.
    fn half(&self) -> i32 {
        (self.weapon.radius / 2).max(1)
    }

    /// Starts muzzle flash `which` of the weapon in hand.
    fn start_flash(&mut self, which: usize) {
        let flashes = weapon_info(self.ready_weapon()).flashes;
        if let Some(frames) = flashes.get(which).or(flashes.first()) {
            self.weapon.flash = Some((frames, 0, frames[0].tics));
        }
    }

    /// Radians of DOOM's `(P_Random() − P_Random()) << shift`.
    fn spread(&mut self, shift: u32) -> f32 {
        self.arms.rng.spread() as f32 * (1u64 << shift) as f32 / 4_294_967_296.0
            * std::f32::consts::TAU
    }

    /// The view turned `yaw` radians left and `pitch` up.
    fn turned(&self, yaw: f32, pitch: f32) -> Vec3 {
        let forward = self.rotation * Vec3::NEG_Z;
        let right = self.rotation * Vec3::X;
        Quat::from_rotation_y(yaw) * (Quat::from_axis_angle(right, pitch) * forward)
    }

    /// p_pspr.c `P_GunShot`: a bullet for 5 × (1 to 3), along the view if
    /// `accurate`, else spread, carving `carve` voxels.
    fn gun_shot(&mut self, accurate: bool, carve: i32) {
        let damage = 5 * (self.arms.rng.next() % 3 + 1);
        let yaw = if accurate { 0.0 } else { self.spread(18) };
        let dir = self.turned(yaw, 0.0);
        self.line_attack(dir, self.weapon.range, damage, carve);
    }

    /// A hitscan from the eye along `dir` for `range` voxels: hurts the first
    /// thing it crosses, or carves the world `carve` voxels round.
    fn line_attack(&mut self, dir: Vec3, range: f32, damage: i32, carve: i32) -> Struck {
        let origin = self.eye;
        let arms = &mut *self.arms;
        let targets: Vec<_> = arms
            .actors
            .iter()
            .filter(|(_, a)| a.shootable)
            .map(|(e, a)| (e, a.feet, a.radius, a.height, a.info))
            .collect();
        let world = arms.carver.solid();
        let shot = Shot {
            origin,
            dir,
            range,
            source: Who::Player,
        };
        match shot.fire(&world, self.player, &targets) {
            ShotEnd::Thing(target, at) => {
                arms.queue.0.push(Hit {
                    target,
                    amount: damage,
                    source: Who::Player,
                    from: origin,
                });
                let barrel = matches!(target, Who::Actor(e) if arms.actors.get(e).is_ok_and(|(_, a)| a.info.inert));
                let (sprite, frames) = if barrel {
                    ("PUFF", info::PUFF)
                } else {
                    ("BLUD", info::BLOOD)
                };
                spawn_effect(&mut arms.commands, &arms.bank, sprite, frames, at, 0.0);
                Struck::Thing
            }
            ShotEnd::World(hit, at) => {
                self.weapon.landing = Some((Instant::now(), 0, 0.0));
                arms.carver.carve(hit.voxel, Some(hit.normal), carve);
                let rise = 0.5 / arms.map.units;
                spawn_effect(
                    &mut arms.commands,
                    &arms.bank,
                    "PUFF",
                    info::PUFF,
                    at,
                    rise,
                );
                light_flash(
                    &mut arms.carver,
                    self.weapon,
                    &arms.map,
                    hit.voxel + hit.normal,
                );
                Struck::World
            }
            ShotEnd::Nothing => Struck::Nothing,
        }
    }

    /// p_mobj.c `P_SpawnPlayerMissile`: a projectile from the eye along the
    /// view, carving `crater` voxels where it bursts.
    fn launch(&mut self, info: &'static MissileInfo, crater: i32) {
        let s = self.arms.map.units;
        let dir = self.rotation * Vec3::NEG_Z;
        // A projectile's position is its bottom; its middle starts at the eye.
        let pos = self.eye - Vec3::Y * 4.0 / s;
        if !info.sound.is_empty() {
            self.sound(info.sound);
        }
        spawn_missile(
            &mut self.arms.commands,
            &self.arms.bank,
            info,
            pos,
            dir * info.speed / s,
            Who::Player,
            None,
            crater,
        );
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
    mut shots: ResMut<ScriptedShots>,
    mut settled: ResMut<WorldSettled>,
    camera: Single<(&Transform, &mut Player), With<FlyCamera>>,
    mut weapon: ResMut<Weapon>,
    mut arms: Arms,
) {
    if shots.fired >= shots.total || (shots.fired == 0 && !settled.is_settled()) {
        return;
    }
    let (camera, mut player) = camera.into_inner();
    if shots.fired == 0 {
        let world = arms.carver.solid();
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
    let mut hand = Hand {
        weapon: &mut weapon,
        arms: &mut arms,
        player: &mut player,
        eye: camera.translation,
        rotation: camera.rotation * aim,
        fire: false,
    };
    hand.sound("PISTOL");
    hand.arms.noise.0 = Some(camera.translation);
    let damage = 5 * (hand.arms.rng.next() % 3 + 1);
    let (range, radius) = (hand.weapon.range, hand.weapon.radius);
    let dir = hand.turned(0.0, 0.0);
    match hand.line_attack(dir, range, damage, radius) {
        Struck::World => println!("shot {}: hit the world", i + 1),
        _ => println!("shot {}: hit a thing or nothing", i + 1),
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
