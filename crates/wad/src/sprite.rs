//! Sprite names: which lump draws a sprite's frame from which angle.
//!
//! Sprite lumps lie between `S_START` and `S_END` (or `SS_START` and
//! `SS_END` in PWADs). A name is a four-character prefix, a frame letter and
//! a rotation digit, optionally followed by a second frame letter and
//! rotation that use the same lump mirrored: `TROOA2A8` is frame A of `TROO`
//! from rotation 2, and mirrored from rotation 8. Rotation 0 means the lump
//! is seen the same from every angle; 1 is from the front, and the rest
//! follow every 45° counter-clockwise around the thing, seen from above.

use std::collections::HashMap;

use crate::Wad;

/// One sprite lump as a frame from one rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteLump {
    /// The lump's name, for [`Wad::patch`].
    pub name: String,
    /// Drawn mirrored left to right.
    pub mirrored: bool,
}

/// The lumps of one frame: one per rotation 1 to 8, at index `rotation − 1`.
#[derive(Clone, Debug, Default)]
pub struct SpriteFrame {
    pub rotations: [Option<SpriteLump>; 8],
}

/// Every sprite of a WAD by prefix.
#[derive(Clone, Debug, Default)]
pub struct SpriteSet {
    /// By upper-case prefix; frames by letter, `A` at index 0.
    sprites: HashMap<String, Vec<SpriteFrame>>,
}

impl SpriteSet {
    /// The lump drawing frame `frame` (`b'A'` is the first) of sprite
    /// `prefix` from rotation `rotation`: 1 to 8, or 0 for any angle, which
    /// takes rotation 1's lump.
    pub fn get(&self, prefix: &str, frame: u8, rotation: u8) -> Option<&SpriteLump> {
        let frames = self.sprites.get(&prefix.to_ascii_uppercase())?;
        let frame = frames.get(frame.checked_sub(b'A')? as usize)?;
        let rotation = rotation.max(1);
        frame.rotations.get(rotation as usize - 1)?.as_ref()
    }

    /// Whether frame `frame` of `prefix` looks different from different
    /// angles.
    pub fn rotates(&self, prefix: &str, frame: u8) -> bool {
        let Some(frames) = self.sprites.get(&prefix.to_ascii_uppercase()) else {
            return false;
        };
        let Some(frame) = frame.checked_sub(b'A').and_then(|f| frames.get(f as usize))
        else {
            return false;
        };
        frame.rotations.iter().any(|r| r != &frame.rotations[0])
    }

    /// The prefixes of every sprite, in no order.
    pub fn prefixes(&self) -> impl Iterator<Item = &str> {
        self.sprites.keys().map(String::as_str)
    }

    /// How many frames `prefix` has, counting gaps before its last.
    pub fn frame_count(&self, prefix: &str) -> usize {
        self.sprites
            .get(&prefix.to_ascii_uppercase())
            .map_or(0, Vec::len)
    }

    /// Every distinct lump name the set uses.
    pub fn lump_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .sprites
            .values()
            .flatten()
            .flat_map(|f| f.rotations.iter().flatten())
            .map(|l| l.name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Adds lump `name`, as its name says. Later lumps replace earlier ones
    /// for the same frame and rotation. Names that don't parse are ignored.
    fn install(&mut self, name: &str) {
        let bytes = name.as_bytes();
        if bytes.len() < 6 {
            return;
        }
        let prefix = &name[..4];
        self.install_one(prefix, name, bytes[4], bytes[5], false);
        if bytes.len() >= 8 {
            self.install_one(prefix, name, bytes[6], bytes[7], true);
        }
    }

    fn install_one(
        &mut self,
        prefix: &str,
        name: &str,
        frame: u8,
        rotation: u8,
        mirrored: bool,
    ) {
        let (Some(frame), Some(rotation)) =
            (frame.checked_sub(b'A'), rotation.checked_sub(b'0'))
        else {
            return;
        };
        // Vanilla stops at frame ']' (29); allow a little more for PWADs.
        if frame >= 32 || rotation > 8 {
            return;
        }
        let frames = self.sprites.entry(prefix.to_string()).or_default();
        if frames.len() <= frame as usize {
            frames.resize_with(frame as usize + 1, SpriteFrame::default);
        }
        let lump = SpriteLump {
            name: name.to_string(),
            mirrored,
        };
        let rotations = &mut frames[frame as usize].rotations;
        if rotation == 0 {
            *rotations = std::array::from_fn(|_| Some(lump.clone()));
        } else {
            rotations[rotation as usize - 1] = Some(lump);
        }
    }
}

impl Wad {
    /// The sprites between every `S_START`/`SS_START` and the
    /// `S_END`/`SS_END` after it.
    pub fn sprites(&self) -> SpriteSet {
        let mut set = SpriteSet::default();
        let mut inside = false;
        for e in &self.entries {
            match e.name.as_str() {
                "S_START" | "SS_START" => inside = true,
                "S_END" | "SS_END" => inside = false,
                name if inside && e.size > 0 => set.install(name),
                _ => {}
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::tests::encode_patch;
    use crate::test_support::build_wad;

    fn lump(name: &str, mirrored: bool) -> Option<SpriteLump> {
        Some(SpriteLump {
            name: name.to_string(),
            mirrored,
        })
    }

    #[test]
    fn rotations_mirrors_and_any_angle() {
        let patch = encode_patch(2, &[vec![(0, vec![7, 8])]]);
        let wad = Wad::from_bytes(build_wad(&[
            ("TROOA1", &patch),
            ("S_START", b""),
            ("TROOA1", &patch),
            ("TROOA2A8", &patch),
            ("TROOA3A7", &patch),
            ("TROOA4A6", &patch),
            ("TROOA5", &patch),
            ("BAL1A0", &patch),
            ("SARGB2C8", &patch),
            ("S_END", b""),
            ("POSSA0", &patch),
            ("SS_START", b""),
            ("POSSA0", &patch),
            ("SS_END", b""),
        ]))
        .unwrap();
        let set = wad.sprites();
        assert_eq!(set.get("TROO", b'A', 1).cloned(), lump("TROOA1", false));
        assert_eq!(set.get("TROO", b'A', 2).cloned(), lump("TROOA2A8", false));
        assert_eq!(set.get("TROO", b'A', 8).cloned(), lump("TROOA2A8", true));
        assert_eq!(set.get("TROO", b'A', 6).cloned(), lump("TROOA4A6", true));
        assert_eq!(set.get("troo", b'A', 5).cloned(), lump("TROOA5", false));
        assert!(set.rotates("TROO", b'A'));
        // Rotation 0 serves every angle.
        for r in 0..=8 {
            assert_eq!(set.get("BAL1", b'A', r).cloned(), lump("BAL1A0", false));
        }
        assert!(!set.rotates("BAL1", b'A'));
        // The mirrored half may name another frame.
        assert_eq!(set.get("SARG", b'B', 2).cloned(), lump("SARGB2C8", false));
        assert_eq!(set.get("SARG", b'C', 8).cloned(), lump("SARGB2C8", true));
        assert_eq!(set.get("SARG", b'A', 1), None);
        assert_eq!(set.frame_count("SARG"), 3);
        // Only lumps between the markers count, in SS_ too.
        assert_eq!(set.get("POSS", b'A', 3).cloned(), lump("POSSA0", false));
        assert_eq!(set.get("NOPE", b'A', 1), None);
        assert_eq!(set.get("TROO", b'Z', 1), None);
        // And they decode as pictures.
        let name = &set.get("TROO", b'A', 1).unwrap().name;
        assert_eq!(wad.patch(name).unwrap().picture.pixels[1], Some(8));
    }

    #[test]
    fn later_lumps_replace_earlier_ones_and_bad_names_are_skipped() {
        let mut set = SpriteSet::default();
        set.install("PLAYA0");
        set.install("PLAYA1");
        set.install("PLAY");
        set.install("PLAY?1");
        set.install("PLAYB9");
        assert_eq!(set.get("PLAY", b'A', 1).cloned(), lump("PLAYA1", false));
        assert_eq!(set.get("PLAY", b'A', 2).cloned(), lump("PLAYA0", false));
        assert_eq!(set.frame_count("PLAY"), 1);
        assert_eq!(set.lump_names(), ["PLAYA0", "PLAYA1"]);
    }
}
