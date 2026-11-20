//! What the player carries: weapons, ammo, the backpack, keys and berserk,
//! and the rules for picking things up, from p_inter.c. No Bevy here.
//!
//! The player starts as DOOM's does, with the fist, the pistol and 50
//! bullets. Picking up a weapon gives two clips of its ammo (one when a
//! zombie dropped it) and, if it is new, switches to it. Ammo stops at
//! DOOM's limits, twice as high with the backpack; an item the player has
//! no room for stays where it is. Picking up ammo of a kind the player had
//! none of switches from the fist or pistol to a weapon that fires it, as
//! `P_GiveAmmo` does, and running dry switches to the best weapon that
//! still has ammo, as `P_CheckAmmo` does.

/// DOOM's weapons, in `weapontype_t` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    Fist,
    Pistol,
    Shotgun,
    Chaingun,
    Missile,
    Plasma,
    Bfg,
    Chainsaw,
    SuperShotgun,
}

impl WeaponKind {
    pub const ALL: [WeaponKind; 9] = [
        WeaponKind::Fist,
        WeaponKind::Pistol,
        WeaponKind::Shotgun,
        WeaponKind::Chaingun,
        WeaponKind::Missile,
        WeaponKind::Plasma,
        WeaponKind::Bfg,
        WeaponKind::Chainsaw,
        WeaponKind::SuperShotgun,
    ];

    /// The order the mouse wheel steps through: by slot.
    pub const BY_SLOT: [WeaponKind; 9] = [
        WeaponKind::Fist,
        WeaponKind::Chainsaw,
        WeaponKind::Pistol,
        WeaponKind::Shotgun,
        WeaponKind::SuperShotgun,
        WeaponKind::Chaingun,
        WeaponKind::Missile,
        WeaponKind::Plasma,
        WeaponKind::Bfg,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// The name the command line uses.
    pub fn name(self) -> &'static str {
        match self {
            WeaponKind::Fist => "fist",
            WeaponKind::Pistol => "pistol",
            WeaponKind::Shotgun => "shotgun",
            WeaponKind::Chaingun => "chaingun",
            WeaponKind::Missile => "rocket",
            WeaponKind::Plasma => "plasma",
            WeaponKind::Bfg => "bfg",
            WeaponKind::Chainsaw => "chainsaw",
            WeaponKind::SuperShotgun => "ssg",
        }
    }

    pub fn parse(name: &str) -> Option<WeaponKind> {
        WeaponKind::ALL.into_iter().find(|w| w.name() == name)
    }

    /// The number key that selects it.
    pub fn slot(self) -> u8 {
        match self {
            WeaponKind::Fist | WeaponKind::Chainsaw => 1,
            WeaponKind::Pistol => 2,
            WeaponKind::Shotgun | WeaponKind::SuperShotgun => 3,
            WeaponKind::Chaingun => 4,
            WeaponKind::Missile => 5,
            WeaponKind::Plasma => 6,
            WeaponKind::Bfg => 7,
        }
    }

    pub fn ammo(self) -> Option<AmmoKind> {
        match self {
            WeaponKind::Fist | WeaponKind::Chainsaw => None,
            WeaponKind::Pistol | WeaponKind::Chaingun => Some(AmmoKind::Bullets),
            WeaponKind::Shotgun | WeaponKind::SuperShotgun => Some(AmmoKind::Shells),
            WeaponKind::Missile => Some(AmmoKind::Rockets),
            WeaponKind::Plasma | WeaponKind::Bfg => Some(AmmoKind::Cells),
        }
    }

    /// Ammo one shot takes: p_pspr.c `BFGCELLS`, and the super shotgun's two.
    pub fn per_shot(self) -> i32 {
        match self {
            WeaponKind::Bfg => 40,
            WeaponKind::SuperShotgun => 2,
            _ => 1,
        }
    }

    /// What picking it up says.
    fn message(self) -> &'static str {
        match self {
            WeaponKind::Fist => "Fists!",
            WeaponKind::Pistol => "Picked up a pistol.",
            WeaponKind::Shotgun => "You got the shotgun!",
            WeaponKind::Chaingun => "You got the chaingun!",
            WeaponKind::Missile => "You got the rocket launcher!",
            WeaponKind::Plasma => "You got the plasma gun!",
            WeaponKind::Bfg => "You got the BFG9000!",
            WeaponKind::Chainsaw => "A chainsaw!",
            WeaponKind::SuperShotgun => "You got the super shotgun!",
        }
    }
}

/// The four kinds of ammo, in the status bar's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AmmoKind {
    Bullets,
    Shells,
    Rockets,
    Cells,
}

impl AmmoKind {
    pub const ALL: [AmmoKind; 4] = [
        AmmoKind::Bullets,
        AmmoKind::Shells,
        AmmoKind::Rockets,
        AmmoKind::Cells,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// p_inter.c `clipammo`: what one clip holds.
    pub fn clip(self) -> i32 {
        [10, 4, 1, 20][self.index()]
    }

    /// p_inter.c `maxammo`, without the backpack.
    pub fn max(self) -> i32 {
        [200, 50, 50, 300][self.index()]
    }

    /// Its status bar label.
    pub fn label(self) -> &'static str {
        ["BULL", "SHEL", "RCKT", "CELL"][self.index()]
    }
}

/// A key colour, which doors ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyColour {
    Blue,
    Yellow,
    Red,
}

impl KeyColour {
    pub fn name(self) -> &'static str {
        match self {
            KeyColour::Blue => "blue",
            KeyColour::Yellow => "yellow",
            KeyColour::Red => "red",
        }
    }
}

/// DOOM's six keys, in `card_t` order: the key cards, then the skull keys.
/// Either key of a colour opens its doors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    BlueCard,
    YellowCard,
    RedCard,
    BlueSkull,
    YellowSkull,
    RedSkull,
}

impl Key {
    pub const ALL: [Key; 6] = [
        Key::BlueCard,
        Key::YellowCard,
        Key::RedCard,
        Key::BlueSkull,
        Key::YellowSkull,
        Key::RedSkull,
    ];

    pub fn colour(self) -> KeyColour {
        match self {
            Key::BlueCard | Key::BlueSkull => KeyColour::Blue,
            Key::YellowCard | Key::YellowSkull => KeyColour::Yellow,
            Key::RedCard | Key::RedSkull => KeyColour::Red,
        }
    }

    /// Its item's sprite, which the HUD shows too.
    pub fn sprite(self) -> &'static str {
        ["BKEY", "YKEY", "RKEY", "BSKU", "YSKU", "RSKU"][self as usize]
    }
}

/// What an item gives when touched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pickup {
    /// `gives` health up to `up_to`.
    Health {
        gives: i32,
        up_to: i32,
        message: &'static str,
    },
    Weapon(WeaponKind),
    /// `clips` clips of ammo, half a clip when dropped.
    Ammo {
        kind: AmmoKind,
        clips: i32,
        message: &'static str,
    },
    Backpack,
    Key(Key),
    /// Health up to 100 and a fist ten times as strong.
    Berserk,
}

/// What a pickup says and sounds like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picked {
    pub message: &'static str,
    /// A `DS` lump name.
    pub sound: &'static str,
}

/// What the player carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inventory {
    owned: [bool; 9],
    ammo: [i32; 4],
    pub backpack: bool,
    keys: [bool; 6],
    /// The berserk pack's strength: the fist does ten times the damage.
    pub berserk: bool,
    /// The weapon in hand, and the one it is switching to.
    pub ready: WeaponKind,
    pub pending: Option<WeaponKind>,
}

impl Default for Inventory {
    /// DOOM's pistol start: the fist, the pistol and 50 bullets.
    fn default() -> Self {
        let mut inv = Inventory {
            owned: [false; 9],
            ammo: [0; 4],
            backpack: false,
            keys: [false; 6],
            berserk: false,
            ready: WeaponKind::Pistol,
            pending: None,
        };
        inv.owned[WeaponKind::Fist.index()] = true;
        inv.owned[WeaponKind::Pistol.index()] = true;
        inv.ammo[AmmoKind::Bullets.index()] = 50;
        inv
    }
}

impl Inventory {
    pub fn has(&self, weapon: WeaponKind) -> bool {
        self.owned[weapon.index()]
    }

    /// Gives `weapon` without ammo or a switch, for the command line.
    pub fn add(&mut self, weapon: WeaponKind) {
        self.owned[weapon.index()] = true;
    }

    /// Takes `weapon` away, for weapons the WAD has no sprites for. The fist
    /// stays.
    pub fn remove(&mut self, weapon: WeaponKind) {
        if weapon == WeaponKind::Fist {
            return;
        }
        self.owned[weapon.index()] = false;
        if self.ready == weapon {
            self.ready = self.best_with_ammo();
        }
        if self.pending == Some(weapon) {
            self.pending = None;
        }
    }

    /// The weapons owned, in `WeaponKind::ALL` order.
    pub fn weapons(&self) -> impl Iterator<Item = WeaponKind> + '_ {
        WeaponKind::ALL.into_iter().filter(|w| self.has(*w))
    }

    pub fn ammo(&self, kind: AmmoKind) -> i32 {
        self.ammo[kind.index()]
    }

    /// Sets the ammo of `kind`, up to its limit.
    pub fn set_ammo(&mut self, kind: AmmoKind, amount: i32) {
        self.ammo[kind.index()] = amount.clamp(0, self.max(kind));
    }

    /// Takes the ammo one shot of the weapon in hand uses.
    pub fn use_ammo(&mut self) {
        let weapon = self.ready;
        if let Some(kind) = weapon.ammo() {
            let left = self.ammo(kind) - weapon.per_shot();
            self.ammo[kind.index()] = left.max(0);
        }
    }

    pub fn max(&self, kind: AmmoKind) -> i32 {
        kind.max() * if self.backpack { 2 } else { 1 }
    }

    pub fn has_key(&self, colour: KeyColour) -> bool {
        Key::ALL
            .into_iter()
            .any(|k| k.colour() == colour && self.keys[k as usize])
    }

    pub fn holds(&self, key: Key) -> bool {
        self.keys[key as usize]
    }

    /// Whether `weapon` has the ammo for a shot.
    pub fn can_fire(&self, weapon: WeaponKind) -> bool {
        weapon
            .ammo()
            .is_none_or(|kind| self.ammo(kind) >= weapon.per_shot())
    }

    /// p_pspr.c `P_CheckAmmo`'s choice: the best weapon that can fire.
    pub fn best_with_ammo(&self) -> WeaponKind {
        use WeaponKind::*;
        let can = |w: WeaponKind| self.has(w) && self.can_fire(w);
        [
            Plasma,
            SuperShotgun,
            Chaingun,
            Shotgun,
            Pistol,
            Chainsaw,
            Missile,
            Bfg,
        ]
        .into_iter()
        .find(|&w| can(w))
        .unwrap_or(Fist)
    }

    /// The weapon number key `slot` picks, as `P_PlayerThink` does: 1 is the
    /// chainsaw when owned, unless the fist is wanted with berserk, and 3 the
    /// super shotgun when owned, pressed again for the shotgun.
    pub fn for_slot(&self, slot: u8) -> Option<WeaponKind> {
        use WeaponKind::*;
        let current = self.pending.unwrap_or(self.ready);
        let weapon = match slot {
            1 if self.has(Chainsaw) && !(current == Chainsaw && self.berserk) => Chainsaw,
            1 => Fist,
            2 => Pistol,
            3 if self.has(SuperShotgun) && current != SuperShotgun => SuperShotgun,
            3 => Shotgun,
            4 => Chaingun,
            5 => Missile,
            6 => Plasma,
            7 => Bfg,
            _ => return None,
        };
        (self.has(weapon) && weapon != self.ready).then_some(weapon)
    }

    /// The owned weapon `step` places along the slot order from the one in
    /// hand (or switching to), wrapping round, skipping weapons with no ammo.
    pub fn stepped(&self, step: i32) -> Option<WeaponKind> {
        if step == 0 {
            return None;
        }
        let order = WeaponKind::BY_SLOT;
        let current = self.pending.unwrap_or(self.ready);
        let at = order.iter().position(|w| *w == current)? as i32;
        let n = order.len() as i32;
        let dir = step.signum();
        let mut left = step.abs();
        let mut i = at;
        for _ in 0..n * left {
            i = (i + dir).rem_euclid(n);
            let w = order[i as usize];
            if self.has(w) && self.can_fire(w) {
                left -= 1;
                if left == 0 {
                    return (w != self.ready).then_some(w);
                }
            }
        }
        None
    }

    /// p_inter.c `P_GiveAmmo`: `clips` clips of `kind`, half a clip for 0.
    /// False when there was no room. Switches to a weapon for it when there
    /// was none before.
    pub fn give_ammo(&mut self, kind: AmmoKind, clips: i32) -> bool {
        let old = self.ammo(kind);
        let max = self.max(kind);
        if old >= max {
            return false;
        }
        let amount = if clips > 0 {
            clips * kind.clip()
        } else {
            kind.clip() / 2
        };
        self.ammo[kind.index()] = (old + amount).min(max);
        if old != 0 {
            return true;
        }
        use WeaponKind::*;
        let ready = self.ready;
        let switch = match kind {
            AmmoKind::Bullets if ready == Fist => {
                Some(if self.has(Chaingun) { Chaingun } else { Pistol })
            }
            AmmoKind::Shells if matches!(ready, Fist | Pistol) => {
                self.has(Shotgun).then_some(Shotgun)
            }
            AmmoKind::Cells if matches!(ready, Fist | Pistol) => {
                self.has(Plasma).then_some(Plasma)
            }
            AmmoKind::Rockets if ready == Fist => self.has(Missile).then_some(Missile),
            _ => None,
        };
        if let Some(w) = switch.filter(|w| self.has(*w)) {
            self.pending = Some(w);
        }
        true
    }

    /// p_inter.c `P_GiveWeapon`: the weapon and two clips of its ammo, one
    /// when `dropped`. False when it gave neither. A new weapon is switched
    /// to.
    pub fn give_weapon(&mut self, weapon: WeaponKind, dropped: bool) -> bool {
        let gave_ammo = weapon
            .ammo()
            .is_some_and(|kind| self.give_ammo(kind, if dropped { 1 } else { 2 }));
        if self.has(weapon) {
            return gave_ammo;
        }
        self.owned[weapon.index()] = true;
        self.pending = Some(weapon);
        true
    }

    /// p_inter.c `P_TouchSpecialThing` for everything but health: what the
    /// item gives, or `None` when it gives nothing and stays.
    pub fn pick_up(&mut self, item: Pickup, dropped: bool) -> Option<Picked> {
        let item_up = |message| Picked {
            message,
            sound: "ITEMUP",
        };
        match item {
            Pickup::Health { .. } => None,
            Pickup::Weapon(w) => self.give_weapon(w, dropped).then_some(Picked {
                message: w.message(),
                sound: "WPNUP",
            }),
            Pickup::Ammo {
                kind,
                clips,
                message,
            } => {
                // A clip a zombie dropped is half a clip.
                let clips = if dropped { 0 } else { clips };
                self.give_ammo(kind, clips).then(|| item_up(message))
            }
            Pickup::Backpack => {
                self.backpack = true;
                for kind in AmmoKind::ALL {
                    self.give_ammo(kind, 1);
                }
                Some(item_up("Picked up a backpack full of ammo!"))
            }
            Pickup::Key(key) => {
                if self.keys[key as usize] {
                    return None;
                }
                self.keys[key as usize] = true;
                Some(item_up(match key {
                    Key::BlueCard => "Picked up a blue keycard.",
                    Key::YellowCard => "Picked up a yellow keycard.",
                    Key::RedCard => "Picked up a red keycard.",
                    Key::BlueSkull => "Picked up a blue skull key.",
                    Key::YellowSkull => "Picked up a yellow skull key.",
                    Key::RedSkull => "Picked up a red skull key.",
                }))
            }
            Pickup::Berserk => {
                self.berserk = true;
                if self.ready != WeaponKind::Fist {
                    self.pending = Some(WeaponKind::Fist);
                }
                Some(Picked {
                    message: "Berserk!",
                    sound: "GETPOW",
                })
            }
        }
    }

    /// What dying leaves: DOOM restarts the map, but here the world goes on,
    /// so the player keeps what they found, loses berserk, and has at least
    /// a pistol start's bullets.
    pub fn after_death(&mut self) {
        self.berserk = false;
        self.pending = None;
        let bullets = AmmoKind::Bullets.index();
        self.ammo[bullets] = self.ammo[bullets].max(50);
        if !self.can_fire(self.ready) {
            self.ready = self.best_with_ammo();
        }
    }

    /// What carries on to the next map: everything but the keys and
    /// berserk, as `G_PlayerFinishLevel` clears them.
    pub fn next_map(&self) -> Inventory {
        Inventory {
            keys: [false; 6],
            berserk: false,
            ready: self.pending.unwrap_or(self.ready),
            pending: None,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use WeaponKind::*;

    #[test]
    fn a_pistol_start() {
        let inv = Inventory::default();
        assert!(inv.has(Fist) && inv.has(Pistol) && !inv.has(Shotgun));
        assert_eq!(inv.ammo(AmmoKind::Bullets), 50);
        assert_eq!(inv.ready, Pistol);
        assert_eq!(inv.best_with_ammo(), Pistol);
    }

    #[test]
    fn weapons_give_two_clips_and_switch_when_new() {
        let mut inv = Inventory::default();
        assert!(inv.give_weapon(Shotgun, false));
        assert_eq!(inv.ammo(AmmoKind::Shells), 8);
        assert_eq!(inv.pending, Some(Shotgun));
        inv.pending = None;
        // Owned already: only the ammo, no switch.
        assert!(inv.give_weapon(Shotgun, true));
        assert_eq!(inv.ammo(AmmoKind::Shells), 12);
        assert_eq!(inv.pending, None);
        inv.set_ammo(AmmoKind::Shells, 50);
        assert!(!inv.give_weapon(Shotgun, false));
        // The chainsaw has no ammo: a second one isn't picked up.
        assert!(inv.give_weapon(Chainsaw, false));
        assert!(!inv.give_weapon(Chainsaw, false));
    }

    #[test]
    fn ammo_stops_at_the_limit_and_the_backpack_doubles_it() {
        let mut inv = Inventory::default();
        let box_of = Pickup::Ammo {
            kind: AmmoKind::Bullets,
            clips: 5,
            message: "",
        };
        for _ in 0..3 {
            assert!(inv.pick_up(box_of, false).is_some());
        }
        assert_eq!(inv.ammo(AmmoKind::Bullets), 200);
        assert!(inv.pick_up(box_of, false).is_none());
        assert!(inv.pick_up(Pickup::Backpack, false).is_some());
        assert_eq!(inv.max(AmmoKind::Bullets), 400);
        assert_eq!(inv.ammo(AmmoKind::Bullets), 210);
        assert_eq!(inv.ammo(AmmoKind::Rockets), 1);
        // A dropped clip is half a clip.
        let clip = Pickup::Ammo {
            kind: AmmoKind::Bullets,
            clips: 1,
            message: "",
        };
        inv.pick_up(clip, true);
        assert_eq!(inv.ammo(AmmoKind::Bullets), 215);
    }

    #[test]
    fn new_ammo_switches_from_the_fist_and_pistol() {
        let mut inv = Inventory::default();
        inv.add(Shotgun);
        inv.ready = Pistol;
        assert!(inv.give_ammo(AmmoKind::Shells, 1));
        assert_eq!(inv.pending, Some(Shotgun));
        let mut inv = Inventory::default();
        inv.set_ammo(AmmoKind::Bullets, 0);
        inv.ready = Fist;
        inv.give_ammo(AmmoKind::Bullets, 1);
        assert_eq!(inv.pending, Some(Pistol));
        // Not without the weapon.
        let mut inv = Inventory::default();
        inv.give_ammo(AmmoKind::Rockets, 1);
        assert_eq!(inv.pending, None);
    }

    #[test]
    fn running_dry_picks_the_best_weapon_left() {
        let mut inv = Inventory::default();
        inv.add(Shotgun);
        inv.add(Chaingun);
        inv.add(Bfg);
        inv.set_ammo(AmmoKind::Shells, 3);
        assert_eq!(inv.best_with_ammo(), Chaingun);
        inv.set_ammo(AmmoKind::Bullets, 0);
        assert_eq!(inv.best_with_ammo(), Shotgun);
        inv.set_ammo(AmmoKind::Shells, 0);
        inv.set_ammo(AmmoKind::Cells, 39);
        assert_eq!(inv.best_with_ammo(), Fist);
        inv.set_ammo(AmmoKind::Cells, 40);
        assert_eq!(inv.best_with_ammo(), Bfg);
        inv.ready = Chaingun;
        inv.use_ammo();
        assert_eq!(inv.ammo(AmmoKind::Bullets), 0);
    }

    #[test]
    fn number_keys_follow_doom() {
        let mut inv = Inventory::default();
        assert_eq!(inv.for_slot(1), Some(Fist));
        assert_eq!(inv.for_slot(2), None, "already in hand");
        assert_eq!(inv.for_slot(3), None, "not owned");
        inv.add(Chainsaw);
        inv.add(Shotgun);
        inv.add(SuperShotgun);
        assert_eq!(inv.for_slot(1), Some(Chainsaw));
        assert_eq!(inv.for_slot(3), Some(SuperShotgun));
        inv.ready = SuperShotgun;
        assert_eq!(inv.for_slot(3), Some(Shotgun));
        inv.ready = Chainsaw;
        inv.berserk = true;
        assert_eq!(inv.for_slot(1), Some(Fist));
    }

    #[test]
    fn the_wheel_steps_through_weapons_with_ammo() {
        let mut inv = Inventory::default();
        inv.add(Shotgun);
        inv.add(Chaingun);
        assert_eq!(inv.stepped(1), Some(Chaingun), "no shells: skipped");
        inv.set_ammo(AmmoKind::Shells, 4);
        assert_eq!(inv.stepped(1), Some(Shotgun));
        assert_eq!(inv.stepped(2), Some(Chaingun));
        assert_eq!(inv.stepped(-1), Some(Fist));
        inv.ready = Fist;
        assert_eq!(inv.stepped(-1), Some(Chaingun), "wraps round");
    }

    #[test]
    fn keys_open_by_colour_and_stay_behind() {
        let mut inv = Inventory::default();
        assert!(!inv.has_key(KeyColour::Red));
        assert!(inv.pick_up(Pickup::Key(Key::RedSkull), false).is_some());
        assert!(inv.has_key(KeyColour::Red) && !inv.has_key(KeyColour::Blue));
        assert!(inv.pick_up(Pickup::Key(Key::RedSkull), false).is_none());
        assert!(inv.pick_up(Pickup::Key(Key::RedCard), false).is_some());
        inv.add(Plasma);
        inv.set_ammo(AmmoKind::Cells, 40);
        inv.ready = Plasma;
        let next = inv.next_map();
        assert!(!next.has_key(KeyColour::Red));
        assert!(next.has(Plasma) && next.ready == Plasma);
        assert_eq!(next.ammo(AmmoKind::Cells), 40);
    }

    #[test]
    fn berserk_switches_to_the_fist() {
        let mut inv = Inventory::default();
        let picked = inv.pick_up(Pickup::Berserk, false).unwrap();
        assert_eq!(picked.sound, "GETPOW");
        assert!(inv.berserk && inv.pending == Some(Fist));
        inv.after_death();
        assert!(!inv.berserk);
    }

    #[test]
    fn names_round_trip() {
        for w in WeaponKind::ALL {
            assert_eq!(WeaponKind::parse(w.name()), Some(w));
        }
    }
}
