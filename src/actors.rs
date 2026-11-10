//! Things from the map's THINGS lump: monsters and barrels that act, the
//! projectiles monsters fire, decorations and items that stand and animate,
//! and the puffs and blood shots leave.
//!
//! Everything that acts runs at DOOM's 35 tics a second in `FixedUpdate`,
//! following p_enemy.c: a monster stands in its spawn frames looking for the
//! player, wakes when it sees them or hears them fire, then chases in DOOM's
//! eight directions, trying the direct ones first and others when blocked,
//! and attacks with DOOM's odds, damage and frame timing (see `info`). Moves
//! go through the same kinematic collision as the player's, so monsters
//! step up 24 units, fall, and are blocked by each other and the player.
//!
//! Sight is a voxel line trace from the monster's eye to the player's, so a
//! hole shot through a wall lets monsters see through it. Hitscan bullets
//! are traces too, tested against the bodies of the player and other
//! monsters first; zombies' bullets that miss pit the wall. Projectiles fly
//! as DOOM's do and, when they hit the world, carve a small crater through
//! the shared [`Carver`]. A barrel blasts everything within 128 units it
//! can see and carves a big crater.
//!
//! Damage is queued as [`Hit`]s and applied once a tic, so any source, the
//! player's gun included, damages things the same way.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use doom_wad::Thing;

use crate::collide::{move_among, ray_cylinder, trace, Blocker, Body, Solid};
use crate::info::{
    self, Action, DecorInfo, Frame, MissileInfo, MonsterInfo, SeqKind, Then,
};
use crate::player::{Blockers, Player, PLAYER_ID};
use crate::sound::PlaySound;
use crate::sprites::{billboard_bundle, Billboard, SpriteBank};
use crate::weapon::Carver;
use crate::world::{GameMap, WorldSolid};

/// DOOM's tic rate.
pub const TIC_RATE: f64 = 35.0;
/// Units: p_local.h MELEERANGE and MISSILERANGE, and the reach of noise.
const MELEE_RANGE: f32 = 64.0;
const MISSILE_RANGE: f32 = 2048.0;
const NOISE_RANGE: f32 = 1024.0;
/// Units a tic: the lost soul's charge and a flier's climb.
const SKULL_SPEED: f32 = 20.0;
const FLOAT_SPEED: f32 = 4.0;
/// Units a tic a tic.
const GRAVITY: f32 = 1.0;
/// A barrel's blast reaches this far, in units, and carves a crater of this
/// radius, in voxels.
const BLAST_RANGE: f32 = 128.0;
const BARREL_CRATER: i32 = 11;
/// A projectile's crater and a missed bullet's pit, in voxels.
const MISSILE_CRATER: i32 = 3;
const BULLET_PIT: i32 = 1;
/// Tics between sight checks, awake and asleep.
const SIGHT_EVERY_AWAKE: u64 = 3;
const SIGHT_EVERY_ASLEEP: u64 = 6;
/// Tics a projectile flies before it is dropped.
const MISSILE_LIFE: u32 = 35 * 12;
/// The longest trial move when choosing a chase direction, in voxels.
const TRIAL_STEP: f32 = 1.0;
/// Directions: DOOM's east, north-east, … south-east, and none.
const NO_DIR: u8 = 8;

pub struct ActorsPlugin {
    pub things: Vec<Thing>,
    /// Monsters start awake, chasing the player.
    pub awake: bool,
    /// Print where each monster stands.
    pub print: bool,
    /// Things stand still and don't think, for screenshots.
    pub freeze: bool,
}

#[derive(Resource)]
struct SpawnList {
    things: Vec<Thing>,
    awake: bool,
    print: bool,
}

impl Plugin for ActorsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TIC_RATE))
            .insert_resource(SpawnList {
                things: self.things.clone(),
                awake: self.awake,
                print: self.print,
            })
            .init_resource::<Rng>()
            .init_resource::<Tic>()
            .init_resource::<TicProfile>()
            .init_resource::<DamageQueue>()
            .init_resource::<Noise>()
            .add_systems(Startup, spawn_things.after(crate::sprites::setup_sprites))
            .add_systems(
                FixedUpdate,
                (
                    mark::<0>,
                    crate::world::track_edited_chunks,
                    gather_blockers,
                    mark::<1>,
                    update_sight,
                    mark::<2>,
                    tic_actors,
                    mark::<3>,
                    move_actors,
                    mark::<4>,
                    tic_missiles,
                    mark::<5>,
                    apply_damage,
                    tic_decor,
                    tic_effects,
                    count_tic,
                    mark::<6>,
                )
                    .chain()
                    .run_if(|frozen: Res<Frozen>| !frozen.0),
            )
            .insert_resource(Frozen(self.freeze));
    }
}

/// Where tics spend their time, for `--frame-times`: seconds in each stage
/// since the last report, and the longest tic.
#[derive(Resource, Default)]
pub struct TicProfile {
    last: Option<std::time::Instant>,
    /// Edits and blockers, sight, actions, moves, projectiles, the rest.
    pub stages: [f32; 6],
    tic: f32,
    pub longest_tic: f32,
    pub tics: u32,
}

impl TicProfile {
    pub fn report(&mut self) -> String {
        let names = ["setup", "sight", "act", "move", "missiles", "rest"];
        let per = |t: f32| t * 1000.0 / self.tics.max(1) as f32;
        let text = names
            .iter()
            .zip(self.stages)
            .map(|(n, t)| format!("{n} {:.2}", per(t)))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            "{} tics, ms a tic: {text}; longest tic {:.1} ms",
            self.tics,
            self.longest_tic * 1000.0
        );
        *self = TicProfile::default();
        text
    }
}

/// Ends profile stage `N − 1` and starts stage `N`.
fn mark<const N: usize>(mut profile: ResMut<TicProfile>) {
    let now = std::time::Instant::now();
    if N > 0 {
        if let Some(last) = profile.last {
            let t = (now - last).as_secs_f32();
            profile.stages[N - 1] += t;
            profile.tic += t;
        }
    }
    if N == 6 {
        profile.tics += 1;
        profile.longest_tic = profile.longest_tic.max(profile.tic);
        profile.tic = 0.0;
    }
    profile.last = Some(now);
}

/// Whether things are frozen.
#[derive(Resource)]
struct Frozen(bool);

/// DOOM's random numbers: 0 to 255.
#[derive(Resource)]
pub struct Rng(u32);

impl Default for Rng {
    fn default() -> Self {
        Rng(0x9e37_79b9)
    }
}

impl Rng {
    pub fn next(&mut self) -> i32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 24) as i32
    }

    /// `P_Random() − P_Random()`: −255 to 255, most often near 0.
    pub fn spread(&mut self) -> i32 {
        self.next() - self.next()
    }
}

/// Tics since start.
#[derive(Resource, Default)]
pub struct Tic(pub u64);

fn count_tic(mut tic: ResMut<Tic>) {
    tic.0 += 1;
}

/// Who did or takes damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Player,
    Actor(Entity),
    Nobody,
}

/// Damage waiting to land.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub target: Who,
    pub amount: i32,
    pub source: Who,
    /// Where it came from, for the push.
    pub from: Vec3,
}

#[derive(Resource, Default)]
pub struct DamageQueue(pub Vec<Hit>);

/// Where the player last fired, for monsters to hear.
#[derive(Resource, Default)]
pub struct Noise(pub Option<Vec3>);

/// A monster or barrel.
#[derive(Component, Debug)]
pub struct Actor {
    pub info: &'static MonsterInfo,
    seq: SeqKind,
    frame: usize,
    tics: i32,
    /// A sequence to enter at the next tic, set by damage.
    enter: Option<SeqKind>,
    pub health: i32,
    /// In voxels; `vel` in voxels a tic.
    pub feet: Vec3,
    vel: Vec3,
    /// Radians counter-clockwise from DOOM's east.
    pub angle: f32,
    movedir: u8,
    movecount: i32,
    reaction: i32,
    /// Has the player as its target.
    pub awake: bool,
    ambush: bool,
    heard: bool,
    just_attacked: bool,
    just_hit: bool,
    skull_fly: bool,
    skull_hit: Option<Who>,
    pub solid: bool,
    pub shootable: bool,
    float: bool,
    /// Whether it saw the player at its last sight check.
    pub sees: bool,
    phase: u64,
    /// In voxels.
    pub radius: f32,
    pub height: f32,
    blocked: bool,
    killer: Who,
    remove: bool,
}

impl Actor {
    fn new(
        info: &'static MonsterInfo,
        feet: Vec3,
        angle: f32,
        ambush: bool,
        s: f32,
        phase: u64,
    ) -> Self {
        Actor {
            info,
            seq: SeqKind::Spawn,
            frame: 0,
            tics: info.spawn.frames[0].tics,
            enter: None,
            health: info.health,
            feet,
            vel: Vec3::ZERO,
            angle,
            movedir: NO_DIR,
            movecount: 0,
            reaction: info.reaction,
            awake: false,
            ambush,
            heard: false,
            just_attacked: false,
            just_hit: false,
            skull_fly: false,
            skull_hit: None,
            solid: true,
            shootable: true,
            float: info.float,
            sees: false,
            phase,
            radius: info.radius / s,
            height: info.height / s,
            blocked: false,
            killer: Who::Nobody,
            remove: false,
        }
    }

    fn frame(&self) -> &'static Frame {
        &self.info.seq(self.seq).frames[self.frame]
    }

    /// Its eye, where its sight starts: DOOM's three quarters up.
    fn eye(&self) -> Vec3 {
        self.feet + Vec3::Y * self.height * 0.75
    }

    pub fn centre(&self) -> Vec3 {
        self.feet + Vec3::Y * self.height / 2.0
    }

    pub fn alive(&self) -> bool {
        self.shootable
    }

    fn body(&self, s: f32) -> Body {
        Body {
            radius: self.radius,
            height: self.height,
            // Fliers float over what stands in their way.
            step: if self.float { 32.0 } else { 24.0 } / s,
        }
    }

    fn id(entity: Entity) -> u64 {
        entity.to_bits()
    }
}

/// A decoration or item.
#[derive(Component)]
pub struct Decor {
    info: &'static DecorInfo,
    frame: usize,
    tics: i32,
    feet: Vec3,
}

/// A projectile.
#[derive(Component)]
pub struct Missile {
    info: &'static MissileInfo,
    /// Its bottom, as DOOM's z, in voxels; `vel` in voxels a tic.
    pos: Vec3,
    vel: Vec3,
    owner: Option<Entity>,
    owner_info: Option<&'static MonsterInfo>,
    bursting: bool,
    frame: usize,
    tics: i32,
    age: u32,
}

/// A puff of smoke or blood that plays once.
#[derive(Component)]
pub struct Effect {
    frames: &'static [Frame],
    frame: usize,
    tics: i32,
    /// Voxels a tic.
    rise: f32,
}

/// Radians of DOOM's `(P_Random() − P_Random()) << shift`.
fn spread_angle(rng: &mut Rng, shift: u32) -> f32 {
    rng.spread() as f32 * (1u64 << shift) as f32 / 4_294_967_296.0 * std::f32::consts::TAU
}

/// The unit vector of DOOM angle `angle` in voxel space.
fn heading(angle: f32) -> Vec3 {
    Vec3::new(angle.cos(), 0.0, -angle.sin())
}

/// The DOOM angle from `from` to `to`.
fn angle_to(from: Vec3, to: Vec3) -> f32 {
    (from.z - to.z).atan2(to.x - from.x)
}

fn spawn_things(
    mut commands: Commands,
    list: Res<SpawnList>,
    map: Res<GameMap>,
    mut bank: ResMut<SpriteBank>,
) {
    let s = map.units;
    let (mut monsters, mut decor) = (0, 0);
    for (i, thing) in list.things.iter().enumerate() {
        // Skill 3 ("Hurt me plenty": MTF_NORMAL, 2), single player.
        if thing.flags & 2 == 0 || thing.flags & 16 != 0 {
            continue;
        }
        let (x, z) = (thing.x as f32 / s, -(thing.y as f32) / s);
        let Some((floor, ceiling)) =
            map.map.column_air(x.floor() as i32, z.floor() as i32)
        else {
            continue;
        };
        let angle = (thing.angle as f32).to_radians();
        if let Some(info) = info::monster(thing.kind) {
            let feet = Vec3::new(x, floor as f32, z);
            let mut actor =
                Actor::new(info, feet, angle, thing.flags & 8 != 0, s, i as u64);
            if list.awake && !info.inert {
                actor.awake = true;
                actor.seq = SeqKind::See;
                actor.tics = info.see.frames[0].tics;
            }
            let mut board = Billboard::new(info.sprite, b'A', feet);
            board.facing = angle;
            board.shadow = info.shadow;
            if list.print {
                println!(
                    "{} at {:.0},{:.0},{:.0} facing {:.0}°",
                    info.name, feet.x, feet.y, feet.z, thing.angle
                );
            }
            commands.spawn((actor, board, billboard_bundle(&bank)));
            monsters += !info.inert as i32;
        } else if let Some(info) = info::decor_info(thing.kind) {
            let letter = info.frames[0].letter;
            let y = if info.hanging {
                let top = bank.patch(info.sprite, letter).map_or(0, |p| p.top_offset);
                ceiling as f32 - top as f32 / s
            } else {
                floor as f32
            };
            let feet = Vec3::new(x, y, z);
            let mut board = Billboard::new(info.sprite, letter, feet);
            board.facing = angle;
            board.bright = info.frames[0].bright;
            commands.spawn((
                Decor {
                    info,
                    frame: 0,
                    tics: info.frames[0].tics,
                    feet,
                },
                board,
                billboard_bundle(&bank),
            ));
            decor += 1;
        }
    }
    println!("things: {monsters} monsters, {decor} decorations and items");
}

/// Lists what blocks bodies: living solid monsters, solid decorations and
/// the player.
fn gather_blockers(
    mut blockers: ResMut<Blockers>,
    map: Res<GameMap>,
    actors: Query<(Entity, &Actor)>,
    decor: Query<(Entity, &Decor)>,
    player: Single<&Player>,
) {
    let s = map.units;
    blockers.0.clear();
    blockers.0.extend(
        actors
            .iter()
            .filter(|(_, a)| a.solid)
            .map(|(e, a)| Blocker {
                feet: a.feet,
                radius: a.radius,
                height: a.height,
                id: Actor::id(e),
            }),
    );
    blockers.0.extend(
        decor
            .iter()
            .filter(|(_, d)| d.info.solid)
            .map(|(e, d)| Blocker {
                feet: d.feet,
                radius: 16.0 / s,
                height: 64.0 / s,
                id: e.to_bits(),
            }),
    );
    if player.alive() {
        blockers.0.push(player.blocker());
    }
}

/// Traces sight from monsters due a check to the player's eye, in parallel.
fn update_sight(
    tic: Res<Tic>,
    map: Res<GameMap>,
    carver: Carver,
    player: Single<&Player>,
    mut actors: Query<&mut Actor>,
) {
    let world = carver.solid();
    let target = player.eye();
    let alive = player.alive();
    let far = MISSILE_RANGE * 1.5 / map.units;
    actors.par_iter_mut().for_each(|mut a| {
        if a.info.inert || !a.alive() {
            return;
        }
        let every = if a.awake {
            SIGHT_EVERY_AWAKE
        } else {
            SIGHT_EVERY_ASLEEP
        };
        if !(tic.0 + a.phase).is_multiple_of(every) {
            return;
        }
        let eye = a.eye();
        let to = target - eye;
        let distance = to.length();
        a.sees = alive
            && distance < far
            && (distance < 1e-3 || trace(&world, eye, to / distance, distance).is_none());
    });
}

/// What a frame's action asks for.
enum Jump {
    Stay,
    To(SeqKind, usize),
}

/// What actions change.
#[derive(SystemParam)]
struct Acting<'w, 's> {
    commands: Commands<'w, 's>,
    carver: Carver<'w, 's>,
    map: Res<'w, GameMap>,
    bank: Res<'w, SpriteBank>,
    blockers: Res<'w, Blockers>,
    rng: ResMut<'w, Rng>,
    queue: ResMut<'w, DamageQueue>,
    sounds: MessageWriter<'w, PlaySound>,
}

/// Everything an action may touch.
struct Ctx<'a, 'w, 's> {
    p: &'a mut Acting<'w, 's>,
    world: WorldSolid,
    player: &'a Player,
    /// Shootable things: entity, feet, radius, height, info.
    targets: &'a [(Entity, Vec3, f32, f32, &'static MonsterInfo)],
}

impl Ctx<'_, '_, '_> {
    fn s(&self) -> f32 {
        self.p.map.units
    }

    fn sound(&mut self, name: &'static str, at: Vec3) {
        if !name.is_empty() {
            self.p.sounds.write(PlaySound::at(name, at));
        }
    }

    fn sound_of(&mut self, names: &'static [&'static str], at: Vec3) {
        if !names.is_empty() {
            let pick = self.p.rng.next() as usize % names.len();
            self.sound(names[pick], at);
        }
    }

    /// Enters frame `idx` of `kind` and runs actions until one sticks.
    fn enter(&mut self, e: Entity, a: &mut Actor, mut kind: SeqKind, mut idx: usize) {
        for _ in 0..16 {
            a.seq = kind;
            a.frame = idx;
            let frame = a.frame();
            a.tics = frame.tics;
            match self.act(e, a, frame.action) {
                Jump::To(k, i) => {
                    (kind, idx) = (k, i);
                    continue;
                }
                Jump::Stay => {}
            }
            if a.tics != 0 {
                return;
            }
            match next_frame(a) {
                Some((k, i)) => (kind, idx) = (k, i),
                None => {
                    a.remove = true;
                    return;
                }
            }
        }
    }

    fn act(&mut self, e: Entity, a: &mut Actor, action: Action) -> Jump {
        let s = self.s();
        let player = self.player;
        match action {
            Action::Idle => {}
            Action::Look => {
                let to = player.feet - a.feet;
                let behind = heading(a.angle).dot(to.with_y(0.0)) < 0.0;
                let close = to.with_y(0.0).length() * s <= MELEE_RANGE;
                let sees = a.sees && (!behind || close);
                if a.awake || (a.heard && (!a.ambush || a.sees)) || sees {
                    a.awake = true;
                    self.sound_of(a.info.sounds.see, a.feet);
                    return Jump::To(SeqKind::See, 0);
                }
            }
            Action::Chase => return self.chase(e, a),
            Action::Face => self.face(a),
            Action::PosAttack | Action::CPosAttack => {
                self.face(a);
                self.sound(a.info.sounds.attack, a.feet);
                let damage = (self.p.rng.next() % 5 + 1) * 3;
                let spread = spread_angle(&mut self.p.rng, 20);
                self.bullet(e, a, spread, damage);
            }
            Action::SPosAttack => {
                self.face(a);
                self.sound(a.info.sounds.attack, a.feet);
                for _ in 0..3 {
                    let damage = (self.p.rng.next() % 5 + 1) * 3;
                    let spread = spread_angle(&mut self.p.rng, 20);
                    self.bullet(e, a, spread, damage);
                }
            }
            Action::CPosRefire => {
                self.face(a);
                let odds = if a.info.doomednum == info::ARACHNOTRON.doomednum {
                    10
                } else {
                    40
                };
                if self.p.rng.next() >= odds && (!a.awake || !player.alive() || !a.sees) {
                    return Jump::To(SeqKind::See, 0);
                }
            }
            Action::TroopAttack => {
                self.face(a);
                if self.melee_range(a) {
                    self.sound("CLAW", a.feet);
                    let damage = (self.p.rng.next() % 8 + 1) * 3;
                    self.hurt_player(a, e, damage);
                } else {
                    self.missile(e, a, &info::IMP_BALL, 0.0, 0.0);
                }
            }
            Action::SargAttack => {
                self.face(a);
                if self.melee_range(a) {
                    let damage = (self.p.rng.next() % 10 + 1) * 4;
                    self.hurt_player(a, e, damage);
                }
            }
            Action::HeadAttack => {
                self.face(a);
                if self.melee_range(a) {
                    let damage = (self.p.rng.next() % 6 + 1) * 10;
                    self.hurt_player(a, e, damage);
                } else {
                    self.missile(e, a, &info::CACO_BALL, 0.0, 0.0);
                }
            }
            Action::BruisAttack => {
                if self.melee_range(a) {
                    self.sound("CLAW", a.feet);
                    let damage = (self.p.rng.next() % 8 + 1) * 10;
                    self.hurt_player(a, e, damage);
                } else {
                    self.missile(e, a, &info::BARON_BALL, 0.0, 0.0);
                }
            }
            Action::SkullAttack => {
                a.skull_fly = true;
                self.sound(a.info.sounds.attack, a.feet);
                self.face(a);
                let to = player.centre() - a.centre();
                a.vel = to.normalize_or_zero() * SKULL_SPEED / s;
            }
            Action::SkelMissile => {
                self.face(a);
                self.missile(e, a, &info::REVENANT_ROCKET, 0.0, 16.0);
            }
            Action::SkelFist => {
                self.face(a);
                if self.melee_range(a) {
                    self.sound("SKEPCH", a.feet);
                    let damage = (self.p.rng.next() % 10 + 1) * 6;
                    self.hurt_player(a, e, damage);
                }
            }
            Action::FatAttack1 | Action::FatAttack2 | Action::FatAttack3 => {
                self.face(a);
                let spread = 11.25f32.to_radians();
                let turns = match action {
                    Action::FatAttack1 => [0.0, spread],
                    Action::FatAttack2 => [0.0, -2.0 * spread],
                    _ => [-spread / 2.0, spread / 2.0],
                };
                for turn in turns {
                    self.missile(e, a, &info::MANCUBUS_BALL, turn, 0.0);
                }
            }
            Action::BspiAttack => {
                self.face(a);
                self.missile(e, a, &info::PLASMA_BOLT, 0.0, 0.0);
            }
            Action::Pain => self.sound(a.info.sounds.pain, a.feet),
            Action::Scream => self.sound_of(a.info.sounds.death, a.feet),
            Action::XScream => self.sound("SLOP", a.feet),
            Action::Fall => a.solid = false,
            Action::Explode => self.explode(e, a),
        }
        Jump::Stay
    }

    fn face(&mut self, a: &mut Actor) {
        a.angle = angle_to(a.feet, self.player.feet);
    }

    /// A_Chase.
    fn chase(&mut self, e: Entity, a: &mut Actor) -> Jump {
        if a.reaction > 0 {
            a.reaction -= 1;
        }
        // Turn 45° toward the direction it moves in.
        if a.movedir < NO_DIR {
            let step = std::f32::consts::FRAC_PI_4;
            let snapped = (a.angle / step).floor() * step;
            let delta = wrap(snapped - a.movedir as f32 * step);
            a.angle = if delta > 1e-3 {
                snapped - step
            } else if delta < -1e-3 {
                snapped + step
            } else {
                snapped
            };
        }
        if !a.awake || !self.player.alive() {
            a.awake = false;
            a.heard = false;
            return Jump::To(SeqKind::Spawn, 0);
        }
        if a.just_attacked {
            a.just_attacked = false;
            self.new_chase_dir(e, a);
            return Jump::Stay;
        }
        if a.info.melee.is_some() && self.melee_range(a) {
            self.sound(a.info.sounds.attack, a.feet);
            return Jump::To(SeqKind::Melee, 0);
        }
        if a.info.missile.is_some() && a.movecount == 0 && self.missile_range(a) {
            a.just_attacked = true;
            return Jump::To(SeqKind::Missile, 0);
        }
        a.movecount -= 1;
        if a.movecount < 0 || a.blocked {
            self.new_chase_dir(e, a);
        }
        if self.p.rng.next() < 3 {
            self.sound(a.info.sounds.active, a.feet);
        }
        Jump::Stay
    }

    /// P_CheckMeleeRange.
    fn melee_range(&self, a: &Actor) -> bool {
        let s = self.s();
        let player = self.player;
        let to = player.feet - a.feet;
        let reach = MELEE_RANGE - 20.0 + crate::player::RADIUS;
        a.sees
            && player.alive()
            && to.with_y(0.0).length() * s < reach
            && (player.centre().y - a.centre().y).abs() * s < MELEE_RANGE
    }

    /// P_CheckMissileRange.
    fn missile_range(&mut self, a: &mut Actor) -> bool {
        if !a.sees {
            return false;
        }
        if a.just_hit {
            a.just_hit = false;
            return true;
        }
        if a.reaction > 0 {
            return false;
        }
        let to = self.player.feet - a.feet;
        let mut dist = to.with_y(0.0).length() * self.s() - 64.0;
        if a.info.melee.is_none() {
            dist -= 128.0;
        }
        if a.info.doomednum == info::REVENANT.doomednum {
            if dist < 196.0 {
                return false;
            }
            dist /= 2.0;
        }
        if a.info.doomednum == info::LOST_SOUL.doomednum {
            dist /= 2.0;
        }
        let dist = dist.min(200.0);
        self.p.rng.next() as f32 >= dist
    }

    /// Whether `a` could take a chase step in direction `dir`.
    fn try_walk(&mut self, e: Entity, a: &mut Actor, dir: u8) -> bool {
        let s = self.s();
        // A full chase step for small monsters, but at most a voxel: the
        // slab a big monster's box sweeps is large, and what blocks it is
        // already within that.
        let length = (a.info.speed / s).min(TRIAL_STEP);
        let step = heading(dir as f32 * std::f32::consts::FRAC_PI_4) * length;
        let moved = move_among(
            &self.world,
            &a.body(s),
            a.feet,
            step,
            &self.p.blockers.0,
            Actor::id(e),
        );
        if moved.blocked[0] || moved.blocked[2] {
            return false;
        }
        a.movedir = dir;
        a.movecount = self.p.rng.next() & 15;
        a.blocked = false;
        true
    }

    /// P_NewChaseDir.
    fn new_chase_dir(&mut self, e: Entity, a: &mut Actor) {
        let s = self.s();
        let old = a.movedir;
        let turnaround = if old < NO_DIR { (old + 4) % 8 } else { NO_DIR };
        let dx = (self.player.feet.x - a.feet.x) * s;
        let dy = -(self.player.feet.z - a.feet.z) * s;
        let mut d1 = if dx > 10.0 {
            0
        } else if dx < -10.0 {
            4
        } else {
            NO_DIR
        };
        let mut d2 = if dy < -10.0 {
            6
        } else if dy > 10.0 {
            2
        } else {
            NO_DIR
        };
        // The direct diagonal.
        if d1 != NO_DIR && d2 != NO_DIR {
            let diagonal = match (d1, d2) {
                (0, 2) => 1,
                (4, 2) => 3,
                (4, 6) => 5,
                _ => 7,
            };
            if diagonal != turnaround && self.try_walk(e, a, diagonal) {
                return;
            }
        }
        // The two straight directions, the larger first, mostly.
        if self.p.rng.next() > 200 || dy.abs() > dx.abs() {
            std::mem::swap(&mut d1, &mut d2);
        }
        for d in [&mut d1, &mut d2] {
            if *d == turnaround {
                *d = NO_DIR;
            }
        }
        for d in [d1, d2] {
            if d != NO_DIR && self.try_walk(e, a, d) {
                return;
            }
        }
        if old != NO_DIR && self.try_walk(e, a, old) {
            return;
        }
        let order: [u8; 8] = if self.p.rng.next() & 1 == 1 {
            [0, 1, 2, 3, 4, 5, 6, 7]
        } else {
            [7, 6, 5, 4, 3, 2, 1, 0]
        };
        for d in order {
            if d != turnaround && self.try_walk(e, a, d) {
                return;
            }
        }
        if turnaround != NO_DIR && self.try_walk(e, a, turnaround) {
            return;
        }
        a.movedir = NO_DIR;
    }

    fn hurt_player(&mut self, a: &Actor, e: Entity, damage: i32) {
        self.p.queue.0.push(Hit {
            target: Who::Player,
            amount: damage,
            source: Who::Actor(e),
            from: a.feet,
        });
    }

    /// A hitscan bullet from `a` at the player, turned `spread` radians.
    fn bullet(&mut self, e: Entity, a: &Actor, spread: f32, damage: i32) {
        let s = self.s();
        let origin = a.feet + Vec3::Y * (a.height / 2.0 + 8.0 / s);
        let aim = self.player.centre() - origin;
        let flat = aim.with_y(0.0).length().max(1e-3);
        let angle = angle_to(origin, self.player.centre()) + spread;
        let dir = (heading(angle) * flat + Vec3::Y * aim.y).normalize_or_zero();
        let range = MISSILE_RANGE / s;
        let shot = Shot {
            origin,
            dir,
            range,
            source: Who::Actor(e),
        };
        match shot.fire(&self.world, self.player, self.targets) {
            ShotEnd::Thing(target, at) => {
                self.p.queue.0.push(Hit {
                    target,
                    amount: damage,
                    source: Who::Actor(e),
                    from: origin,
                });
                spawn_effect(
                    &mut self.p.commands,
                    &self.p.bank,
                    "BLUD",
                    info::BLOOD,
                    at,
                    0.0,
                );
            }
            ShotEnd::World(hit, at) => {
                self.p.carver.carve(hit.voxel, Some(hit.normal), BULLET_PIT);
                spawn_effect(
                    &mut self.p.commands,
                    &self.p.bank,
                    "PUFF",
                    info::PUFF,
                    at,
                    0.5 / s,
                );
            }
            ShotEnd::Nothing => {}
        }
    }

    /// P_SpawnMissile from `a` at the player, turned `turn` radians, from
    /// `raise` units higher than usual.
    fn missile(
        &mut self,
        e: Entity,
        a: &Actor,
        info: &'static MissileInfo,
        turn: f32,
        raise: f32,
    ) {
        let s = self.s();
        let pos = a.feet + Vec3::Y * (32.0 + raise) / s;
        let dest = self.player.feet;
        let speed = info.speed / s;
        let angle = angle_to(pos, dest) + turn;
        let flat = (dest - pos).with_y(0.0).length();
        let tics = (flat / speed).max(1.0);
        let vel = heading(angle) * speed + Vec3::Y * (dest.y - pos.y) / tics;
        self.sound(info.sound, pos);
        let mut board = Billboard::new(info.sprite, info.fly[0].letter, pos);
        board.bright = true;
        self.p.commands.spawn((
            Missile {
                info,
                // P_CheckMissileSpawn: half a tic's move out of the shooter.
                pos,
                vel,
                owner: Some(e),
                owner_info: Some(a.info),
                bursting: false,
                frame: 0,
                tics: info.fly[0].tics,
                age: 0,
            },
            board,
            billboard_bundle(&self.p.bank),
        ));
    }

    /// A barrel's blast: P_RadiusAttack with 128, then a crater.
    fn explode(&mut self, e: Entity, a: &Actor) {
        let s = self.s();
        let centre = a.feet + Vec3::Y * 4.0 / s;
        let range = BLAST_RANGE / s;
        let mut victims: Vec<(Who, Vec3, f32, f32)> = self
            .targets
            .iter()
            .filter(|t| t.0 != e)
            .map(|t| (Who::Actor(t.0), t.1, t.2, t.3))
            .collect();
        if self.player.alive() {
            let p = self.player;
            victims.push((Who::Player, p.feet, p.body().radius, p.body().height));
        }
        for (who, feet, radius, height) in victims {
            let gap = (feet.x - centre.x).abs().max((feet.z - centre.z).abs()) - radius;
            let dist = gap.max(0.0);
            if dist >= range || (feet.y + height / 2.0 - centre.y).abs() > range {
                continue;
            }
            let to = feet + Vec3::Y * height / 2.0 - centre;
            let len = to.length();
            if len > 1e-3 && trace(&self.world, centre, to / len, len).is_some() {
                continue;
            }
            self.p.queue.0.push(Hit {
                target: who,
                amount: ((range - dist) * s) as i32,
                source: a.killer,
                from: centre,
            });
        }
        self.p.carver.carve(
            centre.floor().as_ivec3() - IVec3::Y * 2,
            None,
            BARREL_CRATER,
        );
    }
}

/// The frame after `a`'s current one, or `None` when it vanishes.
fn next_frame(a: &Actor) -> Option<(SeqKind, usize)> {
    let seq = a.info.seq(a.seq);
    if a.frame + 1 < seq.frames.len() {
        return Some((a.seq, a.frame + 1));
    }
    match seq.then {
        Then::Loop => Some((a.seq, 0)),
        Then::See => Some((SeqKind::See, 0)),
        Then::Goto(n) => Some((a.seq, n)),
        Then::Remove => None,
    }
}

/// An angle wrapped to −π..π.
fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// A hitscan shot.
pub struct Shot {
    pub origin: Vec3,
    pub dir: Vec3,
    pub range: f32,
    pub source: Who,
}

pub enum ShotEnd {
    /// It hit a thing, at a point.
    Thing(Who, Vec3),
    /// It hit the world: the trace, and the point in front of the face.
    World(crate::collide::TraceHit, Vec3),
    Nothing,
}

impl Shot {
    /// The first thing or voxel along the shot. The shooter is never hit.
    pub fn fire(
        &self,
        world: &impl Solid,
        player: &Player,
        targets: &[(Entity, Vec3, f32, f32, &'static MonsterInfo)],
    ) -> ShotEnd {
        let wall = trace(world, self.origin, self.dir, self.range);
        let mut best = wall.map_or(self.range, |h| h.distance);
        let mut who = None;
        if self.source != Who::Player && player.alive() {
            let b = player.body();
            if let Some(t) =
                ray_cylinder(self.origin, self.dir, best, player.feet, b.radius, b.height)
            {
                best = t;
                who = Some(Who::Player);
            }
        }
        for &(e, feet, radius, height, _) in targets {
            if Who::Actor(e) == self.source {
                continue;
            }
            if let Some(t) =
                ray_cylinder(self.origin, self.dir, best, feet, radius, height)
            {
                best = t;
                who = Some(Who::Actor(e));
            }
        }
        let at = self.origin + self.dir * best;
        match (who, wall) {
            (Some(who), _) => ShotEnd::Thing(who, at - self.dir * 0.5),
            (None, Some(hit)) => ShotEnd::World(hit, at - self.dir * 0.5),
            (None, None) => ShotEnd::Nothing,
        }
    }
}

/// Every shootable actor, for shots and blasts.
pub fn shootables(
    actors: &Query<(Entity, &mut Actor, &mut Billboard)>,
) -> Vec<(Entity, Vec3, f32, f32, &'static MonsterInfo)> {
    actors
        .iter()
        .filter(|(_, a, _)| a.shootable)
        .map(|(e, a, _)| (e, a.feet, a.radius, a.height, a.info))
        .collect()
}

pub fn spawn_effect(
    commands: &mut Commands,
    bank: &SpriteBank,
    sprite: &'static str,
    frames: &'static [Frame],
    at: Vec3,
    rise: f32,
) {
    let mut board = Billboard::new(sprite, frames[0].letter, at);
    board.bright = frames[0].bright;
    commands.spawn((
        Effect {
            frames,
            frame: 0,
            tics: frames[0].tics,
            rise,
        },
        board,
        billboard_bundle(bank),
    ));
}

/// Runs every actor's frame timer and actions for one tic.
fn tic_actors(
    mut acting: Acting,
    mut noise: ResMut<Noise>,
    player: Single<&Player>,
    mut actors: Query<(Entity, &mut Actor, &mut Billboard)>,
) {
    let s = acting.map.units;
    if let Some(at) = noise.0.take() {
        for (_, mut a, _) in &mut actors {
            if !a.awake && a.alive() && a.feet.distance(at) * s < NOISE_RANGE {
                a.heard = true;
            }
        }
    }
    let targets = shootables(&actors);
    let world = acting.carver.solid();
    let mut ctx = Ctx {
        p: &mut acting,
        world,
        player: &player,
        targets: &targets,
    };
    for (e, mut a, mut board) in &mut actors {
        let a = &mut *a;
        if let Some(kind) = a.enter.take() {
            ctx.enter(e, a, kind, 0);
        } else if a.tics > 0 {
            a.tics -= 1;
            if a.tics == 0 {
                match next_frame(a) {
                    Some((k, i)) => ctx.enter(e, a, k, i),
                    None => a.remove = true,
                }
            }
        }
        if let Some(who) = a.skull_hit.take() {
            if who != Who::Nobody {
                let damage = (ctx.p.rng.next() % 8 + 1) * a.info.damage;
                ctx.p.queue.0.push(Hit {
                    target: who,
                    amount: damage,
                    source: Who::Actor(e),
                    from: a.feet,
                });
            }
            a.skull_fly = false;
            a.vel = Vec3::ZERO;
            ctx.enter(e, a, SeqKind::Spawn, 0);
        }
        if a.remove {
            ctx.p.commands.entity(e).despawn();
            continue;
        }
        let frame = a.frame();
        board.sprite = a.info.seq(a.seq).sprite.unwrap_or(a.info.sprite);
        board.letter = frame.letter;
        board.bright = frame.bright;
        board.facing = a.angle;
    }
}

/// Moves every actor for one tic: chase steps, the lost soul's charge,
/// fliers' climbs, knockback and gravity. In parallel, since a move reads
/// only the world and last tic's blockers.
fn move_actors(
    map: Res<GameMap>,
    carver: Carver,
    blockers: Res<Blockers>,
    player: Single<&Player>,
    mut actors: Query<(Entity, &mut Actor, &mut Billboard)>,
) {
    let s = map.units;
    let world = carver.solid();
    let target = player.centre();
    let player_alive = player.alive();
    let blockers = &blockers.0;
    actors.par_iter_mut().for_each(|(e, mut a, mut board)| {
        let a = &mut *a;
        let body = a.body(s);
        let none: &[Blocker] = &[];
        let others = if a.solid { blockers.as_slice() } else { none };
        if a.skull_fly {
            let moved = move_among(&world, &body, a.feet, a.vel, others, Actor::id(e));
            a.feet = moved.feet;
            if moved.blocked.iter().any(|&b| b) {
                let reach = a.radius + 2.0 / s;
                a.skull_hit = Some(
                    blockers
                        .iter()
                        .filter(|b| b.id != Actor::id(e))
                        .find(|b| {
                            (b.feet.x - a.feet.x).abs().max((b.feet.z - a.feet.z).abs())
                                < reach + b.radius
                                && a.feet.y < b.feet.y + b.height + 1.0
                                && a.feet.y + a.height > b.feet.y - 1.0
                        })
                        .map_or(Who::Nobody, |b| {
                            if b.id == PLAYER_ID {
                                Who::Player
                            } else {
                                Who::Actor(Entity::from_bits(b.id))
                            }
                        }),
                );
            }
            board.place(a.feet);
            return;
        }
        let mut delta = a.vel.with_y(0.0);
        let frame = a.frame();
        let chasing = a.alive() && frame.action == Action::Chase && a.movedir < NO_DIR;
        if chasing && frame.tics > 0 {
            delta += heading(a.movedir as f32 * std::f32::consts::FRAC_PI_4)
                * a.info.speed
                / s
                / frame.tics as f32;
        }
        if a.float && a.alive() {
            a.vel.y = 0.0;
            if a.awake && player_alive {
                let dist = (target - a.feet).with_y(0.0).length();
                let dz = target.y - a.centre().y;
                if dz < 0.0 && dist < -dz * 3.0 {
                    delta.y -= FLOAT_SPEED / s;
                } else if dz > 0.0 && dist < dz * 3.0 {
                    delta.y += FLOAT_SPEED / s;
                }
            }
        } else {
            a.vel.y -= GRAVITY / s;
            delta.y += a.vel.y;
        }
        let moved = move_among(&world, &body, a.feet, delta, others, Actor::id(e));
        a.feet = moved.feet;
        if chasing {
            a.blocked = moved.blocked[0] || moved.blocked[2];
        }
        if moved.grounded || moved.blocked[1] {
            a.vel.y = 0.0;
        }
        // Knockback slows with DOOM's friction on the ground.
        if moved.grounded || a.float {
            a.vel.x *= 0.90625;
            a.vel.z *= 0.90625;
        }
        if moved.blocked[0] {
            a.vel.x = 0.0;
        }
        if moved.blocked[2] {
            a.vel.z = 0.0;
        }
        board.place(a.feet);
    });
}

/// Flies projectiles, and plays their bursts.
#[allow(clippy::too_many_arguments)]
fn tic_missiles(
    mut commands: Commands,
    mut carver: Carver,
    map: Res<GameMap>,
    mut rng: ResMut<Rng>,
    mut queue: ResMut<DamageQueue>,
    mut sounds: MessageWriter<PlaySound>,
    player: Single<&Player>,
    actors: Query<(Entity, &Actor)>,
    mut missiles: Query<(Entity, &mut Missile, &mut Billboard)>,
) {
    let s = map.units;
    let world = carver.solid();
    let targets: Vec<_> = actors
        .iter()
        .filter(|(_, a)| a.shootable)
        .map(|(e, a)| (e, a.feet, a.radius, a.height, a.info))
        .collect();
    for (e, mut m, mut board) in &mut missiles {
        let m = &mut *m;
        if m.bursting {
            m.tics -= 1;
            if m.tics <= 0 {
                m.frame += 1;
                let Some(frame) = m.info.burst.get(m.frame) else {
                    commands.entity(e).despawn();
                    continue;
                };
                m.tics = frame.tics;
                board.letter = frame.letter;
            }
            board.place(m.pos);
            continue;
        }
        m.age += 1;
        if m.age > MISSILE_LIFE {
            commands.entity(e).despawn();
            continue;
        }
        // The revenant's rocket turns toward the player every 4 tics.
        if m.info.homing && m.age % 4 == 0 && player.alive() {
            let speed = m.vel.with_y(0.0).length();
            let current = angle_to(Vec3::ZERO, m.vel);
            let wanted = angle_to(m.pos, player.centre());
            let turn = wrap(wanted - current).clamp(-0.2945, 0.2945);
            let flat = heading(current + turn) * speed;
            let tics = ((player.centre() - m.pos).with_y(0.0).length() / speed.max(1e-3))
                .max(1.0);
            let slope = (player.centre().y - m.pos.y) / tics;
            let vy =
                m.vel.y + (slope - m.vel.y).clamp(-0.0625 / s * 8.0, 0.0625 / s * 8.0);
            m.vel = flat + Vec3::Y * vy;
        }
        m.tics -= 1;
        if m.tics <= 0 {
            m.frame = (m.frame + 1) % m.info.fly.len();
            m.tics = m.info.fly[m.frame].tics;
            board.letter = m.info.fly[m.frame].letter;
        }
        let half = 4.0 / s;
        let radius = m.info.radius / s;
        let centre = m.pos + Vec3::Y * half;
        let length = m.vel.length();
        let dir = m.vel / length.max(1e-6);
        // Things, their bodies grown by the projectile's radius.
        let wall = trace(&world, centre, dir, length);
        let mut best = wall.map_or(length, |h| h.distance);
        let mut hit_who = None;
        if player.alive() {
            let b = player.body();
            if let Some(t) = ray_cylinder(
                centre,
                dir,
                best,
                player.feet - Vec3::Y * half,
                b.radius + radius,
                b.height + 2.0 * half,
            ) {
                best = t;
                hit_who = Some(Who::Player);
            }
        }
        for &(te, feet, r, h, info) in &targets {
            if Some(te) == m.owner {
                continue;
            }
            if let Some(t) = ray_cylinder(
                centre,
                dir,
                best,
                feet - Vec3::Y * half,
                r + radius,
                h + 2.0 * half,
            ) {
                best = t;
                hit_who = Some(Who::Actor(te));
                // The same species takes no damage, but it still bursts.
                if m.owner_info.is_some_and(|o| std::ptr::eq(o, info)) {
                    hit_who = Some(Who::Nobody);
                }
            }
        }
        m.pos += dir * best;
        let burst = match (hit_who, wall) {
            (Some(who), _) => {
                if who != Who::Nobody {
                    let damage = (rng.next() % 8 + 1) * m.info.damage;
                    queue.0.push(Hit {
                        target: who,
                        amount: damage,
                        source: m.owner.map_or(Who::Nobody, Who::Actor),
                        from: m.pos - dir,
                    });
                }
                true
            }
            (None, Some(hit)) => {
                carver.carve(hit.voxel, Some(hit.normal), MISSILE_CRATER);
                true
            }
            (None, None) => false,
        };
        if burst {
            m.pos -= dir * radius.min(best);
            m.bursting = true;
            m.frame = 0;
            m.tics = m.info.burst[0].tics;
            board.sprite = m.info.burst_sprite;
            board.letter = m.info.burst[0].letter;
            sounds.write(PlaySound::at(m.info.burst_sound, m.pos));
        }
        board.place(m.pos);
    }
}

/// Lands queued damage: P_DamageMobj.
fn apply_damage(
    map: Res<GameMap>,
    mut rng: ResMut<Rng>,
    mut queue: ResMut<DamageQueue>,
    mut sounds: MessageWriter<PlaySound>,
    mut player: Single<&mut Player>,
    mut actors: Query<&mut Actor>,
) {
    let s = map.units;
    for hit in std::mem::take(&mut queue.0) {
        match hit.target {
            Who::Player => {
                let mass = 100.0;
                let push = push(hit.from, player.centre(), hit.amount, mass, s) * 35.0;
                let was_alive = player.alive();
                if player.damage(hit.amount, push) {
                    sounds.write(PlaySound::global(if hit.amount > 50 {
                        "PDIEHI"
                    } else {
                        "PLDETH"
                    }));
                    println!("killed by {} damage", hit.amount);
                } else if was_alive && !player.god {
                    sounds.write(PlaySound::global("PLPAIN"));
                }
            }
            Who::Actor(e) => {
                let Ok(mut a) = actors.get_mut(e) else {
                    continue;
                };
                if !a.shootable {
                    continue;
                }
                if a.skull_fly {
                    a.skull_fly = false;
                    a.vel = Vec3::ZERO;
                }
                let kick = push(hit.from, a.centre(), hit.amount, a.info.mass, s);
                if a.info.speed > 0.0 {
                    a.vel += kick.with_y(0.0);
                }
                a.health -= hit.amount;
                if a.health <= 0 {
                    a.shootable = false;
                    a.float = a.info.doomednum == info::LOST_SOUL.doomednum;
                    a.height /= 4.0;
                    a.killer = hit.source;
                    a.enter = Some(match a.info.xdeath {
                        Some(_) if a.health < -a.info.health => SeqKind::XDeath,
                        _ => SeqKind::Death,
                    });
                    continue;
                }
                if rng.next() < a.info.painchance && !a.info.inert {
                    a.just_hit = true;
                    a.enter = Some(SeqKind::Pain);
                }
                a.reaction = 0;
                if hit.source == Who::Player && !a.info.inert && !a.awake {
                    a.awake = true;
                    if a.seq == SeqKind::Spawn && a.enter.is_none() {
                        a.enter = Some(SeqKind::See);
                    }
                }
            }
            Who::Nobody => {}
        }
    }
}

/// DOOM's thrust from damage: `damage / 8 × 100 / mass` units a tic, away
/// from `from`, in voxels a tic.
fn push(from: Vec3, to: Vec3, damage: i32, mass: f32, s: f32) -> Vec3 {
    let dir = (to - from).with_y(0.0).normalize_or_zero();
    dir * damage as f32 / 8.0 * 100.0 / mass / s
}

/// Animates decorations and items, and picks up health.
fn tic_decor(
    mut commands: Commands,
    mut sounds: MessageWriter<PlaySound>,
    mut player: Single<&mut Player>,
    mut decor: Query<(Entity, &mut Decor, &mut Billboard)>,
) {
    let reach = player.body().radius;
    for (e, mut d, mut board) in &mut decor {
        if let Some((gives, up_to)) = d.info.health {
            let gap = (d.feet.x - player.feet.x)
                .abs()
                .max((d.feet.z - player.feet.z).abs());
            let near = gap < reach + 10.0
                && (d.feet.y - player.feet.y).abs() < player.body().height;
            if near && player.heal(gives, up_to) {
                sounds.write(PlaySound::global("ITEMUP"));
                commands.entity(e).despawn();
                continue;
            }
        }
        if d.tics < 0 {
            continue;
        }
        d.tics -= 1;
        if d.tics <= 0 {
            d.frame = (d.frame + 1) % d.info.frames.len();
            let frame = d.info.frames[d.frame];
            d.tics = frame.tics;
            board.letter = frame.letter;
            board.bright = frame.bright;
        }
    }
}

fn tic_effects(
    mut commands: Commands,
    mut effects: Query<(Entity, &mut Effect, &mut Billboard)>,
) {
    for (e, mut fx, mut board) in &mut effects {
        let at = board.feet + Vec3::Y * fx.rise;
        board.place(at);
        fx.tics -= 1;
        if fx.tics <= 0 {
            fx.frame += 1;
            let Some(frame) = fx.frames.get(fx.frame) else {
                commands.entity(e).despawn();
                continue;
            };
            fx.tics = frame.tics;
            board.letter = frame.letter;
            board.bright = frame.bright;
        }
    }
}

/// The monsters' state, for the HUD: living, awake.
pub fn census(actors: &Query<&Actor>) -> (usize, usize) {
    let living = actors.iter().filter(|a| !a.info.inert && a.alive());
    let (mut alive, mut awake) = (0, 0);
    for a in living {
        alive += 1;
        awake += a.awake as usize;
    }
    (alive, awake)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_is_doom_sized() {
        let mut rng = Rng::default();
        let mut widest = 0f32;
        for _ in 0..1000 {
            widest = widest.max(spread_angle(&mut rng, 20).abs());
        }
        // (255 << 20) / 2³² of a turn is 22.4°.
        assert!(
            widest.to_degrees() < 22.5 && widest.to_degrees() > 10.0,
            "{widest}"
        );
    }

    #[test]
    fn headings_follow_doom_angles() {
        assert!(heading(0.0).abs_diff_eq(Vec3::X, 1e-6));
        // DOOM's north is voxel −z.
        assert!(heading(std::f32::consts::FRAC_PI_2).abs_diff_eq(Vec3::NEG_Z, 1e-6));
        let a = angle_to(Vec3::ZERO, Vec3::new(0.0, 0.0, -5.0));
        assert!((a - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn push_is_dooms_thrust() {
        // 40 damage on mass 100 is 5 units a tic, 2.5 voxels at 2 a voxel.
        let p = push(Vec3::ZERO, Vec3::new(3.0, 1.0, 0.0), 40, 100.0, 2.0);
        assert!(p.abs_diff_eq(Vec3::new(2.5, 0.0, 0.0), 1e-6));
    }
}
