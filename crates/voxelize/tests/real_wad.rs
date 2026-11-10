//! Builds maps from the WAD at `DOOM_WAD`; skipped when it is unset.

use std::time::Instant;

use doom_voxelize::{Options, Voxel, VoxelMap, INTERIOR_DEPTH};
use doom_wad::{MapData, Wad};

fn wad() -> Option<Wad> {
    let Some(path) = std::env::var_os("DOOM_WAD") else {
        println!("DOOM_WAD is unset; skipping the real-WAD test");
        return None;
    };
    Some(Wad::open(path).expect("open the WAD"))
}

fn first_map(wad: &Wad) -> String {
    let names = wad.map_names();
    if names.iter().any(|n| n == "E1M1") {
        "E1M1".into()
    } else {
        names[0].clone()
    }
}

#[test]
fn first_map_spawns_in_air_over_a_floor() {
    let Some(wad) = wad() else { return };
    let name = first_map(&wad);
    let started = Instant::now();
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    println!("{name} built in {:.3} s", started.elapsed().as_secs_f32());
    let (pos, yaw) = m.spawn();
    let [x, y, z] = pos.map(|v| v.floor() as i32);
    println!("spawn {pos:?} yaw {:.1}°", yaw.to_degrees());
    let (floor, ceiling) = m.column_air(x, z).expect("spawn column is in a sector");
    assert!((floor..ceiling).contains(&y));
    for h in floor..=y {
        assert_eq!(m.voxel([x, h, z]), Voxel::Air, "spawn column at {h}");
    }
    assert!(matches!(m.voxel([x, floor - 1, z]), Voxel::Solid(_)));
}

/// The sector (or void, `None`) DOOM's geometry puts map point `(px, py)`
/// in, judged by the side of the nearest linedef; `None` when the nearest
/// point is a linedef's end or the point lies on the line, where the side
/// is ambiguous.
fn nearest_line_side(map: &MapData, px: f64, py: f64) -> Option<Option<u16>> {
    let mut best = (f64::MAX, None);
    for l in &map.linedefs {
        let (a, b) = (map.vertexes[l.v1 as usize], map.vertexes[l.v2 as usize]);
        let (ax, ay) = (a.x as f64, a.y as f64);
        let (dx, dy) = (b.x as f64 - ax, b.y as f64 - ay);
        let len2 = dx * dx + dy * dy;
        if len2 == 0.0 {
            continue;
        }
        let t = ((px - ax) * dx + (py - ay) * dy) / len2;
        let tc = t.clamp(0.0, 1.0);
        let (cx, cy) = (ax + dx * tc, ay + dy * tc);
        let d = (px - cx).hypot(py - cy);
        if d < best.0 {
            let len = len2.sqrt();
            let interior = t * len > 0.5 && (1.0 - t) * len > 0.5 && d > 1e-6;
            let right = dx * (py - ay) - dy * (px - ax) < 0.0;
            let side = if right { l.front } else { l.back };
            best = (
                d,
                interior.then(|| side.map(|s| map.sidedefs[s as usize].sector)),
            );
        }
    }
    best.1
}

/// Distance from `(px, py)` to the nearest linedef with a missing side.
fn nearest_one_sided(map: &MapData, px: f64, py: f64) -> f64 {
    let mut best = f64::MAX;
    for l in map.linedefs.iter().filter(|l| l.back.is_none()) {
        let (a, b) = (map.vertexes[l.v1 as usize], map.vertexes[l.v2 as usize]);
        let (ax, ay) = (a.x as f64, a.y as f64);
        let (dx, dy) = (b.x as f64 - ax, b.y as f64 - ay);
        let len2 = (dx * dx + dy * dy).max(1e-9);
        let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
        best = best.min((px - ax - dx * t).hypot(py - ay - dy * t));
    }
    best
}

/// Columns sampled every `step`: how many the nearest-linedef side judges
/// unambiguously, and how many of those disagree on void-or-not.
fn void_mismatches(wad: &Wad, name: &str, step: usize) -> (usize, usize) {
    let map = wad.map(name).unwrap();
    let m = VoxelMap::build(wad, name, &Options::default()).unwrap();
    let s = m.units_per_voxel() as f64;
    let (min, max) = m.bounds();
    let (mut checked, mut mismatches) = (0, 0);
    for z in (min[2]..max[2]).step_by(step) {
        for x in (min[0]..max[0]).step_by(step) {
            let (px, py) = ((x as f64 + 0.5) * s, -(z as f64 + 0.5) * s);
            let Some(want) = nearest_line_side(&map, px, py) else {
                continue;
            };
            checked += 1;
            let got = m.column_sector(x, z);
            if got.is_some() != want.is_some() {
                mismatches += 1;
                if mismatches <= 5 {
                    println!("{name}: void mismatch at DOOM ({px}, {py}): {got:?}, nearest line {want:?}");
                }
            }
        }
    }
    (checked, mismatches)
}

#[test]
fn void_test_agrees_with_the_nearest_linedef_on_every_map() {
    let Some(wad) = wad() else { return };
    for name in wad.map_names() {
        let (checked, mismatches) = void_mismatches(&wad, &name, 9);
        println!("{name}: {checked} columns checked, {mismatches} void mismatches");
        assert!(
            mismatches * 1000 <= checked,
            "{name}: {mismatches} of {checked}"
        );
    }
}

#[test]
fn first_map_has_no_stray_voids() {
    let Some(wad) = wad() else { return };
    let name = first_map(&wad);
    let map = wad.map(&name).unwrap();
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    let s = m.units_per_voxel() as f64;
    let (min, max) = m.bounds();
    let (checked, mismatches) = void_mismatches(&wad, &name, 2);
    println!("{name}: {checked} columns checked, {mismatches} void mismatches");
    assert_eq!(mismatches, 0);

    // A void column beside a sector column must sit on a wall: a linedef
    // with a missing side within a diagonal voxel step.
    let mut stray = 0;
    for z in min[2]..max[2] {
        for x in min[0]..max[0] {
            if m.column_sector(x, z).is_some() {
                continue;
            }
            let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dz)| m.column_sector(x + dx, z + dz).is_some());
            let (px, py) = ((x as f64 + 0.5) * s, -(z as f64 + 0.5) * s);
            if beside && nearest_one_sided(&map, px, py) > s * 1.5 {
                stray += 1;
                if stray <= 10 {
                    println!("stray void at column ({x}, {z}), DOOM ({px}, {py})");
                }
            }
        }
    }
    println!("{name}: {stray} stray void columns");
    assert_eq!(stray, 0);
}

#[test]
fn uniform_in_matches_brute_force_on_chunks() {
    let Some(wad) = wad() else { return };
    let name = first_map(&wad);
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    let (min, max) = m.bounds();
    let mut rng = 0x9e3779b97f4a7c15u64;
    let mut next = |lo: i32, hi: i32| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        lo + (rng % (hi - lo) as u64) as i32
    };
    let (mut claims, mut nones, mut voxels) = (0, 0, 0u64);
    let started = Instant::now();
    for i in 0..600 {
        // Chunk boxes as the app asks: 32 << lod voxels plus one of padding.
        let lod = [0, 0, 0, 1, 2][i % 5];
        let size = 32 << lod;
        let mut c = |lo: i32, hi: i32| {
            next(lo.div_euclid(size) - 1, hi.div_euclid(size) + 2) * size
        };
        let lo = [
            c(min[0], max[0]) - 1,
            c(min[1], max[1]) - 1,
            c(min[2], max[2]) - 1,
        ];
        let hi = lo.map(|v| v + size + 2);
        match m.uniform_in(lo, hi) {
            Some(v) => {
                claims += 1;
                // Every voxel for small boxes, a lattice for large ones.
                let step = 1 << lod;
                for x in (lo[0]..hi[0]).step_by(step) {
                    for y in lo[1]..hi[1] {
                        for z in (lo[2]..hi[2]).step_by(step) {
                            voxels += 1;
                            assert_eq!(
                                m.voxel([x, y, z]),
                                v,
                                "{lo:?}..{hi:?} at {x},{y},{z}"
                            );
                        }
                    }
                }
            }
            None => nones += 1,
        }
    }
    println!(
        "{name}: {claims} uniform claims checked over {voxels} voxels, {nones} mixed boxes, {:.2} s",
        started.elapsed().as_secs_f32()
    );
    assert!(claims > 100);
}

/// Boxes placed just past the interior depth below floors, above ceilings
/// and behind walls, where `uniform_in` is closest to being wrong: every
/// voxel of each box it calls uniform must agree.
#[test]
fn uniform_interior_agrees_at_the_interior_depth() {
    let Some(wad) = wad() else { return };
    let name = first_map(&wad);
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    let (min, max) = m.bounds();
    let mut rng = 0x2545f4914f6cdd1du64;
    let mut next = |lo: i32, hi: i32| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        lo + (rng % (hi - lo) as u64) as i32
    };
    let n = INTERIOR_DEPTH;
    let (mut tried, mut claims, mut voxels) = (0, 0, 0u64);
    while tried < 40_000 {
        let (x, z) = (next(min[0], max[0]), next(min[2], max[2]));
        let size = [next(1, 40), next(1, 40), next(1, 40)];
        // The box's near face this far past the interior depth.
        let past = next(-3, 8);
        let lo_y = match (m.column_air(x, z), next(0, 3)) {
            (Some((floor, _)), 0) => floor - 1 - n - past - size[1],
            (Some((_, ceiling)), 1) if ceiling < max[1] => ceiling + n + past,
            (None, _) => next(min[1], max[1]),
            _ => continue,
        };
        tried += 1;
        let lo = [x - size[0] / 2, lo_y, z - size[2] / 2];
        let hi = [lo[0] + size[0], lo[1] + size[1], lo[2] + size[2]];
        let Some(v) = m.uniform_in(lo, hi) else {
            continue;
        };
        claims += 1;
        for x in lo[0]..hi[0] {
            for y in lo[1]..hi[1] {
                for z in lo[2]..hi[2] {
                    voxels += 1;
                    assert_eq!(m.voxel([x, y, z]), v, "{lo:?}..{hi:?} at {x},{y},{z}");
                }
            }
        }
    }
    println!("{name}: {claims} of {tried} boxes uniform, {voxels} voxels checked");
    assert!(claims > 1000, "{claims} claims");
}

/// What one `voxel` call costs over the chunks the app would generate:
/// every 32³ chunk inside the bounds that `uniform_in` can't answer. Only
/// meaningful in a release build (`cargo test --release`).
#[test]
fn voxel_lookup_cost() {
    let Some(wad) = wad() else { return };
    let name = first_map(&wad);
    let m = VoxelMap::build(&wad, &name, &Options::default()).unwrap();
    let (min, max) = m.bounds();
    let mut boxes = Vec::new();
    for cz in min[2].div_euclid(32)..=max[2].div_euclid(32) {
        for cy in min[1].div_euclid(32)..=max[1].div_euclid(32) {
            for cx in min[0].div_euclid(32)..=max[0].div_euclid(32) {
                let lo = [cx * 32 - 1, cy * 32 - 1, cz * 32 - 1];
                let hi = lo.map(|v| v + 34);
                if m.uniform_in(lo, hi).is_none() {
                    boxes.push(lo);
                }
            }
        }
    }
    let started = Instant::now();
    let (mut n, mut solid) = (0u64, 0u64);
    for lo in &boxes {
        for x in lo[0]..lo[0] + 34 {
            for y in lo[1]..lo[1] + 34 {
                for z in lo[2]..lo[2] + 34 {
                    n += 1;
                    solid += matches!(m.voxel([x, y, z]), Voxel::Solid(_)) as u64;
                }
            }
        }
    }
    let t = started.elapsed().as_secs_f64();
    println!(
        "{name}: {} mixed chunks, {n} voxels ({solid} solid) in {t:.3} s: {:.2} ns per voxel",
        boxes.len(),
        t * 1e9 / n as f64
    );
}

#[test]
fn every_map_builds() {
    let Some(wad) = wad() else { return };
    let mut slowest = (0.0f32, String::new());
    for name in wad.map_names() {
        let started = Instant::now();
        let m = VoxelMap::build(&wad, &name, &Options::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let t = started.elapsed().as_secs_f32();
        if t > slowest.0 {
            slowest = (t, name.clone());
        }
        let (pos, _) = m.spawn();
        let [x, y, z] = pos.map(|v| v.floor() as i32);
        assert_eq!(m.voxel([x, y, z]), Voxel::Air, "{name}: spawn is solid");
        let (min, max) = m.bounds();
        println!(
            "{name}: {t:.3} s, {}×{}×{} voxels",
            max[0] - min[0],
            max[1] - min[1],
            max[2] - min[2]
        );
    }
    println!("slowest: {} at {:.3} s", slowest.1, slowest.0);
}
