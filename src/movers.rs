//! Doors, lifts and moving floors as sub-grids, and the use key that sets
//! them off.
//!
//! The world is built with every moving plane at the end of its travel that
//! leaves the most air: doors open, lifts down, floors at their lowest (see
//! `specials`). The voxels between that end and the other, as a second build
//! of the map with the plane there colours them, go into a sub-grid of their
//! own, which slides along y as the plane moves. A door is a slab that rises
//! into the solid above its frame; a lift is a block that sinks into the
//! solid below. At the world's end of its travel a sub-grid is hidden, since
//! its face would lie on the world's.
//!
//! Movers run at DOOM's 35 tics a second with DOOM's speeds and waits, and
//! are drawn between tics. Collision sees each one through
//! [`MoverSolids`]: the rows of its sector's columns between the world's
//! plane and the moving one are solid, whole rows only, so a body stands up
//! to half a voxel off the drawn lift top while it moves. A rising floor
//! carries what stands on it; a closing door that would close on a body goes
//! back up, as DOOM's do.
//!
//! Space uses the line in front of the player within DOOM's 64 units, as
//! p_map.c `P_UseLines` does: manual doors, and switches, which flip
//! between their `SW1` and `SW2` textures by editing the world's voxels.
//! A keyed door or switch does nothing without a key of its colour, but
//! says which it needs.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_voxel_world::prelude::*;
use doom_voxelize::{height_row, VoxelMap};
use doom_wad::MapData;

use crate::actors::Actor;
use crate::hud::HudMessage;
use crate::input::Intent;
use crate::levels::LevelExit;
use crate::player::{Mode, Player};
use crate::sound::PlaySound;
use crate::specials::{
    DoorOp, Effect, LineUse, Mover, Plane, Specials, DOOR_WAIT, PLAT_WAIT,
};
use crate::world::{world_voxel, DoomWorld, GameMap};

/// p_local.h `USERANGE`, in map units.
const USE_RANGE: f32 = 64.0;
/// p_switch.c `BUTTONTIME`: a button pops back after a second.
const BUTTON_TICS: u32 = 35;

/// A linedef as the use trace sees it, in map units.
struct UseLine {
    a: Vec2,
    b: Vec2,
    front: Option<u16>,
    back: Option<u16>,
}

/// What a mover is doing.
#[derive(Clone, Copy, Debug)]
enum Thinker {
    /// p_doors.c `T_VerticalDoor`. `dir` is 1 rising, 0 waiting, −1 closing.
    Door {
        op: DoorOp,
        speed: f32,
        blaze: bool,
        dir: i8,
        wait: u32,
    },
    /// p_plats.c `T_PlatRaise` for `downWaitUpStay`.
    Plat {
        speed: f32,
        low: f32,
        high: f32,
        down: bool,
        wait: u32,
    },
    /// p_floor.c `T_MoveFloor`.
    Floor { speed: f32, target: f32 },
}

/// A moving sector at run time.
struct Running {
    spec: Mover,
    /// The sub-grid holding its voxels, once spawned.
    grid: Option<Entity>,
    /// The plane's height in map units, now and at the tic before.
    height: f32,
    prev: f32,
    thinker: Option<Thinker>,
    /// Its columns' box, `min..=max` in x and z, and its centre in voxels.
    min: IVec2,
    max: IVec2,
    centre: Vec2,
    /// Tics it has moved, for the floor's grinding sound.
    moving: u32,
}

/// Every mover, the use lines and the buttons waiting to pop back.
#[derive(Resource)]
pub struct Movers {
    running: Vec<Running>,
    by_sector: Vec<Option<usize>>,
    specials: Specials,
    use_lines: Vec<UseLine>,
    /// The map built with every mover at its other end, for the sub-grids'
    /// voxels. Dropped once they are filled.
    shut: Option<Arc<VoxelMap>>,
    /// Lines whose switch texture is flipped, with tics left before it pops
    /// back, `None` for a switch that stays.
    flipped: Vec<(u16, Option<u32>)>,
}

/// The rows each mover makes solid, for collision: see [`MoverSolids::solid`].
#[derive(Resource, Clone, Default)]
pub struct MoverSolids(pub Arc<Vec<Option<(Plane, i32, i32)>>>);

impl MoverSolids {
    /// Whether row `y` of a column of `sector` is filled by its mover: the
    /// rows between the world's plane and the moving one.
    pub fn solid(&self, sector: u16, y: i32) -> bool {
        match self.0.get(sector as usize) {
            Some(Some((Plane::Floor, open, now))) => (*open..*now).contains(&y),
            Some(Some((Plane::Ceiling, open, now))) => (*now..*open).contains(&y),
            _ => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Movers {
    /// The movers of `map`, whose world `world` is built with
    /// [`Specials::open_heights`] and `shut` with [`Specials::shut_heights`].
    pub fn new(
        map: &MapData,
        specials: Specials,
        world: &VoxelMap,
        shut: Arc<VoxelMap>,
    ) -> Self {
        let mut by_sector = vec![None; map.sectors.len()];
        let mut running = Vec::new();
        for spec in &specials.movers {
            let columns = world.sector_columns(spec.sector);
            if columns.is_empty() {
                continue;
            }
            let mut min = IVec2::splat(i32::MAX);
            let mut max = IVec2::splat(i32::MIN);
            let mut sum = Vec2::ZERO;
            for &[x, z] in &columns {
                min = min.min(IVec2::new(x, z));
                max = max.max(IVec2::new(x, z));
                sum += Vec2::new(x as f32, z as f32) + 0.5;
            }
            let start = spec.start() as f32;
            by_sector[spec.sector as usize] = Some(running.len());
            running.push(Running {
                spec: spec.clone(),
                grid: None,
                height: start,
                prev: start,
                thinker: None,
                min,
                max,
                centre: sum / columns.len() as f32,
                moving: 0,
            });
        }
        let sector_of = |side: Option<u16>| {
            side.and_then(|i| map.sidedefs.get(i as usize))
                .map(|sd| sd.sector)
        };
        let use_lines = map
            .linedefs
            .iter()
            .map(|l| {
                let v = |i: u16| {
                    let v = map.vertexes[i as usize];
                    Vec2::new(v.x as f32, v.y as f32)
                };
                UseLine {
                    a: v(l.v1),
                    b: v(l.v2),
                    front: sector_of(l.front),
                    back: sector_of(l.back),
                }
            })
            .collect();
        Movers {
            running,
            by_sector,
            specials,
            use_lines,
            shut: Some(shut),
            flipped: Vec::new(),
        }
    }

    fn mover_of(&self, sector: u16) -> Option<usize> {
        self.by_sector.get(sector as usize).copied().flatten()
    }

    /// A sector's floor and ceiling now, in map units, where its stored
    /// heights are `stored`.
    fn heights_now(&self, sector: u16, stored: (i32, i32)) -> (f32, f32) {
        let (f, c) = (stored.0 as f32, stored.1 as f32);
        match self.mover_of(sector).map(|i| &self.running[i]) {
            Some(r) if r.spec.plane == Plane::Floor => (r.height, c),
            Some(r) => (f, r.height),
            None => (f, c),
        }
    }

    fn snapshot(&self, s: f32) -> MoverSolids {
        let mut rows = vec![None; self.by_sector.len()];
        for r in &self.running {
            rows[r.spec.sector as usize] = Some((
                r.spec.plane,
                height_row(r.spec.open, s as i32),
                row(r.height, s),
            ));
        }
        MoverSolids(Arc::new(rows))
    }
}

/// The row a plane at `height` map units starts at: the first voxel whose
/// centre is at or above it, as the voxelizer rounds.
fn row(height: f32, s: f32) -> i32 {
    (height / s - 0.5).ceil() as i32
}

pub struct MoversPlugin;

impl Plugin for MoversPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoverSolids>()
            .add_systems(
                Update,
                (spawn_sub_grids, use_lines)
                    .chain()
                    .after(crate::player::walk),
            )
            .add_systems(Update, place_sub_grids.after(use_lines))
            .add_systems(FixedUpdate, tic_movers);
    }
}

/// Spawns each mover's sub-grid once, filled with the voxels between the
/// two ends of its travel as the shut build colours them.
fn spawn_sub_grids(
    mut movers: ResMut<Movers>,
    map: Res<GameMap>,
    mut solids: ResMut<MoverSolids>,
    mut voxel_world: VoxelWorld<DoomWorld>,
    mut commands: Commands,
) {
    let Some(shut) = movers.shut.take() else {
        return;
    };
    let s = map.map.units_per_voxel();
    let mut voxels = 0usize;
    for r in &mut movers.running {
        let (a, b) = (height_row(r.spec.open, s), height_row(r.spec.shut, s));
        let (lo, hi) = (a.min(b), a.max(b));
        let entity = voxel_world.spawn_sub_grid(Transform::IDENTITY);
        let sector = r.spec.sector;
        if let Some(mut grid) = voxel_world.grid_mut(Grid::SubGrid(entity)) {
            let min = IVec3::new(r.min.x, lo, r.min.y);
            let max = IVec3::new(r.max.x + 1, hi, r.max.y + 1);
            let world = &map.map;
            // Only the shell can ever show: collision doesn't read these
            // voxels, so the inside of a lift's block is left out.
            let in_block = |p: IVec3| {
                (lo..hi).contains(&p.y)
                    && world.column_sector(p.x, p.z) == Some(sector)
                    && shut.is_solid(p.to_array())
            };
            grid.set_voxels_in(min, max, |p| {
                if !in_block(p) {
                    return None;
                }
                let inside = [
                    IVec3::X,
                    IVec3::NEG_X,
                    IVec3::Y,
                    IVec3::NEG_Y,
                    IVec3::Z,
                    IVec3::NEG_Z,
                ]
                .iter()
                .all(|&d| in_block(p + d));
                if inside {
                    return None;
                }
                voxels += 1;
                Some(world_voxel(shut.voxel(p.to_array())))
            });
        }
        commands
            .entity(entity)
            .insert(Name::new(format!("mover {sector}")));
        r.grid = Some(entity);
    }
    *solids = movers.snapshot(s as f32);
    println!(
        "movers: {} doors, lifts and floors, {voxels} voxels in sub-grids",
        movers.running.len()
    );
}

/// Shows each sub-grid where its plane is, between the last two tics.
fn place_sub_grids(
    movers: Res<Movers>,
    map: Res<GameMap>,
    fixed: Res<Time<Fixed>>,
    mut grids: Query<(&mut Transform, &mut Visibility), With<SubGrid<DoomWorld>>>,
) {
    let t = fixed.overstep_fraction();
    for r in &movers.running {
        let Some(Ok((mut transform, mut visibility))) = r.grid.map(|e| grids.get_mut(e))
        else {
            continue;
        };
        let height = r.prev + (r.height - r.prev) * t;
        let y = (height - r.spec.shut as f32) / map.units;
        if transform.translation.y != y {
            transform.translation.y = y;
        }
        let open = r.spec.open as f32;
        let shown = if r.height == open && r.prev == open {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        visibility.set_if_neq(shown);
    }
}

/// A body a mover may carry or close on: feet, half width and height, in
/// voxels.
struct BodyAt {
    feet: Vec3,
    radius: f32,
    height: f32,
}

impl Running {
    /// Whether the body's footprint stands on any column of the mover.
    fn under(&self, body: &BodyAt, map: &VoxelMap) -> bool {
        let lo = (body.feet - body.radius + 1e-3).floor().as_ivec3();
        let hi = (body.feet + body.radius - 1e-3).ceil().as_ivec3() - 1;
        if hi.x < self.min.x
            || lo.x > self.max.x
            || hi.z < self.min.y
            || lo.z > self.max.y
        {
            return false;
        }
        (lo.z..=hi.z).any(|z| {
            (lo.x..=hi.x).any(|x| map.column_sector(x, z) == Some(self.spec.sector))
        })
    }

    fn sound_at(&self, s: f32) -> Vec3 {
        Vec3::new(self.centre.x, self.height / s, self.centre.y)
    }
}

/// Runs every mover one tic, carrying bodies on rising floors and sending
/// closing doors back up from bodies in the way.
fn tic_movers(
    mut movers: ResMut<Movers>,
    map: Res<GameMap>,
    mut solids: ResMut<MoverSolids>,
    mut player: Single<&mut Player>,
    mut actors: Query<&mut Actor>,
    mut voxel_world: VoxelWorld<DoomWorld>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let s = map.units;
    let world = Arc::clone(&map.map);
    let mut changed = false;
    // Buttons popping back.
    let mut pop = Vec::new();
    movers.flipped.retain_mut(|(line, tics)| match tics {
        Some(0) => {
            pop.push(*line);
            false
        }
        Some(t) => {
            *t -= 1;
            true
        }
        None => true,
    });
    for line in pop {
        flip_switch(&world, &mut voxel_world, line, false);
        let at = movers.line_middle(line, s);
        sounds.write(PlaySound::at("SWTCHN", at));
    }

    let walking = player.mode == Mode::Walk;
    for i in 0..movers.running.len() {
        let r = &mut movers.running[i];
        r.prev = r.height;
        let Some(mut thinker) = r.thinker else {
            continue;
        };
        let old = r.height;
        let mut next = old;
        let mut done = false;
        let mut sound: Option<&'static str> = None;
        match &mut thinker {
            Thinker::Door {
                op,
                speed,
                blaze,
                dir,
                wait,
            } => match *dir {
                1 => {
                    next = (old + *speed).min(r.spec.open as f32);
                    if next >= r.spec.open as f32 {
                        match op {
                            DoorOp::Normal => {
                                *dir = 0;
                                *wait = DOOR_WAIT;
                            }
                            _ => done = true,
                        }
                    }
                }
                0 => {
                    *wait = wait.saturating_sub(1);
                    if *wait == 0 {
                        *dir = -1;
                        sound = Some(if *blaze { "BDCLS" } else { "DORCLS" });
                    }
                }
                _ => {
                    next = (old - *speed).max(r.spec.shut as f32);
                    if next <= r.spec.shut as f32 {
                        done = true;
                    }
                }
            },
            Thinker::Plat {
                speed,
                low,
                high,
                down,
                wait,
            } => {
                if *wait > 0 {
                    *wait -= 1;
                    if *wait == 0 {
                        *down = false;
                        sound = Some("PSTART");
                    }
                } else if *down {
                    next = (old - *speed).max(*low);
                    if next <= *low {
                        *wait = PLAT_WAIT;
                        sound = Some("PSTOP");
                    }
                } else {
                    next = (old + *speed).min(*high);
                    if next >= *high {
                        done = true;
                        sound = Some("PSTOP");
                    }
                }
            }
            Thinker::Floor { speed, target } => {
                next = if *target > old {
                    (old + *speed).min(*target)
                } else {
                    (old - *speed).max(*target)
                };
                if next == *target {
                    done = true;
                    sound = Some("PSTOP");
                } else if r.moving.is_multiple_of(8) {
                    sound = Some("STNMOV");
                }
            }
        }

        // Bodies in the way.
        let (old_row, new_row) = (row(old, s), row(next, s));
        let ceiling_row = height_row(r.spec.ceiling, s as i32);
        let player_body = walking.then(|| {
            let body = player.body();
            BodyAt {
                feet: player.feet,
                radius: body.radius,
                height: body.height,
            }
        });
        let actor_body = |a: &Actor| BodyAt {
            feet: a.feet,
            radius: a.radius,
            height: a.height,
        };
        let mut blocked = false;
        match r.spec.plane {
            Plane::Ceiling if new_row < old_row => {
                let in_way = |b: &BodyAt| {
                    b.feet.y + b.height > new_row as f32 + 1e-3
                        && b.feet.y < old_row as f32
                        && r.under(b, &world)
                };
                blocked = player_body.iter().any(in_way)
                    || actors.iter().any(|a| a.solid && in_way(&actor_body(a)));
            }
            Plane::Floor if new_row > old_row => {
                // Carry what stands on the rising rows, unless that pushes
                // it into the ceiling.
                let lifted = |b: &BodyAt| {
                    b.feet.y >= old_row as f32 - 1e-3
                        && b.feet.y < new_row as f32
                        && r.under(b, &world)
                };
                let squashed =
                    |b: &BodyAt| new_row as f32 + b.height > ceiling_row as f32;
                blocked = player_body.iter().any(|b| lifted(b) && squashed(b))
                    || actors.iter().any(|a| {
                        let b = actor_body(a);
                        a.solid && lifted(&b) && squashed(&b)
                    });
                if !blocked {
                    if player_body.as_ref().is_some_and(lifted) {
                        player.feet.y = new_row as f32;
                    }
                    for mut a in &mut actors {
                        if lifted(&actor_body(&a)) {
                            a.feet.y = new_row as f32;
                        }
                    }
                }
            }
            _ => {}
        }
        if blocked {
            next = old;
            done = false;
            sound = None;
            match &mut thinker {
                // A closing door goes back up; one only closing waits.
                Thinker::Door { op, blaze, dir, .. } if *op != DoorOp::Close => {
                    *dir = 1;
                    sound = Some(if *blaze { "BDOPN" } else { "DOROPN" });
                }
                // A rising lift goes back down.
                Thinker::Plat { down, .. } => {
                    *down = true;
                    sound = Some("PSTART");
                }
                _ => {}
            }
        }
        if next != old {
            r.moving += 1;
            changed = true;
        }
        r.height = next;
        r.thinker = (!done).then_some(thinker);
        if done {
            r.moving = 0;
        }
        if let Some(name) = sound {
            sounds.write(PlaySound::at(name, r.sound_at(s)));
        }
    }
    if changed {
        *solids = movers.snapshot(s);
    }
}

impl Movers {
    /// The middle of linedef `line` in voxels, at the player's usual eye
    /// height above the map's origin: for sounds.
    fn line_middle(&self, line: u16, s: f32) -> Vec3 {
        let l = &self.use_lines[line as usize];
        let m = (l.a + l.b) / 2.0 / s;
        let floor = l
            .front
            .and_then(|sec| self.mover_of(sec))
            .map_or(0.0, |i| self.running[i].height / s);
        Vec3::new(m.x, floor, -m.y)
    }

    /// Sets off what using `line` does, as `P_UseSpecialLine` does, with the
    /// line's back sector `back`. Returns whether anything happened.
    fn activate(
        &mut self,
        line: u16,
        use_: LineUse,
        back: Option<u16>,
        sounds: &mut MessageWriter<PlaySound>,
        s: f32,
    ) -> bool {
        if let Effect::Exit { .. } = use_.effect {
            // `use_lines` sends the exit.
            return true;
        }
        let sectors: Vec<u16> = if use_.manual {
            back.into_iter().collect()
        } else {
            let tag = self.line_tag(line);
            self.specials.tagged.get(&tag).cloned().unwrap_or_default()
        };
        let mut any = false;
        for sector in sectors {
            let Some(i) = self.mover_of(sector) else {
                continue;
            };
            let target = match use_.effect {
                Effect::Floor { target, .. } => Some(
                    self.specials
                        .surroundings
                        .floor_target(sector as usize, target) as f32,
                ),
                _ => None,
            };
            let r = &mut self.running[i];
            let sound = match (use_.effect, &mut r.thinker) {
                // p_doors.c `EV_VerticalDoor`: a DR door in motion turns
                // round; any other busy sector is left alone.
                (Effect::Door { .. }, Some(Thinker::Door { op, dir, blaze, .. }))
                    if use_.manual && use_.repeat && *op == DoorOp::Normal =>
                {
                    if *dir == -1 {
                        *dir = 1;
                        Some(if *blaze { "BDOPN" } else { "DOROPN" })
                    } else {
                        *dir = -1;
                        Some(if *blaze { "BDCLS" } else { "DORCLS" })
                    }
                }
                (_, Some(_)) => continue,
                (Effect::Door { op, speed, blaze }, None) => {
                    if r.spec.plane != Plane::Ceiling {
                        continue;
                    }
                    let dir = if op == DoorOp::Close { -1 } else { 1 };
                    r.thinker = Some(Thinker::Door {
                        op,
                        speed,
                        blaze,
                        dir,
                        wait: 0,
                    });
                    Some(match (dir, blaze) {
                        (1, false) => "DOROPN",
                        (1, true) => "BDOPN",
                        (_, false) => "DORCLS",
                        (_, true) => "BDCLS",
                    })
                }
                (Effect::Plat { speed }, None) => {
                    if r.spec.plane != Plane::Floor {
                        continue;
                    }
                    r.thinker = Some(Thinker::Plat {
                        speed,
                        low: r.spec.open as f32,
                        high: r.height,
                        down: true,
                        wait: 0,
                    });
                    Some("PSTART")
                }
                (Effect::Floor { speed, .. }, None) => {
                    let target = target.unwrap_or(r.height);
                    if r.spec.plane != Plane::Floor || target == r.height {
                        continue;
                    }
                    r.thinker = Some(Thinker::Floor { speed, target });
                    r.moving = 0;
                    None
                }
                (Effect::Exit { .. }, None) => continue,
            };
            any = true;
            if let Some(name) = sound {
                sounds.write(PlaySound::at(name, r.sound_at(s)));
            }
        }
        any
    }

    fn line_tag(&self, line: u16) -> u16 {
        self.specials.tags.get(line as usize).copied().unwrap_or(0)
    }
}

/// Flips the switch textures of `line` in the world: to their partners when
/// `on`, else back to the map's. Voxels a shot has emptied stay empty.
fn flip_switch(
    map: &VoxelMap,
    voxel_world: &mut VoxelWorld<DoomWorld>,
    line: u16,
    on: bool,
) {
    for (p, flipped) in map.switch_voxels(line) {
        let p = IVec3::from_array(p);
        if voxel_world.get_voxel(p) == WorldVoxel::Air {
            continue;
        }
        let v = if on { flipped } else { map.voxel(p.to_array()) };
        voxel_world.set_voxel(p, world_voxel(v));
    }
}

/// Uses the line in front of the player when Space is pressed: p_map.c
/// `P_UseLines` and `PTR_UseTraverse`. The trace runs `USE_RANGE` units
/// from the player's centre along the view's yaw, through two-sided lines
/// with an opening, and stops at the first line with a use special, used
/// from its front side, or at a wall, with DOOM's "oof".
#[allow(clippy::too_many_arguments)]
fn use_lines(
    intent: Res<Intent>,
    mut movers: ResMut<Movers>,
    map: Res<GameMap>,
    player: Single<(&Player, &crate::camera::FlyCamera)>,
    mut voxel_world: VoxelWorld<DoomWorld>,
    mut sounds: MessageWriter<PlaySound>,
    mut exits: MessageWriter<LevelExit>,
    mut message: ResMut<HudMessage>,
    sectors: Res<SectorHeights>,
) {
    let (player, fly) = *player;
    if !intent.use_ || !player.alive() {
        return;
    }
    let s = map.units;
    let from = Vec2::new(player.feet.x, -player.feet.z) * s;
    let dir = Vec2::new(-fly.yaw.sin(), fly.yaw.cos());
    let to = from + dir * USE_RANGE;
    // Lines crossed, nearest first.
    let mut crossed: Vec<(f32, u16)> = movers
        .use_lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| Some((crossing(from, to, l.a, l.b)?, i as u16)))
        .collect();
    crossed.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, line) in crossed {
        let l = &movers.use_lines[line as usize];
        let (front, back) = (l.front, l.back);
        let on_back = (l.b - l.a).perp_dot(from - l.a) > 0.0;
        if let Some(use_) = movers.specials.lines.get(&line).copied() {
            if on_back {
                return;
            }
            if let Some(colour) = use_.key.filter(|c| !player.inventory.has_key(*c)) {
                let what = if use_.manual {
                    "open this door"
                } else {
                    "activate this object"
                };
                message.say(format!("You need a {} key to {what}", colour.name()));
                sounds.write(PlaySound::global("OOF"));
                return;
            }
            if movers.activate(line, use_, back, &mut sounds, s) {
                if !use_.manual {
                    let exit = matches!(use_.effect, Effect::Exit { .. });
                    if let Effect::Exit { secret } = use_.effect {
                        exits.write(LevelExit { secret });
                    }
                    flip_switch(&map.map, &mut voxel_world, line, true);
                    movers.flipped.retain(|(l, _)| *l != line);
                    movers
                        .flipped
                        .push((line, use_.repeat.then_some(BUTTON_TICS)));
                    let at = movers.line_middle(line, s);
                    sounds
                        .write(PlaySound::at(if exit { "SWTCHX" } else { "SWTCHN" }, at));
                }
                if !use_.repeat {
                    movers.specials.lines.remove(&line);
                }
            }
            return;
        }
        // No special: walk on through an opening, stop at a wall.
        let open = match (front, back) {
            (Some(f), Some(b)) => {
                let (f0, c0) = movers.heights_now(f, sectors.0[f as usize]);
                let (f1, c1) = movers.heights_now(b, sectors.0[b as usize]);
                c0.min(c1) - f0.max(f1) > 0.0
            }
            _ => false,
        };
        if !open {
            sounds.write(PlaySound::global("NOWAY"));
            return;
        }
    }
}

/// Every sector's floor and ceiling as the map stores them, in map units.
#[derive(Resource)]
pub struct SectorHeights(pub Vec<(i32, i32)>);

/// Where segment `p → q` crosses segment `a → b`, as a share of `p → q`.
fn crossing(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> Option<f32> {
    let r = q - p;
    let e = b - a;
    let denom = r.perp_dot(e);
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = (a - p).perp_dot(e) / denom;
    let u = (a - p).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossings_are_shares_of_the_trace() {
        let t = crossing(
            Vec2::ZERO,
            Vec2::new(64.0, 0.0),
            Vec2::new(16.0, -10.0),
            Vec2::new(16.0, 10.0),
        );
        assert_eq!(t, Some(0.25));
        assert!(crossing(
            Vec2::ZERO,
            Vec2::new(8.0, 0.0),
            Vec2::new(16.0, -10.0),
            Vec2::new(16.0, 10.0)
        )
        .is_none());
    }

    #[test]
    fn mover_rows_fill_between_the_planes() {
        let solids = MoverSolids(Arc::new(vec![
            Some((Plane::Floor, 0, 3)),
            Some((Plane::Ceiling, 10, 6)),
            None,
        ]));
        assert!(solids.solid(0, 0) && solids.solid(0, 2) && !solids.solid(0, 3));
        assert!(!solids.solid(1, 5) && solids.solid(1, 6) && solids.solid(1, 9));
        assert!(!solids.solid(1, 10) && !solids.solid(2, 0) && !solids.solid(9, 0));
    }

    #[test]
    fn rows_round_as_the_voxelizer_does() {
        for h in [-17, -16, 0, 1, 3, 64, 125] {
            assert_eq!(row(h as f32, 2.0), height_row(h, 2), "{h}");
        }
    }
}
