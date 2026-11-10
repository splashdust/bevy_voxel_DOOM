//! Kinematic collision of an upright box against the voxel grid, and a
//! voxel line trace. No physics engine: the player, monsters and corpses
//! move through [`move_body`], and sight lines, bullets and projectiles go
//! through [`trace`].
//!
//! Voxel `p` fills the unit cube from `p` to `p + 1`. A body is a box
//! `2·radius` across in x and z and `height` tall, standing on its `feet`
//! position, the centre of its bottom face. A move goes in substeps of under
//! half a voxel, each along x, then z, then y. A horizontal substep only
//! looks at the slab of voxels the box's leading face enters, since the box
//! was clear before it: when that slab holds solid voxels only within `step`
//! of the feet, and the box fits on top of them, the body steps up onto
//! them; otherwise it stops against them, and the other axes carry on, so it
//! slides along walls. A vertical substep looks at the row of voxels under
//! the feet or over the head. Voxels already inside the box, like debris
//! settling into it, are ignored, so a body can always walk out of them.

use bevy::math::{IVec3, Vec3};

/// Answers whether a voxel stops bodies, sight and shots.
pub trait Solid {
    fn solid(&self, p: IVec3) -> bool;
}

impl<F: Fn(IVec3) -> bool> Solid for F {
    fn solid(&self, p: IVec3) -> bool {
        self(p)
    }
}

/// An upright box, in voxels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Half its width in x and z.
    pub radius: f32,
    pub height: f32,
    /// The highest ledge it walks up onto without jumping.
    pub step: f32,
}

/// What [`move_body`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moved {
    /// The feet's new position.
    pub feet: Vec3,
    /// Per axis, whether a voxel stopped the move along it.
    pub blocked: [bool; 3],
    /// Standing on solid voxels at the end of the move: it was moving down,
    /// or not at all, and the row under its feet is solid.
    pub grounded: bool,
    /// How far it stepped up, in total.
    pub stepped: f32,
}

/// Slack at voxel boundaries, so rounding at a face doesn't count as
/// overlapping the voxel beyond it.
const EPS: f32 = 1e-3;
/// The longest substep along any axis: under half a voxel, so a substep
/// enters at most one new voxel along each axis.
const MAX_SUBSTEP: f32 = 0.45;

/// Moves `body` standing at `feet` by `delta`, stopping at solid voxels,
/// stepping up ledges and sliding along walls.
pub fn move_body(world: &impl Solid, body: &Body, feet: Vec3, delta: Vec3) -> Moved {
    let mut moved = Moved {
        feet,
        ..Moved::default()
    };
    let longest = delta.abs().max_element();
    let steps = (longest / MAX_SUBSTEP).ceil().max(1.0) as u32;
    let step = delta / steps as f32;
    for _ in 0..steps {
        for axis in [0, 2] {
            if !moved.blocked[axis] && step[axis] != 0.0 {
                move_across(world, body, &mut moved, axis, step[axis]);
            }
        }
        if !moved.blocked[1] && step.y != 0.0 {
            move_up_down(world, body, &mut moved, step.y);
        }
    }
    if delta.y <= 0.0 {
        moved.grounded =
            moved.blocked[1] && delta.y < 0.0 || on_ground(world, body, &mut moved.feet);
    }
    moved
}

/// Whether `body` at `feet` stands on solid voxels: its feet within `EPS`
/// of the top of a row with a solid voxel under its footprint. Snaps the
/// feet onto it when so.
pub fn on_ground(world: &impl Solid, body: &Body, feet: &mut Vec3) -> bool {
    let row = (feet.y + EPS).floor();
    if feet.y - row > EPS {
        return false;
    }
    let (lo, hi) = footprint(body, *feet);
    if any_solid(world, lo.with_y(row as i32 - 1), hi.with_y(row as i32 - 1)) {
        feet.y = row;
        true
    } else {
        false
    }
}

/// Whether the box of `body` at `feet` overlaps a solid voxel.
#[cfg(test)]
pub fn overlaps(world: &impl Solid, body: &Body, feet: Vec3) -> bool {
    let (lo, hi) = footprint(body, feet);
    let (y0, y1) = rows(body, feet);
    any_solid(world, lo.with_y(y0), hi.with_y(y1))
}

/// The voxel columns under the box, inclusive, with y left 0.
fn footprint(body: &Body, feet: Vec3) -> (IVec3, IVec3) {
    let lo = (feet - body.radius + EPS).floor().as_ivec3();
    let hi = (feet + body.radius - EPS).ceil().as_ivec3() - 1;
    (lo.with_y(0), hi.with_y(0))
}

/// The voxel rows the box fills, inclusive.
fn rows(body: &Body, feet: Vec3) -> (i32, i32) {
    let y0 = (feet.y + EPS).floor() as i32;
    let y1 = (feet.y + body.height - EPS).ceil() as i32 - 1;
    (y0, y1)
}

fn any_solid(world: &impl Solid, lo: IVec3, hi: IVec3) -> bool {
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                if world.solid(IVec3::new(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

/// One horizontal substep of `d` along `axis` (0 for x, 2 for z).
fn move_across(world: &impl Solid, body: &Body, moved: &mut Moved, axis: usize, d: f32) {
    let feet = moved.feet;
    let (entered, boundary) = if d > 0.0 {
        let lead = feet[axis] + body.radius;
        let old = (lead - EPS).ceil() as i32 - 1;
        let new = (lead + d - EPS).ceil() as i32 - 1;
        (old < new).then_some((new, new as f32 - body.radius))
    } else {
        let lead = feet[axis] - body.radius;
        let old = (lead + EPS).floor() as i32;
        let new = (lead + d + EPS).floor() as i32;
        (new < old).then_some((new, new as f32 + 1.0 + body.radius))
    }
    .unzip();
    let Some(cell) = entered else {
        moved.feet[axis] += d;
        return;
    };
    let (mut lo, mut hi) = footprint(body, feet);
    lo[axis] = cell;
    hi[axis] = cell;
    let (y0, y1) = rows(body, feet);
    // The highest solid row in the slab the leading face enters.
    let top = (y0..=y1)
        .rev()
        .find(|&y| any_solid(world, lo.with_y(y), hi.with_y(y)));
    let Some(top) = top else {
        moved.feet[axis] += d;
        return;
    };
    let rise = (top + 1) as f32 - feet.y;
    if rise <= body.step + EPS {
        let mut up = feet;
        up[axis] += d;
        up.y = (top + 1) as f32;
        // Headroom: the rows the box rises into, over its new footprint.
        let (lo, hi) = footprint(body, up);
        let (_, new_top) = rows(body, up);
        if !any_solid(world, lo.with_y(y1 + 1), hi.with_y(new_top)) {
            moved.feet = up;
            moved.stepped += rise;
            return;
        }
    }
    moved.feet[axis] = boundary.expect("set with the entered cell");
    moved.blocked[axis] = true;
}

/// One vertical substep of `d`.
fn move_up_down(world: &impl Solid, body: &Body, moved: &mut Moved, d: f32) {
    let feet = moved.feet;
    let (lo, hi) = footprint(body, feet);
    let (y0, y1) = rows(body, feet);
    if d < 0.0 {
        let new = (feet.y + d + EPS).floor() as i32;
        for y in (new..y0).rev() {
            if any_solid(world, lo.with_y(y), hi.with_y(y)) {
                moved.feet.y = (y + 1) as f32;
                moved.blocked[1] = true;
                return;
            }
        }
    } else {
        let new = (feet.y + body.height + d - EPS).ceil() as i32 - 1;
        for y in y1 + 1..=new {
            if any_solid(world, lo.with_y(y), hi.with_y(y)) {
                moved.feet.y = y as f32 - body.height;
                moved.blocked[1] = true;
                return;
            }
        }
    }
    moved.feet.y += d;
}

/// A thing that blocks bodies: an upright box `2·radius` across, standing
/// at `feet`. `id` tells a body which blocker is itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blocker {
    pub feet: Vec3,
    pub radius: f32,
    pub height: f32,
    pub id: u64,
}

/// [`move_body`], also stopped by `blockers` other than the one with id
/// `own`: each horizontal axis moves on its own, and a move along one that
/// ends overlapping a blocker and no farther from it is undone, as DOOM's
/// things block each other. Moving away from a blocker it already overlaps
/// is allowed, so bodies pushed together can part.
pub fn move_among(
    world: &impl Solid,
    body: &Body,
    feet: Vec3,
    delta: Vec3,
    blockers: &[Blocker],
    own: u64,
) -> Moved {
    let mut out = Moved {
        feet,
        ..Moved::default()
    };
    for axis in [0, 2] {
        if delta[axis] == 0.0 {
            continue;
        }
        let mut d = Vec3::ZERO;
        d[axis] = delta[axis];
        // Things first: it is cheaper than the voxels, and in a crowd it
        // is what usually blocks.
        if blocked_by(body, out.feet, out.feet + d, blockers, own) {
            out.blocked[axis] = true;
            continue;
        }
        let moved = move_body(world, body, out.feet, d);
        if blocked_by(body, out.feet, moved.feet, blockers, own) {
            out.blocked[axis] = true;
            continue;
        }
        out.feet = moved.feet;
        out.blocked[axis] = moved.blocked[axis];
        out.stepped += moved.stepped;
    }
    let moved = move_body(world, body, out.feet, Vec3::Y * delta.y);
    out.feet = moved.feet;
    out.blocked[1] = moved.blocked[1];
    out.grounded = moved.grounded;
    out
}

/// Whether moving `body` from `from` to `to` runs into a blocker.
fn blocked_by(body: &Body, from: Vec3, to: Vec3, blockers: &[Blocker], own: u64) -> bool {
    blockers.iter().any(|b| {
        if b.id == own {
            return false;
        }
        let reach = body.radius + b.radius;
        let gap = |p: Vec3| (p.x - b.feet.x).abs().max((p.z - b.feet.z).abs());
        let overlaps = gap(to) < reach
            && to.y < b.feet.y + b.height
            && to.y + body.height > b.feet.y;
        overlaps && gap(to) <= gap(from) + 1e-4
    })
}

/// Where a [`trace`] stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceHit {
    /// The solid voxel it hit.
    pub voxel: IVec3,
    /// The face it entered by, pointing back toward the start; zero when it
    /// started inside the voxel.
    pub normal: IVec3,
    /// How far along the ray, in voxels.
    pub distance: f32,
}

/// The first solid voxel along the ray from `origin` in unit direction
/// `dir`, within `max` voxels. Walks the grid one voxel at a time, so it
/// misses nothing.
pub fn trace(world: &impl Solid, origin: Vec3, dir: Vec3, max: f32) -> Option<TraceHit> {
    let mut voxel = origin.floor().as_ivec3();
    if world.solid(voxel) {
        return Some(TraceHit {
            voxel,
            normal: IVec3::ZERO,
            distance: 0.0,
        });
    }
    let step = dir.signum().as_ivec3();
    let next_boundary = |o: f32, d: f32, v: i32| {
        if d > 0.0 {
            (v as f32 + 1.0 - o) / d
        } else if d < 0.0 {
            (v as f32 - o) / d
        } else {
            f32::INFINITY
        }
    };
    let mut t_max = Vec3::new(
        next_boundary(origin.x, dir.x, voxel.x),
        next_boundary(origin.y, dir.y, voxel.y),
        next_boundary(origin.z, dir.z, voxel.z),
    );
    let t_delta = Vec3::new(
        (1.0 / dir.x).abs(),
        (1.0 / dir.y).abs(),
        (1.0 / dir.z).abs(),
    );
    loop {
        let axis = if t_max.x < t_max.y {
            if t_max.x < t_max.z {
                0
            } else {
                2
            }
        } else if t_max.y < t_max.z {
            1
        } else {
            2
        };
        let distance = t_max[axis];
        if distance > max {
            return None;
        }
        voxel[axis] += step[axis];
        t_max[axis] += t_delta[axis];
        if world.solid(voxel) {
            let mut normal = IVec3::ZERO;
            normal[axis] = -step[axis];
            return Some(TraceHit {
                voxel,
                normal,
                distance,
            });
        }
    }
}

/// Where the ray from `origin` along unit `dir` first meets the upright
/// cylinder standing at `feet`, if within `max`: the distance along the ray.
/// Starting inside it counts as meeting it at 0.
pub fn ray_cylinder(
    origin: Vec3,
    dir: Vec3,
    max: f32,
    feet: Vec3,
    radius: f32,
    height: f32,
) -> Option<f32> {
    let (ox, oz) = (origin.x - feet.x, origin.z - feet.z);
    let (y0, y1) = (feet.y, feet.y + height);
    let a = dir.x * dir.x + dir.z * dir.z;
    let c = ox * ox + oz * oz - radius * radius;
    // The span of the ray inside the infinite cylinder.
    let (mut t0, mut t1) = if a < 1e-9 {
        if c > 0.0 {
            return None;
        }
        (f32::NEG_INFINITY, f32::INFINITY)
    } else {
        let b = ox * dir.x + oz * dir.z;
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        ((-b - root) / a, (-b + root) / a)
    };
    // Clip to the slab between its bottom and top.
    if dir.y.abs() < 1e-9 {
        if origin.y < y0 || origin.y > y1 {
            return None;
        }
    } else {
        let (a, b) = ((y0 - origin.y) / dir.y, (y1 - origin.y) / dir.y);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    if t0 > t1 || t1 < 0.0 {
        return None;
    }
    let t = t0.max(0.0);
    (t <= max).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DOOM's player at 2 map units per voxel.
    const PLAYER: Body = Body {
        radius: 8.0,
        height: 28.0,
        step: 12.0,
    };

    /// Solid below y = 0, plus whatever `extra` says.
    fn floor_and(extra: impl Fn(IVec3) -> bool) -> impl Fn(IVec3) -> bool {
        move |p: IVec3| p.y < 0 || extra(p)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 1e-3
    }

    /// Falls under gravity for `seconds` at 60 frames a second.
    fn fall(
        world: &impl Solid,
        body: &Body,
        mut feet: Vec3,
        seconds: f32,
    ) -> (Vec3, bool) {
        let (dt, gravity) = (1.0 / 60.0, 612.5);
        let mut vy = 0.0;
        let mut grounded = false;
        for _ in 0..(seconds * 60.0) as u32 {
            vy -= gravity * dt;
            let moved = move_body(world, body, feet, Vec3::new(0.0, vy * dt, 0.0));
            feet = moved.feet;
            grounded = moved.grounded;
            if grounded {
                vy = 0.0;
            }
        }
        (feet, grounded)
    }

    #[test]
    fn a_falling_body_lands_on_the_floor_and_stays_there() {
        let world = floor_and(|_| false);
        let (feet, grounded) = fall(&world, &PLAYER, Vec3::new(0.5, 40.0, 0.5), 2.0);
        assert_eq!(feet, Vec3::new(0.5, 0.0, 0.5));
        assert!(grounded);
        // Standing still, it stays grounded frame after frame.
        let moved = move_body(&world, &PLAYER, feet, Vec3::new(0.0, -1e-5, 0.0));
        assert!(moved.grounded && moved.feet.y == 0.0, "{moved:?}");
        let moved = move_body(&world, &PLAYER, feet, Vec3::ZERO);
        assert!(moved.grounded);
    }

    #[test]
    fn a_wall_blocks() {
        let world = floor_and(|p| p.x >= 20);
        let moved = move_body(
            &world,
            &PLAYER,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
        );
        assert_eq!(moved.feet.x, 12.0);
        assert!(moved.blocked[0]);
        // And from the other side.
        let world = floor_and(|p| p.x < -20);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(-30.0, 0.0, 0.0));
        assert_eq!(moved.feet.x, -12.0);
    }

    #[test]
    fn a_body_slides_along_a_wall() {
        let world = floor_and(|p| p.x >= 20);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(20.0, 0.0, -20.0));
        assert!(close(moved.feet, Vec3::new(12.0, 0.0, -20.0)), "{moved:?}");
        assert_eq!(moved.blocked, [true, false, false]);
    }

    #[test]
    fn a_fast_body_doesnt_tunnel_through_a_thin_wall() {
        let world = floor_and(|p| p.z == -30);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(0.0, 0.0, -500.0));
        assert_eq!(moved.feet.z, -21.0);
        // Nor through a thin floor when falling fast.
        let world = |p: IVec3| p.y == -50;
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(0.0, -400.0, 0.0));
        assert_eq!(moved.feet.y, -49.0);
        assert!(moved.grounded);
    }

    #[test]
    fn steps_up_to_the_step_height_and_no_higher() {
        // DOOM stairs: 8 units, 4 voxels, a step.
        let stairs = floor_and(|p| p.x >= 10 && p.y < (p.x - 10) / 10 * 4 + 4);
        let moved = move_body(&stairs, &PLAYER, Vec3::ZERO, Vec3::new(50.0, 0.0, 0.0));
        assert!(close(moved.feet, Vec3::new(50.0, 20.0, 0.0)), "{moved:?}");
        assert_eq!(moved.stepped, 20.0);
        // 12 voxels is DOOM's 24 units: a step.
        let ledge = |h: i32| floor_and(move |p| p.x >= 10 && p.y < h);
        let moved = move_body(&ledge(12), &PLAYER, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.feet, Vec3::new(10.0, 12.0, 0.0));
        let moved = move_body(&ledge(13), &PLAYER, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.feet, Vec3::new(2.0, 0.0, 0.0));
        assert!(moved.blocked[0]);
    }

    #[test]
    fn no_step_up_without_headroom() {
        // A step under a ceiling 30 voxels up: 4 + 28 doesn't fit.
        let world = floor_and(|p| (p.x >= 10 && p.y < 4) || p.y >= 30);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.feet, Vec3::new(2.0, 0.0, 0.0));
        let world = floor_and(|p| (p.x >= 10 && p.y < 2) || p.y >= 30);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(moved.feet, Vec3::new(10.0, 2.0, 0.0));
    }

    #[test]
    fn a_ceiling_stops_a_jump() {
        let world = floor_and(|p| p.y >= 40);
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(0.0, 30.0, 0.0));
        assert_eq!(moved.feet.y, 12.0);
        assert!(moved.blocked[1] && !moved.grounded);
    }

    #[test]
    fn falls_into_a_pit_and_jumps_out() {
        // A pit 20 voxels deep with straight sides, deeper than a step.
        let pit =
            |p: IVec3| p.y < 0 && !(p.x.abs() <= 10 && p.z.abs() <= 10 && p.y >= -20);
        let (feet, grounded) = fall(&pit, &PLAYER, Vec3::new(0.5, 0.0, 0.5), 1.0);
        assert!(grounded && feet.y == -20.0, "{feet}");
        // Walking out is blocked by its side...
        let walked = move_body(&pit, &PLAYER, feet, Vec3::new(20.0, 0.0, 0.0));
        assert!(walked.blocked[0] && walked.feet.y == -20.0, "{walked:?}");
        // ...until a jump lifts it within a step of the rim.
        let jumped = move_body(&pit, &PLAYER, feet, Vec3::new(0.0, 9.0, 0.0));
        let out = move_body(&pit, &PLAYER, jumped.feet, Vec3::new(20.0, 0.0, 0.0));
        assert!(!out.blocked[0] && out.feet.y == 0.0, "{out:?}");
    }

    #[test]
    fn walks_out_of_a_shallow_crater() {
        // A ball of radius 16 carved out of the floor: steps lift it out. A
        // body as wide as the player rests on the rim of narrower ones.
        let crater = |p: IVec3| p.y < 0 && (p.as_vec3() + 0.5).length() > 16.0;
        let (feet, _) = fall(&crater, &PLAYER, Vec3::new(0.5, 0.0, 0.5), 1.0);
        assert!(feet.y < -3.0, "{feet}");
        let out = move_body(&crater, &PLAYER, feet, Vec3::new(30.0, 0.0, 0.0));
        assert!(!out.blocked[0] && out.feet.y == 0.0, "{out:?}");
    }

    #[test]
    fn voxels_inside_the_box_dont_trap_it() {
        let world = floor_and(|p| p == IVec3::new(0, 5, 0));
        assert!(overlaps(&world, &PLAYER, Vec3::ZERO));
        let moved = move_body(&world, &PLAYER, Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0));
        assert!((moved.feet.x - 20.0).abs() < 1e-3);
        assert!(!overlaps(&world, &PLAYER, moved.feet));
    }

    #[test]
    fn things_block_and_bodies_can_part() {
        let world = floor_and(|_| false);
        let other = Blocker {
            feet: Vec3::new(30.0, 0.0, 0.0),
            radius: 10.0,
            height: 28.0,
            id: 7,
        };
        let moved = move_among(
            &world,
            &PLAYER,
            Vec3::ZERO,
            Vec3::new(20.0, 0.0, 5.0),
            &[other],
            1,
        );
        // Stopped short along x, sliding along z.
        assert!(moved.blocked[0] && moved.feet.x < 12.0 + 1e-3, "{moved:?}");
        assert_eq!(moved.feet.z, 5.0);
        // Itself doesn't block.
        let moved = move_among(
            &world,
            &PLAYER,
            Vec3::ZERO,
            Vec3::new(20.0, 0.0, 0.0),
            &[other],
            7,
        );
        assert!(!moved.blocked[0]);
        // Overlapping, it may move away but not closer.
        let inside = Vec3::new(25.0, 0.0, 0.0);
        let away = move_among(
            &world,
            &PLAYER,
            inside,
            Vec3::new(-2.0, 0.0, 0.0),
            &[other],
            1,
        );
        assert!(!away.blocked[0]);
        let closer = move_among(
            &world,
            &PLAYER,
            inside,
            Vec3::new(2.0, 0.0, 0.0),
            &[other],
            1,
        );
        assert!(closer.blocked[0]);
        // Over its top, it doesn't block.
        let above = Vec3::new(0.0, 40.0, 0.0);
        let world = |_: IVec3| false;
        let moved = move_among(
            &world,
            &PLAYER,
            above,
            Vec3::new(20.0, 0.0, 0.0),
            &[other],
            1,
        );
        assert!(!moved.blocked[0]);
    }

    #[test]
    fn traces_stop_at_the_first_solid_voxel() {
        let world = |p: IVec3| p.x == 10 && p.y != 3;
        let hit = trace(&world, Vec3::new(0.5, 0.5, 0.5), Vec3::X, 100.0).unwrap();
        assert_eq!(
            (hit.voxel, hit.normal),
            (IVec3::new(10, 0, 0), IVec3::NEG_X)
        );
        assert!((hit.distance - 9.5).abs() < 1e-5);
        assert_eq!(trace(&world, Vec3::new(0.5, 0.5, 0.5), Vec3::X, 9.0), None);
        // Through the gap at y = 3.
        assert_eq!(
            trace(&world, Vec3::new(0.5, 3.5, 0.5), Vec3::X, 100.0),
            None
        );
        // A diagonal ray doesn't slip between voxels.
        let diagonal = |p: IVec3| p.x + p.z == 10;
        let dir = Vec3::new(1.0, 0.0, 1.0).normalize();
        assert!(trace(&diagonal, Vec3::splat(0.5), dir, 100.0).is_some());
    }

    #[test]
    fn rays_meet_cylinders() {
        let feet = Vec3::new(10.0, 0.0, 0.0);
        let hit = ray_cylinder(Vec3::new(0.0, 5.0, 0.0), Vec3::X, 100.0, feet, 2.0, 10.0);
        assert!((hit.unwrap() - 8.0).abs() < 1e-5);
        // Over its top, past its end, or beside it.
        assert_eq!(
            ray_cylinder(Vec3::new(0.0, 11.0, 0.0), Vec3::X, 100.0, feet, 2.0, 10.0),
            None
        );
        assert_eq!(
            ray_cylinder(Vec3::new(0.0, 5.0, 0.0), Vec3::X, 7.0, feet, 2.0, 10.0),
            None
        );
        assert_eq!(
            ray_cylinder(Vec3::new(0.0, 5.0, 3.0), Vec3::X, 100.0, feet, 2.0, 10.0),
            None
        );
        // Down through its top.
        let hit = ray_cylinder(
            Vec3::new(10.0, 20.0, 0.0),
            Vec3::NEG_Y,
            100.0,
            feet,
            2.0,
            10.0,
        );
        assert!((hit.unwrap() - 10.0).abs() < 1e-5);
    }
}
