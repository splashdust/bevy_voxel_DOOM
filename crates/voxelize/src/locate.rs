//! Point location: which sector a map point lies in, or the void.
//!
//! The sector is DOOM's own answer: the BSP walk of r_main.c
//! `R_PointInSubsector` and `R_PointOnSide`, and the subsector's sector
//! from its first seg (p_setup.c `P_GroupLines`). Every point lands in some
//! subsector, so the void needs its own test: a point is void when the
//! nearest linedef has no sidedef on the point's side. Where two lines are
//! equally near (at a shared vertex), the one the point is farther from
//! along its normal decides, which makes this an exact inside test for
//! closed sectors; an exact tie on both counts is inside.
//!
//! The subsector's own segs can't serve as the void test, though a subsector
//! is convex with its segs facing in: a one-sided wall lying along a
//! partition line leaves the void behind it in a leaf whose segs don't bound
//! it (E1M3), and node-builder vertex rounding puts points a fraction of a
//! unit behind a two-sided seg (E1M2, E2M4, MAP16). They do catch a wrong
//! leaf: a point behind one of its leaf's one-sided walls but inside by the
//! nearest line (rounding along a wall, MAP01) takes the nearest line's
//! sector instead.
//!
//! Coordinates are doubled map units, so voxel column centres at odd
//! multiples of half a unit stay integers.

use doom_wad::{MapData, Node, NF_SUBSECTOR};

pub(crate) struct Locator<'a> {
    nodes: &'a [Node],
    subsectors: Vec<Leaf>,
    lines: Vec<Line>,
    cells: Cells,
}

struct Leaf {
    /// The sector of the first seg's sidedef.
    sector: Option<u16>,
    /// The leaf's one-sided segs' linedefs, oriented with the subsector on
    /// their right.
    walls: Vec<[i64; 4]>,
}

/// A linedef in doubled units, and each side's sector.
struct Line {
    a: [f64; 2],
    b: [f64; 2],
    front: Option<u16>,
    back: Option<u16>,
}

/// Cell size of the nearest-line search, in doubled units (64 map units).
const CELL: f64 = 128.0;

/// Linedefs by the cells they pass through, and for cells with none whose
/// every point gets the same answer, that answer.
struct Cells {
    x0: f64,
    y0: f64,
    w: i32,
    h: i32,
    lines: Vec<Vec<u32>>,
    /// `Some(sector or void)` for a cell known to be all one.
    uniform: Vec<Option<Option<u16>>>,
}

impl<'a> Locator<'a> {
    pub(crate) fn new(map: &'a MapData) -> Locator<'a> {
        let vertex = |i: u16| map.vertexes.get(i as usize).copied();
        let sidedef = |s: Option<u16>| s.filter(|&s| (s as usize) < map.sidedefs.len());
        let sector_of =
            |s: Option<u16>| sidedef(s).map(|s| map.sidedefs[s as usize].sector);
        let subsectors = map
            .subsectors
            .iter()
            .map(|ss| {
                let first = ss.first_seg as usize;
                let segs = map
                    .segs
                    .get(first..first + ss.seg_count as usize)
                    .unwrap_or(&[]);
                let mut sector = None;
                let mut walls = Vec::new();
                for (i, seg) in segs.iter().enumerate() {
                    let Some(line) = map.linedefs.get(seg.linedef as usize) else {
                        continue;
                    };
                    let (side, other) = if seg.side == 0 {
                        (line.front, line.back)
                    } else {
                        (line.back, line.front)
                    };
                    if i == 0 {
                        sector = sector_of(side);
                    }
                    let (Some(a), Some(b)) = (vertex(line.v1), vertex(line.v2)) else {
                        continue;
                    };
                    if sidedef(other).is_none() {
                        let (a, b) = if seg.side == 0 { (a, b) } else { (b, a) };
                        walls.push([a.x, a.y, b.x, b.y].map(|v| 2 * v as i64));
                    }
                }
                Leaf { sector, walls }
            })
            .collect();
        let lines: Vec<Line> = map
            .linedefs
            .iter()
            .filter_map(|l| {
                let (a, b) = (vertex(l.v1)?, vertex(l.v2)?);
                (a.x != b.x || a.y != b.y).then(|| Line {
                    a: [2.0 * a.x as f64, 2.0 * a.y as f64],
                    b: [2.0 * b.x as f64, 2.0 * b.y as f64],
                    front: sector_of(l.front),
                    back: sector_of(l.back),
                })
            })
            .collect();
        let cells = Cells::new(&lines);
        Locator {
            nodes: &map.nodes,
            subsectors,
            lines,
            cells,
        }
    }

    /// The sector at the doubled point `(x, y)`, or `None` in the void.
    pub(crate) fn sector(&self, x: i64, y: i64) -> Option<u16> {
        let leaf = self.subsectors.get(self.subsector(x, y))?;
        let behind_wall = leaf.walls.iter().any(|&[ax, ay, bx, by]| {
            // Left of a→b in y-up map coordinates; on it counts as inside.
            (bx - ax) * (y - ay) - (by - ay) * (x - ax) > 0
        });
        let nearest = self.nearest_side(x as f64, y as f64)?;
        if behind_wall || leaf.sector.is_none() {
            Some(nearest)
        } else {
            leaf.sector
        }
    }

    /// The sector of the BSP leaf at the doubled point, without the void test.
    pub(crate) fn leaf_sector(&self, x: i64, y: i64) -> Option<u16> {
        self.subsectors.get(self.subsector(x, y))?.sector
    }

    /// r_main.c `R_PointInSubsector`.
    fn subsector(&self, x: i64, y: i64) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        let mut n = self.nodes.len() - 1;
        for _ in 0..=self.nodes.len() {
            let child = self.nodes[n].children[point_on_side(x, y, &self.nodes[n])];
            if child & NF_SUBSECTOR != 0 {
                return (child & !NF_SUBSECTOR) as usize;
            }
            n = child as usize;
            if n >= self.nodes.len() {
                break;
            }
        }
        // A corrupt tree: fall back to the first subsector.
        0
    }

    /// The sector on this point's side of the nearest linedef; `None` for
    /// the void.
    fn nearest_side(&self, x: f64, y: f64) -> Option<u16> {
        let c = &self.cells;
        let cx = ((x - c.x0) / CELL).floor() as i32;
        let cy = ((y - c.y0) / CELL).floor() as i32;
        if cx < 0 || cy < 0 || cx >= c.w || cy >= c.h {
            return None;
        }
        let i = (cy * c.w + cx) as usize;
        if let Some(side) = c.uniform[i] {
            return side;
        }
        nearest_side(&self.lines, c, x, y, cx, cy).0
    }
}

impl Cells {
    fn new(lines: &[Line]) -> Cells {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for l in lines {
            for k in 0..2 {
                lo[k] = lo[k].min(l.a[k].min(l.b[k]));
                hi[k] = hi[k].max(l.a[k].max(l.b[k]));
            }
        }
        if lines.is_empty() {
            (lo, hi) = ([0.0; 2], [0.0; 2]);
        }
        // A margin of cells so points just outside the map still search.
        let (x0, y0) = (lo[0] - 2.0 * CELL, lo[1] - 2.0 * CELL);
        let w = ((hi[0] - x0) / CELL).floor() as i32 + 3;
        let h = ((hi[1] - y0) / CELL).floor() as i32 + 3;
        let mut cells = vec![Vec::new(); (w * h) as usize];
        let cell = |v: f64, o: f64| ((v - o) / CELL).floor() as i32;
        // A line passes through a cell when it comes within half the
        // cell's diagonal of its centre (plus a little).
        let reach = CELL * 0.5 * std::f64::consts::SQRT_2 + 1.0;
        for (i, l) in lines.iter().enumerate() {
            for cy in cell(l.a[1].min(l.b[1]), y0)..=cell(l.a[1].max(l.b[1]), y0) {
                for cx in cell(l.a[0].min(l.b[0]), x0)..=cell(l.a[0].max(l.b[0]), x0) {
                    let centre =
                        [x0 + (cx as f64 + 0.5) * CELL, y0 + (cy as f64 + 0.5) * CELL];
                    if measure(l, centre[0], centre[1]).0 <= reach {
                        cells[(cy * w + cx) as usize].push(i as u32);
                    }
                }
            }
        }
        let mut c = Cells {
            x0,
            y0,
            w,
            h,
            lines: cells,
            uniform: Vec::new(),
        };
        c.uniform = (0..w * h)
            .map(|i| c.uniform_answer(lines, i % w, i / w))
            .collect();
        c
    }

    /// A cell no line passes through is all one answer when its sectors
    /// are closed. Where they aren't (MAP30's sector 11 is bounded by one
    /// line), the nearest line can change answer inside the cell, so the
    /// cell is taken as uniform only when its centre and four corners agree.
    fn uniform_answer(&self, lines: &[Line], cx: i32, cy: i32) -> Option<Option<u16>> {
        if !self.lines[(cy * self.w + cx) as usize].is_empty() {
            return None;
        }
        let (x, y) = (
            self.x0 + (cx as f64 + 0.5) * CELL,
            self.y0 + (cy as f64 + 0.5) * CELL,
        );
        let answer = nearest_side(lines, self, x, y, cx, cy).0;
        for (dx, dy) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
            let (px, py) = (x + dx * CELL, y + dy * CELL);
            if nearest_side(lines, self, px, py, cx, cy).0 != answer {
                return None;
            }
        }
        Some(answer)
    }
}

impl Line {
    /// The sector on the side with this cross product: right of v1→v2 is
    /// the front; on the line, either side.
    fn side(&self, cross: f64) -> Option<u16> {
        if cross < 0.0 {
            self.front
        } else if cross > 0.0 {
            self.back
        } else {
            self.front.or(self.back)
        }
    }
}

/// The sector on `(x, y)`'s side of the nearest line (`None` for the void)
/// and the line's distance, searching rings of cells out from cell
/// `(cx, cy)` until no unseen line can be nearer.
fn nearest_side(
    lines: &[Line],
    c: &Cells,
    x: f64,
    y: f64,
    cx: i32,
    cy: i32,
) -> (Option<u16>, f64) {
    // (distance, distance along the normal, the sector on this side)
    let mut best: Option<(f64, f64, Option<u16>)> = None;
    for r in 0..=c.w.max(c.h) {
        for ny in cy - r..=cy + r {
            for nx in cx - r..=cx + r {
                let ring = (nx - cx).abs() == r || (ny - cy).abs() == r;
                if !ring || nx < 0 || ny < 0 || nx >= c.w || ny >= c.h {
                    continue;
                }
                for &l in &c.lines[(ny * c.w + nx) as usize] {
                    let l = &lines[l as usize];
                    let (d, cross, len) = measure(l, x, y);
                    let perp = cross.abs() / len;
                    let side = l.side(cross);
                    let better = match best {
                        None => true,
                        Some((bd, bp, bs)) => {
                            let tie = |a: f64, b: f64| (a - b).abs() <= 1e-6;
                            d < bd - 1e-6
                                || (tie(d, bd)
                                    && (perp > bp + 1e-6
                                        || (tie(perp, bp) && bs.is_none())))
                        }
                    };
                    if better {
                        best = Some((d, perp, side));
                    }
                }
            }
        }
        // Every line within r cells' width of the point has been seen.
        if let Some((d, _, side)) = best {
            if d <= r as f64 * CELL {
                return (side, d);
            }
        }
    }
    best.map_or((None, f64::MAX), |b| (b.2, b.0))
}

/// Distance from `(x, y)` to the line segment, the cross product of v1→v2
/// with v1→point, and the line's length.
fn measure(l: &Line, x: f64, y: f64) -> (f64, f64, f64) {
    let (dx, dy) = (l.b[0] - l.a[0], l.b[1] - l.a[1]);
    let len2 = dx * dx + dy * dy;
    let t = (((x - l.a[0]) * dx + (y - l.a[1]) * dy) / len2).clamp(0.0, 1.0);
    let d = (x - l.a[0] - dx * t).hypot(y - l.a[1] - dy * t);
    let cross = dx * (y - l.a[1]) - dy * (x - l.a[0]);
    (d, cross, len2.sqrt())
}

/// r_main.c `R_PointOnSide`, branch for branch, on doubled coordinates:
/// 0 for the front (right) side, 1 for the back.
fn point_on_side(x: i64, y: i64, node: &Node) -> usize {
    let (nx, ny) = (2 * node.x as i64, 2 * node.y as i64);
    let (ndx, ndy) = (node.dx as i64, node.dy as i64);
    if ndx == 0 {
        return if x <= nx {
            (ndy > 0) as usize
        } else {
            (ndy < 0) as usize
        };
    }
    if ndy == 0 {
        return if y <= ny {
            (ndx < 0) as usize
        } else {
            (ndx > 0) as usize
        };
    }
    let (dx, dy) = (x - nx, y - ny);
    let neg = |v: i64| v < 0;
    if neg(ndy) ^ neg(ndx) ^ neg(dx) ^ neg(dy) {
        return (neg(ndy) ^ neg(dx)) as usize;
    }
    let left = ndy * dx;
    let right = dy * ndx;
    if right < left {
        0
    } else {
        1
    }
}
