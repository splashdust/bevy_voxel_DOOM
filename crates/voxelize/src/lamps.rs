//! Light sources, for a voxel world that propagates light.
//!
//! Three kinds of voxel glow, all worked out at build time so `voxel` only
//! looks them up:
//!
//! - **Light flats**, by name ([`LIGHT_FLATS`]): the ceiling panels and
//!   floor lights of DOOM and DOOM II, whose names Freedoom keeps.
//! - **Light wall textures**, by name: every name holding `LITE` (the
//!   fluorescent strips, `LITEBLU*`, `LITERED`, `TEKLITE`, ...) and
//!   `BRICKLIT`.
//! - **Light-giving things** ([`LAMP_THINGS`]): torches, candles, lamps and
//!   the burning barrel, each a 2×2-voxel pillar about its sprite's height,
//!   a plain stand under an emissive flame.
//!
//! Only a texture's bright texels glow: those whose brightest channel is at
//! least [`GLOW_SHARE`] of the texture's brightest texel's, so a strip light
//! glows and its frame doesn't. A glowing texel keeps its unlit palette
//! colour, without sector light. Only surface voxels glow: the texture
//! carried on into the solid is plain.

use std::collections::BTreeSet;

use doom_wad::{MapData, Picture};

use crate::{light, Voxel, VoxelMap};

/// How bright the light surfaces of a map are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LampLevel {
    /// Every light surface gives off light at level 15.
    Full,
    /// A light surface gives off light at its sector's light level divided
    /// by 16, so a fixture in a dim room lights less far. Pillars are always
    /// 15: DOOM draws their sprites full-bright.
    Sector,
}

/// Flats that are light fixtures.
pub const LIGHT_FLATS: [&str; 13] = [
    "CEIL1_2", "CEIL1_3", "CEIL3_4", "CEIL3_6", "FLAT2", "FLAT17", "FLAT22", "GRNLITE1",
    "TLITE6_1", "TLITE6_4", "TLITE6_5", "TLITE6_6", "FLOOR1_7",
];

/// Whether wall texture `name` holds a light fixture.
pub fn is_light_wall(name: &str) -> bool {
    name.contains("LITE") || name == "BRICKLIT"
}

/// A texel glows when its brightest channel is at least this share of the
/// brightest texel's in its texture, in hundredths.
pub const GLOW_SHARE: u32 = 85;

/// A light-giving thing, drawn as a pillar.
#[derive(Clone, Copy, Debug)]
pub struct LampThing {
    /// The thing type.
    pub kind: u16,
    /// About the sprite's height, in map units.
    pub height: i32,
    /// The glowing top's height, in map units.
    pub flame: i32,
    /// The flame's colour, matched to the nearest palette entry.
    pub flame_rgb: [u8; 3],
    /// The stand's colour, shown in the sector's light.
    pub stand_rgb: [u8; 3],
}

const fn lamp(
    kind: u16,
    height: i32,
    flame: i32,
    flame_rgb: [u8; 3],
    stand_rgb: [u8; 3],
) -> LampThing {
    LampThing {
        kind,
        height,
        flame,
        flame_rgb,
        stand_rgb,
    }
}

const STAND: [u8; 3] = [64, 56, 48];
const METAL: [u8; 3] = [104, 104, 112];
const BLUE: [u8; 3] = [40, 40, 255];
const GREEN: [u8; 3] = [100, 240, 90];
const RED: [u8; 3] = [240, 60, 20];
const WHITE: [u8; 3] = [224, 224, 248];

/// Light-giving things, with heights from their sprites' top offsets.
pub const LAMP_THINGS: [LampThing; 12] = [
    lamp(34, 15, 6, [255, 210, 120], STAND),
    lamp(35, 57, 12, [255, 210, 90], STAND),
    lamp(44, 92, 36, BLUE, STAND),
    lamp(45, 92, 36, GREEN, STAND),
    lamp(46, 92, 36, RED, STAND),
    lamp(55, 72, 30, BLUE, STAND),
    lamp(56, 72, 30, GREEN, STAND),
    lamp(57, 72, 30, RED, STAND),
    lamp(85, 77, 14, WHITE, METAL),
    lamp(86, 57, 12, WHITE, METAL),
    lamp(2028, 43, 14, [240, 240, 180], METAL),
    lamp(70, 49, 20, [240, 170, 50], [70, 90, 60]),
];

/// Things flagged for multiplayer only (p_mobj.c `P_SpawnMapThing`).
const MTF_NOTSINGLE: u16 = 16;

/// A light-giving thing as a 2×2-voxel pillar.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pillar {
    /// The lowest x and z of its columns.
    x: i32,
    z: i32,
    /// Its voxels run from `bottom` up to `top`, exclusive, glowing from
    /// `flame` up.
    bottom: i32,
    flame: i32,
    top: i32,
    flame_index: u8,
    stand_index: u8,
    /// The stand's sector glow level, 0 for none.
    stand_glow: u8,
}

const NO_PILLAR: u32 = u32::MAX;

/// A map's light sources. Empty, with nothing glowing, without lamps.
#[derive(Default)]
pub(crate) struct Lamps {
    /// Per flat id, the brightness from which its texels glow, if it is a
    /// light.
    flat_glow: Vec<Option<u8>>,
    /// Per wall texture id, the same.
    wall_glow: Vec<Option<u8>>,
    /// Each palette entry's brightest channel.
    brightness: Vec<u8>,
    pub pillars: Vec<Pillar>,
    /// Per column, the index of the pillar standing in it, or [`NO_PILLAR`].
    /// Empty when there are no pillars.
    columns: Vec<u32>,
    /// Every emissive material `voxel` can return, sorted.
    pub materials: Vec<(u8, u8)>,
}

impl Lamps {
    /// Whether texel `texel` of flat `flat` glows.
    pub fn flat_glows(&self, flat: u16, texel: u8) -> bool {
        glows(&self.flat_glow, &self.brightness, flat, texel)
    }

    /// Whether texel `texel` of wall texture `tex` glows.
    pub fn wall_glows(&self, tex: u16, texel: u8) -> bool {
        glows(&self.wall_glow, &self.brightness, tex, texel)
    }

    /// The pillar voxel at height `y` in column `col`, if a pillar stands
    /// there. The caller has checked the voxel is air.
    pub fn pillar(&self, col: usize, y: i32) -> Option<Voxel> {
        let &i = self.columns.get(col)?;
        if i == NO_PILLAR {
            return None;
        }
        let p = &self.pillars[i as usize];
        if y < p.bottom || y >= p.top {
            None
        } else if y >= p.flame {
            Some(Voxel::Emissive {
                index: p.flame_index,
                level: 15,
            })
        } else if p.stand_glow > 0 {
            Some(Voxel::Emissive {
                index: p.stand_index,
                level: p.stand_glow,
            })
        } else {
            Some(Voxel::Solid(p.stand_index))
        }
    }

    /// Whether any pillar has a voxel in the box `min..max`, max exclusive.
    pub fn pillar_in(&self, min: [i32; 3], max: [i32; 3]) -> bool {
        self.pillars.iter().any(|p| {
            p.x < max[0]
                && p.x + 2 > min[0]
                && p.z < max[2]
                && p.z + 2 > min[2]
                && p.bottom < max[1]
                && p.top > min[1]
        })
    }

    /// The light sources of `vm`, whose flat and wall ids name `flat_names`
    /// and `wall_names`, and whose sectors and sides already hold their
    /// lamp and glow levels. `level` is `None` without lamps; with
    /// `sector_glow`, the list also holds every glowing surface's material.
    pub fn build(
        vm: &VoxelMap,
        map: &MapData,
        flat_names: &[String],
        wall_names: &[String],
        level: Option<LampLevel>,
        sector_glow: bool,
    ) -> Lamps {
        if level.is_none() {
            let mut lamps = Lamps::default();
            if sector_glow {
                lamps.materials = lamps.list_materials(vm);
            }
            return lamps;
        }
        let brightness: Vec<u8> = vm
            .palette
            .iter()
            .map(|c| c[0].max(c[1]).max(c[2]))
            .collect();
        let threshold = |p: &Picture| {
            let brightest = p
                .pixels
                .iter()
                .flatten()
                .map(|&t| brightness[t as usize])
                .max()?;
            Some((brightest as u32 * GLOW_SHARE).div_ceil(100) as u8)
        };
        let flat_glow: Vec<Option<u8>> = flat_names
            .iter()
            .enumerate()
            .map(|(id, name)| {
                if !LIGHT_FLATS.contains(&name.as_str()) {
                    return None;
                }
                let flat = &vm.flats[id];
                let brightest = flat.iter().map(|&t| brightness[t as usize]).max()?;
                Some((brightest as u32 * GLOW_SHARE).div_ceil(100) as u8)
            })
            .collect();
        let wall_glow: Vec<Option<u8>> = wall_names
            .iter()
            .zip(&vm.walls)
            .map(|(name, p)| {
                if is_light_wall(name) {
                    threshold(p)
                } else {
                    None
                }
            })
            .collect();

        let mut lamps = Lamps {
            flat_glow,
            wall_glow,
            brightness,
            ..Lamps::default()
        };
        lamps.place_pillars(vm, map);
        lamps.materials = lamps.list_materials(vm);
        lamps
    }

    fn place_pillars(&mut self, vm: &VoxelMap, map: &MapData) {
        let s = vm.s as f64;
        for t in &map.things {
            if t.flags & MTF_NOTSINGLE != 0 {
                continue;
            }
            let Some(kind) = LAMP_THINGS.iter().find(|k| k.kind == t.kind) else {
                continue;
            };
            let (fx, fz) = (t.x as f64 / s, -(t.y as f64) / s);
            let Some(sector) = vm.column_sector(fx.floor() as i32, fz.floor() as i32)
            else {
                continue;
            };
            let sec = &vm.sectors[sector as usize];
            let voxels = |units: i32| ((units as f64 / s).round() as i32).max(1);
            let bottom = sec.floor_v;
            let top = bottom + voxels(kind.height).max(2);
            let flame = (top - voxels(kind.flame)).max(bottom);
            let stand = nearest(&vm.palette, kind.stand_rgb);
            self.pillars.push(Pillar {
                x: fx.round() as i32 - 1,
                z: fz.round() as i32 - 1,
                bottom,
                flame,
                top,
                flame_index: nearest(&vm.palette, kind.flame_rgb),
                stand_index: vm.tables[sec.flat_table as usize][stand as usize],
                stand_glow: sec.flat_glow,
            });
        }
        if self.pillars.is_empty() {
            return;
        }
        self.columns = vec![NO_PILLAR; (vm.width * vm.depth) as usize];
        for (i, p) in self.pillars.iter().enumerate() {
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                if let Some(col) = vm.column(p.x + dx, p.z + dz) {
                    if self.columns[col] == NO_PILLAR {
                        self.columns[col] = i as u32;
                    }
                }
            }
        }
    }

    /// Every `(palette index, level)` a glowing flat, wall or pillar can
    /// show: a superset of what `voxel` returns.
    fn list_materials(&self, vm: &VoxelMap) -> Vec<(u8, u8)> {
        let mut found = BTreeSet::new();
        let bright =
            |glow: Option<u8>, texels: &mut dyn Iterator<Item = u8>| -> Vec<u8> {
                let Some(t) = glow else { return Vec::new() };
                let mut seen = [false; 256];
                for texel in texels {
                    seen[texel as usize] |= self.brightness[texel as usize] >= t;
                }
                (0..=255u8).filter(|&i| seen[i as usize]).collect()
            };
        let flat_bright: Vec<Vec<u8>> = (0..vm.flats.len())
            .map(|id| {
                bright(
                    self.flat_glow.get(id).copied().flatten(),
                    &mut vm.flats[id].iter().copied(),
                )
            })
            .collect();
        let wall_bright: Vec<Vec<u8>> = (0..vm.walls.len())
            .map(|id| {
                bright(
                    self.wall_glow.get(id).copied().flatten(),
                    &mut vm.walls[id].pixels.iter().flatten().copied(),
                )
            })
            .collect();
        let mut add = |texels: Option<&Vec<u8>>, level: u8| {
            if level > 0 {
                for &texel in texels.into_iter().flatten() {
                    found.insert((texel, level));
                }
            }
        };
        for sec in &vm.sectors {
            add(flat_bright.get(sec.floor_flat as usize), sec.lamp_level);
            if !sec.sky {
                add(flat_bright.get(sec.ceiling_flat as usize), sec.lamp_level);
            }
        }
        for side in vm.line_info.iter().flat_map(|l| l.sides.iter().flatten()) {
            for tex in [side.upper, side.lower, side.middle] {
                add(wall_bright.get(tex as usize), side.lamp_level);
            }
        }
        for p in &self.pillars {
            found.insert((p.flame_index, 15));
            if p.stand_glow > 0 {
                found.insert((p.stand_index, p.stand_glow));
            }
        }
        // Sector glow: every texel of every surface, in the light table it
        // is drawn in. Lamp texels are listed at both levels, which costs a
        // little list length and nothing else.
        let mut add_glow = |texels: &mut dyn Iterator<Item = u8>, table: u8, glow: u8| {
            if glow == 0 {
                return;
            }
            let mut seen = [false; 256];
            for texel in texels {
                seen[vm.tables[table as usize][texel as usize] as usize] = true;
            }
            for index in (0..=255u8).filter(|&i| seen[i as usize]) {
                found.insert((index, glow));
            }
        };
        for sec in &vm.sectors {
            let mut flats = vec![sec.floor_flat];
            if !sec.sky {
                flats.push(sec.ceiling_flat);
            }
            for flat in flats {
                // A missing flat shows the fallback grey.
                match vm.flats.get(flat as usize) {
                    Some(flat) => {
                        add_glow(&mut flat.iter().copied(), sec.flat_table, sec.flat_glow)
                    }
                    None => add_glow(
                        &mut std::iter::once(crate::colour::FALLBACK),
                        sec.flat_table,
                        sec.flat_glow,
                    ),
                }
            }
        }
        for side in vm.line_info.iter().flat_map(|l| l.sides.iter().flatten()) {
            for tex in [side.upper, side.lower, side.middle] {
                if let Some(wall) = vm.walls.get(tex as usize) {
                    add_glow(
                        &mut wall.pixels.iter().flatten().copied(),
                        side.table,
                        side.glow,
                    );
                }
            }
        }
        // A wall with no texture shows the floor flat of the sector it faces,
        // listed above with that sector's level.
        found.into_iter().collect()
    }
}

fn glows(glow: &[Option<u8>], brightness: &[u8], tex: u16, texel: u8) -> bool {
    match glow.get(tex as usize) {
        Some(Some(t)) => brightness[texel as usize] >= *t,
        _ => false,
    }
}

/// The emission level of a sector's light surfaces.
pub(crate) fn sector_level(level: Option<LampLevel>, lightlevel: u16) -> u8 {
    match level {
        None => 0,
        Some(LampLevel::Full) => 15,
        Some(LampLevel::Sector) => light::level(lightlevel) as u8,
    }
}

/// The palette entry nearest `rgb`.
pub(crate) fn nearest(palette: &[[u8; 3]; 256], rgb: [u8; 3]) -> u8 {
    let distance = |c: &[u8; 3]| {
        (0..3)
            .map(|i| (c[i] as i32 - rgb[i] as i32).pow(2))
            .sum::<i32>()
    };
    (0..=255u8)
        .min_by_key(|&i| distance(&palette[i as usize]))
        .unwrap_or(0)
}
