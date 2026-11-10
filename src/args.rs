//! Command-line arguments, parsed by hand.

use std::path::PathBuf;

use bevy::prelude::*;

pub const USAGE: &str = "\
doom_voxels: a vanilla DOOM map as a bevy_voxel_world voxel world

usage: doom_voxels --wad PATH [options]

  --wad PATH              the WAD file; falls back to the DOOM_WAD environment variable
  --map NAME              the map to show, like E1M1 or MAP01; the WAD's first map by default
  --list-maps             print the WAD's maps and exit
  --units-per-voxel N     DOOM map units per voxel (default 2)
  --no-sector-light       full-bright colours instead of DOOM's sector light
  --closed-doors          keep door sectors closed
  --pos X,Y,Z             start the camera here, in voxel coordinates
  --yaw DEG               start yaw, 0 facing DOOM's north, positive turning left
  --pitch DEG             start pitch, positive looking up
  --screenshot PATH       wait for the world around the camera to finish, save a PNG, exit
  --quit-after S          exit after S seconds
  --help                  this text";

#[derive(Resource, Clone, Debug)]
pub struct Args {
    pub wad: PathBuf,
    pub map: Option<String>,
    pub list_maps: bool,
    pub units_per_voxel: i32,
    pub sector_light: bool,
    pub closed_doors: bool,
    pub pos: Option<Vec3>,
    /// Degrees.
    pub yaw: Option<f32>,
    /// Degrees.
    pub pitch: Option<f32>,
    pub screenshot: Option<PathBuf>,
    pub quit_after: Option<f32>,
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
        sector_light: true,
        closed_doors: false,
        pos: None,
        yaw: None,
        pitch: None,
        screenshot: None,
        quit_after: None,
    };
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--help" | "-h" => return Err(String::new()),
            "--list-maps" => args.list_maps = true,
            "--no-sector-light" => args.sector_light = false,
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
        assert!(parse(&["--bogus"], Some("a.wad")).is_err());
    }
}
