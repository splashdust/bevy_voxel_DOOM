//! Debris: cubes one voxel across that a carve throws out, which fall under
//! avian3d physics and turn back into world voxels once they come to rest.
//!
//! Anything that carves the world sends [`SprayDebris`] with the voxels it
//! carved. [`DebrisPlugin`] picks a sample of them and launches a cube from
//! each, in the carved voxel's colour, out along the hit normal.
//!
//! The voxel world has no colliders of its own. Static colliders are built
//! only around live cubes, in regions of `REGION`³ voxels aligned to the
//! world grid, each a parry voxel shape (`Collider::voxels`) holding the
//! region's solid voxels. A region is rebuilt when the world changes inside
//! it: at once for a carve, since cubes start inside the carved space, and
//! `REBUILD_DELAY` seconds after a cube settles into it, so a burst of
//! settling cubes costs one rebuild. A region no cube has come near for
//! `FREE_AFTER` seconds is dropped.
//!
//! A cube settles once avian puts it to sleep, once it has been slow for
//! `SLOW_TIME` seconds, or at `LIFETIME`. It becomes a world voxel at the
//! cell its centre is in, or one of the two cells above if that is solid.
//! It then stops moving and a copy of it slides and turns into that cell
//! over `SNAP_TIME`, and stays until the chunk holding the cell has remeshed
//! and the regions around it have been rebuilt, so neither the voxel nor the
//! floor under the cubes resting on it blinks out.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

use avian3d::prelude::*;
use bevy::{diagnostic::FrameCount, prelude::*};
use bevy_voxel_world::prelude::*;
use doom_voxelize::VoxelMap;

use crate::capture::WorldSettled;
use crate::world::DoomWorld;

/// A cube's edge in voxels: a little under one, so cubes don't jam.
const SIZE: f32 = 0.9;
/// The most cubes in flight; past it the oldest settle at once.
const MAX_LIVE: usize = 500;
/// Seconds a cube flies at most.
const LIFETIME: f32 = 8.0;
/// A cube slower than this, in metres per second, and turning slower than
/// `SLOW_SPIN` radians per second, for `SLOW_TIME` seconds settles.
const SLOW_SPEED: f32 = 0.1;
const SLOW_SPIN: f32 = 1.0;
const SLOW_TIME: f32 = 0.3;
/// Seconds a settled cube takes to slide into its cell.
const SNAP_TIME: f32 = 0.12;
/// A settled cube's scale in its cell: just inside the world voxel, so the
/// voxel's faces win once it is meshed.
const SNAPPED_SCALE: f32 = 0.995;
/// Seconds a settled cube waits at most for its chunk to remesh.
const SNAP_TIMEOUT: f32 = 3.0;
/// DOOM map units per metre: a 56-unit player is about 1.75 m tall.
const UNITS_PER_METRE: f32 = 32.0;
/// Launch speed along the cone, in metres per second, from and to.
const LAUNCH_SPEED: (f32, f32) = (2.0, 5.0);
/// Upward speed added at launch, in metres per second, at most.
const LAUNCH_UP: f32 = 2.0;
/// The launch cone's half angle, in radians.
const CONE: f32 = 0.7;
/// Spin at launch, in radians per second, at most about each axis.
const SPIN: f32 = 20.0;
/// A collider region's edge, in voxels.
const REGION: i32 = 16;
/// Seconds a region waits after a cube settles in it before it is rebuilt.
const REBUILD_DELAY: f32 = 0.1;
/// Rebuilds of settled-cube edits per frame at most. Carves and new regions
/// are built at once.
const MAX_DELAYED_REBUILDS: usize = 8;
/// Seconds a region stays after no cube has come near it.
const FREE_AFTER: f32 = 2.0;
/// A cube needs the regions it can reach in this many seconds at its speed,
/// and within `REACH_MARGIN` voxels.
const LOOKAHEAD: f32 = 0.1;
const REACH_MARGIN: f32 = 2.0;

/// Throws debris out of a carve: a sample of `carved`, one cube per voxel,
/// each in the colour of its material.
///
/// Each cube flies out along `normal` with some spread, or straight away
/// from `hit` when `normal` is zero, as for an explosion. Send it after the
/// carve's edits: the colliders around the carve are rebuilt from the world
/// as it is when the message is read.
#[derive(Message, Clone, Debug)]
pub struct SprayDebris {
    /// Where the carve happened, in world space.
    pub hit: Vec3,
    /// The direction debris flies, such as the normal of the face a shot
    /// hit, or zero to fly away from `hit`.
    pub normal: Vec3,
    /// The voxels carved, and the material each held.
    pub carved: Vec<(IVec3, u16)>,
}

pub struct DebrisPlugin {
    /// The map the world is built from, read where the voxel world has no
    /// voxel loaded.
    pub map: Arc<VoxelMap>,
    /// Cubes per spray at most; 0 turns debris off.
    pub count: u32,
    pub units_per_voxel: i32,
    /// The material of a shot's flash, which rays and debris pass through.
    pub flash: Option<u16>,
}

impl Plugin for DebrisPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SprayDebris>();
        if self.count == 0 {
            return;
        }
        let voxels_per_metre = UNITS_PER_METRE / self.units_per_voxel as f32;
        app.add_plugins(PhysicsPlugins::default().with_length_unit(voxels_per_metre))
            .insert_resource(Gravity(Vec3::NEG_Y * 9.81 * voxels_per_metre))
            .insert_resource(Debris {
                map: Arc::clone(&self.map),
                count: self.count as usize,
                flash: self.flash,
                voxels_per_metre,
                rng: Rng(0x5eed_d0d0),
                materials: HashMap::new(),
                flying_mesh: Handle::default(),
                snapping_mesh: Handle::default(),
                next_order: 0,
            })
            .init_resource::<Regions>()
            .init_resource::<Stats>()
            .add_systems(Startup, make_meshes)
            .add_systems(
                FixedPostUpdate,
                (
                    start_step.before(PhysicsSystems::First),
                    end_step.after(PhysicsSystems::Last),
                ),
            )
            .add_systems(
                Update,
                (spray, settle, snap, update_regions, report).chain(),
            );
    }
}

#[derive(Resource)]
struct Debris {
    map: Arc<VoxelMap>,
    count: usize,
    flash: Option<u16>,
    voxels_per_metre: f32,
    rng: Rng,
    /// One unlit material per palette index, made on first use.
    materials: HashMap<u8, Handle<StandardMaterial>>,
    /// A cube `SIZE` across, for cubes in flight.
    flying_mesh: Handle<Mesh>,
    /// A cube one voxel across, scaled while it slides into its cell.
    snapping_mesh: Handle<Mesh>,
    /// Counts cubes launched, so the oldest can be found.
    next_order: u64,
}

impl Debris {
    /// Whether `voxel`, read at `p`, is solid to debris: a flash isn't, and
    /// an unset voxel is what the map generates.
    fn solid(&self, voxel: WorldVoxel<u16>, p: IVec3) -> bool {
        match voxel {
            WorldVoxel::Unset => self.map.voxel(p.to_array()).is_solid(),
            WorldVoxel::Solid(m) => Some(m) != self.flash,
            WorldVoxel::Air => false,
        }
    }

    /// The material showing palette colour `index` as the world draws it:
    /// unlit, with the map's contrast gain on its sRGB value. A lone voxel
    /// has no ambient occlusion, so none is applied.
    fn material(
        &mut self,
        index: u8,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let map = &self.map;
        self.materials
            .entry(index)
            .or_insert_with(|| {
                let [r, g, b] = map.palette()[index as usize];
                let gain = map.colour_gain();
                let channel = |c: u8| (c as f32 / 255.0 * gain).min(1.0);
                materials.add(StandardMaterial {
                    base_color: Color::srgb(channel(r), channel(g), channel(b)),
                    unlit: true,
                    ..default()
                })
            })
            .clone()
    }
}

/// A cube in flight.
#[derive(Component)]
struct Cube {
    material: u16,
    /// Launch order: lower is older.
    order: u64,
    /// Seconds since launch.
    age: f32,
    /// Seconds it has been slow for.
    slow_for: f32,
}

/// A settled cube sliding into its cell, until the world voxel replacing it
/// shows and the colliders around it hold that voxel.
#[derive(Component)]
struct Snapping {
    /// The settled cube's body, kept static under any cubes resting on it.
    body: Entity,
    from: Transform,
    to: Transform,
    /// Seconds since it settled.
    time: f32,
    cell: IVec3,
    /// The chunk that shows the cell, if one is spawned.
    chunk: Option<ChunkKey>,
    /// The frame the voxel was set in.
    set_frame: u32,
    /// The edit's number in [`Regions`].
    edit: u64,
    /// The frame the chunk published with the voxel.
    shown_frame: Option<u32>,
}

#[derive(Component)]
struct RegionCollider;

/// Frame times, collider build times and physics step times while debris is
/// about, printed once it has all settled.
#[derive(Resource, Default)]
struct Stats {
    active_since: Option<Instant>,
    launched: u32,
    placed: u32,
    /// Placed or dropped when its time or the cap ran out, not at rest.
    forced: u32,
    dropped: u32,
    most_live: usize,
    /// The longest frames, longest first, in seconds.
    worst_frames: Vec<f32>,
    builds: u32,
    build_secs: f64,
    longest_build: f64,
    /// The most build time in one frame.
    longest_frame_builds: f64,
    /// When the physics step running now started.
    step_started: Option<Instant>,
    steps: u32,
    step_secs: f64,
    longest_step: f64,
    /// When this frame's debris systems started, and the longest they took
    /// in a frame, from `spray` to `report`.
    update_started: Option<Instant>,
    longest_update: f64,
}

fn start_step(mut stats: ResMut<Stats>) {
    stats.step_started = Some(Instant::now());
}

fn end_step(mut stats: ResMut<Stats>) {
    if let Some(started) = stats.step_started.take() {
        if stats.active_since.is_some() {
            let secs = started.elapsed().as_secs_f64();
            stats.steps += 1;
            stats.step_secs += secs;
            stats.longest_step = stats.longest_step.max(secs);
        }
    }
}

fn make_meshes(mut debris: ResMut<Debris>, mut meshes: ResMut<Assets<Mesh>>) {
    debris.flying_mesh = meshes.add(Cuboid::from_length(SIZE));
    debris.snapping_mesh = meshes.add(Cuboid::from_length(1.0));
}

/// Launches cubes for each [`SprayDebris`], and marks the regions around
/// each carve for a rebuild this frame.
fn spray(
    mut commands: Commands,
    mut sprays: MessageReader<SprayDebris>,
    mut debris: ResMut<Debris>,
    mut regions: ResMut<Regions>,
    mut stats: ResMut<Stats>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
) {
    stats.update_started = Some(Instant::now());
    let now = time.elapsed_secs();
    for message in sprays.read() {
        let Some((min, max)) = bounds(message.carved.iter().map(|(p, _)| *p)) else {
            continue;
        };
        regions.edited(min - IVec3::ONE, max + IVec3::ONE, now, true);
        let mut candidates: Vec<(IVec3, u16)> = message
            .carved
            .iter()
            .copied()
            .filter(|&(_, m)| Some(m) != debris.flash)
            .collect();
        let count = debris.count.min(candidates.len());
        for i in 0..count {
            let j = i + debris.rng.below(candidates.len() - i);
            candidates.swap(i, j);
        }
        let vpm = debris.voxels_per_metre;
        for &(cell, material) in &candidates[..count] {
            let centre = cell.as_vec3() + 0.5;
            let axis = Dir3::new(message.normal)
                .or_else(|_| Dir3::new(centre - message.hit))
                .unwrap_or(Dir3::Y);
            let rng = &mut debris.rng;
            let direction = in_cone(axis, CONE, rng.unit(), rng.unit());
            let speed = LAUNCH_SPEED.0 + (LAUNCH_SPEED.1 - LAUNCH_SPEED.0) * rng.unit();
            let velocity = (direction * speed + Vec3::Y * LAUNCH_UP * rng.unit()) * vpm;
            let spin = Vec3::new(rng.signed(), rng.signed(), rng.signed()) * SPIN;
            let order = debris.next_order;
            debris.next_order += 1;
            let handle = debris.material(material as u8, &mut materials);
            commands.spawn((
                Cube {
                    material,
                    order,
                    age: 0.0,
                    slow_for: 0.0,
                },
                RigidBody::Dynamic,
                Collider::cuboid(SIZE, SIZE, SIZE),
                Friction::new(0.8),
                Restitution::new(0.15),
                LinearVelocity(velocity),
                AngularVelocity(spin),
                TransformInterpolation,
                Transform::from_translation(centre),
                Mesh3d(debris.flying_mesh.clone()),
                MeshMaterial3d(handle),
            ));
            let reach = Vec3::splat(REACH_MARGIN) + velocity.abs() * LOOKAHEAD;
            regions.need(
                (centre - reach).floor().as_ivec3(),
                (centre + reach).floor().as_ivec3(),
                now,
            );
            stats.launched += 1;
        }
    }
}

/// A cube in flight and how it moves.
type CubeMotion = (
    Entity,
    &'static mut Cube,
    &'static Transform,
    &'static LinearVelocity,
    &'static AngularVelocity,
    Has<Sleeping>,
);

/// Settles each cube that sleeps, has been slow long enough or is too old,
/// and the oldest cubes past `MAX_LIVE`. Lower cubes settle first, so a cube
/// resting on another finds it solid.
#[allow(clippy::too_many_arguments)]
fn settle(
    mut commands: Commands,
    mut cubes: Query<CubeMotion>,
    mut debris: ResMut<Debris>,
    mut regions: ResMut<Regions>,
    mut stats: ResMut<Stats>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut voxel_world: VoxelWorld<DoomWorld>,
    frame: Res<FrameCount>,
    time: Res<Time>,
) {
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());
    let slow = SLOW_SPEED * debris.voxels_per_metre;
    let mut due = Vec::new();
    let mut flying = Vec::new();
    for (entity, mut cube, transform, velocity, spin, sleeping) in &mut cubes {
        cube.age += dt;
        if velocity.length() < slow && spin.length() < SLOW_SPIN {
            cube.slow_for += dt;
        } else {
            cube.slow_for = 0.0;
        }
        let old = cube.age >= LIFETIME;
        if old || sleeping || cube.slow_for >= SLOW_TIME {
            due.push((entity, *transform, cube.material, old));
        } else {
            flying.push((cube.order, entity, *transform, cube.material));
        }
    }
    stats.most_live = stats.most_live.max(due.len() + flying.len());
    if flying.len() > MAX_LIVE {
        flying.sort_by_key(|&(order, ..)| order);
        let excess = flying.len() - MAX_LIVE;
        due.extend(
            flying[..excess]
                .iter()
                .map(|&(_, entity, transform, material)| {
                    (entity, transform, material, true)
                }),
        );
    }
    due.sort_by(|a, b| a.1.translation.y.total_cmp(&b.1.translation.y));
    for (body, transform, material, forced) in due {
        let landing = landing_cell(transform.translation, |p| {
            debris.solid(voxel_world.get_voxel(p), p)
        });
        let cell = match landing {
            Landing::Cell(cell) => cell,
            // Resting on a cube still in flight: wait for it to settle, or
            // for its own time to run out.
            Landing::Unsupported(_) if !forced => continue,
            Landing::Unsupported(_) | Landing::Blocked => {
                commands.entity(body).despawn();
                stats.dropped += 1;
                stats.forced += forced as u32;
                continue;
            }
        };
        voxel_world.set_voxel(cell, WorldVoxel::Solid(material));
        let edit = regions.edited(cell - IVec3::ONE, cell + IVec3::ONE, now, false);
        let from = transform.with_scale(Vec3::splat(SIZE));
        let to = Transform {
            translation: cell.as_vec3() + 0.5,
            rotation: nearest_cube_rotation(transform.rotation),
            scale: Vec3::splat(SNAPPED_SCALE),
        };
        let handle = debris.material(material as u8, &mut materials);
        commands.spawn((
            Snapping {
                body,
                from,
                to,
                time: 0.0,
                cell,
                chunk: voxel_world.chunk_at(cell),
                set_frame: frame.0,
                edit,
                shown_frame: None,
            },
            from,
            Mesh3d(debris.snapping_mesh.clone()),
            MeshMaterial3d(handle),
        ));
        commands
            .entity(body)
            .remove::<(Cube, Mesh3d)>()
            .insert(RigidBody::Static);
        stats.placed += 1;
        stats.forced += forced as u32;
    }
}

/// Slides settled cubes into their cells, and removes each once the world
/// shows its voxel and the colliders around it hold it.
fn snap(
    mut commands: Commands,
    mut snapping: Query<(Entity, &mut Snapping, &mut Transform)>,
    mut published: MessageReader<ChunkPublished<DoomWorld>>,
    regions: Res<Regions>,
    voxel_world: VoxelWorld<DoomWorld>,
    frame: Res<FrameCount>,
    time: Res<Time>,
) {
    let published: Vec<ChunkKey> = published.read().map(|p| p.chunk_key).collect();
    for (entity, mut snap, mut transform) in &mut snapping {
        snap.time += time.delta_secs();
        let t = (snap.time / SNAP_TIME).min(1.0);
        let t = t * t * (3.0 - 2.0 * t);
        *transform = Transform {
            translation: snap.from.translation.lerp(snap.to.translation, t),
            rotation: snap.from.rotation.slerp(snap.to.rotation, t),
            scale: snap.from.scale.lerp(snap.to.scale, t),
        };
        if snap.shown_frame.is_none() {
            let shown = match snap.chunk {
                None => true,
                // The edit commits next frame, and a chunk publishing then
                // may still have a remesh to come.
                Some(key) => {
                    frame.0 > snap.set_frame
                        && published.contains(&key)
                        && voxel_world.chunk_state(key) == Some(ChunkStatus::Ready)
                }
            };
            if shown {
                snap.shown_frame = Some(frame.0);
            }
        }
        // Two frames for the published mesh to reach the screen.
        let drawn = snap.shown_frame.is_some_and(|f| frame.0 >= f + 2);
        let held = regions.holds(snap.cell, snap.edit);
        if (snap.time >= SNAP_TIME && drawn && held) || snap.time >= SNAP_TIMEOUT {
            commands.entity(entity).despawn();
            commands.entity(snap.body).despawn();
        }
    }
}

/// Keeps a collider region around each cube in flight, rebuilds regions the
/// world changed in, and drops regions no cube needs.
fn update_regions(
    mut commands: Commands,
    cubes: Query<(&Transform, &LinearVelocity), With<Cube>>,
    debris: Res<Debris>,
    mut regions: ResMut<Regions>,
    mut stats: ResMut<Stats>,
    voxel_world: VoxelWorld<DoomWorld>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    for (transform, velocity) in &cubes {
        let reach = Vec3::splat(REACH_MARGIN) + velocity.abs() * LOOKAHEAD;
        let p = transform.translation;
        regions.need(
            (p - reach).floor().as_ivec3(),
            (p + reach).floor().as_ivec3(),
            now,
        );
    }
    for entity in regions.expire(now) {
        commands.entity(entity).despawn();
    }
    let due = regions.due(now, MAX_DELAYED_REBUILDS);
    if due.is_empty() {
        return;
    }
    let read = voxel_world.get_voxel_fn();
    let mut frame_secs = 0.0;
    for key in due {
        let started = Instant::now();
        let origin = key * REGION;
        let voxels = solid_voxels(origin, |p| debris.solid(read(p), p));
        let collider = (!voxels.is_empty()).then(|| Collider::voxels(Vec3::ONE, &voxels));
        let entity = match (regions.entity(key), collider) {
            (Some(entity), Some(collider)) => {
                commands.entity(entity).insert(collider);
                Some(entity)
            }
            (None, Some(collider)) => Some(
                commands
                    .spawn((
                        RegionCollider,
                        RigidBody::Static,
                        collider,
                        Transform::from_translation(origin.as_vec3()),
                    ))
                    .id(),
            ),
            (Some(entity), None) => {
                commands.entity(entity).despawn();
                None
            }
            (None, None) => None,
        };
        regions.built(key, entity);
        let secs = started.elapsed().as_secs_f64();
        stats.builds += 1;
        stats.build_secs += secs;
        stats.longest_build = stats.longest_build.max(secs);
        frame_secs += secs;
    }
    stats.longest_frame_builds = stats.longest_frame_builds.max(frame_secs);
}

/// Holds a screenshot back while debris is about, tracks the longest frames
/// meanwhile, and prints them once it has all settled.
fn report(
    cubes: Query<(), With<Cube>>,
    snapping: Query<(), With<Snapping>>,
    regions: Res<Regions>,
    mut stats: ResMut<Stats>,
    settled: Option<ResMut<WorldSettled>>,
    time: Res<Time<Real>>,
) {
    let busy = !cubes.is_empty() || !snapping.is_empty();
    if let Some(started) = stats.update_started.take() {
        if stats.active_since.is_some() {
            let secs = started.elapsed().as_secs_f64();
            stats.longest_update = stats.longest_update.max(secs);
        }
    }
    if busy {
        if let Some(mut settled) = settled {
            settled.hold();
        }
        if stats.active_since.is_none() {
            stats.active_since = Some(Instant::now());
        } else {
            let frame = time.delta_secs();
            stats.worst_frames.push(frame);
            stats.worst_frames.sort_by(|a, b| b.total_cmp(a));
            stats.worst_frames.truncate(5);
        }
        return;
    }
    let Some(since) = stats.active_since else {
        return;
    };
    let ms = |s: f64| s * 1000.0;
    let worst: Vec<String> = stats
        .worst_frames
        .iter()
        .map(|f| format!("{:.1}", ms(*f as f64)))
        .collect();
    println!(
        "debris settled after {:.1} s: {} launched, {} placed, {} dropped, {} not at rest, at most {} in flight; \
         worst frames {} ms; {} region builds, {:.2} ms mean, {:.2} ms longest, \
         {:.2} ms most in a frame; {} regions left; {} physics steps, {:.2} ms mean, \
         {:.2} ms longest; debris systems {:.2} ms longest in a frame",
        since.elapsed().as_secs_f32(),
        stats.launched,
        stats.placed,
        stats.dropped,
        stats.forced,
        stats.most_live,
        worst.join(", "),
        stats.builds,
        ms(stats.build_secs / stats.builds.max(1) as f64),
        ms(stats.longest_build),
        ms(stats.longest_frame_builds),
        regions.len(),
        stats.steps,
        ms(stats.step_secs / stats.steps.max(1) as f64),
        ms(stats.longest_step),
        ms(stats.longest_update),
    );
    *stats = Stats::default();
}

/// The smallest box holding `points`, both corners inclusive.
fn bounds(points: impl Iterator<Item = IVec3>) -> Option<(IVec3, IVec3)> {
    points.fold(None, |acc, p| match acc {
        None => Some((p, p)),
        Some((min, max)) => Some((min.min(p), max.max(p))),
    })
}

/// Where a settled cube goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Landing {
    /// This cell: open, with something solid beside it or in one of the
    /// nine cells below.
    Cell(IVec3),
    /// This cell is open but nothing solid is below it or beside it.
    Unsupported(IVec3),
    /// The cell and the two above it are solid.
    Blocked,
}

/// Where a cube centred at `centre` settles: the cell holding its centre,
/// or the first open one of the two above. A cell needs something solid
/// beside it or in the nine cells below, which takes in a cube resting on
/// the edge of a voxel, so a cube resting on another cube in flight, or
/// stopped in mid-air, doesn't leave a floating voxel.
fn landing_cell(centre: Vec3, solid: impl Fn(IVec3) -> bool) -> Landing {
    let start = centre.floor().as_ivec3();
    let Some(cell) = (0..=2)
        .map(|dy| start + IVec3::Y * dy)
        .find(|&cell| !solid(cell))
    else {
        return Landing::Blocked;
    };
    let below = (-1..=1).flat_map(|z| (-1..=1).map(move |x| IVec3::new(x, -1, z)));
    let beside = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
    let supported = below.chain(beside).any(|d| solid(cell + d));
    if supported {
        Landing::Cell(cell)
    } else {
        Landing::Unsupported(cell)
    }
}

/// The rotation closest to `rotation` that leaves a cube on the voxel grid,
/// out of the 24 that turn each axis onto a world axis.
fn nearest_cube_rotation(rotation: Quat) -> Quat {
    let axes = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ];
    let mut best = (f32::NEG_INFINITY, Quat::IDENTITY);
    for x in axes {
        for y in axes {
            if x.dot(y) != 0.0 {
                continue;
            }
            let grid = Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y)));
            // The larger |q·g|, the smaller the angle between them.
            let closeness = rotation.dot(grid).abs();
            if closeness > best.0 {
                best = (closeness, grid);
            }
        }
    }
    best.1
}

/// A direction within `half_angle` of `axis`, spread evenly over the cone's
/// solid angle by `u` and `v` in 0 to 1.
fn in_cone(axis: Dir3, half_angle: f32, u: f32, v: f32) -> Vec3 {
    let cos = 1.0 - u * (1.0 - half_angle.cos());
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let (a, b) = axis.any_orthonormal_pair();
    let turn = TAU * v;
    *axis * cos + (a * turn.cos() + b * turn.sin()) * sin
}

/// The solid voxels of the region whose corner is at `origin`, and of one
/// voxel around it so neighbouring regions overlap, as positions from
/// `origin`; none if the region holds no air, since no cube can reach it.
///
/// It takes every solid voxel, not only those with a face open to air: a
/// shell one voxel thick lets a cube that other cubes push into it come out
/// on its far side, inside the solid, where nothing pushes it back. The
/// voxel shape only collides on faces between a voxel and a voxel it
/// doesn't hold, so voxels deep inside cost memory, not contacts.
fn solid_voxels(origin: IVec3, solid: impl Fn(IVec3) -> bool) -> Vec<IVec3> {
    let mut voxels = Vec::new();
    let mut air = false;
    for z in -1..REGION + 1 {
        for y in -1..REGION + 1 {
            for x in -1..REGION + 1 {
                let l = IVec3::new(x, y, z);
                if solid(origin + l) {
                    voxels.push(l);
                } else {
                    air = true;
                }
            }
        }
    }
    if !air {
        voxels.clear();
    }
    voxels
}

/// The collider regions: which exist, when a cube last needed each, and
/// which the world has changed in since they were built.
///
/// Region `k` covers voxels `k·REGION` to `(k + 1)·REGION`, and its collider
/// one voxel more on every side.
#[derive(Resource, Default)]
struct Regions {
    regions: HashMap<IVec3, Region>,
    /// Numbers the edits; a region built after edit `n` holds it.
    edits: u64,
}

#[derive(Debug)]
struct Region {
    entity: Option<Entity>,
    /// The last edit this region's collider holds, or `None` if it was
    /// never built.
    built: Option<u64>,
    /// When a cube last needed this region, in seconds.
    needed_at: f32,
    /// When an edit since the build first changed it, and whether one
    /// was a carve, which can't wait.
    changed: Option<(f32, bool)>,
}

impl Regions {
    /// The keys of the regions whose colliders reach into the box from `min`
    /// to `max`, both inclusive.
    fn keys(min: IVec3, max: IVec3) -> impl Iterator<Item = IVec3> {
        let lo = (min - IVec3::ONE).div_euclid(IVec3::splat(REGION));
        let hi = (max + IVec3::ONE).div_euclid(IVec3::splat(REGION));
        (lo.z..=hi.z).flat_map(move |z| {
            (lo.y..=hi.y)
                .flat_map(move |y| (lo.x..=hi.x).map(move |x| IVec3::new(x, y, z)))
        })
    }

    /// Marks the regions reaching into the box as needed at `now`, adding
    /// those that don't exist yet, to be built at once.
    fn need(&mut self, min: IVec3, max: IVec3, now: f32) {
        for key in Self::keys(min, max) {
            self.regions
                .entry(key)
                .or_insert(Region {
                    entity: None,
                    built: None,
                    needed_at: now,
                    changed: None,
                })
                .needed_at = now;
        }
    }

    /// Records an edit that changed the voxels in the box, both corners
    /// inclusive, and returns its number. A box one voxel wider than the
    /// voxels changed also takes in the neighbours whose faces they open or
    /// close. `urgent` edits are rebuilt this frame, others after
    /// `REBUILD_DELAY`.
    fn edited(&mut self, min: IVec3, max: IVec3, now: f32, urgent: bool) -> u64 {
        self.edits += 1;
        for key in Self::keys(min, max) {
            if let Some(region) = self.regions.get_mut(&key) {
                let since = region.changed.map_or(now, |(since, _)| since);
                let was_urgent = region.changed.is_some_and(|(_, u)| u);
                region.changed = Some((since, urgent || was_urgent));
            }
        }
        self.edits
    }

    /// The regions to build now: those never built, those a carve changed,
    /// and up to `max_delayed` that other edits changed `REBUILD_DELAY` ago.
    fn due(&self, now: f32, max_delayed: usize) -> Vec<IVec3> {
        let mut due = Vec::new();
        let mut delayed = Vec::new();
        for (&key, region) in &self.regions {
            match (region.built, region.changed) {
                (None, _) | (_, Some((_, true))) => due.push(key),
                (_, Some((since, false))) if now - since >= REBUILD_DELAY => {
                    delayed.push((since, key))
                }
                _ => {}
            }
        }
        delayed.sort_by(|a, b| a.0.total_cmp(&b.0));
        due.extend(delayed.into_iter().take(max_delayed).map(|(_, key)| key));
        due
    }

    fn entity(&self, key: IVec3) -> Option<Entity> {
        self.regions.get(&key).and_then(|r| r.entity)
    }

    /// Records that region `key` was built from the world as it is now.
    fn built(&mut self, key: IVec3, entity: Option<Entity>) {
        if let Some(region) = self.regions.get_mut(&key) {
            region.entity = entity;
            region.built = Some(self.edits);
            region.changed = None;
        }
    }

    /// Removes the regions not needed for `FREE_AFTER` seconds, and returns
    /// their collider entities.
    fn expire(&mut self, now: f32) -> Vec<Entity> {
        let mut freed = Vec::new();
        self.regions.retain(|_, region| {
            let keep = now - region.needed_at < FREE_AFTER;
            if !keep {
                freed.extend(region.entity);
            }
            keep
        });
        freed
    }

    /// Whether every region whose collider could show voxel `cell` holds edit
    /// `edit`, or doesn't exist.
    fn holds(&self, cell: IVec3, edit: u64) -> bool {
        Self::keys(cell - IVec3::ONE, cell + IVec3::ONE).all(|key| {
            self.regions
                .get(&key)
                .is_none_or(|r| r.built.is_some_and(|built| built >= edit))
        })
    }

    fn len(&self) -> usize {
        self.regions.len()
    }
}

/// SplitMix64: enough randomness for spray, with no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// From 0 to 1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// From −1 to 1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// From 0 up to `n`, not including it.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_at_zero(p: IVec3) -> bool {
        p.y < 0
    }

    #[test]
    fn a_cube_on_the_floor_lands_in_the_cell_above_it() {
        let landing = landing_cell(Vec3::new(3.7, 0.45, -2.2), floor_at_zero);
        assert_eq!(landing, Landing::Cell(IVec3::new(3, 0, -3)));
    }

    #[test]
    fn a_cube_sunk_into_solid_rises_up_to_two_cells() {
        // Its centre is inside a voxel another cube already settled into.
        let solid = |p: IVec3| p.y < 0 || p == IVec3::new(0, 0, 0);
        assert_eq!(
            landing_cell(Vec3::new(0.5, 0.6, 0.5), solid),
            Landing::Cell(IVec3::new(0, 1, 0))
        );
        let column = |p: IVec3| p.y < 0 || (p.x == 0 && p.z == 0 && p.y < 2);
        assert_eq!(
            landing_cell(Vec3::new(0.5, 0.6, 0.5), column),
            Landing::Cell(IVec3::new(0, 2, 0))
        );
        let tall = |p: IVec3| p.y < 0 || (p.x == 0 && p.z == 0 && p.y < 3);
        assert_eq!(
            landing_cell(Vec3::new(0.5, 0.6, 0.5), tall),
            Landing::Blocked
        );
    }

    #[test]
    fn a_cube_in_mid_air_has_no_support() {
        assert_eq!(
            landing_cell(Vec3::new(0.5, 5.5, 0.5), floor_at_zero),
            Landing::Unsupported(IVec3::new(0, 5, 0))
        );
        // Against a wall counts as support.
        let wall = |p: IVec3| p.x >= 1;
        assert_eq!(
            landing_cell(Vec3::new(0.5, 5.5, 0.5), wall),
            Landing::Cell(IVec3::new(0, 5, 0))
        );
        // So does the edge of a voxel diagonally below.
        let edge = |p: IVec3| p == IVec3::new(1, 4, 1);
        assert_eq!(
            landing_cell(Vec3::new(0.9, 5.5, 0.9), edge),
            Landing::Cell(IVec3::new(0, 5, 0))
        );
    }

    #[test]
    fn rotations_snap_to_the_nearest_grid_rotation() {
        let near = |a: Quat, b: Quat| a.dot(b).abs() > 0.99999;
        assert!(near(
            nearest_cube_rotation(Quat::from_rotation_y(0.4)),
            Quat::IDENTITY
        ));
        assert!(near(
            nearest_cube_rotation(Quat::from_rotation_y(1.2)),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
        ));
        // Any rotation snaps to a proper grid rotation at most about 63° away.
        let mut rng = Rng(7);
        for _ in 0..200 {
            let q = Quat::from_euler(
                EulerRot::XYZ,
                rng.signed() * 3.2,
                rng.signed() * 3.2,
                rng.signed() * 3.2,
            );
            let snapped = nearest_cube_rotation(q);
            let m = Mat3::from_quat(snapped);
            assert!((m.determinant() - 1.0).abs() < 1e-4);
            for axis in [m.x_axis, m.y_axis, m.z_axis] {
                assert!(axis.abs().max_element() > 0.9999, "{axis}");
            }
            assert!(q.angle_between(snapped) < 1.1, "{q} to {snapped}");
        }
    }

    #[test]
    fn the_cone_stays_within_its_angle() {
        let mut rng = Rng(3);
        let axis = Dir3::new(Vec3::new(1.0, 0.2, -0.3)).unwrap();
        for _ in 0..500 {
            let d = in_cone(axis, CONE, rng.unit(), rng.unit());
            assert!((d.length() - 1.0).abs() < 1e-4);
            assert!(d.angle_between(*axis) <= CONE + 1e-3);
        }
    }

    #[test]
    fn regions_collide_with_their_solid_voxels_and_one_more_round() {
        let side = (REGION + 2) as usize;
        // A floor: every solid layer, over the region and one voxel round.
        let floor = solid_voxels(IVec3::new(32, -8, 0), |p| p.y < 0);
        assert_eq!(floor.len(), 9 * side * side);
        assert!(floor.iter().all(|l| (-1..8).contains(&l.y)));
        // A wall one voxel thick.
        assert_eq!(solid_voxels(IVec3::ZERO, |p| p.x == 5).len(), side * side);
        // Solid all through: nothing a cube could reach.
        assert!(solid_voxels(IVec3::ZERO, |_| true).is_empty());
        // One voxel past the region still counts, so regions overlap.
        let lone = solid_voxels(IVec3::ZERO, |p| p == IVec3::new(-1, 3, REGION));
        assert_eq!(lone, vec![IVec3::new(-1, 3, REGION)]);
        assert!(solid_voxels(IVec3::ZERO, |p| p == IVec3::new(-2, 3, 3)).is_empty());
    }

    #[test]
    fn region_keys_cover_every_collider_reaching_the_box() {
        let keys: Vec<_> =
            Regions::keys(IVec3::new(3, 3, 3), IVec3::new(3, 3, 3)).collect();
        assert_eq!(keys, vec![IVec3::ZERO]);
        // Voxel 0 is in region 0's core and region −1's border.
        let keys: Vec<_> = Regions::keys(IVec3::ZERO, IVec3::ZERO).collect();
        assert_eq!(keys.len(), 8);
        assert!(keys.contains(&IVec3::NEG_ONE) && keys.contains(&IVec3::ZERO));
        // Voxel 16 is region 1's and in region 0's border.
        let keys: Vec<_> =
            Regions::keys(IVec3::new(16, 8, 8), IVec3::new(16, 8, 8)).collect();
        assert_eq!(keys, vec![IVec3::ZERO, IVec3::X]);
    }

    #[test]
    fn regions_build_when_needed_and_rebuild_after_edits() {
        let mut regions = Regions::default();
        let p = IVec3::new(5, 5, 5);
        regions.need(p, p, 0.0);
        assert_eq!(regions.due(0.0, 8), vec![IVec3::ZERO]);
        regions.built(IVec3::ZERO, None);
        assert!(regions.due(0.0, 8).is_empty());

        // A settled cube waits for the delay, and is held only after the build.
        let edit = regions.edited(p, p, 1.0, false);
        assert!(!regions.holds(p, edit));
        assert!(regions.due(1.0 + REBUILD_DELAY / 2.0, 8).is_empty());
        // More edits in the meantime don't push the rebuild back.
        regions.edited(p, p, 1.0 + REBUILD_DELAY / 2.0, false);
        let later = 1.0 + REBUILD_DELAY + 0.01;
        assert_eq!(regions.due(later, 8), vec![IVec3::ZERO]);
        assert!(regions.due(later, 0).is_empty());
        regions.built(IVec3::ZERO, None);
        assert!(regions.holds(p, edit));

        // A carve rebuilds at once; a region that doesn't exist holds anything.
        regions.edited(p, p, 2.0, true);
        assert_eq!(regions.due(2.0, 0), vec![IVec3::ZERO]);
        assert!(regions.holds(IVec3::splat(100), 99));
    }

    #[test]
    fn regions_no_cube_needs_are_freed() {
        let mut regions = Regions::default();
        regions.need(IVec3::ZERO, IVec3::new(20, 0, 0), 0.0);
        assert_eq!(regions.len(), 2 * 2 * 2 + 2 * 2);
        regions.need(IVec3::new(5, 5, 5), IVec3::new(5, 5, 5), 1.5);
        assert!(regions.expire(1.0).is_empty());
        regions.expire(FREE_AFTER + 0.5);
        assert_eq!(regions.len(), 1);
        assert!(regions.regions.contains_key(&IVec3::ZERO));
    }
}
