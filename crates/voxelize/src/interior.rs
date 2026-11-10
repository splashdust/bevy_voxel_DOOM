//! Colours inside the solid, so a voxel dug out shows the material carried
//! on from the surface rather than a black hole.
//!
//! A hidden solid voxel takes the colour of the surface nearest it, darkened
//! through COLORMAP with depth: slowly near the surface, so a shallow crater
//! still shows its texture, and reaching the darkest table at
//! [`INTERIOR_DEPTH`], so the colours fade into [`INTERIOR`] without a step.
//! Its depth is one less than the Chebyshev distance to the air it is
//! measured from, so a surface voxel has depth 0. Two kinds of air are tried:
//!
//! - its own column's air: under a floor the floor flat goes on down, over
//!   a ceiling the ceiling flat goes on up;
//! - the nearest column of another sector with air, found at build time and
//!   stored per column in [`Near`]: the linedef between them is extruded
//!   into the solid with its wall texture, pegged as the surface wall is.
//!
//! At depth [`INTERIOR_DEPTH`] or more a voxel is [`INTERIOR`].
//!
//! `uniform_in` answers "all interior" only for boxes whose every voxel is
//! more than `INTERIOR_DEPTH + 1` voxels from any column's air span or
//! surface, judged by the tile summaries. Each depth here is measured to a
//! real column's air span, over a distance never shorter than the true one,
//! so such voxels always come out deeper than [`INTERIOR_DEPTH`]. Taking
//! the nearest column only approximately can make a voxel darker than it
//! needs to be, never lighter than `uniform_in` allows.
//!
//! [`INTERIOR`]: crate::INTERIOR

use std::collections::VecDeque;

use crate::{VoxelMap, INTERIOR, MASKED, NO_LINE, VOID};

/// Voxels of depth at or past this are [`INTERIOR`].
pub const INTERIOR_DEPTH: i32 = 10;

/// The darkest COLORMAP table.
const DARKEST: i32 = 31;

/// A column's nearest column of another sector with air, within
/// [`INTERIOR_DEPTH`] columns.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Near {
    /// That column's sector, or [`VOID`] when none is near.
    pub air: u16,
    /// The linedef between them, or [`NO_LINE`].
    pub line: u16,
    /// An upper bound on the Chebyshev distance between the two columns.
    pub dist: u8,
}

impl Near {
    const NONE: Near = Near {
        air: VOID,
        line: NO_LINE,
        dist: u8::MAX,
    };
}

impl VoxelMap {
    /// Darkens palette index `index` by `steps` COLORMAP tables.
    pub fn shade(&self, index: u8, steps: u8) -> u8 {
        if steps == 0 {
            return index;
        }
        let table = 1 + (steps as usize).min(DARKEST as usize);
        self.tables[table][index as usize]
    }

    /// Unlit `texel` in light table `table` (0 the identity, 1 + k COLORMAP
    /// table k), darkened for `depth` voxels inside the solid: by
    /// `DARKEST · (depth / INTERIOR_DEPTH)²` tables.
    fn darkened(&self, texel: u8, table: u8, depth: i32) -> u8 {
        let darker = DARKEST * depth * depth / (INTERIOR_DEPTH * INTERIOR_DEPTH);
        let k = (table as i32 - 1).max(0) + darker;
        self.tables[1 + k.min(DARKEST) as usize][texel as usize]
    }

    /// The colour of solid voxel `[x, y, z]` that touches no air, in a
    /// column of sector `sector` (or [`VOID`]) whose nearest other air is
    /// `near`.
    pub(crate) fn interior(&self, near: Near, sector: u16, p: [i32; 3]) -> u8 {
        let [x, y, z] = p;
        let mut depth = INTERIOR_DEPTH;
        let mut own = None;
        if sector != VOID {
            let sec = &self.sectors[sector as usize];
            if sec.sky || sec.ceiling_v > sec.floor_v {
                if y < sec.floor_v {
                    depth = sec.floor_v - 1 - y;
                    own = Some((sec.floor_flat, sec.flat_table));
                } else if !sec.sky && y >= sec.ceiling_v {
                    depth = y - sec.ceiling_v;
                    own = Some((sec.ceiling_flat, sec.flat_table));
                }
            }
        }
        if near.air != VOID {
            let air = &self.sectors[near.air as usize];
            let top = if air.sky { self.max[1] } else { air.ceiling_v };
            let dy = if y < air.floor_v {
                air.floor_v - y
            } else if y >= top {
                y - top + 1
            } else {
                0
            };
            let wall_depth = (near.dist as i32).max(dy) - 1;
            if wall_depth < depth {
                if wall_depth >= INTERIOR_DEPTH {
                    return INTERIOR;
                }
                // A point mirrored across the line, on the air's side.
                let (px, py, _) = self.centre(x, y, z);
                let air_point = if near.line == NO_LINE {
                    (px, py)
                } else {
                    let li = &self.line_info[near.line as usize];
                    let c = li.cross(px, py);
                    (px + 2.0 * c * li.dir[1], py - 2.0 * c * li.dir[0])
                };
                let (texel, table) =
                    self.wall_texel_at(near.line, near.air, p, air_point, (0, 0));
                return self.darkened(texel, table, wall_depth);
            }
        }
        match own {
            Some((flat, table)) if depth < INTERIOR_DEPTH => {
                self.darkened(self.flat_texel(flat, x, z), table, depth)
            }
            _ => INTERIOR,
        }
    }

    /// Per column, the nearest column of another sector with air: a
    /// breadth-first search over 8-neighbours, out from the columns beside
    /// such a column and through columns of their own sector.
    pub(crate) fn near_grid(&self) -> Vec<Near> {
        let (w, d) = (self.width, self.depth);
        let mut near = vec![Near::NONE; (w * d) as usize];
        let has_air = |s: u16| {
            s != VOID && {
                let sec = &self.sectors[s as usize];
                sec.sky || sec.ceiling_v > sec.floor_v
            }
        };
        const NEIGHBOURS: [(i32, i32); 8] = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ];
        let at = |cx: i32, cz: i32| {
            ((0..w).contains(&cx) && (0..d).contains(&cz)).then(|| (cz * w + cx) as usize)
        };
        crate::build::par_rows(&mut near, w as usize, |cz, row| {
            let cz = cz as i32;
            for (cx, out) in row.iter_mut().enumerate() {
                let cx = cx as i32;
                let col = (cz * w + cx) as usize;
                let own = self.columns[col];
                let own_line = self.lines[col] & !MASKED;
                let line_sides = (own_line != NO_LINE).then(|| {
                    self.line_info[own_line as usize]
                        .sides
                        .each_ref()
                        .map(|s| s.as_ref().map_or(VOID, |s| s.sector))
                });
                let mut best: Option<(u16, usize)> = None;
                for (dx, dz) in NEIGHBOURS {
                    let Some(n) = at(cx + dx, cz + dz) else {
                        continue;
                    };
                    let other = self.columns[n];
                    if other == own || !has_air(other) {
                        continue;
                    }
                    let on_line = line_sides.is_some_and(|s| s.contains(&other));
                    if best.is_none() || on_line {
                        best = Some((other, n));
                    }
                    if on_line {
                        break;
                    }
                }
                if let Some((air, n)) = best {
                    let line = if own_line != NO_LINE {
                        own_line
                    } else {
                        self.lines[n] & !MASKED
                    };
                    *out = Near { air, line, dist: 1 };
                }
            }
        });

        let mut queue: VecDeque<usize> =
            (0..near.len()).filter(|&i| near[i].air != VOID).collect();
        while let Some(i) = queue.pop_front() {
            let from = near[i];
            if from.dist as i32 >= INTERIOR_DEPTH {
                continue;
            }
            let (cx, cz) = (i as i32 % w, i as i32 / w);
            let own = self.columns[i];
            for (dx, dz) in NEIGHBOURS {
                let Some(n) = at(cx + dx, cz + dz) else {
                    continue;
                };
                if self.columns[n] == own && near[n].air == VOID {
                    near[n] = Near {
                        dist: from.dist + 1,
                        ..from
                    };
                    queue.push_back(n);
                }
            }
        }
        near
    }
}
