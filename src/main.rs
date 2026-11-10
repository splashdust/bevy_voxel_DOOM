//! doom_voxels: a vanilla DOOM map from a WAD file, shown as a
//! `bevy_voxel_world` voxel world with a fly camera. See `args::USAGE` for
//! the flags.

mod args;
mod camera;
mod capture;
mod hud;
mod weapon;
mod world;

use std::sync::Arc;
use std::time::Instant;

use bevy::{diagnostic::FrameTimeDiagnosticsPlugin, prelude::*};
use doom_voxelize::{Options, VoxelMap};
use doom_wad::Wad;

use crate::camera::CameraStart;
use crate::hud::MapLabel;
use crate::world::DoomWorldPlugin;

fn main() -> AppExit {
    let args = args::parse();

    let wad = Wad::open(&args.wad).unwrap_or_else(|e| {
        eprintln!("error: can't read {}: {e}", args.wad.display());
        std::process::exit(1);
    });
    let maps = wad.map_names();
    if args.list_maps {
        if maps.is_empty() {
            println!("{} lists no maps", args.wad.display());
        }
        for name in &maps {
            println!("{name}");
        }
        return AppExit::Success;
    }
    let map_name = args
        .map
        .clone()
        .or_else(|| maps.first().cloned())
        .unwrap_or_else(|| "E1M1".to_string());
    if !maps.is_empty() && !maps.contains(&map_name) {
        eprintln!(
            "error: {} has no map {map_name}; it has {}",
            args.wad.display(),
            maps.join(" ")
        );
        std::process::exit(1);
    }

    let options = Options {
        units_per_voxel: args.units_per_voxel,
        sector_light: args.sector_light,
        open_doors: !args.closed_doors,
    };
    let started = Instant::now();
    let map = VoxelMap::build(&wad, &map_name, &options).unwrap_or_else(|e| {
        eprintln!("error: can't build {map_name}: {e}");
        std::process::exit(1);
    });
    drop(wad);
    let (min, max) = map.bounds();
    println!(
        "{map_name} at {} units/voxel: built in {:.2} s, bounds {min:?}..{max:?} ({}×{}×{} voxels)",
        args.units_per_voxel,
        started.elapsed().as_secs_f32(),
        max[0] - min[0],
        max[1] - min[1],
        max[2] - min[2],
    );

    let (spawn, spawn_yaw) = map.spawn();
    let map = Arc::new(map);
    let start = CameraStart {
        position: args.pos.unwrap_or(Vec3::from_array(spawn)),
        yaw: args.yaw.map_or(spawn_yaw, f32::to_radians),
        pitch: args.pitch.unwrap_or(0.0).to_radians(),
        speed: CameraStart::speed_for(args.units_per_voxel),
        view_distance: world::view_distance_voxels(args.units_per_voxel),
    };
    let [r, g, b] = map.sky_colour();
    let capturing = args.screenshot.is_some();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("doom_voxels: {map_name}"),
            resolution: (1280, 800).into(),
            ..default()
        }),
        primary_cursor_options: Some(camera::captured(!capturing)),
        ..default()
    }))
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
    .add_plugins(DoomWorldPlugin {
        map: Arc::clone(&map),
        units_per_voxel: args.units_per_voxel,
    })
    .add_plugins(capture::CapturePlugin {
        screenshot: args.screenshot.clone(),
        quit_after: args.quit_after,
        track_settling: args.fire > 0,
    })
    .add_plugins(weapon::WeaponPlugin {
        map,
        radius: args.blast_radius,
        range: world::view_distance_voxels(args.units_per_voxel),
        scripted_shots: args.fire,
    })
    .insert_resource(ClearColor(Color::srgb_u8(r, g, b)))
    .insert_resource(start)
    .insert_resource(MapLabel {
        name: map_name,
        units_per_voxel: args.units_per_voxel,
    })
    .add_systems(
        Startup,
        (camera::spawn_camera, hud::spawn_hud, hud::spawn_crosshair),
    )
    .add_systems(
        Update,
        (camera::grab_cursor, camera::fly, hud::update_hud).chain(),
    );
    app.run()
}
