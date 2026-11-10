//! Import: sectors (doors opened), the column → sector grid, the column →
//! linedef grid for wall texturing, tile summaries, spawn and sky.

use std::collections::HashMap;

use doom_wad::{
    Colormap, LineDef, MapData, Palette, Picture, TextureSet, WadError, ML_TWOSIDED,
};

use crate::interior::INTERIOR_DEPTH;
use crate::locate::Locator;
use crate::tiles::{Stats, Tiles};
use crate::{
    light, LineInfo, Options, SectorInfo, SideInfo, VoxelMap, MASKED, NO_LINE, NO_TEX,
    VOID,
};

/// DOOM's `VIEWHEIGHT` (p_local.h).
const VIEW_HEIGHT: i32 = 41;
/// Headroom above the highest ceiling, in map units.
const TOP_MARGIN: i32 = 32;
/// Void columns around the map's bounding box: room for wall colours to
/// reach [`INTERIOR_DEPTH`] into the void behind the outermost walls.
const PAD: i32 = INTERIOR_DEPTH + 2;

/// Linedefs whose back sector is a door opened by using the line: p_switch.c
/// `P_UseSpecialLine` → p_doors.c `EV_VerticalDoor`.
const MANUAL_DOORS: [u16; 10] = [1, 26, 27, 28, 31, 32, 33, 34, 117, 118];
/// Linedefs that open the doors of their tagged sectors: `EV_DoDoor` and
/// `EV_DoLockedDoor` with open, normal (raise) or blaze variants, from
/// p_spec.c `P_CrossSpecialLine` and `P_ShootSpecialLine` and p_switch.c
/// `P_UseSpecialLine`. Close-only specials (3, 42, 50, 75, 107, 110, 113,
/// 116) and close-then-reopen ones (16, 76) are left out.
const REMOTE_DOORS: [u16; 23] = [
    2, 4, 29, 46, 61, 63, 86, 90, 99, 103, 105, 106, 108, 109, 111, 112, 114, 115, 133,
    134, 135, 136, 137,
];
/// Sector special 14 raises its door after 5 minutes (p_spec.c
/// `P_SpawnSpecials`).
const SECTOR_DOOR_RAISE: u16 = 14;

/// The sky texture DOOM uses for a map (g_game.c `G_InitNew`).
pub fn sky_texture_name(map: &str) -> &'static str {
    let map = map.to_ascii_uppercase();
    if let Some(n) = map.strip_prefix("MAP").and_then(|n| n.parse::<u32>().ok()) {
        return match n {
            ..12 => "SKY1",
            12..21 => "SKY2",
            _ => "SKY3",
        };
    }
    match map.as_bytes().get(1) {
        Some(b'2') => "SKY2",
        Some(b'3') => "SKY3",
        Some(b'4') => "SKY4",
        _ => "SKY1",
    }
}

impl VoxelMap {
    /// Builds from already-decoded WAD data.
    pub fn from_map_data(
        map: &MapData,
        palette: Palette,
        colormap: &Colormap,
        textures: &TextureSet,
        options: &Options,
    ) -> Result<VoxelMap, WadError> {
        let s = options.units_per_voxel;
        if s <= 0 {
            return Err(WadError::Malformed(format!("{s} units per voxel")));
        }
        if map.linedefs.len() >= NO_LINE as usize || map.sectors.len() >= VOID as usize {
            return Err(WadError::Malformed(format!(
                "{}: {} linedefs and {} sectors is too many",
                map.name,
                map.linedefs.len(),
                map.sectors.len()
            )));
        }
        if map.vertexes.is_empty() || map.sectors.is_empty() {
            return Err(WadError::Malformed(format!("{}: no geometry", map.name)));
        }
        validate(map)?;

        let mut flat_ids = Names::default();
        let mut wall_ids = Names::default();
        let ceilings = if options.open_doors {
            opened_ceilings(map)
        } else {
            map.sectors.iter().map(|s| s.ceiling as i32).collect()
        };
        let table = |t: u8| if options.sector_light { t + 1 } else { 0 };
        let mut sectors: Vec<SectorInfo> = map
            .sectors
            .iter()
            .zip(&ceilings)
            .map(|(sec, &ceiling)| SectorInfo {
                floor: sec.floor as i32,
                ceiling,
                floor_v: to_voxel(sec.floor as i32, s),
                ceiling_v: to_voxel(ceiling, s),
                // r_sky.h SKYFLATNAME.
                sky: sec.ceiling_flat == "F_SKY1",
                floor_flat: flat_ids.id(&sec.floor_flat, &textures.flats),
                ceiling_flat: flat_ids.id(&sec.ceiling_flat, &textures.flats),
                flat_table: table(light::flat_table(sec.light)),
            })
            .collect();
        // A sector with its ceiling below its floor has no air.
        for sec in &mut sectors {
            sec.ceiling_v = sec.ceiling_v.max(sec.floor_v);
        }

        // Bounds: the vertexes' box plus a void margin; vertically the floor
        // voxel and the interior colours under the lowest floor, then one
        // more.
        let (mut x0, mut x1, mut y0, mut y1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for v in &map.vertexes {
            (x0, x1) = (x0.min(v.x as i32), x1.max(v.x as i32));
            (y0, y1) = (y0.min(v.y as i32), y1.max(v.y as i32));
        }
        let gx0 = x0.div_euclid(s) - PAD;
        let gx1 = x1.div_euclid(s) + PAD + 1;
        let gz0 = (-y1).div_euclid(s) - PAD;
        let gz1 = (-y0).div_euclid(s) + PAD + 1;
        let bottom =
            sectors.iter().map(|c| c.floor_v).min().unwrap() - 2 - INTERIOR_DEPTH;
        let top =
            sectors.iter().map(|c| c.ceiling_v).max().unwrap() + (TOP_MARGIN + s - 1) / s;
        let (width, depth) = (gx1 - gx0, gz1 - gz0);

        let locator = Locator::new(map);
        let mut columns = vec![VOID; (width * depth) as usize];
        par_rows(&mut columns, width as usize, |cz, row| {
            let z = gz0 + cz as i32;
            for (cx, c) in row.iter_mut().enumerate() {
                let x = gx0 + cx as i32;
                let (px, py) = doubled_centre(x, z, s);
                *c = locator.sector(px, py).unwrap_or(VOID);
            }
        });

        let walls_by_name = &textures.walls;
        let line_info: Vec<LineInfo> = map
            .linedefs
            .iter()
            .map(|l| {
                let sides = [l.front, l.back].map(|side| {
                    let sd = &map.sidedefs[side? as usize];
                    let mut tex = |name: &str| wall_ids.id(name, walls_by_name);
                    let contrast = contrast(map, l);
                    Some(SideInfo {
                        sector: sd.sector,
                        x_offset: sd.x_offset as f64,
                        y_offset: sd.y_offset as f64,
                        upper: tex(&sd.upper),
                        lower: tex(&sd.lower),
                        middle: tex(&sd.middle),
                        table: table(light::wall_table(
                            map.sectors[sd.sector as usize].light,
                            contrast,
                        )),
                    })
                });
                let a = map.vertexes[l.v1 as usize];
                let b = map.vertexes[l.v2 as usize];
                let (dx, dy) = ((b.x as f64 - a.x as f64), (b.y as f64 - a.y as f64));
                let len = dx.hypot(dy);
                let dir = if len > 0.0 {
                    [dx / len, dy / len]
                } else {
                    [1.0, 0.0]
                };
                let masked_side = match &sides {
                    [Some(f), Some(b)] => [f, b].iter().position(|s| s.middle != NO_TEX),
                    _ => None,
                };
                LineInfo {
                    a: [a.x as f64, a.y as f64],
                    dir,
                    len,
                    flags: l.flags,
                    sides,
                    masked_side,
                }
            })
            .collect();

        let mut vm = VoxelMap {
            palette,
            s,
            min: [gx0, bottom, gz0],
            max: [gx1, top, gz1],
            width,
            depth,
            columns,
            lines: Vec::new(),
            near: Vec::new(),
            sectors,
            line_info,
            flats: flat_ids.take(&textures.flats, |p| {
                let mut f = Box::new([0u8; 4096]);
                for (d, t) in f.iter_mut().zip(&p.pixels) {
                    *d = t.unwrap_or(0);
                }
                f
            }),
            walls: wall_ids.take(walls_by_name, |p| Picture {
                width: p.width,
                height: p.height,
                pixels: p.pixels.clone(),
            }),
            tables: colour_tables(colormap),
            tiles: Tiles::new(0, 0, |_, _| Stats::VOID),
            spawn: ([0.0; 3], 0.0),
            sky: [90, 110, 140],
        };
        vm.lines = vm.line_grid(map);
        vm.near = vm.near_grid();
        vm.tiles = vm.tile_stats();
        vm.spawn = vm.find_spawn(map, &locator);
        if let Some(sky) = textures.walls.get(sky_texture_name(&map.name)) {
            vm.sky = average_colour(sky, &vm.palette).unwrap_or(vm.sky);
        }
        Ok(vm)
    }

    /// Per column, the linedef its wall voxels are textured from.
    ///
    /// A column next to a column of another sector (or the void) takes the
    /// linedef crossing the segment between the two centres nearest its own
    /// centre, preferring one whose sides match the two sectors; with no
    /// crossing, the nearest linedef. Columns on the textured side of a
    /// two-sided line with a middle texture then take that line with
    /// [`MASKED`] set.
    fn line_grid(&self, map: &MapData) -> Vec<u16> {
        let s = self.s;
        let buckets = Buckets::new(self, map);
        let (w, d) = (self.width, self.depth);
        let mut lines = vec![NO_LINE; (w * d) as usize];
        let columns = &self.columns;
        let sector_at = |cx: i32, cz: i32| {
            if cx < 0 || cz < 0 || cx >= w || cz >= d {
                VOID
            } else {
                columns[(cz * w + cx) as usize]
            }
        };
        par_rows(&mut lines, w as usize, |cz, row| {
            let cz = cz as i32;
            for (cx, out) in row.iter_mut().enumerate() {
                let cx = cx as i32;
                let own = sector_at(cx, cz);
                let (x, z) = (self.min[0] + cx, self.min[2] + cz);
                let p = self.centre(x, 0, z);
                let mut best: Option<((u8, f64), u16)> = None;
                let mut edge = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let other = sector_at(cx + dx, cz + dz);
                    if other == own {
                        continue;
                    }
                    edge = true;
                    let q = self.centre(x + dx, 0, z + dz);
                    for &l in buckets.at(p.0, p.1) {
                        let li = &self.line_info[l as usize];
                        let Some(u) = crossing(li, (p.0, p.1), (q.0, q.1)) else {
                            continue;
                        };
                        // Which side p is on: right (front) when cross < 0.
                        let p_front = li.cross(p.0, p.1) < 0.0;
                        let sector =
                            |i: usize| li.sides[i].as_ref().map_or(VOID, |s| s.sector);
                        let (ps, qs) = if p_front {
                            (sector(0), sector(1))
                        } else {
                            (sector(1), sector(0))
                        };
                        let rank = if ps == own && qs == other { 0 } else { 1 };
                        let key = (rank, u);
                        if best.is_none_or(|(b, _)| key < b) {
                            best = Some((key, l));
                        }
                    }
                }
                if !edge {
                    continue;
                }
                if best.is_none() {
                    for &l in buckets.at(p.0, p.1) {
                        let dist = distance(&self.line_info[l as usize], p.0, p.1);
                        if dist <= 2.0 * s as f64
                            && best.is_none_or(|(b, _)| (2, dist) < b)
                        {
                            best = Some(((2, dist), l));
                        }
                    }
                }
                if let Some((_, l)) = best {
                    *out = l;
                }
            }
        });

        // Masked middle slabs: the columns on the textured side whose
        // squares the line passes through.
        for (l, li) in self.line_info.iter().enumerate() {
            let Some(d) = li.masked_side else { continue };
            let n = [li.dir[1], -li.dir[0]]; // right normal
            let thick = s as f64 * (n[0].abs() + n[1].abs());
            let b = [li.a[0] + li.dir[0] * li.len, li.a[1] + li.dir[1] * li.len];
            let pad = s as f64 * 2.0;
            let x_lo = ((li.a[0].min(b[0]) - pad) / s as f64).floor() as i32;
            let x_hi = ((li.a[0].max(b[0]) + pad) / s as f64).ceil() as i32;
            let z_lo = ((-li.a[1].max(b[1]) - pad) / s as f64).floor() as i32;
            let z_hi = ((-li.a[1].min(b[1]) + pad) / s as f64).ceil() as i32;
            for z in z_lo..=z_hi {
                for x in x_lo..=x_hi {
                    let Some(col) = self.column(x, z) else {
                        continue;
                    };
                    let (px, py, _) = self.centre(x, 0, z);
                    let right = (px - li.a[0]) * n[0] + (py - li.a[1]) * n[1];
                    let dist = if d == 0 { right } else { -right };
                    let along = (px - li.a[0]) * li.dir[0] + (py - li.a[1]) * li.dir[1];
                    if (0.0..thick).contains(&dist) && (0.0..=li.len).contains(&along) {
                        lines[col] = l as u16 | MASKED;
                    }
                }
            }
        }
        lines
    }

    fn tile_stats(&self) -> Tiles {
        let top = self.max[1];
        Tiles::new(self.width, self.depth, |cx, cz| {
            let col = (cz * self.width + cx) as usize;
            let s = self.columns[col];
            if s == VOID {
                return Stats::VOID;
            }
            let sec = &self.sectors[s as usize];
            Stats {
                lo: sec.floor_v - 1,
                hi: if sec.sky { top } else { sec.ceiling_v + 1 },
                air_lo: sec.floor_v,
                air_hi: if sec.sky { i32::MAX } else { sec.ceiling_v },
                blocked: self.lines[col] & MASKED != 0,
            }
        })
    }

    /// Player 1's start (thing type 1) at view height over its floor; yaw
    /// turns DOOM's angle (0 east, 90 north, counter-clockwise) into the
    /// app's (0 facing −z, which is north, counter-clockwise around +y).
    fn find_spawn(&self, map: &MapData, locator: &Locator) -> ([f32; 3], f32) {
        let s = self.s as f32;
        let Some(t) = map.things.iter().find(|t| t.kind == 1) else {
            let x = (self.min[0] + self.max[0]) as f32 / 2.0;
            let z = (self.min[2] + self.max[2]) as f32 / 2.0;
            return ([x, self.max[1] as f32, z], 0.0);
        };
        let (x2, y2) = (2 * t.x as i64, 2 * t.y as i64);
        let sector = locator
            .sector(x2, y2)
            .or_else(|| locator.leaf_sector(x2, y2));
        let floor = sector.map_or(0, |i| self.sectors[i as usize].floor);
        let pos = [
            t.x as f32 / s,
            (floor + VIEW_HEIGHT) as f32 / s,
            -(t.y as f32) / s,
        ];
        let yaw = (t.angle as f32 - 90.0).to_radians();
        (pos, yaw)
    }
}

/// The first voxel whose centre is at or above height `h`:
/// `ceil(h/s − 1/2)`.
fn to_voxel(h: i32, s: i32) -> i32 {
    (2 * h - s).div_euclid(2 * s) + ((2 * h - s).rem_euclid(2 * s) != 0) as i32
}

/// A voxel column's centre in doubled map units.
fn doubled_centre(x: i32, z: i32, s: i32) -> (i64, i64) {
    let s = s as i64;
    ((2 * x as i64 + 1) * s, -(2 * z as i64 + 1) * s)
}

/// r_segs.c fake contrast: −1 for lines along x, +1 along y.
fn contrast(map: &MapData, l: &LineDef) -> i32 {
    let (a, b) = (map.vertexes[l.v1 as usize], map.vertexes[l.v2 as usize]);
    if a.y == b.y {
        -1
    } else if a.x == b.x {
        1
    } else {
        0
    }
}

fn validate(map: &MapData) -> Result<(), WadError> {
    let bad = |what: String| Err(WadError::Malformed(format!("{}: {what}", map.name)));
    for (i, l) in map.linedefs.iter().enumerate() {
        if l.v1 as usize >= map.vertexes.len() || l.v2 as usize >= map.vertexes.len() {
            return bad(format!("linedef {i} vertex out of range"));
        }
        for s in [l.front, l.back].into_iter().flatten() {
            if s as usize >= map.sidedefs.len() {
                return bad(format!("linedef {i} sidedef out of range"));
            }
        }
    }
    for (i, s) in map.sidedefs.iter().enumerate() {
        if s.sector as usize >= map.sectors.len() {
            return bad(format!("sidedef {i} sector out of range"));
        }
    }
    Ok(())
}

/// Sector ceilings with every door open: p_doors.c opens a door to
/// p_spec.c `P_FindLowestCeilingSurrounding` − 4. A door already higher
/// stays as stored.
fn opened_ceilings(map: &MapData) -> Vec<i32> {
    let n = map.sectors.len();
    let sector_of =
        |side: Option<u16>| side.map(|s| map.sidedefs[s as usize].sector as usize);
    let mut door = vec![false; n];
    let mut by_tag: HashMap<u16, Vec<usize>> = HashMap::new();
    for (i, s) in map.sectors.iter().enumerate() {
        by_tag.entry(s.tag).or_default().push(i);
        door[i] |= s.special == SECTOR_DOOR_RAISE;
    }
    for l in &map.linedefs {
        if MANUAL_DOORS.contains(&l.special) {
            // p_doors.c `EV_VerticalDoor`: the sector behind the line.
            if let Some(b) = sector_of(l.back) {
                door[b] = true;
            }
        } else if REMOTE_DOORS.contains(&l.special) && l.tag != 0 {
            // Tag 0 would match every untagged sector, a map error.
            for &i in by_tag.get(&l.tag).into_iter().flatten() {
                door[i] = true;
            }
        }
    }
    // p_spec.c `P_FindLowestCeilingSurrounding`, through `getNextSector`.
    let mut lowest = vec![i32::MAX; n];
    for l in &map.linedefs {
        if l.flags & ML_TWOSIDED == 0 {
            continue;
        }
        let (Some(f), Some(b)) = (sector_of(l.front), sector_of(l.back)) else {
            continue;
        };
        lowest[f] = lowest[f].min(map.sectors[b].ceiling as i32);
        lowest[b] = lowest[b].min(map.sectors[f].ceiling as i32);
    }
    map.sectors
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let stored = s.ceiling as i32;
            if door[i] && lowest[i] != i32::MAX && lowest[i] - 4 > stored {
                lowest[i] - 4
            } else {
                stored
            }
        })
        .collect()
}

/// Ids for the flats or wall textures in use, by name.
#[derive(Default)]
struct Names {
    ids: HashMap<String, u16>,
    order: Vec<String>,
}

impl Names {
    fn id(&mut self, name: &str, known: &HashMap<String, Picture>) -> u16 {
        if name == "-" || name.is_empty() || !known.contains_key(name) {
            return NO_TEX;
        }
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = self.order.len() as u16;
        self.ids.insert(name.to_string(), id);
        self.order.push(name.to_string());
        id
    }

    fn take<T>(
        &self,
        known: &HashMap<String, Picture>,
        f: impl Fn(&Picture) -> T,
    ) -> Vec<T> {
        self.order.iter().map(|n| f(&known[n])).collect()
    }
}

/// Table 0 is the identity, then COLORMAP's 32 light tables.
fn colour_tables(colormap: &Colormap) -> Vec<[u8; 256]> {
    let mut identity = [0u8; 256];
    for (i, v) in identity.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut tables = vec![identity];
    for k in 0..32 {
        tables.push(colormap.tables.get(k).copied().unwrap_or(identity));
    }
    tables
}

fn average_colour(p: &Picture, palette: &Palette) -> Option<[u8; 3]> {
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for &t in p.pixels.iter().flatten() {
        for (s, c) in sum.iter_mut().zip(palette[t as usize]) {
            *s += c as u64;
        }
        n += 1;
    }
    (n > 0).then(|| sum.map(|s| (s / n) as u8))
}

/// Where segment `p → q` crosses linedef `li`: the fraction along `p → q`,
/// or `None`. The linedef counts half a unit past each end, so a crossing
/// at a shared vertex isn't missed.
fn crossing(li: &LineInfo, p: (f64, f64), q: (f64, f64)) -> Option<f64> {
    let (cp, cq) = (li.cross(p.0, p.1), li.cross(q.0, q.1));
    if (cp > 0.0 && cq > 0.0) || (cp < 0.0 && cq < 0.0) || cp == cq {
        return None;
    }
    let u = cp / (cp - cq);
    let x = p.0 + (q.0 - p.0) * u;
    let y = p.1 + (q.1 - p.1) * u;
    let along = (x - li.a[0]) * li.dir[0] + (y - li.a[1]) * li.dir[1];
    (-0.5..=li.len + 0.5).contains(&along).then_some(u)
}

/// Distance from map point `(x, y)` to linedef `li`.
fn distance(li: &LineInfo, x: f64, y: f64) -> f64 {
    let along =
        ((x - li.a[0]) * li.dir[0] + (y - li.a[1]) * li.dir[1]).clamp(0.0, li.len);
    let (cx, cy) = (li.a[0] + li.dir[0] * along, li.a[1] + li.dir[1] * along);
    (x - cx).hypot(y - cy)
}

/// Linedefs by 64-unit cell, each listed in every cell within two voxels of it.
struct Buckets {
    x0: f64,
    y0: f64,
    w: i32,
    h: i32,
    cells: Vec<Vec<u16>>,
}

const CELL: f64 = 64.0;

impl Buckets {
    fn new(vm: &VoxelMap, map: &MapData) -> Buckets {
        let s = vm.s as f64;
        let x0 = vm.min[0] as f64 * s;
        let y0 = -(vm.max[2] as f64) * s;
        let w = ((vm.width as f64 * s) / CELL).ceil() as i32 + 1;
        let h = ((vm.depth as f64 * s) / CELL).ceil() as i32 + 1;
        let mut cells = vec![Vec::new(); (w * h) as usize];
        let pad = 2.0 * s + 1.0;
        for (i, l) in map.linedefs.iter().enumerate() {
            let (a, b) = (map.vertexes[l.v1 as usize], map.vertexes[l.v2 as usize]);
            let cell = |v: f64, o: f64, n: i32| {
                (((v - o) / CELL).floor() as i32).clamp(0, n - 1)
            };
            let (lx, hx) = ((a.x.min(b.x) as f64) - pad, (a.x.max(b.x) as f64) + pad);
            let (ly, hy) = ((a.y.min(b.y) as f64) - pad, (a.y.max(b.y) as f64) + pad);
            for cy in cell(ly, y0, h)..=cell(hy, y0, h) {
                for cx in cell(lx, x0, w)..=cell(hx, x0, w) {
                    cells[(cy * w + cx) as usize].push(i as u16);
                }
            }
        }
        Buckets {
            x0,
            y0,
            w,
            h,
            cells,
        }
    }

    fn at(&self, x: f64, y: f64) -> &[u16] {
        let cx = ((x - self.x0) / CELL).floor() as i32;
        let cy = ((y - self.y0) / CELL).floor() as i32;
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return &[];
        }
        &self.cells[(cy * self.w + cx) as usize]
    }
}

/// Runs `f(row index, row)` over the rows of a row-major grid on all cores.
pub(crate) fn par_rows<T: Send>(
    grid: &mut [T],
    width: usize,
    f: impl Fn(usize, &mut [T]) + Sync,
) {
    if width == 0 {
        return;
    }
    let rows = grid.len() / width;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let per = rows.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (i, block) in grid.chunks_mut(per * width).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (j, row) in block.chunks_mut(width).enumerate() {
                    f(i * per + j, row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_round_to_the_voxel_whose_centre_is_inside() {
        assert_eq!(to_voxel(0, 2), 0);
        assert_eq!(to_voxel(1, 2), 0);
        assert_eq!(to_voxel(2, 2), 1);
        assert_eq!(to_voxel(3, 2), 1);
        assert_eq!(to_voxel(-1, 2), -1);
        assert_eq!(to_voxel(-2, 2), -1);
        assert_eq!(to_voxel(-3, 2), -2);
        assert_eq!(to_voxel(5, 4), 1);
        assert_eq!(to_voxel(6, 4), 1);
        assert_eq!(to_voxel(7, 4), 2);
    }

    #[test]
    fn sky_textures_follow_g_game() {
        assert_eq!(sky_texture_name("E1M1"), "SKY1");
        assert_eq!(sky_texture_name("e3m4"), "SKY3");
        assert_eq!(sky_texture_name("MAP11"), "SKY1");
        assert_eq!(sky_texture_name("MAP12"), "SKY2");
        assert_eq!(sky_texture_name("MAP21"), "SKY3");
    }
}
