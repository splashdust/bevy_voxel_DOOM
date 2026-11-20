//! DOOM's linedef specials that the use key sets off, and the sectors they
//! move, worked out from the map at start. No Bevy here: `movers` runs them.
//!
//! A line is used from its front side (p_switch.c `P_UseSpecialLine`). A
//! manual door moves the sector behind the line; every other special moves
//! the sectors carrying the line's tag. Specials that only walking over or
//! shooting a line sets off are left out, and so are stairs, donuts,
//! crushers and light changes.
//!
//! Each moving sector is a [`Mover`]: one of its planes travels between two
//! heights. The world is built with the plane at the end that leaves the
//! most air (a door open, a lift down, a floor at its lowest), and the
//! voxels between the two ends ride on a sub-grid that slides along y.

use std::collections::HashMap;

use doom_wad::{MapData, ML_TWOSIDED};

/// DOOM's door speed and wait (p_spec.h `VDOORSPEED`, `VDOORWAIT`), in map
/// units and tics.
pub const DOOR_SPEED: f32 = 2.0;
pub const DOOR_WAIT: u32 = 150;
/// p_plats.c: lifts wait 3 seconds at the bottom.
pub const PLAT_WAIT: u32 = 105;

/// Which plane of a sector moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Floor,
    Ceiling,
}

/// What a door does: p_doors.c `vldoor_e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorOp {
    /// Opens, waits `DOOR_WAIT` tics and closes.
    Normal,
    /// Opens and stays open.
    Open,
    /// Closes and stays closed.
    Close,
}

/// Where a floor special sends the floor: p_floor.c `EV_DoFloor` and
/// p_plats.c `EV_DoPlat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloorTarget {
    /// `P_FindLowestFloorSurrounding`, its own floor included.
    Lowest,
    /// `P_FindHighestFloorSurrounding`.
    Highest,
    /// The highest around plus 8, when that isn't its own floor (turbo).
    HighestPlus8,
    /// `P_FindNextHighestFloor`: the lowest floor around above its own.
    NextHigher,
    /// `P_FindLowestCeilingSurrounding`, no higher than its own ceiling.
    LowestCeiling,
    /// Its own floor raised by this much, no higher than its ceiling.
    Raise(i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// `speed` in map units a tic; blaze doors make other sounds.
    Door {
        op: DoorOp,
        speed: f32,
        blaze: bool,
    },
    /// Down to the lowest floor around, wait, back up.
    Plat {
        speed: f32,
    },
    Floor {
        target: FloorTarget,
        speed: f32,
    },
    /// Ends the level.
    Exit {
        secret: bool,
    },
}

/// What using a line does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineUse {
    pub effect: Effect,
    /// The sector behind the line, rather than the tagged sectors.
    pub manual: bool,
    /// Usable again (DR and SR lines); otherwise its special goes after one
    /// use, as `P_UseSpecialLine` clears it.
    pub repeat: bool,
}

/// The use special `special`, from p_switch.c `P_UseSpecialLine` and
/// p_doors.c `EV_VerticalDoor`. Keyed doors open without their key.
pub fn line_use(special: u16) -> Option<LineUse> {
    use DoorOp::*;
    use Effect::*;
    let door = |op, speed, blaze| Door { op, speed, blaze };
    let fast = DOOR_SPEED * 4.0;
    let floor = |target, speed| Floor { target, speed };
    let (effect, manual, repeat) = match special {
        1 | 26 | 27 | 28 => (door(Normal, DOOR_SPEED, false), true, true),
        117 => (door(Normal, fast, true), true, true),
        31..=34 => (door(Open, DOOR_SPEED, false), true, false),
        118 => (door(Open, fast, true), true, false),
        29 | 63 => (door(Normal, DOOR_SPEED, false), false, special == 63),
        103 | 61 => (door(Open, DOOR_SPEED, false), false, special == 61),
        50 | 42 => (door(Close, DOOR_SPEED, false), false, special == 42),
        111 | 114 => (door(Normal, fast, true), false, special == 114),
        112 | 115 => (door(Open, fast, true), false, special == 115),
        113 | 116 => (door(Close, fast, true), false, special == 116),
        133 | 135 | 137 => (door(Open, fast, true), false, false),
        99 | 134 | 136 => (door(Open, fast, true), false, true),
        // p_ceilng.c `lowerToFloor` at CEILSPEED: a slow close.
        41 | 43 => (door(Close, 1.0, false), false, special == 43),
        21 | 62 => (Plat { speed: 4.0 }, false, special == 62),
        122 | 123 => (Plat { speed: 8.0 }, false, special == 123),
        18 | 69 => (floor(FloorTarget::NextHigher, 1.0), false, special == 69),
        20 | 68 => (floor(FloorTarget::NextHigher, 0.5), false, special == 68),
        14 | 67 => (floor(FloorTarget::Raise(32), 0.5), false, special == 67),
        15 | 66 => (floor(FloorTarget::Raise(24), 0.5), false, special == 66),
        101 | 64 => (floor(FloorTarget::LowestCeiling, 1.0), false, special == 64),
        131 | 132 => (floor(FloorTarget::NextHigher, 4.0), false, special == 132),
        140 => (floor(FloorTarget::Raise(512), 1.0), false, false),
        23 | 60 => (floor(FloorTarget::Lowest, 1.0), false, special == 60),
        102 | 45 => (floor(FloorTarget::Highest, 1.0), false, special == 45),
        71 | 70 => (floor(FloorTarget::HighestPlus8, 4.0), false, special == 70),
        11 => (Exit { secret: false }, false, false),
        51 => (Exit { secret: true }, false, false),
        _ => return None,
    };
    Some(LineUse {
        effect,
        manual,
        repeat,
    })
}

/// A sector with a moving plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    pub sector: u16,
    pub plane: Plane,
    /// The sector's floor and ceiling as the map stores them.
    pub floor: i32,
    pub ceiling: i32,
    /// The end of the plane's travel that leaves the most air, where the
    /// world is built: the highest ceiling or the lowest floor.
    pub open: i32,
    /// The other end: the lowest ceiling or the highest floor.
    pub shut: i32,
}

impl Mover {
    /// The plane's height at the start.
    pub fn start(&self) -> i32 {
        match self.plane {
            Plane::Floor => self.floor,
            Plane::Ceiling => self.ceiling,
        }
    }

    /// The sector's `(floor, ceiling)` with the plane at `height`.
    pub fn heights_at(&self, height: i32) -> (i32, i32) {
        match self.plane {
            Plane::Floor => (height, self.ceiling),
            Plane::Ceiling => (self.floor, height),
        }
    }
}

/// The heights around each sector that specials move to: p_spec.c's
/// `P_Find…Surrounding` over the sectors across two-sided lines.
pub struct Surroundings {
    neighbours: Vec<Vec<usize>>,
    floors: Vec<i32>,
    ceilings: Vec<i32>,
}

impl Surroundings {
    pub fn new(map: &MapData) -> Self {
        let n = map.sectors.len();
        let mut neighbours = vec![Vec::new(); n];
        let sector_of = |side: Option<u16>| {
            side.and_then(|s| map.sidedefs.get(s as usize))
                .map(|s| s.sector as usize)
                .filter(|&s| s < n)
        };
        for l in &map.linedefs {
            if l.flags & ML_TWOSIDED == 0 {
                continue;
            }
            if let (Some(f), Some(b)) = (sector_of(l.front), sector_of(l.back)) {
                if f != b {
                    neighbours[f].push(b);
                    neighbours[b].push(f);
                }
            }
        }
        Surroundings {
            neighbours,
            floors: map.sectors.iter().map(|s| s.floor as i32).collect(),
            ceilings: map.sectors.iter().map(|s| s.ceiling as i32).collect(),
        }
    }

    fn around(&self, sector: usize) -> impl Iterator<Item = usize> + '_ {
        self.neighbours[sector].iter().copied()
    }

    /// p_doors.c: a door opens to the lowest ceiling around, less 4.
    pub fn door_top(&self, sector: usize) -> Option<i32> {
        self.around(sector)
            .map(|s| self.ceilings[s])
            .min()
            .map(|c| c - 4)
    }

    /// Where `target` sends the floor of `sector`.
    pub fn floor_target(&self, sector: usize, target: FloorTarget) -> i32 {
        let own = self.floors[sector];
        let floors = || self.around(sector).map(|s| self.floors[s]);
        match target {
            FloorTarget::Lowest => floors().fold(own, i32::min),
            FloorTarget::Highest => floors().max().unwrap_or(own),
            FloorTarget::HighestPlus8 => match floors().max() {
                Some(h) if h != own => h + 8,
                Some(h) => h,
                None => own,
            },
            FloorTarget::NextHigher => floors().filter(|&h| h > own).min().unwrap_or(own),
            FloorTarget::LowestCeiling => self
                .around(sector)
                .map(|s| self.ceilings[s])
                .min()
                .unwrap_or(own)
                .min(self.ceilings[sector]),
            FloorTarget::Raise(by) => (own + by).min(self.ceilings[sector]),
        }
    }
}

/// The map's use lines and the sectors they move.
pub struct Specials {
    /// Linedef id to what using it does.
    pub lines: HashMap<u16, LineUse>,
    pub movers: Vec<Mover>,
    pub surroundings: Surroundings,
    /// Sector ids by tag.
    pub tagged: HashMap<u16, Vec<u16>>,
    /// Each linedef's tag.
    pub tags: Vec<u16>,
}

impl Specials {
    pub fn new(map: &MapData) -> Self {
        let surroundings = Surroundings::new(map);
        let mut tagged: HashMap<u16, Vec<u16>> = HashMap::new();
        for (i, s) in map.sectors.iter().enumerate() {
            if s.tag != 0 {
                tagged.entry(s.tag).or_default().push(i as u16);
            }
        }
        let mut lines = HashMap::new();
        // Per sector: the plane that moves and every height it reaches.
        let mut planes: HashMap<u16, (Plane, Vec<i32>)> = HashMap::new();
        for (i, l) in map.linedefs.iter().enumerate() {
            let Some(use_) = line_use(l.special) else {
                continue;
            };
            let sectors: Vec<u16> = if use_.manual {
                l.back
                    .and_then(|b| map.sidedefs.get(b as usize))
                    .map(|b| b.sector)
                    .into_iter()
                    .collect()
            } else if l.tag != 0 {
                tagged.get(&l.tag).cloned().unwrap_or_default()
            } else {
                Vec::new()
            };
            lines.insert(i as u16, use_);
            for sector in sectors {
                let si = sector as usize;
                let Some(sec) = map.sectors.get(si) else {
                    continue;
                };
                let (floor, ceiling) = (sec.floor as i32, sec.ceiling as i32);
                let (plane, heights) = match use_.effect {
                    Effect::Door { .. } => {
                        let top = surroundings.door_top(si).unwrap_or(ceiling);
                        (Plane::Ceiling, vec![floor, top.max(ceiling)])
                    }
                    Effect::Plat { .. } => (
                        Plane::Floor,
                        vec![floor, surroundings.floor_target(si, FloorTarget::Lowest)],
                    ),
                    Effect::Floor { target, .. } => (
                        Plane::Floor,
                        vec![floor, surroundings.floor_target(si, target)],
                    ),
                    Effect::Exit { .. } => continue,
                };
                let entry = planes
                    .entry(sector)
                    .or_insert_with(|| (plane, vec![plane_start(plane, floor, ceiling)]));
                // One moving plane a sector: a second kind is left out.
                if entry.0 == plane {
                    entry.1.extend(heights);
                }
            }
        }
        let mut movers: Vec<Mover> = planes
            .into_iter()
            .filter_map(|(sector, (plane, heights))| {
                let sec = &map.sectors[sector as usize];
                let (lo, hi) = (*heights.iter().min()?, *heights.iter().max()?);
                let (open, shut) = match plane {
                    Plane::Floor => (lo, hi),
                    Plane::Ceiling => (hi, lo),
                };
                (lo < hi).then_some(Mover {
                    sector,
                    plane,
                    floor: sec.floor as i32,
                    ceiling: sec.ceiling as i32,
                    open,
                    shut,
                })
            })
            .collect();
        movers.sort_by_key(|m| m.sector);
        Specials {
            lines,
            movers,
            surroundings,
            tagged,
            tags: map.linedefs.iter().map(|l| l.tag).collect(),
        }
    }

    /// Each mover's sector heights at the end where the world is built.
    pub fn open_heights(&self) -> HashMap<u16, (i32, i32)> {
        self.movers
            .iter()
            .map(|m| (m.sector, m.heights_at(m.open)))
            .collect()
    }

    /// Each mover's sector heights at the other end.
    pub fn shut_heights(&self) -> HashMap<u16, (i32, i32)> {
        self.movers
            .iter()
            .map(|m| (m.sector, m.heights_at(m.shut)))
            .collect()
    }
}

fn plane_start(plane: Plane, floor: i32, ceiling: i32) -> i32 {
    match plane {
        Plane::Floor => floor,
        Plane::Ceiling => ceiling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doom_wad::{LineDef, Sector, SideDef, Vertex};

    /// Three sectors in a row: a room (0), a door (1) and a room (2), a
    /// lift (3) beside room 2, and a switch in room 0 tagged for the lift.
    fn map() -> MapData {
        let sector = |floor, ceiling, tag| Sector {
            floor,
            ceiling,
            floor_flat: "FLOOR".into(),
            ceiling_flat: "CEIL".into(),
            light: 160,
            special: 0,
            tag,
        };
        let side = |sector| SideDef {
            x_offset: 0,
            y_offset: 0,
            upper: "-".into(),
            lower: "-".into(),
            middle: "-".into(),
            sector,
        };
        let line = |front, back: Option<u16>, special, tag| LineDef {
            v1: 0,
            v2: 1,
            flags: if back.is_some() { ML_TWOSIDED } else { 0 },
            special,
            tag,
            front: Some(front),
            back,
        };
        MapData {
            name: "E1M1".into(),
            vertexes: vec![Vertex { x: 0, y: 0 }, Vertex { x: 64, y: 0 }],
            sectors: vec![
                sector(0, 128, 0),
                sector(0, 0, 0),
                sector(-16, 136, 0),
                sector(64, 136, 7),
            ],
            sidedefs: vec![
                side(0),
                side(1),
                side(1),
                side(2),
                side(2),
                side(3),
                side(0),
            ],
            linedefs: vec![
                // Room 0 into the door, a manual door line.
                line(0, Some(1), 1, 0),
                // The door into room 2.
                line(3, Some(2), 1, 0),
                // Room 2 and the lift.
                line(4, Some(5), 0, 0),
                // A switch in room 0 lowering the lift.
                line(6, None, 62, 7),
            ],
            ..MapData::default()
        }
    }

    #[test]
    fn a_manual_door_opens_to_the_lowest_ceiling_around_less_4() {
        let sp = Specials::new(&map());
        let door = sp.movers.iter().find(|m| m.sector == 1).unwrap();
        assert_eq!(door.plane, Plane::Ceiling);
        assert_eq!((door.shut, door.open), (0, 124));
        assert_eq!(door.start(), 0);
        assert_eq!(sp.open_heights()[&1], (0, 124));
        assert_eq!(sp.shut_heights()[&1], (0, 0));
    }

    #[test]
    fn a_lift_goes_down_to_the_lowest_floor_around() {
        let sp = Specials::new(&map());
        let lift = sp.movers.iter().find(|m| m.sector == 3).unwrap();
        assert_eq!(lift.plane, Plane::Floor);
        assert_eq!((lift.open, lift.shut), (-16, 64));
        assert_eq!(sp.lines[&3].effect, Effect::Plat { speed: 4.0 });
        assert!(sp.lines[&3].repeat && !sp.lines[&3].manual);
        assert_eq!(sp.movers.len(), 2);
    }

    #[test]
    fn floor_targets_follow_p_spec() {
        let sp = Specials::new(&map());
        let s = &sp.surroundings;
        assert_eq!(s.floor_target(3, FloorTarget::Lowest), -16);
        assert_eq!(s.floor_target(3, FloorTarget::Highest), -16);
        assert_eq!(s.floor_target(2, FloorTarget::NextHigher), 0);
        assert_eq!(s.floor_target(2, FloorTarget::HighestPlus8), 72);
        assert_eq!(s.floor_target(2, FloorTarget::LowestCeiling), 0);
        assert_eq!(s.floor_target(0, FloorTarget::Raise(512)), 128);
    }

    #[test]
    fn only_use_specials_count() {
        assert!(line_use(1).unwrap().manual);
        assert!(!line_use(31).unwrap().repeat);
        assert_eq!(line_use(11).unwrap().effect, Effect::Exit { secret: false });
        // Walk-over door and lift.
        assert!(line_use(2).is_none());
        assert!(line_use(88).is_none());
    }
}
