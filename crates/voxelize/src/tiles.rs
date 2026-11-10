//! Per-tile height summaries, so `uniform_in` answers in a few lookups.
//!
//! Level 0 tiles are 32×32 columns; each level above merges 2×2 tiles of
//! the one below. A box no wider than a level's tile overlaps at most 2×2
//! of its tiles.

/// What a group of columns allows a box to claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stats {
    /// The lowest surface voxel over the columns and their 4-neighbours: a
    /// floor surface, or a wall facing a neighbour's air. Below it every
    /// voxel is solid and touches no air.
    pub lo: i32,
    /// One above the highest such voxel: a ceiling surface or a wall.
    pub hi: i32,
    /// The highest floor (first air voxel) over the columns.
    pub air_lo: i32,
    /// The lowest ceiling (first solid voxel) over the columns; `i32::MAX`
    /// for sky.
    pub air_hi: i32,
    /// Some column is void or holds a masked-texture slab.
    pub blocked: bool,
}

impl Stats {
    /// No columns.
    pub const EMPTY: Stats = Stats {
        lo: i32::MAX,
        hi: i32::MIN,
        air_lo: i32::MIN,
        air_hi: i32::MAX,
        blocked: false,
    };

    /// Void, or outside the map.
    pub const VOID: Stats = Stats {
        lo: i32::MAX,
        hi: i32::MIN,
        air_lo: i32::MAX,
        air_hi: i32::MIN,
        blocked: true,
    };

    pub fn merge(self, o: Stats) -> Stats {
        Stats {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
            air_lo: self.air_lo.max(o.air_lo),
            air_hi: self.air_hi.min(o.air_hi),
            blocked: self.blocked || o.blocked,
        }
    }
}

pub(crate) const TILE: i32 = 32;

pub(crate) struct Tiles {
    width: i32,
    depth: i32,
    /// Per level: tiles across, tiles down, tiles row-major.
    levels: Vec<(i32, i32, Vec<Stats>)>,
}

impl Tiles {
    /// `column(x, z)` gives a column's own stats (`lo`/`hi` of the column
    /// alone, no neighbours) for `x` in `0..width`, `z` in `0..depth`.
    pub fn new(width: i32, depth: i32, column: impl Fn(i32, i32) -> Stats) -> Tiles {
        let (tw, td) = (div_ceil(width, TILE), div_ceil(depth, TILE));
        let mut base = Vec::with_capacity((tw * td) as usize);
        for tz in 0..td {
            for tx in 0..tw {
                let (x0, z0) = (tx * TILE, tz * TILE);
                let (x1, z1) = ((x0 + TILE).min(width), (z0 + TILE).min(depth));
                let mut s = Stats::EMPTY;
                for z in z0 - 1..z1 + 1 {
                    for x in x0 - 1..x1 + 1 {
                        let inside = (0..width).contains(&x) && (0..depth).contains(&z);
                        let c = if inside { column(x, z) } else { Stats::VOID };
                        let own = (x0..x1).contains(&x) && (z0..z1).contains(&z);
                        s.lo = s.lo.min(c.lo);
                        s.hi = s.hi.max(c.hi);
                        if own {
                            s.air_lo = s.air_lo.max(c.air_lo);
                            s.air_hi = s.air_hi.min(c.air_hi);
                            s.blocked |= c.blocked;
                        }
                    }
                }
                base.push(s);
            }
        }
        let mut levels = vec![(tw, td, base)];
        loop {
            let (w, d, below) = levels.last().unwrap();
            if *w <= 1 && *d <= 1 {
                break;
            }
            let (w2, d2) = (div_ceil(*w, 2), div_ceil(*d, 2));
            let mut up = Vec::with_capacity((w2 * d2) as usize);
            for z in 0..d2 {
                for x in 0..w2 {
                    let mut s = Stats::EMPTY;
                    for (cx, cz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (bx, bz) = (2 * x + cx, 2 * z + cz);
                        if bx < *w && bz < *d {
                            s = s.merge(below[(bz * w + bx) as usize]);
                        }
                    }
                    up.push(s);
                }
            }
            levels.push((w2, d2, up));
        }
        Tiles {
            width,
            depth,
            levels,
        }
    }

    /// Stats covering every column in `x0..x1`, `z0..z1` (grid-relative,
    /// may reach outside the grid, which counts as void).
    pub fn stats(&self, x0: i32, x1: i32, z0: i32, z1: i32) -> Stats {
        if x0 >= x1 || z0 >= z1 {
            return Stats::EMPTY;
        }
        let outside = x0 < 0 || z0 < 0 || x1 > self.width || z1 > self.depth;
        let mut s = if outside { Stats::VOID } else { Stats::EMPTY };
        let (cx0, cx1) = (x0.max(0), x1.min(self.width));
        let (cz0, cz1) = (z0.max(0), z1.min(self.depth));
        if cx0 >= cx1 || cz0 >= cz1 {
            return s;
        }
        let size = (cx1 - cx0).max(cz1 - cz0);
        let mut level = 0;
        while level + 1 < self.levels.len() && (TILE << level) < size {
            level += 1;
        }
        let t = TILE << level;
        let (w, _, tiles) = &self.levels[level];
        for tz in cz0 / t..=(cz1 - 1) / t {
            for tx in cx0 / t..=(cx1 - 1) / t {
                s = s.merge(tiles[(tz * w + tx) as usize]);
            }
        }
        s
    }
}

fn div_ceil(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}
