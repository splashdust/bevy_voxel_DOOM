//! Turns a vanilla DOOM map into voxels, one palette colour per voxel.
//!
//! Coordinates: voxel `[x, y, z]` with y up. With `s` map units per voxel,
//! voxel `[x, y, z]` covers DOOM map x in `[x*s, (x+1)*s)`, height in
//! `[y*s, (y+1)*s)` and DOOM map y in `(-(z+1)*s, -z*s]`. So DOOM's east is
//! +x, DOOM's north is −z (Bevy's forward), and the result is right-handed.
//!
//! DOOM is 2.5D: each map point lies in one sector, with one floor and one
//! ceiling height. At import every voxel column gets its sector (or the
//! void) from the map's BSP, see `locate`. A voxel is then tested at its
//! centre: solid below the floor, air up to the ceiling (or to the map top
//! under a sky ceiling), solid above. Void columns are solid up to the top.
//!
//! Colours: the solid voxel under a sector's air is its floor flat, the one
//! over it its ceiling flat; other solid voxels next to air are walls,
//! textured from the linedef between them. Hidden solid voxels carry the
//! nearest surface's flat or wall texture on into the solid, darker with
//! depth, and are [`INTERIOR`] from [`INTERIOR_DEPTH`] voxels in. See
//! `build` for the import, `colour` for the texturing rules and `interior`
//! for the solid's inside.
//!
//! With [`Options::lamps`] set, the bright texels of light flats and light
//! wall textures are [`Voxel::Emissive`], and light-giving things stand as
//! small pillars with an emissive top: see `lamps`. With
//! [`Options::sector_glow`], every surface voxel also glows at a level that
//! stands for its sector's light: see `light::GlowLevels`.

use doom_wad::{Palette, Picture, Wad, WadError};

mod build;
mod colour;
mod interior;
mod lamps;
mod light;
mod locate;
mod tiles;

pub use build::sky_texture_name;
pub use interior::INTERIOR_DEPTH;
pub use lamps::{LampLevel, LAMP_THINGS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voxel {
    Air,
    /// A palette index into [`VoxelMap::palette`].
    Solid(u8),
    /// A solid voxel that gives off light at `level`, from 1 to 15, shown
    /// in palette colour `index` without sector light. Only with
    /// [`Options::lamps`].
    Emissive {
        index: u8,
        level: u8,
    },
}

impl Voxel {
    pub fn is_solid(self) -> bool {
        !matches!(self, Voxel::Air)
    }
}

/// The colour of solid voxels deep inside the solid: palette index 0, black.
pub const INTERIOR: u8 = 0;

#[derive(Clone, Debug)]
pub struct Options {
    /// Map units per voxel.
    pub units_per_voxel: i32,
    /// Pass colours through COLORMAP at each sector's light level, so DOOM's
    /// sector lighting is baked in. Off gives full-bright colours.
    pub sector_light: bool,
    /// How far baked sector light is stretched away from a middle light:
    /// 1 is DOOM's own, above 1 makes bright sectors brighter and dark ones
    /// darker. Darker comes from darker COLORMAP tables, brighter from
    /// [`VoxelMap::colour_gain`], which the renderer applies. See
    /// `light::stretch`.
    pub contrast: f32,
    /// Bake door sectors open, as if every door had been used.
    pub open_doors: bool,
    /// Light sources: the bright texels of light flats and wall textures
    /// glow, and light-giving things stand as pillars. `None` leaves both
    /// out.
    pub lamps: Option<LampLevel>,
    /// Every solid voxel beside air glows at a level standing for the light
    /// DOOM would draw it in: its sector's light through the floor, ceiling
    /// or wall table, measured against COLORMAP (see `light::GlowLevels`).
    /// Lamps keep level 15, and voxels inside the solid don't glow.
    pub sector_glow: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            units_per_voxel: 2,
            sector_light: true,
            contrast: 1.0,
            open_doors: true,
            lamps: None,
            sector_glow: false,
        }
    }
}

/// A column's sector id when it lies in no sector.
const VOID: u16 = u16::MAX;
/// A column's line id when no linedef is near it.
const NO_LINE: u16 = 0x7fff;
/// Set on a column's line id when the column holds that line's masked
/// middle texture.
const MASKED: u16 = 0x8000;
/// A texture or flat id for "none".
const NO_TEX: u16 = u16::MAX;

/// A sector as the voxels see it.
#[derive(Clone, Debug)]
struct SectorInfo {
    /// Heights in map units, doors opened when asked.
    floor: i32,
    ceiling: i32,
    /// The air span in voxels: `floor_v..ceiling_v`, or `floor_v..top` under sky.
    floor_v: i32,
    ceiling_v: i32,
    sky: bool,
    floor_flat: u16,
    ceiling_flat: u16,
    /// The light table for its flats.
    flat_table: u8,
    /// The emission level of its glowing flats and walls, 0 for none.
    lamp_level: u8,
    /// With sector glow, the level its floor and ceiling glow at; else 0.
    flat_glow: u8,
}

#[derive(Clone, Debug)]
struct SideInfo {
    sector: u16,
    x_offset: f64,
    y_offset: f64,
    upper: u16,
    lower: u16,
    middle: u16,
    /// The light table for its walls.
    table: u8,
    /// The emission level of its glowing texels, 0 for none.
    lamp_level: u8,
    /// With sector glow, the level its walls glow at; else 0.
    glow: u8,
}

#[derive(Clone, Debug)]
struct LineInfo {
    /// v1 in map units.
    a: [f64; 2],
    /// Unit direction v1 → v2.
    dir: [f64; 2],
    len: f64,
    flags: u16,
    /// Front (right) then back sidedef.
    sides: [Option<SideInfo>; 2],
    /// The side whose masked middle texture this two-sided line draws.
    masked_side: Option<usize>,
}

pub struct VoxelMap {
    palette: Palette,
    s: i32,
    min: [i32; 3],
    max: [i32; 3],
    /// Columns across (x) and down (z); column (x, z) is at
    /// `(z - min[2]) * width + (x - min[0])`.
    width: i32,
    depth: i32,
    /// Per column: sector id or [`VOID`].
    columns: Vec<u16>,
    /// Per column: the linedef its walls are textured from, or [`NO_LINE`],
    /// with [`MASKED`] set where the column holds a masked slab.
    lines: Vec<u16>,
    /// Per column: the nearest column of another sector with air.
    near: Vec<interior::Near>,
    sectors: Vec<SectorInfo>,
    line_info: Vec<LineInfo>,
    flats: Vec<Box<[u8; 4096]>>,
    walls: Vec<Picture>,
    /// Colour tables: 0 is the identity, 1 + k is COLORMAP table k.
    tables: Vec<[u8; 256]>,
    tiles: tiles::Tiles,
    spawn: ([f32; 3], f32),
    /// See [`VoxelMap::colour_gain`].
    gain: f32,
    sky: [u8; 3],
    /// Light sources, empty without [`Options::lamps`].
    lamps: lamps::Lamps,
    /// Sector glow's levels, measured from COLORMAP.
    glow: light::GlowLevels,
}

impl VoxelMap {
    pub fn build(wad: &Wad, map: &str, options: &Options) -> Result<VoxelMap, WadError> {
        let data = wad.map(map)?;
        let palette = wad.palette()?;
        let colormap = wad.colormap()?;
        let textures = wad.textures()?;
        VoxelMap::from_map_data(&data, palette, &colormap, &textures, options)
    }

    /// The voxel at `p`. Thread-safe and deterministic: the app calls this
    /// from the voxel world's worker threads.
    pub fn voxel(&self, p: [i32; 3]) -> Voxel {
        let [x, y, z] = p;
        if y >= self.max[1] {
            return Voxel::Air;
        }
        if y < self.min[1] {
            return Voxel::Solid(INTERIOR);
        }
        let Some(col) = self.column(x, z) else {
            return Voxel::Solid(INTERIOR);
        };
        let sector = self.columns[col];
        if sector != VOID {
            let sec = &self.sectors[sector as usize];
            if y >= sec.floor_v && (sec.sky || y < sec.ceiling_v) {
                let line = self.lines[col];
                if line & MASKED != 0 {
                    return self.masked(line & !MASKED, x, y, z);
                }
                return self.lamps.pillar(col, y).unwrap_or(Voxel::Air);
            }
            if y == sec.floor_v - 1 {
                return self.flat(sec.floor_flat, sec, x, z);
            }
            if !sec.sky && y == sec.ceiling_v {
                return self.flat(sec.ceiling_flat, sec, x, z);
            }
        }
        // Air beside a solid voxel is another sector's, so only columns next
        // to one can have a wall.
        let near = self.near[col];
        if near.dist == 1 {
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(air) = self.air_sector(x + dx, y, z + dz) {
                    return self.wall(col, air, p, (dx, dz));
                }
            }
        }
        Voxel::Solid(self.interior(near, sector, p))
    }

    /// `Some(v)` when every voxel in the box `min..max` (max exclusive) is
    /// known to be `v` without visiting them; `None` when it may vary. Must
    /// be cheap: the app calls it once per chunk to skip generating uniform
    /// chunks.
    ///
    /// It says [`INTERIOR`] only when the box grown by `INTERIOR_DEPTH + 1`
    /// on every side holds no surface, since colours reach that far into the
    /// solid, and air only when the box holds no pillar.
    pub fn uniform_in(&self, min: [i32; 3], max: [i32; 3]) -> Option<Voxel> {
        let (bottom, top) = (self.min[1], self.max[1]);
        if min[1] >= top {
            return Some(Voxel::Air);
        }
        if max[1] <= bottom {
            return Some(Voxel::Solid(INTERIOR));
        }
        let stats = |grow: i32| {
            self.tiles.stats(
                min[0] - grow - self.min[0],
                max[0] + grow - self.min[0],
                min[2] - grow - self.min[2],
                max[2] + grow - self.min[2],
            )
        };
        let grow = INTERIOR_DEPTH + 1;
        let around = stats(grow);
        let (lo, hi) = (min[1] - grow, max[1] + grow);
        if hi <= top && (hi <= around.lo || lo >= around.hi) {
            return Some(Voxel::Solid(INTERIOR));
        }
        let st = stats(0);
        if !st.blocked
            && min[1] >= st.air_lo
            && max[1] <= st.air_hi
            && !self.lamps.pillar_in(min, max)
        {
            return Some(Voxel::Air);
        }
        None
    }

    /// The box holding every voxel that isn't plain solid below or air above
    /// the map: `(min, max)`, max exclusive.
    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        (self.min, self.max)
    }

    /// Palette 0 of PLAYPAL.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Player 1's start, in voxel coordinates at eye height (DOOM's view
    /// height, 41 units), and the yaw in radians around +y, 0 facing −z.
    pub fn spawn(&self) -> ([f32; 3], f32) {
        self.spawn
    }

    /// The gain on every colour's sRGB value that [`Options::contrast`]
    /// needs to brighten bright sectors past the palette, 1 without it. The
    /// renderer applies it; palette indices can't carry it.
    pub fn colour_gain(&self) -> f32 {
        self.gain
    }

    /// An RGB colour for the sky, for the clear colour.
    pub fn sky_colour(&self) -> [u8; 3] {
        self.sky
    }

    /// Every emissive material [`voxel`](Self::voxel) can return, as
    /// `(palette index, level)`, sorted. Empty without [`Options::lamps`].
    pub fn emissive_materials(&self) -> &[(u8, u8)] {
        &self.lamps.materials
    }

    /// The palette entry nearest `rgb`.
    pub fn nearest_colour(&self, rgb: [u8; 3]) -> u8 {
        lamps::nearest(&self.palette, rgb)
    }

    /// With [`Options::sector_glow`], the share of its sRGB value a face
    /// reading each light level from 0 to 15 should show to look as DOOM
    /// draws it: a face beside a surface glowing at level `E` reads `E − 1`
    /// at a voxel reach of 14.
    pub fn glow_brightness(&self) -> [f32; 16] {
        self.glow.brightness
    }

    /// How many light-giving things stand as pillars.
    pub fn pillar_count(&self) -> usize {
        self.lamps.pillars.len()
    }

    /// Map units per voxel.
    pub fn units_per_voxel(&self) -> i32 {
        self.s
    }

    /// The sector of voxel column `(x, z)`, or `None` for the void and
    /// outside the map.
    pub fn column_sector(&self, x: i32, z: i32) -> Option<u16> {
        let s = self.columns[self.column(x, z)?];
        (s != VOID).then_some(s)
    }

    /// The air span of column `(x, z)` in voxels, `floor..ceiling`, with the
    /// ceiling at the map top under sky; `None` for the void.
    pub fn column_air(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        let sec = &self.sectors[self.column_sector(x, z)? as usize];
        let ceiling = if sec.sky { self.max[1] } else { sec.ceiling_v };
        Some((sec.floor_v, ceiling))
    }

    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let (cx, cz) = (x - self.min[0], z - self.min[2]);
        if cx < 0 || cz < 0 || cx >= self.width || cz >= self.depth {
            return None;
        }
        Some((cz * self.width + cx) as usize)
    }

    /// The sector of column `(x, z)` when voxel height `y` in it is air.
    fn air_sector(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let s = self.columns[self.column(x, z)?];
        if s == VOID {
            return None;
        }
        let sec = &self.sectors[s as usize];
        (y >= sec.floor_v && (sec.sky || y < sec.ceiling_v)).then_some(s)
    }
}

#[cfg(test)]
mod tests;
