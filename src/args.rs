//! Command-line arguments, parsed by hand.

use std::path::PathBuf;

use bevy::prelude::*;
use doom_voxelize::LampLevel;

pub const USAGE: &str = "\
doom_voxels: a vanilla DOOM map as a bevy_voxel_world voxel world

usage: doom_voxels --wad PATH [options]

  --wad PATH              the WAD file; falls back to the DOOM_WAD environment variable
  --map NAME              the map to show, like E1M1 or MAP01; the WAD's first map by default
  --list-maps             print the WAD's maps and exit
  --units-per-voxel N     DOOM map units per voxel (default 2)
  --light MODE            sector (default): DOOM's sector light baked in, no propagation;
                          propagated: full-bright colours times propagated light;
                          both: sector light baked in, times propagated light;
                          sector-glow: full-bright colours, every surface glowing at its
                          sector's light, carried by propagated light;
                          flat: full-bright, no light at all
  --contrast K            stretch baked sector light: 1 is DOOM's, above 1 brightens bright
                          sectors and darkens dark ones (default 1.25)
  --min-light B           propagated light's least brightness, 0 to 1
                          (default 0.08, 0 in sector-glow)
  --falloff BASE          each light level is BASE times as bright as the one above (default 0.9);
                          sector-glow's lamp light follows COLORMAP instead
  --sky on|off            sky light (default on, off in sector-glow)
  --sky-reach N           voxels sky light spreads past where it falls, 1 to 32 (default 32)
  --voxel-reach N         voxels a lamp's light spreads, 1 to 32 (default 32, 14 in sector-glow)
  --lamp-gain G           lamp light at level 15 as a brightness, before the cap at 1
                          (default 1.5, 1 in sector-glow)
  --precision 16|256      light levels per voxel (default 16, 256 in sector-glow)
  --sky-fill on|off       carry sky light through openings into rooms (default on)
  --lamp-level full|sector
                          light surfaces glow at level 15, or at their sector's level (default full)
  --stepped               show each voxel's light as a flat step instead of smoothing it
  --flash-time S          how long a shot's flash glows, in seconds (default 0.12)
  --closed-doors          keep door sectors closed
  --pos X,Y,Z             start the camera here, in voxel coordinates
  --yaw DEG               start yaw, 0 facing DOOM's north, positive turning left
  --pitch DEG             start pitch, positive looking up
  --blast-radius R        the weapon's carved ball radius in voxels (default 4)
  --fire N                fire N shots around the view centre once the world is ready
  --debris N              debris cubes a shot throws out (default 16, 0 for none)
  --screenshot PATH       wait for the world around the camera to finish, save a PNG, exit
  --screenshot-delay S    take the screenshot S seconds after the last scripted shot instead
  --quit-after S          exit after S seconds
  --no-vsync              don't wait for the display, so the HUD's frame rate shows the cost
  --help                  this text";

#[derive(Resource, Clone, Debug)]
pub struct Args {
    pub wad: PathBuf,
    pub map: Option<String>,
    pub list_maps: bool,
    pub units_per_voxel: i32,
    pub light: LightMode,
    pub knobs: LightKnobs,
    pub closed_doors: bool,
    pub pos: Option<Vec3>,
    /// Degrees.
    pub yaw: Option<f32>,
    /// Degrees.
    pub pitch: Option<f32>,
    pub contrast: f32,
    pub blast_radius: i32,
    pub fire: u32,
    pub debris: u32,
    pub screenshot_delay: Option<f32>,
    pub screenshot: Option<PathBuf>,
    pub quit_after: Option<f32>,
    pub no_vsync: bool,
}

/// How the map is lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightMode {
    /// DOOM's sector light baked into the colours, no propagation.
    Sector,
    /// Full-bright colours times propagated light.
    Propagated,
    /// Sector light baked in, times propagated light.
    Both,
    /// Full-bright colours, every surface glowing at its sector's light,
    /// carried by propagated light.
    SectorGlow,
    /// Full-bright colours, no light at all.
    Flat,
}

impl LightMode {
    fn parse(text: &str) -> Option<LightMode> {
        Some(match text {
            "sector" => LightMode::Sector,
            "propagated" => LightMode::Propagated,
            "both" => LightMode::Both,
            "sector-glow" => LightMode::SectorGlow,
            "flat" => LightMode::Flat,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            LightMode::Sector => "sector",
            LightMode::Propagated => "propagated",
            LightMode::Both => "both",
            LightMode::SectorGlow => "sector-glow",
            LightMode::Flat => "flat",
        }
    }

    /// Whether DOOM's sector light is baked into the colours.
    pub fn sector_light(self) -> bool {
        matches!(self, LightMode::Sector | LightMode::Both)
    }

    /// Whether the world propagates light, with lamps and flashes.
    pub fn propagated(self) -> bool {
        matches!(
            self,
            LightMode::Propagated | LightMode::Both | LightMode::SectorGlow
        )
    }

    /// Whether every surface glows at its sector's light.
    pub fn sector_glow(self) -> bool {
        self == LightMode::SectorGlow
    }
}

/// The settings of propagated light the command line can change. Those
/// whose default depends on the mode are `None` until set: read them through
/// the methods of the same name.
#[derive(Clone, Debug, PartialEq)]
pub struct LightKnobs {
    pub min_light: Option<f32>,
    pub falloff: f32,
    pub sky: Option<bool>,
    pub sky_reach: u8,
    pub voxel_reach: Option<u8>,
    pub lamp_gain: Option<f32>,
    pub steps_256: Option<bool>,
    pub sky_fill: bool,
    pub lamp_level: LampLevel,
    pub stepped: bool,
    /// Seconds.
    pub flash_time: f32,
}

impl Default for LightKnobs {
    fn default() -> Self {
        LightKnobs {
            min_light: None,
            falloff: 0.9,
            sky: None,
            sky_reach: 32,
            voxel_reach: None,
            lamp_gain: None,
            steps_256: None,
            sky_fill: true,
            lamp_level: LampLevel::Full,
            stepped: false,
            flash_time: 0.12,
        }
    }
}

impl LightKnobs {
    /// Sector glow is calibrated against COLORMAP: a face lit by its own
    /// surface reads one level below it, which takes one level lost per
    /// voxel, a voxel reach of 14, and lamp light at gain 1. Its sky is off,
    /// since sky sectors glow at their own sector light, and nothing is
    /// floored, since DOOM's darkest table is all but black.
    pub fn min_light(&self, mode: LightMode) -> f32 {
        self.min_light
            .unwrap_or(if mode.sector_glow() { 0.0 } else { 0.08 })
    }

    pub fn sky(&self, mode: LightMode) -> bool {
        self.sky.unwrap_or(!mode.sector_glow())
    }

    pub fn voxel_reach(&self, mode: LightMode) -> u8 {
        self.voxel_reach
            .unwrap_or(if mode.sector_glow() { 14 } else { 32 })
    }

    pub fn lamp_gain(&self, mode: LightMode) -> f32 {
        self.lamp_gain
            .unwrap_or(if mode.sector_glow() { 1.0 } else { 1.5 })
    }

    /// Whether light holds 256 levels per voxel: sector glow's gradients
    /// at sector edges and around holes come out smoother.
    pub fn steps_256(&self, mode: LightMode) -> bool {
        self.steps_256.unwrap_or(mode.sector_glow())
    }
}

/// Parses the process arguments, printing an error and exiting on bad input.
pub fn parse() -> Args {
    match parse_from(std::env::args().skip(1), std::env::var_os("DOOM_WAD")) {
        Ok(args) => args,
        Err(message) if message.is_empty() => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    }
}

/// Parses `words`. An empty error means `--help`.
fn parse_from(
    words: impl IntoIterator<Item = String>,
    wad_env: Option<std::ffi::OsString>,
) -> Result<Args, String> {
    let mut wad = None;
    let mut args = Args {
        wad: PathBuf::new(),
        map: None,
        list_maps: false,
        units_per_voxel: 2,
        light: LightMode::Sector,
        knobs: LightKnobs::default(),
        closed_doors: false,
        pos: None,
        yaw: None,
        pitch: None,
        contrast: 1.25,
        blast_radius: 4,
        fire: 0,
        debris: 16,
        screenshot_delay: None,
        screenshot: None,
        quit_after: None,
        no_vsync: false,
    };
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--help" | "-h" => return Err(String::new()),
            "--list-maps" => args.list_maps = true,
            "--stepped" => args.knobs.stepped = true,
            "--no-vsync" => args.no_vsync = true,
            "--closed-doors" => args.closed_doors = true,
            _ => {
                let value = words
                    .next()
                    .ok_or_else(|| format!("{word} needs a value"))?;
                let number = |what: &str| -> Result<f32, String> {
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| format!("{word} needs {what}, not {value:?}"))
                };
                match word.as_str() {
                    "--wad" => wad = Some(PathBuf::from(&value)),
                    "--map" => args.map = Some(value.to_uppercase()),
                    "--units-per-voxel" => {
                        args.units_per_voxel =
                            value.parse().ok().filter(|n: &i32| *n >= 1).ok_or_else(
                                || format!("{word} needs a whole number from 1"),
                            )?
                    }
                    "--contrast" => {
                        args.contrast = number("a number above 0")?;
                        if args.contrast <= 0.0 {
                            return Err(format!("{word} needs a number above 0"));
                        }
                    }
                    "--blast-radius" => {
                        args.blast_radius =
                            value.parse().ok().filter(|n: &i32| *n >= 1).ok_or_else(
                                || format!("{word} needs a whole number from 1"),
                            )?
                    }
                    "--fire" => {
                        args.fire = value
                            .parse()
                            .map_err(|_| format!("{word} needs a whole number"))?
                    }
                    "--debris" => {
                        args.debris = value
                            .parse()
                            .map_err(|_| format!("{word} needs a whole number"))?
                    }
                    "--screenshot-delay" => {
                        args.screenshot_delay = Some(number("seconds")?)
                            .filter(|s| *s >= 0.0)
                            .ok_or_else(|| format!("{word} needs seconds, at least 0"))
                            .map(Some)?
                    }
                    "--light" => {
                        args.light = LightMode::parse(&value).ok_or_else(|| {
                            format!(
                                "{word} needs sector, propagated, both, sector-glow or flat, \
                                 not {value:?}"
                            )
                        })?
                    }
                    "--min-light" => {
                        args.knobs.min_light = Some(
                            Some(number("a brightness")?)
                                .filter(|b| (0.0..=1.0).contains(b))
                                .ok_or_else(|| format!("{word} needs a brightness from 0 to 1"))?,
                        )
                    }
                    "--falloff" => {
                        args.knobs.falloff = Some(number("a base")?)
                            .filter(|b| *b > 0.0 && *b <= 1.0)
                            .ok_or_else(|| format!("{word} needs a base above 0, up to 1"))?
                    }
                    "--sky-reach" | "--voxel-reach" => {
                        let reach = value
                            .parse()
                            .ok()
                            .filter(|n: &u8| (1..=32).contains(n))
                            .ok_or_else(|| format!("{word} needs a whole number from 1 to 32"))?;
                        if word == "--sky-reach" {
                            args.knobs.sky_reach = reach;
                        } else {
                            args.knobs.voxel_reach = Some(reach);
                        }
                    }
                    "--lamp-gain" => {
                        args.knobs.lamp_gain = Some(
                            Some(number("a gain")?)
                                .filter(|g| *g >= 0.0)
                                .ok_or_else(|| format!("{word} needs a gain, at least 0"))?,
                        )
                    }
                    "--sky-fill" | "--sky" => {
                        let on = match value.as_str() {
                            "on" => true,
                            "off" => false,
                            _ => return Err(format!("{word} needs on or off, not {value:?}")),
                        };
                        if word == "--sky" {
                            args.knobs.sky = Some(on);
                        } else {
                            args.knobs.sky_fill = on;
                        }
                    }
                    "--precision" => {
                        args.knobs.steps_256 = Some(match value.as_str() {
                            "16" => false,
                            "256" => true,
                            _ => return Err(format!("{word} needs 16 or 256, not {value:?}")),
                        })
                    }
                    "--lamp-level" => {
                        args.knobs.lamp_level = match value.as_str() {
                            "full" => LampLevel::Full,
                            "sector" => LampLevel::Sector,
                            _ => {
                                return Err(format!("{word} needs full or sector, not {value:?}"))
                            }
                        }
                    }
                    "--flash-time" => {
                        args.knobs.flash_time = Some(number("seconds")?)
                            .filter(|s| *s >= 0.0)
                            .ok_or_else(|| format!("{word} needs seconds, at least 0"))?
                    }
                    "--pos" => args.pos = Some(parse_vec3(&value)?),
                    "--yaw" => args.yaw = Some(number("degrees")?),
                    "--pitch" => args.pitch = Some(number("degrees")?),
                    "--screenshot" => args.screenshot = Some(PathBuf::from(&value)),
                    "--quit-after" => {
                        args.quit_after = Some(number("seconds")?).filter(|s| *s >= 0.0)
                    }
                    _ => return Err(format!("unknown argument {word}")),
                }
            }
        }
    }
    args.wad = wad.or_else(|| wad_env.map(PathBuf::from)).ok_or_else(|| {
        "no WAD file: pass --wad PATH or set the DOOM_WAD environment variable"
            .to_string()
    })?;
    Ok(args)
}

fn parse_vec3(text: &str) -> Result<Vec3, String> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("--pos needs X,Y,Z, not {text:?}"))?;
    match parts[..] {
        [x, y, z] => Ok(Vec3::new(x, y, z)),
        _ => Err(format!("--pos needs three numbers X,Y,Z, not {text:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_flags() {
        let args = parse(&[], Some("a.wad")).unwrap();
        assert_eq!((args.blast_radius, args.fire), (4, 0));
        let args = parse(&["--blast-radius", "6", "--fire", "8"], Some("a.wad")).unwrap();
        assert_eq!((args.blast_radius, args.fire), (6, 8));
    }

    fn parse(words: &[&str], env: Option<&str>) -> Result<Args, String> {
        parse_from(words.iter().map(|w| w.to_string()), env.map(Into::into))
    }

    #[test]
    fn wad_falls_back_to_the_environment() {
        assert_eq!(
            parse(&[], Some("a.wad")).unwrap().wad,
            PathBuf::from("a.wad")
        );
        assert_eq!(
            parse(&["--wad", "b.wad"], Some("a.wad")).unwrap().wad,
            PathBuf::from("b.wad")
        );
        let missing = parse(&[], None).unwrap_err();
        assert!(missing.contains("--wad") && missing.contains("DOOM_WAD"));
    }

    #[test]
    fn light_flags() {
        let args = parse(&[], Some("a.wad")).unwrap();
        assert_eq!(args.light, LightMode::Sector);
        assert_eq!(args.knobs, LightKnobs::default());
        for (mode, sector, propagated) in [
            ("sector", true, false),
            ("propagated", false, true),
            ("both", true, true),
            ("sector-glow", false, true),
            ("flat", false, false),
        ] {
            let args = parse(&["--light", mode], Some("a.wad")).unwrap();
            assert_eq!(args.light.name(), mode);
            assert_eq!(args.light.sector_light(), sector, "{mode}");
            assert_eq!(args.light.propagated(), propagated, "{mode}");
        }
        let args = parse(
            &[
                "--min-light",
                "0.1",
                "--falloff",
                "0.9",
                "--sky-reach",
                "20",
                "--voxel-reach",
                "12",
                "--lamp-gain",
                "3",
                "--sky-fill",
                "off",
                "--lamp-level",
                "sector",
                "--stepped",
                "--flash-time",
                "5",
                "--sky",
                "on",
                "--precision",
                "16",
            ],
            Some("a.wad"),
        )
        .unwrap();
        let knobs = args.knobs;
        let glow = LightMode::SectorGlow;
        assert_eq!((knobs.min_light(glow), knobs.falloff), (0.1, 0.9));
        assert_eq!((knobs.sky_reach, knobs.voxel_reach(glow)), (20, 12));
        assert_eq!(knobs.lamp_gain(glow), 3.0);
        assert!(knobs.sky(glow) && !knobs.steps_256(glow));
        assert!(!knobs.sky_fill && knobs.stepped);
        assert_eq!(knobs.lamp_level, LampLevel::Sector);
        assert_eq!(knobs.flash_time, 5.0);
        for bad in [
            &["--light", "dark"][..],
            &["--min-light", "2"],
            &["--falloff", "0"],
            &["--sky-reach", "33"],
            &["--voxel-reach", "0"],
            &["--sky-fill", "maybe"],
            &["--lamp-level", "7"],
            &["--sky", "maybe"],
            &["--precision", "8"],
            &["--no-sector-light"],
        ] {
            assert!(parse(bad, Some("a.wad")).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sector_glow_defaults_differ_from_propagated() {
        let knobs = LightKnobs::default();
        let (glow, propagated) = (LightMode::SectorGlow, LightMode::Propagated);
        assert!(!knobs.sky(glow) && knobs.sky(propagated));
        assert_eq!(
            (knobs.voxel_reach(glow), knobs.voxel_reach(propagated)),
            (14, 32)
        );
        assert_eq!(
            (knobs.lamp_gain(glow), knobs.lamp_gain(propagated)),
            (1.0, 1.5)
        );
        assert_eq!(
            (knobs.min_light(glow), knobs.min_light(propagated)),
            (0.0, 0.08)
        );
        assert!(knobs.steps_256(glow) && !knobs.steps_256(propagated));
    }

    #[test]
    fn camera_flags() {
        let args = parse(
            &[
                "--pos", "1,2.5,-3", "--yaw", "90", "--pitch", "-10", "--map", "e1m2",
            ],
            Some("a.wad"),
        )
        .unwrap();
        assert_eq!(args.pos, Some(Vec3::new(1.0, 2.5, -3.0)));
        assert_eq!(args.yaw, Some(90.0));
        assert_eq!(args.pitch, Some(-10.0));
        assert_eq!(args.map.as_deref(), Some("E1M2"));
        assert!(parse(&["--pos", "1,2"], Some("a.wad")).is_err());
        assert!(parse(&["--units-per-voxel", "0"], Some("a.wad")).is_err());
        assert!(parse(&["--blast-radius", "0"], Some("a.wad")).is_err());
        assert!(parse(&["--fire", "-1"], Some("a.wad")).is_err());
        assert!(parse(&["--bogus"], Some("a.wad")).is_err());
    }

    #[test]
    fn debris_flags() {
        let args = parse(&[], Some("a.wad")).unwrap();
        assert_eq!((args.debris, args.screenshot_delay), (16, None));
        let args = parse(
            &["--debris", "0", "--screenshot-delay", "0.5"],
            Some("a.wad"),
        )
        .unwrap();
        assert_eq!((args.debris, args.screenshot_delay), (0, Some(0.5)));
        assert!(parse(&["--debris", "-1"], Some("a.wad")).is_err());
        assert!(parse(&["--screenshot-delay", "-1"], Some("a.wad")).is_err());
    }
}
