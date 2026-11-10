//! Turns a vanilla DOOM map into voxels, one palette colour per voxel.
//!
//! Coordinates: voxel `[x, y, z]` with y up. With `s` map units per voxel,
//! voxel `[x, y, z]` covers DOOM map x in `[x*s, (x+1)*s)`, height in
//! `[y*s, (y+1)*s)` and DOOM map y in `(-(z+1)*s, -z*s]`. So DOOM's east is
//! +x, DOOM's north is −z (Bevy's forward), and the result is right-handed.
//!
//! This is a placeholder: it builds a small test room so the app can run
//! before the real voxelizer exists.

use doom_wad::{Palette, Wad, WadError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voxel {
    Air,
    /// A palette index into [`VoxelMap::palette`].
    Solid(u8),
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Map units per voxel.
    pub units_per_voxel: i32,
    /// Pass colours through COLORMAP at each sector's light level, so DOOM's
    /// sector lighting is baked in. Off gives full-bright colours.
    pub sector_light: bool,
    /// Bake door sectors open, as if every door had been used.
    pub open_doors: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            units_per_voxel: 2,
            sector_light: true,
            open_doors: true,
        }
    }
}

pub struct VoxelMap {
    palette: Palette,
    min: [i32; 3],
    max: [i32; 3],
}

impl VoxelMap {
    pub fn build(_wad: &Wad, _map: &str, _options: &Options) -> Result<VoxelMap, WadError> {
        let mut palette = [[0u8; 3]; 256];
        for (i, c) in palette.iter_mut().enumerate() {
            *c = [i as u8, (255 - i) as u8, ((i * 7) % 256) as u8];
        }
        Ok(VoxelMap {
            palette,
            min: [-64, -8, -64],
            max: [64, 64, 64],
        })
    }

    /// The voxel at `p`. Thread-safe and deterministic: the app calls this
    /// from the voxel world's worker threads.
    pub fn voxel(&self, p: [i32; 3]) -> Voxel {
        let inside = (-48..48).contains(&p[0]) && (0..40).contains(&p[1]) && (-48..48).contains(&p[2]);
        if inside {
            Voxel::Air
        } else {
            Voxel::Solid(((p[0] + p[1] * 3 + p[2] * 5).rem_euclid(256)) as u8)
        }
    }

    /// `Some(v)` when every voxel in the box `min..max` (max exclusive) is
    /// known to be `v` without visiting them; `None` when it may vary. Must
    /// be cheap: the app calls it once per chunk to skip generating uniform
    /// chunks.
    pub fn uniform_in(&self, min: [i32; 3], max: [i32; 3]) -> Option<Voxel> {
        if max[1] <= self.min[1] {
            return Some(Voxel::Solid(0));
        }
        if min[1] >= self.max[1] {
            return Some(Voxel::Air);
        }
        None
    }

    /// The box holding every voxel that isn't plain solid below or air above
    /// the map: `(min, max)`, max exclusive.
    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        (self.min, self.max)
    }

    /// Palette 0 of PLAYPAL.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Player 1's start, in voxel coordinates at eye height (DOOM's view
    /// height, 41 units), and the yaw in radians around +y, 0 facing −z.
    pub fn spawn(&self) -> ([f32; 3], f32) {
        ([0.0, 20.0, 0.0], 0.0)
    }

    /// An RGB colour for the sky, for the clear colour.
    pub fn sky_colour(&self) -> [u8; 3] {
        [90, 110, 140]
    }
}
