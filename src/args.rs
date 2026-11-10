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
                          flat: full-bright, no light at all
  --min-light B           propagated light's least brightness, 0 to 1 (default 0.08)
  --falloff BASE          each light level is BASE times as bright as the one above (default 0.9)
  --sky-reach N           voxels sky light spreads past where it falls, 1 to 32 (default 32)
  --voxel-reach N         voxels a lamp's light spreads, 1 to 32 (default 32)
  --lamp-gain G           lamp light at level 15 as a brightness, before the cap at 1 (default 1.5)
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
  --screenshot PATH       wait for the world around the camera to finish, save a PNG, exit
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
    pub blast_radius: i32,
    pub fire: u32,
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
    /// Full-bright colours, no light at all.
    Flat,
}

impl LightMode {
    fn parse(text: &str) -> Option<LightMode> {
        Some(match text {
            "sector" => LightMode::Sector,
            "propagated" => LightMode::Propagated,
            "both" => LightMode::Both,
            "flat" => LightMode::Flat,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            LightMode::Sector => "sector",
            LightMode::Propagated => "propagated",
            LightMode::Both => "both",
            LightMode::Flat => "flat",
        }
    }

    /// Whether DOOM's sector light is baked into the colours.
    pub fn sector_light(self) -> bool {
        matches!(self, LightMode::Sector | LightMode::Both)
    }

    /// Whether the world propagates light, with lamps and flashes.
    pub fn propagated(self) -> bool {
        matches!(self, LightMode::Propagated | LightMode::Both)
    }
}

/// The settings of propagated light the command line can change.
#[derive(Clone, Debug, PartialEq)]
pub struct LightKnobs {
    pub min_light: f32,
    pub falloff: f32,
    pub sky_reach: u8,
    pub voxel_reach: u8,
    pub lamp_gain: f32,
    pub sky_fill: bool,
    pub lamp_level: LampLevel,
    pub stepped: bool,
    /// Seconds.
    pub flash_time: f32,
}

impl Default for LightKnobs {
    fn default() -> Self {
        LightKnobs {
            min_light: 0.08,
            falloff: 0.9,
            sky_reach: 32,
            voxel_reach: 32,
            lamp_gain: 1.5,
            sky_fill: true,
            lamp_level: LampLevel::Full,
            stepped: false,
            flash_time: 0.12,
        }
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
        blast_radius: 4,
        fire: 0,
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
                    "--light" => {
                        args.light = LightMode::parse(&value).ok_or_else(|| {
                            format!("{word} needs sector, propagated, both or flat, not {value:?}")
                        })?
                    }
                    "--min-light" => {
                        args.knobs.min_light = Some(number("a brightness")?)
                            .filter(|b| (0.0..=1.0).contains(b))
                            .ok_or_else(|| format!("{word} needs a brightness from 0 to 1"))?
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
                            args.knobs.voxel_reach = reach;
                        }
                    }
                    "--lamp-gain" => {
                        args.knobs.lamp_gain = Some(number("a gain")?)
                            .filter(|g| *g >= 0.0)
                            .ok_or_else(|| format!("{word} needs a gain, at least 0"))?
                    }
                    "--sky-fill" => {
                        args.knobs.sky_fill = match value.as_str() {
                            "on" => true,
                            "off" => false,
                            _ => return Err(format!("{word} needs on or off, not {value:?}")),
                        }
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
            ],
            Some("a.wad"),
        )
        .unwrap();
        let knobs = args.knobs;
        assert_eq!((knobs.min_light, knobs.falloff), (0.1, 0.9));
        assert_eq!((knobs.sky_reach, knobs.voxel_reach), (20, 12));
        assert_eq!(knobs.lamp_gain, 3.0);
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
            &["--no-sector-light"],
        ] {
            assert!(parse(bad, Some("a.wad")).is_err(), "{bad:?}");
        }
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
}
