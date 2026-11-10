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

#[cfg(test)]
mod tests {
    use super::*;

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
