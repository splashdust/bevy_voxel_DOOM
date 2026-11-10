//! Reads vanilla DOOM WAD files: the map lumps, palette 0 of PLAYPAL,
//! COLORMAP, flats, composite wall textures, sprites and sound effects.
//!
//! Every name (lump, texture, flat) is stored upper-case with trailing NULs
//! removed, so lookups compare upper-case strings.

use std::collections::HashMap;
use std::path::Path;

mod bytes;
mod directory;
mod map;
mod picture;
mod sound;
mod sprite;
mod texture;

pub use picture::{decode_patch, Patch};
pub use sound::Sound;
pub use sprite::{SpriteFrame, SpriteLump, SpriteSet};
pub use texture::SkippedTexture;

#[derive(Debug)]
pub enum WadError {
    Io(std::io::Error),
    /// The header, a directory entry or a lump is shorter or larger than its format allows.
    Malformed(String),
    MissingLump(String),
    MissingMap(String),
}

impl std::fmt::Display for WadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WadError::Io(e) => write!(f, "{e}"),
            WadError::Malformed(m) => write!(f, "malformed WAD: {m}"),
            WadError::MissingLump(n) => write!(f, "no lump named {n}"),
            WadError::MissingMap(n) => write!(f, "no map named {n}"),
        }
    }
}

impl std::error::Error for WadError {}

impl From<std::io::Error> for WadError {
    fn from(e: std::io::Error) -> Self {
        WadError::Io(e)
    }
}

pub struct Wad {
    data: Vec<u8>,
    entries: Vec<directory::Entry>,
}

impl Wad {
    pub fn open(path: impl AsRef<Path>) -> Result<Wad, WadError> {
        Wad::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Wad, WadError> {
        let entries = directory::parse(&data)?;
        Ok(Wad { data, entries })
    }

    /// The last lump with this name, as later lumps override earlier ones.
    pub fn lump(&self, name: &str) -> Option<&[u8]> {
        let name = name.to_ascii_uppercase();
        let e = self.entries.iter().rev().find(|e| e.name == name)?;
        Some(self.entry_data(e))
    }

    /// Every lump's name, in directory order.
    pub fn lump_names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// The names of the maps in the WAD, `E1M1` or `MAP01` style, in directory order.
    pub fn map_names(&self) -> Vec<String> {
        self.entries
            .windows(2)
            .filter(|w| map::is_map_name(&w[0].name) && w[1].name == "THINGS")
            .map(|w| w[0].name.clone())
            .collect()
    }

    pub fn map(&self, name: &str) -> Result<MapData, WadError> {
        map::decode(self, name)
    }

    /// Palette 0 of PLAYPAL.
    pub fn palette(&self) -> Result<Palette, WadError> {
        let lump = self
            .lump("PLAYPAL")
            .ok_or_else(|| WadError::MissingLump("PLAYPAL".into()))?;
        let bytes = lump.get(..768).ok_or_else(|| {
            WadError::Malformed(format!("PLAYPAL is {} bytes, under 768", lump.len()))
        })?;
        let mut palette = [[0; 3]; 256];
        for (colour, rgb) in palette.iter_mut().zip(bytes.as_chunks::<3>().0) {
            *colour = *rgb;
        }
        Ok(palette)
    }

    pub fn colormap(&self) -> Result<Colormap, WadError> {
        const SIZE: usize = 34 * 256;
        let lump = self
            .lump("COLORMAP")
            .ok_or_else(|| WadError::MissingLump("COLORMAP".into()))?;
        let bytes = lump.get(..SIZE).ok_or_else(|| {
            WadError::Malformed(format!("COLORMAP is {} bytes, under {SIZE}", lump.len()))
        })?;
        let tables = bytes.as_chunks::<256>().0.to_vec();
        Ok(Colormap { tables })
    }

    /// Every flat between F_START and F_END, and every wall texture from
    /// TEXTURE1 and TEXTURE2, composited from its patches.
    pub fn textures(&self) -> Result<TextureSet, WadError> {
        Ok(self.textures_with_skipped()?.textures)
    }

    /// `textures`, plus the wall textures left out because a patch is
    /// missing or corrupt.
    pub fn textures_with_skipped(&self) -> Result<LoadedTextures, WadError> {
        let flats = texture::flats(self)?;
        let (walls, skipped) = texture::walls(self)?;
        Ok(LoadedTextures {
            textures: TextureSet { flats, walls },
            skipped,
        })
    }

    /// The last lump with this name, decoded as a picture: a patch or a sprite.
    pub fn patch(&self, name: &str) -> Result<Patch, WadError> {
        let lump = self
            .lump(name)
            .ok_or_else(|| WadError::MissingLump(name.to_ascii_uppercase()))?;
        decode_patch(lump).map_err(|e| match e {
            WadError::Malformed(m) => WadError::Malformed(format!("{name}: {m}")),
            e => e,
        })
    }

    fn entry_data(&self, e: &directory::Entry) -> &[u8] {
        if e.size == 0 {
            &[]
        } else {
            &self.data[e.offset..e.offset + e.size]
        }
    }
}

/// What `Wad::textures_with_skipped` returns.
pub struct LoadedTextures {
    pub textures: TextureSet,
    pub skipped: Vec<SkippedTexture>,
}

/// 256 RGB colours.
pub type Palette = [[u8; 3]; 256];

/// COLORMAP: 34 tables mapping a palette index to a palette index. Tables
/// 0–31 run from brightest to darkest, 32 is invulnerability, 33 is black.
pub struct Colormap {
    pub tables: Vec<[u8; 256]>,
}

/// A picture of palette indices. `None` is a transparent texel.
pub struct Picture {
    pub width: u16,
    pub height: u16,
    /// Row-major: the texel at (x, y) is `pixels[y * width + x]`, with y = 0 the top row.
    pub pixels: Vec<Option<u8>>,
}

impl Picture {
    /// The texel at (x, y), wrapping both coordinates, as walls and flats tile.
    pub fn texel_wrapped(&self, x: i32, y: i32) -> Option<u8> {
        let w = self.width as i32;
        let h = self.height as i32;
        if w == 0 || h == 0 {
            return None;
        }
        let x = x.rem_euclid(w);
        let y = y.rem_euclid(h);
        self.pixels[(y * w + x) as usize]
    }
}

#[derive(Default)]
pub struct TextureSet {
    /// Flats by upper-case name, all 64×64.
    pub flats: HashMap<String, Picture>,
    /// Wall textures by upper-case name.
    pub walls: HashMap<String, Picture>,
}

/// One map's lumps, decoded. Coordinates are DOOM map units.
pub struct MapData {
    pub name: String,
    pub things: Vec<Thing>,
    pub linedefs: Vec<LineDef>,
    pub sidedefs: Vec<SideDef>,
    pub vertexes: Vec<Vertex>,
    pub segs: Vec<Seg>,
    pub subsectors: Vec<SubSector>,
    pub nodes: Vec<Node>,
    pub sectors: Vec<Sector>,
    pub blockmap: Option<Blockmap>,
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub x: i16,
    pub y: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct Thing {
    pub x: i16,
    pub y: i16,
    /// Degrees, 0 = east, counter-clockwise.
    pub angle: i16,
    /// Thing type: 1 is player 1's start.
    pub kind: u16,
    pub flags: u16,
}

pub const ML_BLOCKING: u16 = 1;
pub const ML_TWOSIDED: u16 = 4;
pub const ML_DONTPEGTOP: u16 = 8;
pub const ML_DONTPEGBOTTOM: u16 = 16;

#[derive(Clone, Copy, Debug)]
pub struct LineDef {
    pub v1: u16,
    pub v2: u16,
    pub flags: u16,
    pub special: u16,
    pub tag: u16,
    /// The right (front) sidedef. `None` when the lump stores 0xFFFF.
    pub front: Option<u16>,
    /// The left (back) sidedef. `None` when the lump stores 0xFFFF.
    pub back: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct SideDef {
    pub x_offset: i16,
    pub y_offset: i16,
    /// Upper-case texture names; "-" means no texture.
    pub upper: String,
    pub lower: String,
    pub middle: String,
    pub sector: u16,
}

#[derive(Clone, Debug)]
pub struct Sector {
    pub floor: i16,
    pub ceiling: i16,
    pub floor_flat: String,
    pub ceiling_flat: String,
    pub light: u16,
    pub special: u16,
    pub tag: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub v1: u16,
    pub v2: u16,
    pub angle: i16,
    pub linedef: u16,
    /// 0 when the seg runs along the linedef's front side, 1 for the back.
    pub side: u16,
    pub offset: i16,
}

#[derive(Clone, Copy, Debug)]
pub struct SubSector {
    pub seg_count: u16,
    pub first_seg: u16,
}

/// A BSP node. A child with bit 15 set (`NF_SUBSECTOR`) is a subsector index
/// in its low 15 bits; otherwise it is a node index.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub x: i16,
    pub y: i16,
    pub dx: i16,
    pub dy: i16,
    /// Bounding boxes of the right and left children: top, bottom, left, right.
    pub bbox: [[i16; 4]; 2],
    /// Right (front) child, then left (back) child.
    pub children: [u16; 2],
}

pub const NF_SUBSECTOR: u16 = 0x8000;

/// BLOCKMAP: a grid of 128×128-unit cells, each listing the linedefs that touch it.
#[derive(Clone, Debug)]
pub struct Blockmap {
    pub origin_x: i16,
    pub origin_y: i16,
    pub columns: u16,
    pub rows: u16,
    /// Linedef indices per cell, row-major from the origin: cell (c, r) is `cells[r * columns + c]`.
    pub cells: Vec<Vec<u16>>,
}

#[cfg(test)]
mod test_support {
    /// An 8-byte, NUL-padded name field.
    pub(crate) fn name_field(name: &str) -> [u8; 8] {
        let mut f = [0; 8];
        for (d, s) in f.iter_mut().zip(name.bytes()) {
            *d = s;
        }
        f
    }

    /// A PWAD holding these lumps in order, directory last.
    pub(crate) fn build_wad(lumps: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = b"PWAD".to_vec();
        data.extend_from_slice(&[0; 8]);
        let mut dir = Vec::new();
        for (name, lump) in lumps {
            dir.extend_from_slice(&(data.len() as i32).to_le_bytes());
            dir.extend_from_slice(&(lump.len() as i32).to_le_bytes());
            dir.extend_from_slice(&name_field(name));
            data.extend_from_slice(lump);
        }
        let table = data.len() as i32;
        data[4..8].copy_from_slice(&(lumps.len() as i32).to_le_bytes());
        data[8..12].copy_from_slice(&table.to_le_bytes());
        data.extend(dir);
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::build_wad;

    #[test]
    fn directory_names_are_upper_case_and_last_lump_wins() {
        let wad = Wad::from_bytes(build_wad(&[
            ("demo1", b"first"),
            ("OTHER", b""),
            ("DEMO1", b"second"),
        ]))
        .unwrap();
        assert_eq!(wad.lump("DEMO1"), Some(&b"second"[..]));
        assert_eq!(wad.lump("demo1"), Some(&b"second"[..]));
        assert_eq!(wad.lump("OTHER"), Some(&b""[..]));
        assert_eq!(wad.lump("NOPE"), None);
    }

    #[test]
    fn iwad_header_is_accepted() {
        let mut data = build_wad(&[("A", b"x")]);
        data[..4].copy_from_slice(b"IWAD");
        assert!(Wad::from_bytes(data).is_ok());
    }

    #[test]
    fn corrupt_headers_and_directories_are_malformed() {
        let good = build_wad(&[("A", b"abcd")]);
        let malformed =
            |d: Vec<u8>| matches!(Wad::from_bytes(d), Err(WadError::Malformed(_)));
        assert!(malformed(good[..11].to_vec()));
        let mut bad_ident = good.clone();
        bad_ident[..4].copy_from_slice(b"ZWAD");
        assert!(malformed(bad_ident));
        // Directory cut short.
        assert!(malformed(good[..good.len() - 1].to_vec()));
        // Lump size past the end of the file.
        let mut long_lump = good.clone();
        let dir = good.len() - 16;
        long_lump[dir + 4..dir + 8].copy_from_slice(&1000i32.to_le_bytes());
        assert!(malformed(long_lump));
        let mut negative = good.clone();
        negative[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(malformed(negative));
    }

    #[test]
    fn palette_and_colormap() {
        let playpal: Vec<u8> = (0..768 * 14).map(|i| (i % 251) as u8).collect();
        let colormap: Vec<u8> = (0..34 * 256).map(|i| (i / 256) as u8).collect();
        let wad =
            Wad::from_bytes(build_wad(&[("PLAYPAL", &playpal), ("COLORMAP", &colormap)]))
                .unwrap();
        let pal = wad.palette().unwrap();
        assert_eq!(pal[1], [3, 4, 5]);
        let cm = wad.colormap().unwrap();
        assert_eq!(cm.tables.len(), 34);
        assert_eq!(cm.tables[33][0], 33);

        let short = Wad::from_bytes(build_wad(&[
            ("PLAYPAL", &playpal[..767]),
            ("COLORMAP", &colormap[..100]),
        ]))
        .unwrap();
        assert!(matches!(short.palette(), Err(WadError::Malformed(_))));
        assert!(matches!(short.colormap(), Err(WadError::Malformed(_))));
        let empty = Wad::from_bytes(build_wad(&[])).unwrap();
        assert!(matches!(empty.palette(), Err(WadError::MissingLump(_))));
    }
}
