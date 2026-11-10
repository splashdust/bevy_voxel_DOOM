//! Baked sector light: which COLORMAP table a surface uses.
//!
//! DOOM picks a colormap from the sector's light level and the distance to
//! the viewer. A baked voxel has no viewer, so the distance is fixed at
//! [`BAKE_DISTANCE`] map units. With `L = lightlevel >> 4`:
//!
//! - walls: `L ± 1` for axis-aligned lines ("fake contrast"), then table
//!   `4·(15 − L) − 10`, as r_main.c `scalelight` gives at 128 units;
//! - flats: table `4·(15 − L) − 8`, as r_main.c `zlight` gives at 128 units;
//!
//! clamped to tables 0–31 (0 brightest).

/// The viewer distance the light is baked for, in map units.
pub(crate) const BAKE_DISTANCE: i32 = 128;

const LIGHTLEVELS: i32 = 16;
const NUMCOLORMAPS: i32 = 32;
const MAXLIGHTSCALE: i32 = 48;
const MAXLIGHTZ: i32 = 128;

/// r_main.c `R_ExecuteSetViewSize`: the first table of light level `l`.
fn startmap(l: i32) -> i32 {
    (LIGHTLEVELS - 1 - l) * 2 * NUMCOLORMAPS / LIGHTLEVELS
}

/// `lightlevel >> LIGHTSEGSHIFT` (r_main.h), clamped to the 16 levels.
pub(crate) fn level(lightlevel: u16) -> i32 {
    (lightlevel as i32 >> 4).clamp(0, LIGHTLEVELS - 1)
}

/// The colormap table of a wall seen at [`BAKE_DISTANCE`]: r_segs.c
/// `walllights[rw_scale >> LIGHTSCALESHIFT]` on a full-width 320 view, where
/// the scale index is `160·16 / distance`.
pub(crate) fn wall_table(lightlevel: u16, contrast: i32) -> u8 {
    let l = (level(lightlevel) + contrast).clamp(0, LIGHTLEVELS - 1);
    let j = (160 * 16 / BAKE_DISTANCE).min(MAXLIGHTSCALE - 1);
    // DISTMAP is 2.
    (startmap(l) - j / 2).clamp(0, NUMCOLORMAPS - 1) as u8
}

/// The colormap table of a flat seen at [`BAKE_DISTANCE`]: r_main.c
/// `R_InitLightTables` `zlight`, indexed by `distance >> LIGHTZSHIFT` in
/// r_plane.c `R_MapPlane`.
pub(crate) fn flat_table(lightlevel: u16) -> u8 {
    let j = (BAKE_DISTANCE / 16).min(MAXLIGHTZ - 1);
    let scale = 160 / (j + 1);
    (startmap(level(lightlevel)) - scale / 2).clamp(0, NUMCOLORMAPS - 1) as u8
}

/// The glow levels of sector glow: for each COLORMAP table, the level a
/// surface drawn in it glows at, and for each light level the brightness a
/// face reading it shows, so that a face lit only by its own surface looks
/// as bright as DOOM draws it.
///
/// Light loses one level per voxel at a voxel reach of 14, so the open voxel
/// in front of a surface glowing at `E` holds `E − 1`: a table's level is one
/// more than the light level its face reads.
///
/// COLORMAP darkens nearly in proportion in sRGB: table `k` leaves a colour
/// at about `1 − k/32` of its sRGB value, whether the colour is dark or
/// bright. So each table is measured as the mean ratio of sRGB luma with and
/// without it over the palette's entries that aren't near black, and a
/// brightness here is such a ratio, for the shader to multiply in sRGB.
/// Levels are spaced evenly in it, and a level's brightness is the mean of
/// the tables that read it.
#[derive(Clone, Debug, PartialEq)]
pub struct GlowLevels {
    /// Per COLORMAP table, the level a surface glows at, from 1 to 15.
    pub level: [u8; 32],
    /// Per light level, the share of its sRGB value a face reading it shows.
    pub brightness: [f32; 16],
}

/// The highest level a face lit by its own surface reads.
const TOP_READ: f32 = 14.0;

/// Palette entries darker than this sRGB luma, out of 255, are left out of
/// the measure: COLORMAP rounds them to black or near it.
const DARKEST_MEASURED: f32 = 20.0;

impl GlowLevels {
    pub fn measure(palette: &[[u8; 3]; 256], colormap: &doom_wad::Colormap) -> Self {
        let luma = |[r, g, b]: [u8; 3]| {
            0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
        };
        let measured: Vec<usize> = (0..256)
            .filter(|&i| luma(palette[i]) > DARKEST_MEASURED)
            .collect();
        let mut level = [1u8; 32];
        let mut sums = [(0.0f32, 0u32); 16];
        for (k, out) in level.iter_mut().enumerate() {
            let ratio = match colormap.tables.get(k) {
                Some(table) if !measured.is_empty() => {
                    let sum: f32 = measured
                        .iter()
                        .map(|&i| luma(palette[table[i] as usize]) / luma(palette[i]))
                        .sum();
                    (sum / measured.len() as f32).min(1.0)
                }
                _ => 1.0,
            };
            let read = (TOP_READ * ratio).round() as usize;
            *out = read as u8 + 1;
            sums[read].0 += ratio;
            sums[read].1 += 1;
        }
        let brightness = std::array::from_fn(|read| match (read, sums[read]) {
            (0, _) => 0.0,
            (_, (sum, n)) if n > 0 => sum / n as f32,
            _ => (read as f32 / TOP_READ).min(1.0),
        });
        GlowLevels { level, brightness }
    }

    /// The level a surface drawn in COLORMAP table `table` glows at.
    pub fn of(&self, table: u8) -> u8 {
        self.level[(table as usize).min(31)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glow_levels_follow_how_much_each_table_darkens() {
        // A grey palette and tables that scale each entry's value by
        // (32 − k) / 32.
        let palette: [[u8; 3]; 256] = std::array::from_fn(|i| [i as u8; 3]);
        let tables = (0..34)
            .map(|k| std::array::from_fn(|i| (i as u32 * (32 - k.min(32)) / 32) as u8))
            .collect();
        let glow = GlowLevels::measure(&palette, &doom_wad::Colormap { tables });
        assert_eq!(glow.of(0), 15);
        assert!(
            glow.level.windows(2).all(|w| w[0] >= w[1]),
            "{:?}",
            glow.level
        );
        assert!(glow.of(31) <= 2);
        // Dark tables still get levels of their own.
        let distinct = glow.level.iter().collect::<std::collections::BTreeSet<_>>();
        assert!(distinct.len() >= 12, "{:?}", glow.level);
        assert_eq!(glow.brightness[0], 0.0);
        // Level 14 holds the brightest tables, 0 and its neighbours.
        assert!(glow.brightness[14] > 0.9 && glow.brightness[14] <= 1.0);
        assert!(glow.brightness.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn full_light_is_the_brightest_table_and_dark_sectors_darken() {
        assert_eq!(wall_table(255, 0), 0);
        assert_eq!(flat_table(255), 0);
        // Light 160: level 10, start table 20.
        assert_eq!(wall_table(160, 0), 10);
        assert_eq!(wall_table(160, -1), 14);
        assert_eq!(wall_table(160, 1), 6);
        assert_eq!(flat_table(160), 12);
        assert_eq!(wall_table(0, 0), 31);
        assert_eq!(flat_table(0), 31);
    }
}
