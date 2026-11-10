//! Texturing: the palette index of a flat, wall or masked-slab voxel.
//!
//! Every texel is sampled at the voxel's centre, so at `s` units per voxel a
//! voxel shows one of the `s×s` texels it covers.

use doom_wad::{ML_DONTPEGBOTTOM, ML_DONTPEGTOP};

use crate::{LineInfo, SectorInfo, SideInfo, Voxel, VoxelMap, NO_LINE, NO_TEX};

/// A wall texel as [`VoxelMap::wall_texel_at`] finds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WallTexel {
    /// The unlit palette index.
    pub texel: u8,
    /// The light table.
    pub table: u8,
    /// The emission level when the texel glows, else 0.
    pub lamp: u8,
}

impl VoxelMap {
    /// The voxel centre in map units: x, DOOM y, height.
    pub(crate) fn centre(&self, x: i32, y: i32, z: i32) -> (f64, f64, f64) {
        let s = self.s as f64;
        (
            (x as f64 + 0.5) * s,
            -(z as f64 + 0.5) * s,
            (y as f64 + 0.5) * s,
        )
    }

    /// The voxel of a flat of sector `sec` above or below voxel column
    /// `(x, z)`: emissive where it is a bright texel of a light flat, and
    /// otherwise solid in the sector's light.
    pub(crate) fn flat(&self, flat: u16, sec: &SectorInfo, x: i32, z: i32) -> Voxel {
        let texel = self.flat_texel(flat, x, z);
        if sec.lamp_level > 0 && self.lamps.flat_glows(flat, texel) {
            return Voxel::Emissive {
                index: texel,
                level: sec.lamp_level,
            };
        }
        Voxel::Solid(self.tables[sec.flat_table as usize][texel as usize])
    }

    /// A flat's texel above or below voxel column `(x, z)`, unlit. Flats
    /// tile on the world grid: r_plane.c `R_MapPlane` passes `x` and `−y` to
    /// r_draw.c `R_DrawSpan`, which reads texel `(x & 63, −y & 63)`.
    pub(crate) fn flat_texel(&self, flat: u16, x: i32, z: i32) -> u8 {
        if flat == NO_TEX {
            return FALLBACK;
        }
        // floor((2x + 1)·s / 2), the centre in whole map units.
        let s = self.s as i64;
        let u = ((2 * x as i64 + 1) * s).div_euclid(2) & 63;
        let v = ((2 * z as i64 + 1) * s).div_euclid(2) & 63;
        self.flats[flat as usize][(v * 64 + u) as usize]
    }

    /// A wall voxel: solid voxel `[x, y, z]` in column `col` beside air of
    /// sector `air` at `(x + dx, z + dz)`.
    pub(crate) fn wall(
        &self,
        col: usize,
        air: u16,
        [x, y, z]: [i32; 3],
        (dx, dz): (i32, i32),
    ) -> Voxel {
        let line = self.lines[col] & !crate::MASKED;
        let (ax, ay, _) = self.centre(x + dx, y, z + dz);
        let w = self.wall_texel_at(line, air, [x, y, z], (ax, ay), (dx, dz));
        if w.lamp > 0 {
            return Voxel::Emissive {
                index: w.texel,
                level: w.lamp,
            };
        }
        Voxel::Solid(self.tables[w.table as usize][w.texel as usize])
    }

    /// The unlit texel and light table of the wall of linedef `line` at
    /// voxel `[x, y, z]`, as seen from air of sector `air`, which lies on the
    /// side of map point `air_point`. Without a texture there it is the air
    /// sector's floor flat at column `(x + dx, z + dz)`, which never glows.
    pub(crate) fn wall_texel_at(
        &self,
        line: u16,
        air: u16,
        [x, y, z]: [i32; 3],
        air_point: (f64, f64),
        (dx, dz): (i32, i32),
    ) -> WallTexel {
        let air_sec = &self.sectors[air as usize];
        let fallback = WallTexel {
            texel: self.flat_texel(air_sec.floor_flat, x + dx, z + dz),
            table: air_sec.flat_table,
            lamp: 0,
        };
        if line == NO_LINE {
            return fallback;
        }
        let li = &self.line_info[line as usize];
        let (px, py, h) = self.centre(x, y, z);
        let facing = match (&li.sides[0], &li.sides[1]) {
            (Some(f), Some(b)) => {
                if f.sector == air {
                    0
                } else if b.sector == air {
                    1
                } else {
                    (li.cross(air_point.0, air_point.1) > 0.0) as usize
                }
            }
            (Some(_), None) => 0,
            (None, Some(_)) => 1,
            (None, None) => return fallback,
        };
        let side = li.sides[facing].as_ref().unwrap();
        let front = &self.sectors[side.sector as usize];
        let back = li.sides[1 - facing]
            .as_ref()
            .map(|b| &self.sectors[b.sector as usize]);
        // r_segs.c `R_StoreWallRange`: which part, and the height of the
        // texture's top row ("texturemid" without the view height).
        let (tex, top) = match back {
            None => {
                // One-sided middle: top at the ceiling, or bottom at the
                // floor when lower unpegged.
                let top = if li.flags & ML_DONTPEGBOTTOM != 0 {
                    front.floor + self.height_of(side.middle)
                } else {
                    front.ceiling
                };
                (side.middle, top)
            }
            Some(back) => {
                // The sky hack: between two sky sectors the front ceiling
                // is taken as the back's.
                let world_top = if front.sky && back.sky {
                    back.ceiling
                } else {
                    front.ceiling
                };
                if h >= back.ceiling as f64 {
                    // Upper: bottom at the back ceiling, or top at the front
                    // ceiling when upper unpegged.
                    let top = if li.flags & ML_DONTPEGTOP != 0 {
                        world_top
                    } else {
                        back.ceiling + self.height_of(side.upper)
                    };
                    (side.upper, top)
                } else if h < back.floor as f64 {
                    // Lower: top at the back floor, or as if hanging from
                    // the front ceiling when lower unpegged.
                    let top = if li.flags & ML_DONTPEGBOTTOM != 0 {
                        world_top
                    } else {
                        back.floor
                    };
                    (side.lower, top)
                } else {
                    return fallback;
                }
            }
        };
        match self.wall_texel(li, side, facing, tex, top, px, py, h) {
            Some(texel) => WallTexel {
                texel,
                table: side.table,
                lamp: if side.lamp_level > 0 && self.lamps.wall_glows(tex, texel) {
                    side.lamp_level
                } else {
                    0
                },
            },
            None => fallback,
        }
    }

    /// A voxel of a masked middle texture's slab: r_segs.c
    /// `R_RenderMaskedSegRange` draws the texture once, not tiled, with its
    /// top at the lower ceiling, or its bottom at the higher floor when lower
    /// unpegged, clipped to the opening. Transparent texels are air.
    pub(crate) fn masked(&self, line: u16, x: i32, y: i32, z: i32) -> Voxel {
        let li = &self.line_info[line as usize];
        let (Some(d), [Some(_), Some(_)]) = (li.masked_side, &li.sides) else {
            return Voxel::Air;
        };
        let side = li.sides[d].as_ref().unwrap();
        let other = li.sides[1 - d].as_ref().unwrap();
        let (f, b) = (
            &self.sectors[side.sector as usize],
            &self.sectors[other.sector as usize],
        );
        let (lo, hi) = (f.floor.max(b.floor), f.ceiling.min(b.ceiling));
        let (px, py, h) = self.centre(x, y, z);
        if h < lo as f64 || h >= hi as f64 {
            return Voxel::Air;
        }
        let height = self.height_of(side.middle);
        let top = if li.flags & ML_DONTPEGBOTTOM != 0 {
            lo + height
        } else {
            hi
        };
        let row = (top as f64 + side.y_offset - h).floor();
        if row < 0.0 || row >= height as f64 {
            return Voxel::Air;
        }
        match self.wall_texel(li, side, d, side.middle, top, px, py, h) {
            Some(t) => Voxel::Solid(self.tables[side.table as usize][t as usize]),
            None => Voxel::Air,
        }
    }

    /// The texel of wall texture `tex` on `side` of `li` at map point
    /// `(px, py)` and height `h`, with the texture's top row at `top`.
    /// r_segs.c: the column is `textureoffset + seg offset + distance along
    /// the seg`, where a back seg runs from v2 (p_setup.c, node builders);
    /// the row is `top + rowoffset − h`.
    #[allow(clippy::too_many_arguments)]
    fn wall_texel(
        &self,
        li: &LineInfo,
        side: &SideInfo,
        facing: usize,
        tex: u16,
        top: i32,
        px: f64,
        py: f64,
        h: f64,
    ) -> Option<u8> {
        if tex == NO_TEX {
            return None;
        }
        let along = (px - li.a[0]) * li.dir[0] + (py - li.a[1]) * li.dir[1];
        let along = if facing == 0 { along } else { li.len - along };
        let u = (side.x_offset + along).floor() as i32;
        let v = (top as f64 + side.y_offset - h).floor() as i32;
        self.walls[tex as usize].texel_wrapped(u, v)
    }

    fn height_of(&self, tex: u16) -> i32 {
        if tex == NO_TEX {
            0
        } else {
            self.walls[tex as usize].height as i32
        }
    }
}

impl LineInfo {
    /// Positive when map point `(x, y)` is left of v1 → v2 (the back side).
    pub(crate) fn cross(&self, x: f64, y: f64) -> f64 {
        self.dir[0] * (y - self.a[1]) - self.dir[1] * (x - self.a[0])
    }
}

/// The colour where a texture is missing: a mid grey.
pub(crate) const FALLBACK: u8 = 96;
