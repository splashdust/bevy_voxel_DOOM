//! DOOM's things: the monsters' numbers and state sequences from info.c,
//! the projectiles they fire, the barrel, and the decorations and items that
//! only stand and animate.
//!
//! Distances and speeds are DOOM's map units and units per tic, at 35 tics
//! a second; the app scales them to voxels. A monster's states are grouped
//! into sequences (spawn, see, melee, missile, pain, death, gib), each a run
//! of frames ending in a jump: back to its start, to the see sequence, to a
//! frame within it, or nowhere, staying on the last frame or vanishing.

/// What a frame does as it starts: p_enemy.c's action functions, or the
/// part of them this app plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Idle,
    Look,
    Chase,
    Face,
    /// Zombieman: one bullet.
    PosAttack,
    /// Shotgun guy: three pellets.
    SPosAttack,
    /// Chaingunner, SS: one bullet a frame.
    CPosAttack,
    /// Keep firing while the target is in sight, or go back to chasing.
    CPosRefire,
    /// Imp: claw in melee range, else a fireball.
    TroopAttack,
    /// Demon and spectre: bite.
    SargAttack,
    /// Cacodemon: bite in melee range, else a fireball.
    HeadAttack,
    /// Baron and hell knight: claw in melee range, else a fireball.
    BruisAttack,
    /// Lost soul: charge.
    SkullAttack,
    /// Revenant: a homing rocket.
    SkelMissile,
    /// Revenant: punch.
    SkelFist,
    /// Mancubus: three volleys of two fireballs.
    FatAttack1,
    FatAttack2,
    FatAttack3,
    /// Arachnotron: a plasma bolt.
    BspiAttack,
    Pain,
    Scream,
    XScream,
    /// The corpse stops being solid.
    Fall,
    /// Barrel: blast everything within 128 units.
    Explode,
}

use Action::*;

use crate::inventory::{AmmoKind, Key, Pickup, WeaponKind};

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The frame letter, `b'A'` first.
    pub letter: u8,
    /// How long it shows; −1 forever.
    pub tics: i32,
    /// Drawn full-bright.
    pub bright: bool,
    pub action: Action,
}

const fn f(letter: u8, tics: i32, action: Action) -> Frame {
    Frame {
        letter,
        tics,
        bright: false,
        action,
    }
}

/// A full-bright frame.
const fn b(letter: u8, tics: i32, action: Action) -> Frame {
    Frame {
        letter,
        tics,
        bright: true,
        action,
    }
}

/// Where a sequence goes after its last frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    /// Its own first frame.
    Loop,
    /// The see (chase) sequence.
    See,
    /// Frame `n` of itself.
    Goto(usize),
    /// Nowhere: the thing is removed.
    Remove,
}

#[derive(Clone, Copy, Debug)]
pub struct Seq {
    /// The sprite, when not the thing's own.
    pub sprite: Option<&'static str>,
    pub frames: &'static [Frame],
    pub then: Then,
}

const fn seq(frames: &'static [Frame], then: Then) -> Seq {
    Seq {
        sprite: None,
        frames,
        then,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SeqKind {
    Spawn,
    See,
    Melee,
    Missile,
    Pain,
    Death,
    XDeath,
}

/// The sound lumps a monster plays, without their `DS` prefix; one of a
/// list at random.
#[derive(Clone, Copy, Debug)]
pub struct Sounds {
    pub see: &'static [&'static str],
    pub attack: &'static str,
    pub pain: &'static str,
    pub death: &'static [&'static str],
    pub active: &'static str,
}

/// A shootable thing: a monster, or the barrel.
#[derive(Debug)]
pub struct MonsterInfo {
    pub name: &'static str,
    pub doomednum: u16,
    pub sprite: &'static str,
    pub health: i32,
    /// Units per move: a monster moves this far each chase frame.
    pub speed: f32,
    pub radius: f32,
    pub height: f32,
    pub mass: f32,
    /// Out of 256: the chance damage sends it into pain.
    pub painchance: i32,
    /// Chase frames before it may attack once awake.
    pub reaction: i32,
    /// Flies: no gravity, and it rises and sinks toward its target.
    pub float: bool,
    /// Drawn dark and see-through: the spectre.
    pub shadow: bool,
    /// The lost soul's charge damage factor.
    pub damage: i32,
    pub spawn: Seq,
    pub see: Seq,
    pub melee: Option<Seq>,
    pub missile: Option<Seq>,
    pub pain: Seq,
    pub death: Seq,
    pub xdeath: Option<Seq>,
    pub sounds: Sounds,
    /// Not a monster: never wakes, and isn't counted as one.
    pub inert: bool,
}

impl MonsterInfo {
    pub fn seq(&self, kind: SeqKind) -> &Seq {
        match kind {
            SeqKind::Spawn => &self.spawn,
            SeqKind::See => &self.see,
            SeqKind::Melee => self.melee.as_ref().unwrap_or(&self.see),
            SeqKind::Missile => self.missile.as_ref().unwrap_or(&self.see),
            SeqKind::Pain => &self.pain,
            SeqKind::Death => &self.death,
            SeqKind::XDeath => self.xdeath.as_ref().unwrap_or(&self.death),
        }
    }
}

const STAND: Seq = seq(&[f(b'A', 10, Look), f(b'B', 10, Look)], Then::Loop);

/// Eight chase frames, two of each of A to D.
const fn run(tics: i32) -> [Frame; 8] {
    [
        f(b'A', tics, Chase),
        f(b'A', tics, Chase),
        f(b'B', tics, Chase),
        f(b'B', tics, Chase),
        f(b'C', tics, Chase),
        f(b'C', tics, Chase),
        f(b'D', tics, Chase),
        f(b'D', tics, Chase),
    ]
}

const RUN4: [Frame; 8] = run(4);
const RUN3: [Frame; 8] = run(3);
const RUN2: [Frame; 8] = run(2);

const ZOMBIE_SOUNDS: Sounds = Sounds {
    see: &["POSIT1", "POSIT2", "POSIT3"],
    attack: "PISTOL",
    pain: "POPAIN",
    death: &["PODTH1", "PODTH2", "PODTH3"],
    active: "POSACT",
};

const POSS_GIB: Seq = seq(
    &[
        f(b'M', 5, Idle),
        f(b'N', 5, XScream),
        f(b'O', 5, Fall),
        f(b'P', 5, Idle),
        f(b'Q', 5, Idle),
        f(b'R', 5, Idle),
        f(b'S', 5, Idle),
        f(b'T', 5, Idle),
        f(b'U', -1, Idle),
    ],
    Then::Loop,
);

pub const ZOMBIEMAN: MonsterInfo = MonsterInfo {
    name: "zombieman",
    doomednum: 3004,
    sprite: "POSS",
    health: 20,
    speed: 8.0,
    radius: 20.0,
    height: 56.0,
    mass: 100.0,
    painchance: 200,
    reaction: 8,
    float: false,
    shadow: false,
    damage: 0,
    spawn: STAND,
    see: seq(&RUN4, Then::Loop),
    melee: None,
    missile: Some(seq(
        &[f(b'E', 10, Face), f(b'F', 8, PosAttack), f(b'E', 8, Idle)],
        Then::See,
    )),
    pain: seq(&[f(b'G', 3, Idle), f(b'G', 3, Pain)], Then::See),
    death: seq(
        &[
            f(b'H', 5, Idle),
            f(b'I', 5, Scream),
            f(b'J', 5, Fall),
            f(b'K', 5, Idle),
            f(b'L', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: Some(POSS_GIB),
    sounds: ZOMBIE_SOUNDS,
    inert: false,
};

pub const SHOTGUN_GUY: MonsterInfo = MonsterInfo {
    name: "shotgun guy",
    doomednum: 9,
    sprite: "SPOS",
    health: 30,
    painchance: 170,
    see: seq(&RUN3, Then::Loop),
    missile: Some(seq(
        &[
            f(b'E', 10, Face),
            b(b'F', 10, SPosAttack),
            f(b'E', 10, Idle),
        ],
        Then::See,
    )),
    sounds: Sounds {
        attack: "SHOTGN",
        ..ZOMBIE_SOUNDS
    },
    ..ZOMBIEMAN
};

pub const CHAINGUNNER: MonsterInfo = MonsterInfo {
    name: "chaingunner",
    doomednum: 65,
    sprite: "CPOS",
    health: 70,
    painchance: 170,
    see: seq(&RUN3, Then::Loop),
    missile: Some(seq(
        &[
            f(b'E', 10, Face),
            b(b'F', 4, CPosAttack),
            b(b'E', 4, CPosAttack),
            f(b'F', 1, CPosRefire),
        ],
        Then::Goto(1),
    )),
    death: seq(
        &[
            f(b'H', 5, Idle),
            f(b'I', 5, Scream),
            f(b'J', 5, Fall),
            f(b'K', 5, Idle),
            f(b'L', 5, Idle),
            f(b'M', 5, Idle),
            f(b'N', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: Some(seq(
        &[
            f(b'O', 5, Idle),
            f(b'P', 5, XScream),
            f(b'Q', 5, Fall),
            f(b'R', 5, Idle),
            f(b'S', 5, Idle),
            f(b'T', -1, Idle),
        ],
        Then::Loop,
    )),
    sounds: Sounds {
        see: &["POSIT2", "POSIT3"],
        attack: "SHOTGN",
        ..ZOMBIE_SOUNDS
    },
    ..ZOMBIEMAN
};

pub const WOLF_SS: MonsterInfo = MonsterInfo {
    name: "wolfenstein ss",
    doomednum: 84,
    sprite: "SSWV",
    health: 50,
    painchance: 170,
    see: seq(&RUN3, Then::Loop),
    missile: Some(seq(
        &[
            f(b'E', 10, Face),
            f(b'F', 10, Face),
            b(b'G', 4, CPosAttack),
            f(b'F', 6, Face),
            b(b'G', 4, CPosAttack),
            f(b'F', 1, CPosRefire),
        ],
        Then::Goto(1),
    )),
    pain: seq(&[f(b'H', 3, Idle), f(b'H', 3, Pain)], Then::See),
    death: seq(
        &[
            f(b'I', 5, Idle),
            f(b'J', 5, Scream),
            f(b'K', 5, Fall),
            f(b'L', 5, Idle),
            f(b'M', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: Some(seq(
        &[
            f(b'N', 5, Idle),
            f(b'O', 5, XScream),
            f(b'P', 5, Fall),
            f(b'Q', 5, Idle),
            f(b'R', 5, Idle),
            f(b'S', 5, Idle),
            f(b'T', 5, Idle),
            f(b'U', 5, Idle),
            f(b'V', -1, Idle),
        ],
        Then::Loop,
    )),
    sounds: Sounds {
        see: &["SSSIT"],
        attack: "SHOTGN",
        death: &["SSDTH"],
        ..ZOMBIE_SOUNDS
    },
    ..ZOMBIEMAN
};

const IMP_ATTACK: Seq = seq(
    &[f(b'E', 8, Face), f(b'F', 8, Face), f(b'G', 6, TroopAttack)],
    Then::See,
);

pub const IMP: MonsterInfo = MonsterInfo {
    name: "imp",
    doomednum: 3001,
    sprite: "TROO",
    health: 60,
    painchance: 200,
    see: seq(&RUN3, Then::Loop),
    melee: Some(IMP_ATTACK),
    missile: Some(IMP_ATTACK),
    pain: seq(&[f(b'H', 2, Idle), f(b'H', 2, Pain)], Then::See),
    death: seq(
        &[
            f(b'I', 8, Idle),
            f(b'J', 8, Scream),
            f(b'K', 6, Idle),
            f(b'L', 6, Fall),
            f(b'M', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: Some(seq(
        &[
            f(b'N', 5, Idle),
            f(b'O', 5, XScream),
            f(b'P', 5, Idle),
            f(b'Q', 5, Fall),
            f(b'R', 5, Idle),
            f(b'S', 5, Idle),
            f(b'T', 5, Idle),
            f(b'U', -1, Idle),
        ],
        Then::Loop,
    )),
    sounds: Sounds {
        see: &["BGSIT1", "BGSIT2"],
        attack: "CLAW",
        pain: "POPAIN",
        death: &["BGDTH1", "BGDTH2"],
        active: "BGACT",
    },
    ..ZOMBIEMAN
};

pub const DEMON: MonsterInfo = MonsterInfo {
    name: "demon",
    doomednum: 3002,
    sprite: "SARG",
    health: 150,
    speed: 10.0,
    radius: 30.0,
    mass: 400.0,
    painchance: 180,
    see: seq(&RUN2, Then::Loop),
    melee: Some(seq(
        &[f(b'E', 8, Face), f(b'F', 8, Face), f(b'G', 8, SargAttack)],
        Then::See,
    )),
    missile: None,
    pain: seq(&[f(b'H', 2, Idle), f(b'H', 2, Pain)], Then::See),
    death: seq(
        &[
            f(b'I', 8, Idle),
            f(b'J', 8, Scream),
            f(b'K', 4, Idle),
            f(b'L', 4, Fall),
            f(b'M', 4, Idle),
            f(b'N', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["SGTSIT"],
        attack: "SGTATK",
        pain: "DMPAIN",
        death: &["SGTDTH"],
        active: "DMACT",
    },
    ..ZOMBIEMAN
};

pub const SPECTRE: MonsterInfo = MonsterInfo {
    name: "spectre",
    doomednum: 58,
    shadow: true,
    ..DEMON
};

const BRUISER_ATTACK: Seq = seq(
    &[f(b'E', 8, Face), f(b'F', 8, Face), f(b'G', 8, BruisAttack)],
    Then::See,
);

pub const BARON: MonsterInfo = MonsterInfo {
    name: "baron of hell",
    doomednum: 3003,
    sprite: "BOSS",
    health: 1000,
    speed: 8.0,
    radius: 24.0,
    height: 64.0,
    mass: 1000.0,
    painchance: 50,
    see: seq(&RUN3, Then::Loop),
    melee: Some(BRUISER_ATTACK),
    missile: Some(BRUISER_ATTACK),
    pain: seq(&[f(b'H', 2, Idle), f(b'H', 2, Pain)], Then::See),
    death: seq(
        &[
            f(b'I', 8, Idle),
            f(b'J', 8, Scream),
            f(b'K', 8, Idle),
            f(b'L', 8, Fall),
            f(b'M', 8, Idle),
            f(b'N', 8, Idle),
            f(b'O', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["BRSSIT"],
        attack: "CLAW",
        pain: "DMPAIN",
        death: &["BRSDTH"],
        active: "DMACT",
    },
    ..ZOMBIEMAN
};

pub const HELL_KNIGHT: MonsterInfo = MonsterInfo {
    name: "hell knight",
    doomednum: 69,
    sprite: "BOS2",
    health: 500,
    sounds: Sounds {
        see: &["KNTSIT"],
        death: &["KNTDTH"],
        ..BARON.sounds
    },
    ..BARON
};

pub const LOST_SOUL: MonsterInfo = MonsterInfo {
    name: "lost soul",
    doomednum: 3006,
    sprite: "SKUL",
    health: 100,
    speed: 8.0,
    radius: 16.0,
    height: 56.0,
    mass: 50.0,
    painchance: 256,
    float: true,
    damage: 3,
    spawn: seq(&[b(b'A', 10, Look), b(b'B', 10, Look)], Then::Loop),
    see: seq(&[b(b'A', 6, Chase), b(b'B', 6, Chase)], Then::Loop),
    melee: None,
    missile: Some(seq(
        &[
            b(b'C', 10, Face),
            b(b'D', 4, SkullAttack),
            b(b'C', 4, Idle),
            b(b'D', 4, Idle),
        ],
        Then::Goto(2),
    )),
    pain: seq(&[b(b'E', 3, Idle), b(b'E', 3, Pain)], Then::See),
    death: seq(
        &[
            b(b'F', 6, Idle),
            b(b'G', 6, Scream),
            b(b'H', 6, Idle),
            b(b'I', 6, Fall),
            f(b'J', 6, Idle),
            f(b'K', 6, Idle),
        ],
        Then::Remove,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &[],
        attack: "SKLATK",
        pain: "DMPAIN",
        death: &["FIRXPL"],
        active: "DMACT",
    },
    ..ZOMBIEMAN
};

pub const CACODEMON: MonsterInfo = MonsterInfo {
    name: "cacodemon",
    doomednum: 3005,
    sprite: "HEAD",
    health: 400,
    speed: 8.0,
    radius: 31.0,
    height: 56.0,
    mass: 400.0,
    painchance: 128,
    float: true,
    spawn: seq(&[f(b'A', 10, Look)], Then::Loop),
    see: seq(&[f(b'A', 3, Chase)], Then::Loop),
    melee: Some(CACO_ATTACK),
    missile: Some(CACO_ATTACK),
    pain: seq(
        &[f(b'E', 3, Idle), f(b'E', 3, Pain), f(b'F', 6, Idle)],
        Then::See,
    ),
    death: seq(
        &[
            f(b'G', 8, Idle),
            f(b'H', 8, Scream),
            f(b'I', 8, Idle),
            f(b'J', 8, Idle),
            f(b'K', 8, Fall),
            f(b'L', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["CACSIT"],
        attack: "FIRSHT",
        pain: "DMPAIN",
        death: &["CACDTH"],
        active: "DMACT",
    },
    ..ZOMBIEMAN
};

const CACO_ATTACK: Seq = seq(
    &[f(b'B', 5, Face), f(b'C', 5, Face), b(b'D', 5, HeadAttack)],
    Then::See,
);

pub const REVENANT: MonsterInfo = MonsterInfo {
    name: "revenant",
    doomednum: 66,
    sprite: "SKEL",
    health: 300,
    speed: 10.0,
    radius: 20.0,
    height: 56.0,
    mass: 500.0,
    painchance: 100,
    see: seq(
        &[
            f(b'A', 2, Chase),
            f(b'A', 2, Chase),
            f(b'B', 2, Chase),
            f(b'B', 2, Chase),
            f(b'C', 2, Chase),
            f(b'C', 2, Chase),
            f(b'D', 2, Chase),
            f(b'D', 2, Chase),
            f(b'E', 2, Chase),
            f(b'E', 2, Chase),
            f(b'F', 2, Chase),
            f(b'F', 2, Chase),
        ],
        Then::Loop,
    ),
    melee: Some(seq(
        &[f(b'G', 6, Face), f(b'H', 6, Face), f(b'I', 6, SkelFist)],
        Then::See,
    )),
    missile: Some(seq(
        &[
            b(b'J', 10, Face),
            f(b'K', 10, SkelMissile),
            f(b'K', 10, Face),
        ],
        Then::See,
    )),
    pain: seq(&[f(b'L', 5, Idle), f(b'L', 5, Pain)], Then::See),
    death: seq(
        &[
            f(b'L', 7, Idle),
            f(b'M', 7, Idle),
            f(b'N', 7, Scream),
            f(b'O', 7, Fall),
            f(b'P', 7, Idle),
            f(b'Q', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["SKESIT"],
        attack: "SKEATK",
        pain: "POPAIN",
        death: &["SKEDTH"],
        active: "SKEACT",
    },
    ..ZOMBIEMAN
};

pub const MANCUBUS: MonsterInfo = MonsterInfo {
    name: "mancubus",
    doomednum: 67,
    sprite: "FATT",
    health: 600,
    speed: 8.0,
    radius: 48.0,
    height: 64.0,
    mass: 1000.0,
    painchance: 80,
    spawn: seq(&[f(b'A', 15, Look), f(b'B', 15, Look)], Then::Loop),
    see: seq(
        &[
            f(b'A', 4, Chase),
            f(b'A', 4, Chase),
            f(b'B', 4, Chase),
            f(b'B', 4, Chase),
            f(b'C', 4, Chase),
            f(b'C', 4, Chase),
            f(b'D', 4, Chase),
            f(b'D', 4, Chase),
            f(b'E', 4, Chase),
            f(b'E', 4, Chase),
            f(b'F', 4, Chase),
            f(b'F', 4, Chase),
        ],
        Then::Loop,
    ),
    melee: None,
    missile: Some(seq(
        &[
            f(b'G', 20, Face),
            b(b'H', 10, FatAttack1),
            f(b'I', 5, Face),
            f(b'G', 5, Face),
            b(b'H', 10, FatAttack2),
            f(b'I', 5, Face),
            f(b'G', 5, Face),
            b(b'H', 10, FatAttack3),
            f(b'I', 5, Face),
            f(b'G', 5, Face),
        ],
        Then::See,
    )),
    pain: seq(&[f(b'J', 3, Idle), f(b'J', 3, Pain)], Then::See),
    death: seq(
        &[
            f(b'K', 6, Idle),
            f(b'L', 6, Scream),
            f(b'M', 6, Fall),
            f(b'N', 6, Idle),
            f(b'O', 6, Idle),
            f(b'P', 6, Idle),
            f(b'Q', 6, Idle),
            f(b'R', 6, Idle),
            f(b'S', 6, Idle),
            f(b'T', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["MANSIT"],
        attack: "MANATK",
        pain: "MNPAIN",
        death: &["MANDTH"],
        active: "POSACT",
    },
    ..ZOMBIEMAN
};

pub const ARACHNOTRON: MonsterInfo = MonsterInfo {
    name: "arachnotron",
    doomednum: 68,
    sprite: "BSPI",
    health: 500,
    speed: 12.0,
    radius: 64.0,
    height: 64.0,
    mass: 600.0,
    painchance: 128,
    spawn: seq(&[f(b'A', 10, Look), f(b'B', 10, Look)], Then::Loop),
    see: seq(
        &[
            f(b'A', 3, Chase),
            f(b'A', 3, Chase),
            f(b'B', 3, Chase),
            f(b'B', 3, Chase),
            f(b'C', 3, Chase),
            f(b'C', 3, Chase),
            f(b'D', 3, Chase),
            f(b'D', 3, Chase),
            f(b'E', 3, Chase),
            f(b'E', 3, Chase),
            f(b'F', 3, Chase),
            f(b'F', 3, Chase),
        ],
        Then::Loop,
    ),
    melee: None,
    missile: Some(seq(
        &[
            b(b'A', 20, Face),
            b(b'G', 4, BspiAttack),
            b(b'H', 4, Idle),
            b(b'H', 1, CPosRefire),
        ],
        Then::Goto(1),
    )),
    pain: seq(&[f(b'I', 3, Idle), f(b'I', 3, Pain)], Then::See),
    death: seq(
        &[
            f(b'J', 20, Scream),
            f(b'K', 7, Fall),
            f(b'L', 7, Idle),
            f(b'M', 7, Idle),
            f(b'N', 7, Idle),
            f(b'O', 7, Idle),
            f(b'P', -1, Idle),
        ],
        Then::Loop,
    ),
    xdeath: None,
    sounds: Sounds {
        see: &["BSPSIT"],
        attack: "PLASMA",
        pain: "DMPAIN",
        death: &["BSPDTH"],
        active: "BSPACT",
    },
    ..ZOMBIEMAN
};

pub const BARREL: MonsterInfo = MonsterInfo {
    name: "barrel",
    doomednum: 2035,
    sprite: "BAR1",
    health: 20,
    speed: 0.0,
    radius: 10.0,
    height: 42.0,
    mass: 100.0,
    painchance: 0,
    spawn: seq(&[f(b'A', 6, Idle), f(b'B', 6, Idle)], Then::Loop),
    see: seq(&[f(b'A', 6, Idle), f(b'B', 6, Idle)], Then::Loop),
    melee: None,
    missile: None,
    pain: seq(&[f(b'A', 6, Idle)], Then::Loop),
    death: Seq {
        sprite: Some("BEXP"),
        frames: &[
            b(b'A', 5, Idle),
            b(b'B', 5, Scream),
            b(b'C', 5, Idle),
            b(b'D', 10, Explode),
            b(b'E', 10, Idle),
        ],
        then: Then::Remove,
    },
    xdeath: None,
    sounds: Sounds {
        see: &[],
        attack: "",
        pain: "",
        death: &["BAREXP"],
        active: "",
    },
    inert: true,
    ..ZOMBIEMAN
};

/// Every shootable thing, by editor number.
pub const MONSTERS: &[&MonsterInfo] = &[
    &ZOMBIEMAN,
    &SHOTGUN_GUY,
    &CHAINGUNNER,
    &WOLF_SS,
    &IMP,
    &DEMON,
    &SPECTRE,
    &BARON,
    &HELL_KNIGHT,
    &LOST_SOUL,
    &CACODEMON,
    &REVENANT,
    &MANCUBUS,
    &ARACHNOTRON,
    &BARREL,
];

pub fn monster(doomednum: u16) -> Option<&'static MonsterInfo> {
    MONSTERS.iter().copied().find(|m| m.doomednum == doomednum)
}

/// A projectile monsters fire.
#[derive(Debug)]
pub struct MissileInfo {
    pub sprite: &'static str,
    pub fly: &'static [Frame],
    /// Its explosion's sprite and frames.
    pub burst_sprite: &'static str,
    pub burst: &'static [Frame],
    /// Units per tic.
    pub speed: f32,
    pub radius: f32,
    /// Damage is `(1..=8) × damage`.
    pub damage: i32,
    /// Turns toward its target as it flies: the revenant's rocket.
    pub homing: bool,
    pub sound: &'static str,
    pub burst_sound: &'static str,
    /// p_map.c `P_RadiusAttack` damage at its burst, in units: the rocket's
    /// 128, or 0 for none.
    pub blast: f32,
    /// The BFG ball: its burst sprays 40 tracers from the player.
    pub spray: bool,
}

const BALL_BURST: [Frame; 3] = [b(b'C', 6, Idle), b(b'D', 6, Idle), b(b'E', 6, Idle)];

pub const IMP_BALL: MissileInfo = MissileInfo {
    sprite: "BAL1",
    fly: &[b(b'A', 4, Idle), b(b'B', 4, Idle)],
    burst_sprite: "BAL1",
    burst: &BALL_BURST,
    speed: 10.0,
    radius: 6.0,
    damage: 3,
    homing: false,
    sound: "FIRSHT",
    burst_sound: "FIRXPL",
    blast: 0.0,
    spray: false,
};

pub const CACO_BALL: MissileInfo = MissileInfo {
    sprite: "BAL2",
    burst_sprite: "BAL2",
    damage: 5,
    ..IMP_BALL
};

pub const BARON_BALL: MissileInfo = MissileInfo {
    sprite: "BAL7",
    burst_sprite: "BAL7",
    speed: 15.0,
    damage: 8,
    ..IMP_BALL
};

pub const REVENANT_ROCKET: MissileInfo = MissileInfo {
    sprite: "FATB",
    fly: &[b(b'A', 2, Idle), b(b'B', 2, Idle)],
    burst_sprite: "FBXP",
    burst: &[b(b'A', 8, Idle), b(b'B', 6, Idle), b(b'C', 4, Idle)],
    speed: 10.0,
    radius: 11.0,
    damage: 10,
    homing: true,
    sound: "SKEATK",
    burst_sound: "BAREXP",
    blast: 0.0,
    spray: false,
};

pub const MANCUBUS_BALL: MissileInfo = MissileInfo {
    sprite: "MANF",
    fly: &[b(b'A', 4, Idle), b(b'B', 4, Idle)],
    burst_sprite: "MISL",
    burst: &[b(b'B', 8, Idle), b(b'C', 6, Idle), b(b'D', 4, Idle)],
    speed: 20.0,
    radius: 6.0,
    damage: 8,
    homing: false,
    sound: "FIRSHT",
    burst_sound: "FIRXPL",
    blast: 0.0,
    spray: false,
};

pub const PLASMA_BOLT: MissileInfo = MissileInfo {
    sprite: "APLS",
    fly: &[b(b'A', 5, Idle), b(b'B', 5, Idle)],
    burst_sprite: "APBX",
    burst: &[
        b(b'A', 5, Idle),
        b(b'B', 5, Idle),
        b(b'C', 5, Idle),
        b(b'D', 5, Idle),
        b(b'E', 5, Idle),
    ],
    speed: 25.0,
    radius: 13.0,
    damage: 5,
    homing: false,
    sound: "PLASMA",
    burst_sound: "FIRXPL",
    blast: 0.0,
    spray: false,
};

/// The player's rocket: MT_ROCKET, which A_Explode bursts for 128.
pub const ROCKET: MissileInfo = MissileInfo {
    sprite: "MISL",
    fly: &[b(b'A', 1, Idle)],
    burst_sprite: "MISL",
    burst: &[b(b'B', 8, Idle), b(b'C', 6, Idle), b(b'D', 4, Idle)],
    speed: 20.0,
    radius: 11.0,
    damage: 20,
    homing: false,
    sound: "RLAUNC",
    burst_sound: "BAREXP",
    blast: 128.0,
    spray: false,
};

/// The plasma rifle's ball: MT_PLASMA.
pub const PLASMA_BALL: MissileInfo = MissileInfo {
    sprite: "PLSS",
    fly: &[b(b'A', 6, Idle), b(b'B', 6, Idle)],
    burst_sprite: "PLSE",
    burst: &[
        b(b'A', 4, Idle),
        b(b'B', 4, Idle),
        b(b'C', 4, Idle),
        b(b'D', 4, Idle),
        b(b'E', 4, Idle),
    ],
    speed: 25.0,
    radius: 13.0,
    damage: 5,
    homing: false,
    sound: "PLASMA",
    burst_sound: "FIRXPL",
    blast: 0.0,
    spray: false,
};

/// The BFG9000's ball: MT_BFG. Its burst's third frame sprays the tracers
/// (A_BFGSpray). The weapon plays its sound as it charges.
pub const BFG_BALL: MissileInfo = MissileInfo {
    sprite: "BFS1",
    fly: &[b(b'A', 4, Idle), b(b'B', 4, Idle)],
    burst_sprite: "BFE1",
    burst: &[
        b(b'A', 8, Idle),
        b(b'B', 8, Idle),
        b(b'C', 8, Idle),
        b(b'D', 8, Idle),
        b(b'E', 8, Idle),
        b(b'F', 8, Idle),
    ],
    speed: 25.0,
    radius: 13.0,
    damage: 100,
    homing: false,
    sound: "",
    burst_sound: "RXPLOD",
    blast: 0.0,
    spray: true,
};

/// The burst a BFG tracer leaves on what it hits: MT_EXTRABFG.
pub const BFG_HIT: &[Frame] = &[
    b(b'A', 8, Idle),
    b(b'B', 8, Idle),
    b(b'C', 8, Idle),
    b(b'D', 8, Idle),
];

/// A thing that only stands, animates and maybe blocks, or is picked up.
#[derive(Debug)]
pub struct DecorInfo {
    pub doomednum: u16,
    pub sprite: &'static str,
    /// Looping.
    pub frames: &'static [Frame],
    /// Blocks bodies, with DOOM's radius 16 (20 for the tall pillars).
    pub solid: bool,
    /// Hangs from the ceiling.
    pub hanging: bool,
    /// What touching it gives, for items.
    pub pickup: Option<Pickup>,
}

/// One frame, shown forever.
macro_rules! still {
    ($letter:expr) => {
        &[f($letter, -1, Idle)]
    };
}

/// A bright four-frame flicker, A B C D.
const FLICKER: &[Frame] = &[
    b(b'A', 4, Idle),
    b(b'B', 4, Idle),
    b(b'C', 4, Idle),
    b(b'D', 4, Idle),
];
/// A bright pulse, A B C D C B.
const PULSE: &[Frame] = &[
    b(b'A', 6, Idle),
    b(b'B', 6, Idle),
    b(b'C', 6, Idle),
    b(b'D', 6, Idle),
    b(b'C', 6, Idle),
    b(b'B', 6, Idle),
];
const BLINK: &[Frame] = &[f(b'A', 10, Idle), b(b'B', 10, Idle)];

const fn decor(
    doomednum: u16,
    sprite: &'static str,
    frames: &'static [Frame],
    solid: bool,
) -> DecorInfo {
    DecorInfo {
        doomednum,
        sprite,
        frames,
        solid,
        hanging: false,
        pickup: Option::None,
    }
}

const fn hanging(
    doomednum: u16,
    sprite: &'static str,
    frames: &'static [Frame],
    solid: bool,
) -> DecorInfo {
    DecorInfo {
        hanging: true,
        ..decor(doomednum, sprite, frames, solid)
    }
}

/// An item that is picked up.
const fn item(
    doomednum: u16,
    sprite: &'static str,
    frames: &'static [Frame],
    pickup: Pickup,
) -> DecorInfo {
    DecorInfo {
        pickup: Some(pickup),
        ..decor(doomednum, sprite, frames, false)
    }
}

const fn health(
    doomednum: u16,
    sprite: &'static str,
    frames: &'static [Frame],
    gives: i32,
    up_to: i32,
    message: &'static str,
) -> DecorInfo {
    item(
        doomednum,
        sprite,
        frames,
        Pickup::Health {
            gives,
            up_to,
            message,
        },
    )
}

/// Frame A, shown forever, for items built by the helpers below.
const STILL_A: &[Frame] = still!(b'A');

const fn ammo(
    doomednum: u16,
    sprite: &'static str,
    kind: AmmoKind,
    clips: i32,
    message: &'static str,
) -> DecorInfo {
    item(
        doomednum,
        sprite,
        STILL_A,
        Pickup::Ammo {
            kind,
            clips,
            message,
        },
    )
}

const fn weapon(doomednum: u16, sprite: &'static str, kind: WeaponKind) -> DecorInfo {
    item(doomednum, sprite, STILL_A, Pickup::Weapon(kind))
}

const fn key(doomednum: u16, sprite: &'static str, key: Key) -> DecorInfo {
    item(doomednum, sprite, BLINK, Pickup::Key(key))
}

/// Decorations and items, by editor number.
pub const DECOR: &[DecorInfo] = &[
    // Items picked up.
    health(2011, "STIM", still!(b'A'), 10, 100, "Picked up a stimpack."),
    health(2012, "MEDI", still!(b'A'), 25, 100, "Picked up a medikit."),
    health(2014, "BON1", PULSE, 1, 200, "Picked up a health bonus."),
    item(2023, "PSTR", &[b(b'A', -1, Idle)], Pickup::Berserk),
    key(5, "BKEY", Key::BlueCard),
    key(6, "YKEY", Key::YellowCard),
    key(13, "RKEY", Key::RedCard),
    key(40, "BSKU", Key::BlueSkull),
    key(39, "YSKU", Key::YellowSkull),
    key(38, "RSKU", Key::RedSkull),
    weapon(2001, "SHOT", WeaponKind::Shotgun),
    weapon(82, "SGN2", WeaponKind::SuperShotgun),
    weapon(2002, "MGUN", WeaponKind::Chaingun),
    weapon(2003, "LAUN", WeaponKind::Missile),
    weapon(2004, "PLAS", WeaponKind::Plasma),
    weapon(2006, "BFUG", WeaponKind::Bfg),
    weapon(2005, "CSAW", WeaponKind::Chainsaw),
    ammo(2007, "CLIP", AmmoKind::Bullets, 1, "Picked up a clip."),
    ammo(
        2048,
        "AMMO",
        AmmoKind::Bullets,
        5,
        "Picked up a box of bullets.",
    ),
    ammo(
        2008,
        "SHEL",
        AmmoKind::Shells,
        1,
        "Picked up 4 shotgun shells.",
    ),
    ammo(
        2049,
        "SBOX",
        AmmoKind::Shells,
        5,
        "Picked up a box of shotgun shells.",
    ),
    ammo(2010, "ROCK", AmmoKind::Rockets, 1, "Picked up a rocket."),
    ammo(
        2046,
        "BROK",
        AmmoKind::Rockets,
        5,
        "Picked up a box of rockets.",
    ),
    ammo(
        2047,
        "CELL",
        AmmoKind::Cells,
        1,
        "Picked up an energy cell.",
    ),
    ammo(
        17,
        "CELP",
        AmmoKind::Cells,
        5,
        "Picked up an energy cell pack.",
    ),
    item(8, "BPAK", still!(b'A'), Pickup::Backpack),
    // Other items, shown only.
    decor(2015, "BON2", PULSE, false),
    decor(2018, "ARM1", BLINK, false),
    decor(2019, "ARM2", BLINK, false),
    decor(2013, "SOUL", PULSE, false),
    decor(2022, "PINV", FLICKER, false),
    decor(2024, "PINS", FLICKER, false),
    decor(2025, "SUIT", &[b(b'A', -1, Idle)], false),
    decor(2026, "PMAP", PULSE, false),
    decor(2045, "PVIS", BLINK, false),
    decor(83, "MEGA", FLICKER, false),
    // Decorations.
    decor(2028, "COLU", &[b(b'A', -1, Idle)], true),
    decor(30, "COL1", still!(b'A'), true),
    decor(31, "COL2", still!(b'A'), true),
    decor(32, "COL3", still!(b'A'), true),
    decor(33, "COL4", still!(b'A'), true),
    decor(36, "COL5", &[f(b'A', 14, Idle), f(b'B', 14, Idle)], true),
    decor(37, "COL6", still!(b'A'), true),
    decor(
        41,
        "CEYE",
        &[
            b(b'A', 6, Idle),
            b(b'B', 6, Idle),
            b(b'C', 6, Idle),
            b(b'B', 6, Idle),
        ],
        true,
    ),
    decor(
        42,
        "FSKU",
        &[b(b'A', 6, Idle), b(b'B', 6, Idle), b(b'C', 6, Idle)],
        true,
    ),
    decor(43, "TRE1", still!(b'A'), true),
    decor(54, "TRE2", still!(b'A'), true),
    decor(44, "TBLU", FLICKER, true),
    decor(45, "TGRN", FLICKER, true),
    decor(46, "TRED", FLICKER, true),
    decor(55, "SMBT", FLICKER, true),
    decor(56, "SMGT", FLICKER, true),
    decor(57, "SMRT", FLICKER, true),
    decor(47, "SMIT", still!(b'A'), true),
    decor(48, "ELEC", still!(b'A'), true),
    decor(34, "CAND", &[b(b'A', -1, Idle)], false),
    decor(35, "CBRA", &[b(b'A', -1, Idle)], true),
    decor(
        70,
        "FCAN",
        &[b(b'A', 4, Idle), b(b'B', 4, Idle), b(b'C', 4, Idle)],
        true,
    ),
    decor(85, "TLMP", FLICKER, true),
    decor(86, "TLP2", FLICKER, true),
    decor(24, "POL5", still!(b'A'), false),
    decor(25, "POL1", still!(b'A'), true),
    decor(26, "POL6", &[f(b'A', 6, Idle), f(b'B', 8, Idle)], true),
    decor(27, "POL4", still!(b'A'), true),
    decor(28, "POL2", still!(b'A'), true),
    decor(29, "POL3", &[b(b'A', 6, Idle), b(b'B', 6, Idle)], true),
    decor(79, "POB1", still!(b'A'), false),
    decor(80, "POB2", still!(b'A'), false),
    decor(81, "BRS1", still!(b'A'), false),
    // Dead things.
    decor(15, "PLAY", still!(b'N'), false),
    decor(18, "POSS", still!(b'L'), false),
    decor(19, "SPOS", still!(b'L'), false),
    decor(20, "TROO", still!(b'M'), false),
    decor(21, "SARG", still!(b'N'), false),
    decor(22, "HEAD", still!(b'L'), false),
    decor(10, "PLAY", still!(b'W'), false),
    decor(12, "PLAY", still!(b'W'), false),
    // Hanging from the ceiling.
    hanging(
        49,
        "GOR1",
        &[
            f(b'A', 10, Idle),
            f(b'B', 15, Idle),
            f(b'C', 8, Idle),
            f(b'B', 6, Idle),
        ],
        true,
    ),
    hanging(50, "GOR2", still!(b'A'), true),
    hanging(51, "GOR3", still!(b'A'), true),
    hanging(52, "GOR4", still!(b'A'), true),
    hanging(53, "GOR5", still!(b'A'), true),
    hanging(59, "GOR2", still!(b'A'), false),
    hanging(60, "GOR4", still!(b'A'), false),
    hanging(61, "GOR3", still!(b'A'), false),
    hanging(62, "GOR5", still!(b'A'), false),
    hanging(
        63,
        "GOR1",
        &[
            f(b'A', 10, Idle),
            f(b'B', 15, Idle),
            f(b'C', 8, Idle),
            f(b'B', 6, Idle),
        ],
        false,
    ),
    hanging(73, "HDB1", still!(b'A'), true),
    hanging(74, "HDB2", still!(b'A'), true),
    hanging(75, "HDB3", still!(b'A'), true),
    hanging(76, "HDB4", still!(b'A'), true),
    hanging(77, "HDB5", still!(b'A'), true),
    hanging(78, "HDB6", still!(b'A'), true),
    // Monsters this app doesn't play, drawn standing.
    decor(64, "VILE", still!(b'A'), true),
    decor(71, "PAIN", still!(b'A'), true),
    decor(7, "SPID", still!(b'A'), true),
    decor(16, "CYBR", still!(b'A'), true),
    hanging(72, "KEEN", still!(b'A'), true),
    decor(88, "BBRN", still!(b'A'), true),
];

pub fn decor_info(doomednum: u16) -> Option<&'static DecorInfo> {
    DECOR.iter().find(|d| d.doomednum == doomednum)
}

/// The item a monster drops when it dies, by editor number: p_inter.c
/// `P_KillMobj`'s clip, shotgun and chaingun.
pub fn drop_of(doomednum: u16) -> Option<&'static DecorInfo> {
    let item = match doomednum {
        3004 | 84 => 2007,
        9 => 2001,
        65 => 2002,
        _ => return None,
    };
    decor_info(item)
}

/// The bullet puff and blood, shown where hitscans land.
pub const PUFF: &[Frame] = &[
    b(b'A', 4, Idle),
    f(b'B', 4, Idle),
    f(b'C', 4, Idle),
    f(b'D', 4, Idle),
];
pub const BLOOD: &[Frame] = &[f(b'C', 8, Idle), f(b'B', 8, Idle), f(b'A', 8, Idle)];

/// What a weapon frame does as it starts: p_pspr.c's action functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WAction {
    None,
    /// A_WeaponReady: lower for a switch or death, fire on the trigger.
    Ready,
    /// A_ReFire: fire again while the trigger is held.
    ReFire,
    /// A_GunFlash: the flash, before the rocket or the BFG's ball.
    GunFlash,
    Punch,
    Saw,
    Pistol,
    Shotgun,
    SuperShotgun,
    /// Lowers the super shotgun when it hasn't two shells.
    CheckReload,
    OpenShotgun2,
    LoadShotgun2,
    /// Its sound, then A_ReFire.
    CloseShotgun2,
    ChainGun,
    Missile,
    Plasma,
    BfgSound,
    Bfg,
}

/// A frame of the weapon in hand, or of its flash.
#[derive(Clone, Copy, Debug)]
pub struct WFrame {
    pub letter: u8,
    pub tics: i32,
    pub bright: bool,
    pub action: WAction,
}

const fn w(letter: u8, tics: i32, action: WAction) -> WFrame {
    WFrame {
        letter,
        tics,
        bright: false,
        action,
    }
}

/// A full-bright flash frame.
const fn wf(letter: u8, tics: i32) -> WFrame {
    WFrame {
        letter,
        tics,
        bright: true,
        action: WAction::None,
    }
}

/// A weapon as the player holds it: its frames from info.c. Raising and
/// lowering show the first ready frame.
#[derive(Debug)]
pub struct WeaponInfo {
    pub sprite: &'static str,
    pub flash_sprite: &'static str,
    /// Looping while it waits for the trigger.
    pub ready: &'static [WFrame],
    /// From the trigger back to `ready`; A_ReFire jumps to its start.
    pub attack: &'static [WFrame],
    /// Muzzle flashes, which firing starts: the chaingun's by its frame,
    /// the plasma rifle's at random.
    pub flashes: &'static [&'static [WFrame]],
}

const fn ready_one(letter: u8) -> [WFrame; 1] {
    [w(letter, 1, WAction::Ready)]
}

pub const FIST: WeaponInfo = WeaponInfo {
    sprite: "PUNG",
    flash_sprite: "PUNG",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'B', 4, WAction::None),
        w(b'C', 4, WAction::Punch),
        w(b'D', 5, WAction::None),
        w(b'C', 4, WAction::None),
        w(b'B', 5, WAction::ReFire),
    ],
    flashes: &[],
};

pub const PISTOL: WeaponInfo = WeaponInfo {
    sprite: "PISG",
    flash_sprite: "PISF",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'A', 4, WAction::None),
        w(b'B', 6, WAction::Pistol),
        w(b'C', 4, WAction::None),
        w(b'B', 5, WAction::ReFire),
    ],
    flashes: &[&[wf(b'A', 7)]],
};

pub const SHOTGUN: WeaponInfo = WeaponInfo {
    sprite: "SHTG",
    flash_sprite: "SHTF",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'A', 3, WAction::None),
        w(b'A', 7, WAction::Shotgun),
        w(b'B', 5, WAction::None),
        w(b'C', 5, WAction::None),
        w(b'D', 4, WAction::None),
        w(b'C', 5, WAction::None),
        w(b'B', 5, WAction::None),
        w(b'A', 3, WAction::None),
        w(b'A', 7, WAction::ReFire),
    ],
    flashes: &[&[wf(b'A', 4), wf(b'B', 3)]],
};

pub const SUPER_SHOTGUN: WeaponInfo = WeaponInfo {
    sprite: "SHT2",
    flash_sprite: "SHT2",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'A', 3, WAction::None),
        w(b'A', 7, WAction::SuperShotgun),
        w(b'B', 7, WAction::None),
        w(b'C', 7, WAction::CheckReload),
        w(b'D', 7, WAction::OpenShotgun2),
        w(b'E', 7, WAction::None),
        w(b'F', 7, WAction::LoadShotgun2),
        w(b'G', 6, WAction::None),
        w(b'H', 6, WAction::CloseShotgun2),
        w(b'A', 5, WAction::None),
    ],
    flashes: &[&[wf(b'I', 5), wf(b'J', 4)]],
};

pub const CHAINGUN: WeaponInfo = WeaponInfo {
    sprite: "CHGG",
    flash_sprite: "CHGF",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'A', 4, WAction::ChainGun),
        w(b'B', 4, WAction::ChainGun),
        w(b'B', 0, WAction::ReFire),
    ],
    flashes: &[&[wf(b'A', 5)], &[wf(b'B', 5)]],
};

pub const ROCKET_LAUNCHER: WeaponInfo = WeaponInfo {
    sprite: "MISG",
    flash_sprite: "MISF",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'B', 8, WAction::GunFlash),
        w(b'B', 12, WAction::Missile),
        w(b'B', 0, WAction::ReFire),
    ],
    flashes: &[&[wf(b'A', 3), wf(b'B', 4), wf(b'C', 4), wf(b'D', 4)]],
};

pub const PLASMA_RIFLE: WeaponInfo = WeaponInfo {
    sprite: "PLSG",
    flash_sprite: "PLSF",
    ready: &ready_one(b'A'),
    attack: &[w(b'A', 3, WAction::Plasma), w(b'B', 20, WAction::ReFire)],
    flashes: &[&[wf(b'A', 4)], &[wf(b'B', 4)]],
};

pub const BFG9000: WeaponInfo = WeaponInfo {
    sprite: "BFGG",
    flash_sprite: "BFGF",
    ready: &ready_one(b'A'),
    attack: &[
        w(b'A', 20, WAction::BfgSound),
        w(b'B', 10, WAction::GunFlash),
        w(b'B', 10, WAction::Bfg),
        w(b'B', 20, WAction::ReFire),
    ],
    flashes: &[&[wf(b'A', 11), wf(b'B', 6)]],
};

pub const CHAINSAW: WeaponInfo = WeaponInfo {
    sprite: "SAWG",
    flash_sprite: "SAWG",
    ready: &[w(b'C', 4, WAction::Ready), w(b'D', 4, WAction::Ready)],
    attack: &[
        w(b'A', 4, WAction::Saw),
        w(b'B', 4, WAction::Saw),
        w(b'B', 0, WAction::ReFire),
    ],
    flashes: &[],
};

pub fn weapon_info(kind: WeaponKind) -> &'static WeaponInfo {
    match kind {
        WeaponKind::Fist => &FIST,
        WeaponKind::Pistol => &PISTOL,
        WeaponKind::Shotgun => &SHOTGUN,
        WeaponKind::Chaingun => &CHAINGUN,
        WeaponKind::Missile => &ROCKET_LAUNCHER,
        WeaponKind::Plasma => &PLASMA_RIFLE,
        WeaponKind::Bfg => &BFG9000,
        WeaponKind::Chainsaw => &CHAINSAW,
        WeaponKind::SuperShotgun => &SUPER_SHOTGUN,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_jump_inside_themselves_and_only_corpses_stay() {
        for m in MONSTERS {
            for kind in [
                SeqKind::Spawn,
                SeqKind::See,
                SeqKind::Melee,
                SeqKind::Missile,
                SeqKind::Pain,
                SeqKind::Death,
                SeqKind::XDeath,
            ] {
                let s = m.seq(kind);
                assert!(!s.frames.is_empty(), "{} {kind:?}", m.name);
                if let Then::Goto(n) = s.then {
                    assert!(n < s.frames.len(), "{} {kind:?}", m.name);
                }
                let forever = s.frames.iter().position(|f| f.tics < 0);
                match kind {
                    SeqKind::Death | SeqKind::XDeath => {
                        assert!(
                            forever == Some(s.frames.len() - 1) || s.then == Then::Remove,
                            "{} {kind:?}",
                            m.name
                        );
                    }
                    _ => assert_eq!(forever, Option::None, "{} {kind:?}", m.name),
                }
            }
            assert!(
                m.inert || m.see.frames.iter().any(|f| f.action == Chase),
                "{}",
                m.name
            );
        }
        let mut numbers: Vec<u16> = MONSTERS.iter().map(|m| m.doomednum).collect();
        numbers.extend(DECOR.iter().map(|d| d.doomednum));
        let count = numbers.len();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), count, "an editor number is listed twice");
    }

    #[test]
    fn weapons_fire_once_a_cycle_and_come_back_to_ready() {
        for kind in WeaponKind::ALL {
            let info = weapon_info(kind);
            assert!(info
                .ready
                .iter()
                .all(|f| f.action == WAction::Ready && f.tics > 0));
            let fires = info
                .attack
                .iter()
                .filter(|f| {
                    !matches!(
                        f.action,
                        WAction::None
                            | WAction::ReFire
                            | WAction::GunFlash
                            | WAction::BfgSound
                            | WAction::CheckReload
                            | WAction::OpenShotgun2
                            | WAction::LoadShotgun2
                            | WAction::CloseShotgun2
                    )
                })
                .count();
            let want = if kind == WeaponKind::Chaingun || kind == WeaponKind::Chainsaw {
                2
            } else {
                1
            };
            assert_eq!(fires, want, "{kind:?}");
            assert!(
                info.attack.iter().any(|f| matches!(
                    f.action,
                    WAction::ReFire | WAction::CloseShotgun2
                )),
                "{kind:?}"
            );
            assert!(info.attack.iter().any(|f| f.tics > 0), "{kind:?}");
        }
        // DOOM's rates held: the pistol every 19 tics, the chaingun two
        // shots in 8, the shotgun every 44.
        let cycle = |w: &WeaponInfo| w.attack.iter().map(|f| f.tics).sum::<i32>();
        assert_eq!(cycle(&PISTOL), 19);
        assert_eq!(cycle(&CHAINGUN), 8);
        assert_eq!(cycle(&SHOTGUN), 44);
    }
}
