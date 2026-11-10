//! Reads vanilla DOOM WAD files: the map lumps, palette 0 of PLAYPAL,
//! COLORMAP, flats and composite wall textures.
//!
//! Every name (lump, texture, flat) is stored upper-case with trailing NULs
//! removed, so lookups compare upper-case strings.

use std::collections::HashMap;
use std::path::Path;

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
    _data: Vec<u8>,
}

impl Wad {
    pub fn open(path: impl AsRef<Path>) -> Result<Wad, WadError> {
        Wad::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Wad, WadError> {
        Ok(Wad { _data: data })
    }

    /// The last lump with this name, as later lumps override earlier ones.
    pub fn lump(&self, _name: &str) -> Option<&[u8]> {
        None
    }

    /// The names of the maps in the WAD, `E1M1` or `MAP01` style, in directory order.
    pub fn map_names(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn map(&self, name: &str) -> Result<MapData, WadError> {
        Err(WadError::MissingMap(name.to_string()))
    }

    /// Palette 0 of PLAYPAL.
    pub fn palette(&self) -> Result<Palette, WadError> {
        Err(WadError::MissingLump("PLAYPAL".into()))
    }

    pub fn colormap(&self) -> Result<Colormap, WadError> {
        Err(WadError::MissingLump("COLORMAP".into()))
    }

    /// Every flat between F_START and F_END, and every wall texture from
    /// TEXTURE1 and TEXTURE2, composited from its patches.
    pub fn textures(&self) -> Result<TextureSet, WadError> {
        Err(WadError::MissingLump("TEXTURE1".into()))
    }
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
