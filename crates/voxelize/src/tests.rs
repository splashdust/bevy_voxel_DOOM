//! A hand-built corridor of rectangular sectors along DOOM x, with a BSP
//! that splits at each sector boundary.

use std::collections::HashMap;

use doom_wad::{
    Colormap, LineDef, MapData, Node, Picture, Sector, Seg, SideDef, SubSector,
    TextureSet, Thing, Vertex, ML_TWOSIDED, NF_SUBSECTOR,
};

use crate::{Options, Voxel, VoxelMap, INTERIOR};

const H: i16 = 128;

struct Room {
    x1: i16,
    floor: i16,
    ceiling: i16,
    ceiling_flat: &'static str,
}

/// How the boundary into room `k` is drawn.
#[derive(Clone, Copy, Default)]
struct Boundary {
    /// Front faces west (into room k − 1) rather than east.
    front_west: bool,
    special: u16,
    /// A middle texture on the front side.
    middle: Option<&'static str>,
}

/// Rooms from x = 0 east, each spanning y 0..128.
fn corridor(rooms: &[Room], boundaries: &[Boundary]) -> MapData {
    let n = rooms.len();
    let xs: Vec<i16> = std::iter::once(0)
        .chain(rooms.iter().map(|r| r.x1))
        .collect();
    let mut vertexes = Vec::new();
    for &x in &xs {
        vertexes.push(Vertex { x, y: 0 });
        vertexes.push(Vertex { x, y: H });
    }
    let mut sidedefs = Vec::new();
    let mut side = |sector: usize, middle: &str| {
        sidedefs.push(SideDef {
            x_offset: 0,
            y_offset: 0,
            upper: "WALL".into(),
            lower: "WALL".into(),
            middle: middle.into(),
            sector: sector as u16,
        });
        Some(sidedefs.len() as u16 - 1)
    };
    let mut linedefs = Vec::new();
    // Per room: (linedef, seg side) bounding it.
    let mut bounds: Vec<Vec<(usize, u16)>> = vec![Vec::new(); n];
    let one_sided = |v1: usize, v2: usize, front| LineDef {
        v1: v1 as u16,
        v2: v2 as u16,
        flags: 1,
        special: 0,
        tag: 0,
        front,
        back: None,
    };
    for (i, b) in bounds.iter_mut().enumerate() {
        // South (x1,0)→(x0,0) and north (x0,H)→(x1,H): right side inside.
        linedefs.push(one_sided(2 * i + 2, 2 * i, side(i, "WALL")));
        b.push((linedefs.len() - 1, 0));
        linedefs.push(one_sided(2 * i + 1, 2 * i + 3, side(i, "COLS")));
        b.push((linedefs.len() - 1, 0));
    }
    linedefs.push(one_sided(0, 1, side(0, "WALL")));
    bounds[0].push((linedefs.len() - 1, 0));
    linedefs.push(one_sided(2 * n + 1, 2 * n, side(n - 1, "WALL")));
    bounds[n - 1].push((linedefs.len() - 1, 0));
    for k in 1..n {
        let b = boundaries.get(k - 1).copied().unwrap_or_default();
        let (west, east) = (k - 1, k);
        let (v1, v2, front, back) = if b.front_west {
            (2 * k + 1, 2 * k, west, east)
        } else {
            (2 * k, 2 * k + 1, east, west)
        };
        let f = side(front, b.middle.unwrap_or("-"));
        let bk = side(back, "-");
        linedefs.push(LineDef {
            v1: v1 as u16,
            v2: v2 as u16,
            flags: ML_TWOSIDED,
            special: b.special,
            tag: 0,
            front: f,
            back: bk,
        });
        let l = linedefs.len() - 1;
        bounds[front].push((l, 0));
        bounds[back].push((l, 1));
    }
    let mut segs = Vec::new();
    let mut subsectors = Vec::new();
    for b in &bounds {
        subsectors.push(SubSector {
            seg_count: b.len() as u16,
            first_seg: segs.len() as u16,
        });
        for &(l, s) in b {
            let line: &LineDef = &linedefs[l];
            let (v1, v2) = if s == 0 {
                (line.v1, line.v2)
            } else {
                (line.v2, line.v1)
            };
            segs.push(Seg {
                v1,
                v2,
                angle: 0,
                linedef: l as u16,
                side: s,
                offset: 0,
            });
        }
    }
    // Node for boundary k (1..n) splits at x = xs[k], pointing north: its
    // back (left) is the west. Listed last-first, so the root is last.
    let mut nodes = Vec::new();
    for k in (1..n).rev() {
        let east = if k == n - 1 {
            NF_SUBSECTOR | (n as u16 - 1)
        } else {
            nodes.len() as u16 - 1
        };
        nodes.push(Node {
            x: xs[k],
            y: 0,
            dx: 0,
            dy: H,
            bbox: [[0; 4]; 2],
            children: [east, NF_SUBSECTOR | (k as u16 - 1)],
        });
    }
    let sectors = rooms
        .iter()
        .map(|r| Sector {
            floor: r.floor,
            ceiling: r.ceiling,
            floor_flat: "FLOOR".into(),
            ceiling_flat: r.ceiling_flat.into(),
            light: 160,
            special: 0,
            tag: 0,
        })
        .collect();
    MapData {
        name: "E1M1".into(),
        things: vec![Thing {
            x: 64,
            y: 64,
            angle: 90,
            kind: 1,
            flags: 7,
        }],
        linedefs,
        sidedefs,
        vertexes,
        segs,
        subsectors,
        nodes,
        sectors,
        blockmap: None,
    }
}

fn picture(w: u16, h: u16, f: impl Fn(u16, u16) -> Option<u8>) -> Picture {
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            pixels.push(f(x, y));
        }
    }
    Picture {
        width: w,
        height: h,
        pixels,
    }
}

fn textures() -> TextureSet {
    let mut flats = HashMap::new();
    // The palette index is u, plus 64 on odd rows.
    flats.insert(
        "FLOOR".into(),
        picture(64, 64, |u, v| Some((u + 64 * (v & 1)) as u8)),
    );
    flats.insert("CEIL".into(), picture(64, 64, |_, _| Some(200)));
    flats.insert("F_SKY1".into(), picture(64, 64, |_, _| Some(201)));
    let mut walls = HashMap::new();
    // The palette index is the texture row.
    walls.insert("WALL".into(), picture(64, 128, |_, v| Some(v as u8)));
    // The palette index is the texture column.
    walls.insert("COLS".into(), picture(128, 128, |u, _| Some(u as u8)));
    // 16×32, every fourth column from 1 transparent, otherwise 100 + row.
    walls.insert(
        "GRATE".into(),
        picture(16, 32, |u, v| (u % 4 != 1).then_some(100 + v as u8)),
    );
    walls.insert("SKY1".into(), picture(4, 4, |_, _| Some(3)));
    TextureSet { flats, walls }
}

fn colormap() -> Colormap {
    let mut tables = Vec::new();
    for k in 0..34u8 {
        let mut t = [0u8; 256];
        for (i, v) in t.iter_mut().enumerate() {
            // Table k shifts by k, so a test can read the table back.
            *v = (i as u8).wrapping_add(k);
        }
        tables.push(t);
    }
    Colormap { tables }
}

fn palette() -> doom_wad::Palette {
    let mut p = [[0u8; 3]; 256];
    for (i, c) in p.iter_mut().enumerate() {
        *c = [i as u8, i as u8, i as u8];
    }
    p
}

/// Rooms: A 0..128 (floor 0), step B 128..192 (floor 16), door C 192..208
/// (closed at 16), sky room D 208..320 (ceiling 144). A grate on the A|B
/// line, manual doors on both lines of C.
fn test_map() -> MapData {
    let room = |x1, floor, ceiling, ceiling_flat| Room {
        x1,
        floor,
        ceiling,
        ceiling_flat,
    };
    corridor(
        &[
            room(128, 0, 128, "CEIL"),
            room(192, 16, 128, "CEIL"),
            room(208, 16, 16, "CEIL"),
            room(320, 16, 144, "F_SKY1"),
        ],
        &[
            Boundary {
                middle: Some("GRATE"),
                ..Default::default()
            },
            Boundary {
                front_west: true,
                special: 1,
                ..Default::default()
            },
            Boundary {
                special: 1,
                ..Default::default()
            },
        ],
    )
}

fn build(open_doors: bool, sector_light: bool) -> VoxelMap {
    let options = Options {
        units_per_voxel: 2,
        sector_light,
        open_doors,
    };
    VoxelMap::from_map_data(&test_map(), palette(), &colormap(), &textures(), &options)
        .unwrap()
}

/// Column x for DOOM x, z for DOOM y, at 2 units per voxel.
fn col(doom_x: i32, doom_y: i32) -> (i32, i32) {
    (doom_x.div_euclid(2), (-doom_y).div_euclid(2))
}

#[test]
fn columns_find_their_sector_and_the_void() {
    let m = build(true, false);
    for (x, y, want) in [
        (5, 5, Some(0)),
        (127, 64, Some(0)),
        (129, 64, Some(1)),
        (200, 64, Some(2)),
        (300, 120, Some(3)),
        (64, 130, None),
        (64, -3, None),
        (-3, 64, None),
        (323, 64, None),
    ] {
        let (cx, cz) = col(x, y);
        assert_eq!(m.column_sector(cx, cz), want, "DOOM ({x}, {y})");
    }
}

#[test]
fn spans_floors_ceilings_sky_and_bounds() {
    let m = build(true, false);
    let (x, z) = col(65, 65);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(1 + 64));
    assert_eq!(m.voxel([x, 0, z]), Voxel::Air);
    assert_eq!(m.voxel([x, 63, z]), Voxel::Air);
    assert_eq!(m.voxel([x, 64, z]), Voxel::Solid(200));
    assert_eq!(m.voxel([x, 65, z]), Voxel::Solid(INTERIOR));
    // The sky room is air to the top: ceiling 144 → voxel 72, plus 16.
    let (sx, sz) = col(301, 61);
    let (min, max) = m.bounds();
    assert_eq!(max[1], 72 + 16);
    assert_eq!(min[1], -2);
    assert_eq!(m.voxel([sx, 80, sz]), Voxel::Air);
    assert_eq!(m.voxel([sx, max[1] + 3, sz]), Voxel::Air);
    // Floor texel (301 & 63, −61 & 63) = (45, 3).
    assert_eq!(m.voxel([sx, 7, sz]), Voxel::Solid(45 + 64));
    // The void is solid up to the top.
    let (vx, vz) = col(64, 140);
    assert_eq!(m.voxel([vx, 20, vz]), Voxel::Solid(INTERIOR));
    assert_eq!(m.voxel([vx, max[1] - 1, vz]), Voxel::Solid(INTERIOR));
    assert_eq!(m.voxel([vx, max[1], vz]), Voxel::Air);
}

#[test]
fn flats_tile_on_the_world_grid() {
    let m = build(true, false);
    // Column centre (65, 65): texel (65 & 63, −65 & 63) = (1, 63).
    let (x, z) = col(65, 65);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(1 + 64));
    // Centre (3, 1): texel (3, −1 & 63 = 63).
    let (x, z) = col(3, 1);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(3 + 64));
    // Centre (3, 3): texel (3, 61).
    let (x, z) = col(3, 3);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(3 + 64));
    // Centre (3, 63): texel (3, 1).
    let (x, z) = col(3, 63);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(3 + 64));
}

#[test]
fn step_walls_use_the_lower_texture_pegged_to_the_back_floor() {
    let m = build(true, false);
    // B's column next to A's air: the A|B line, seen from A, lower part
    // from 0 to 16 with its top row at B's floor (16).
    let (x, z) = col(129, 61);
    assert_eq!(m.column_sector(x, z), Some(1));
    assert_eq!(m.voxel([x - 1, 4, z]), Voxel::Air);
    // Centre height 9: row 16 − 9 = 7.
    assert_eq!(m.voxel([x, 4, z]), Voxel::Solid(7));
    assert_eq!(m.voxel([x, 0, z]), Voxel::Solid(15));
    // Floor surface of B under its air.
    assert!(matches!(m.voxel([x, 7, z]), Voxel::Solid(_)));
    assert_eq!(m.voxel([x, 8, z]), Voxel::Air);
}

#[test]
fn one_sided_walls_align_on_x_and_peg_to_the_ceiling() {
    let m = build(true, false);
    // The void column north of A's north wall (0,128)→(128,128), COLS.
    let (x, z) = col(21, 129);
    assert_eq!(m.column_sector(x, z), None);
    assert_eq!(m.column_sector(x, z + 1), Some(0));
    // u = distance from v1 = 21.
    assert_eq!(m.voxel([x, 10, z]), Voxel::Solid(21));
    // A's south wall (128,0)→(0,0), WALL: row = 128 − centre height.
    let (x, z) = col(21, -1);
    assert_eq!(m.voxel([x, 10, z]), Voxel::Solid(128 - 21));
}

#[test]
fn doors_open_to_the_lowest_neighbour_ceiling_minus_4() {
    let (x, z) = col(201, 61);
    let open = build(true, false);
    assert_eq!(open.column_air(x, z), Some((8, 62)));
    assert_eq!(open.voxel([x, 8, z]), Voxel::Air);
    assert_eq!(open.voxel([x, 61, z]), Voxel::Air);
    assert!(matches!(open.voxel([x, 62, z]), Voxel::Solid(_)));
    let closed = build(false, false);
    assert_eq!(closed.column_air(x, z), Some((8, 8)));
    assert!(matches!(closed.voxel([x, 8, z]), Voxel::Solid(_)));
    assert!(matches!(closed.voxel([x, 30, z]), Voxel::Solid(_)));
}

#[test]
fn masked_middle_is_a_slab_drawn_once_with_holes() {
    let m = build(true, false);
    // The A|B line runs north at x = 128 with its front (and grate) east.
    let x = 64;
    // Centre y 23: u = 23 & 15 = 7, opaque. Height 113: row 128 − 113 = 15.
    let z = (-23i32).div_euclid(2);
    assert_eq!(m.voxel([x, 56, z]), Voxel::Solid(115));
    // Below the grate's 32 rows: air.
    assert_eq!(m.voxel([x, 40, z]), Voxel::Air);
    // u = 21 & 15 = 5, a transparent column.
    let z = (-21i32).div_euclid(2);
    assert_eq!(m.voxel([x, 56, z]), Voxel::Air);
    // One voxel thick.
    assert_eq!(m.voxel([x + 1, 56, (-23i32).div_euclid(2)]), Voxel::Air);
    assert_eq!(m.voxel([x - 1, 56, (-23i32).div_euclid(2)]), Voxel::Air);
}

#[test]
fn sector_light_passes_through_colormap() {
    let m = build(true, true);
    // Light 160: flats use table 12, walls along y table 10 − 4 = 6; the
    // test colormap adds the table number.
    let (x, z) = col(65, 65);
    assert_eq!(m.voxel([x, -1, z]), Voxel::Solid(1 + 64 + 12));
    let (x, z) = col(129, 61);
    assert_eq!(m.voxel([x, 4, z]), Voxel::Solid(7 + 6));
}

#[test]
fn spawn_is_at_view_height_facing_north() {
    let m = build(true, false);
    let (pos, yaw) = m.spawn();
    assert_eq!(pos, [32.0, 20.5, -32.0]);
    assert_eq!(yaw, 0.0);
    assert_eq!(m.sky_colour(), [3, 3, 3]);
}

#[test]
fn uniform_in_never_claims_wrongly() {
    for m in [build(true, false), build(false, true)] {
        let (min, max) = m.bounds();
        let mut claims = 0;
        let mut rng = 12345u64;
        let mut next = |n: i32| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((rng >> 33) % n as u64) as i32
        };
        for _ in 0..3000 {
            let size = [1 + next(40), 1 + next(40), 1 + next(40)];
            let lo = [
                min[0] - 10 + next(max[0] - min[0] + 20),
                min[1] - 10 + next(max[1] - min[1] + 20),
                min[2] - 10 + next(max[2] - min[2] + 20),
            ];
            let hi = [lo[0] + size[0], lo[1] + size[1], lo[2] + size[2]];
            if let Some(v) = m.uniform_in(lo, hi) {
                claims += 1;
                for x in lo[0]..hi[0] {
                    for y in lo[1]..hi[1] {
                        for z in lo[2]..hi[2] {
                            assert_eq!(
                                m.voxel([x, y, z]),
                                v,
                                "{lo:?}..{hi:?} at {x},{y},{z}"
                            );
                        }
                    }
                }
            }
        }
        assert!(claims > 500, "{claims} claims");
    }
}
