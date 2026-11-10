//! doom_voxels: a vanilla DOOM map from a WAD file as a `bevy_voxel_world`
//! voxel world you walk through, with DOOM's monsters. See `args::USAGE`
//! for the flags.

mod actors;
mod args;
mod camera;
mod capture;
mod collide;
mod hud;
mod info;
mod input;
mod player;
mod sound;
mod sprites;
mod weapon;
mod world;

use std::sync::Arc;
use std::time::Instant;

use bevy::{diagnostic::FrameTimeDiagnosticsPlugin, prelude::*, window::PresentMode};
use doom_voxelize::{Options, VoxelMap};
use doom_wad::Wad;

use crate::camera::CameraStart;
use crate::hud::MapLabel;
use crate::player::{Mode, PlayerStart};
use crate::world::{DoomWorldPlugin, GameMap};

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
        sector_light: args.light.sector_light(),
        contrast: args.contrast,
        open_doors: !args.closed_doors,
        lamps: args.light.propagated().then_some(args.knobs.lamp_level),
        sector_glow: args.light.sector_glow(),
    };
    let started = Instant::now();
    let map = VoxelMap::build(&wad, &map_name, &options).unwrap_or_else(|e| {
        eprintln!("error: can't build {map_name}: {e}");
        std::process::exit(1);
    });
    let things = wad.map(&map_name).map(|m| m.things).unwrap_or_default();
    let sprites = sprites::SpriteBank::load(&wad);
    let sounds = (!args.mute).then(|| sound::SoundBank::load(&wad));
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
    if args.light.propagated() {
        println!(
            "light {}: {} lamp pillars, {} emissive materials",
            args.light.name(),
            map.pillar_count(),
            map.emissive_materials().len()
        );
    }

    let (spawn, spawn_yaw) = map.spawn();
    let map = Arc::new(map);
    let s = args.units_per_voxel as f32;
    let start = CameraStart {
        position: args.pos.unwrap_or(Vec3::from_array(spawn)),
        yaw: args.yaw.map_or(spawn_yaw, f32::to_radians),
        pitch: args.pitch.unwrap_or(0.0).to_radians(),
        speed: CameraStart::speed_for(args.units_per_voxel),
        view_distance: world::view_distance_voxels(args.units_per_voxel),
    };
    let player_start = PlayerStart {
        feet: start.position - Vec3::Y * player::EYE / s,
        yaw: start.yaw,
        mode: if args.fly { Mode::Fly } else { Mode::Walk },
        god: args.god,
        units: s,
    };
    let [r, g, b] = map.sky_colour();
    let capturing = args.screenshot.is_some() || args.script.is_some();
    let flash = args.light.propagated().then(|| world::flash_material(&map));
    let game_map = GameMap {
        map: Arc::clone(&map),
        flash,
        units: s,
        sector_light: matches!(
            args.light,
            args::LightMode::Sector | args::LightMode::Both | args::LightMode::SectorGlow
        ),
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("doom_voxels: {map_name}"),
            resolution: (1280, 800).into(),
            present_mode: if args.no_vsync {
                PresentMode::AutoNoVsync
            } else {
                PresentMode::AutoVsync
            },
            ..default()
        }),
        primary_cursor_options: Some(camera::captured(!capturing)),
        ..default()
    }))
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
    .add_plugins(DoomWorldPlugin {
        map: Arc::clone(&map),
        units_per_voxel: args.units_per_voxel,
        light: args.light,
        knobs: args.knobs.clone(),
    })
    .add_plugins(capture::CapturePlugin {
        screenshot: args.screenshot.clone(),
        quit_after: args.quit_after,
        track_settling: args.fire > 0 || args.script.is_some(),
        frame_times: args.frame_times,
    })
    .add_plugins(weapon::WeaponPlugin {
        radius: args.blast_radius,
        range: world::view_distance_voxels(args.units_per_voxel),
        scripted_shots: args.fire,
        flash_time: args.knobs.flash_time,
    })
    .add_plugins(actors::ActorsPlugin {
        things,
        awake: args.awake,
        print: args.list_things,
        freeze: args.freeze,
    })
    .add_plugins(sound::SoundPlugin { bank: sounds })
    .insert_resource(ClearColor(Color::srgb_u8(r, g, b)))
    .insert_resource(start)
    .insert_resource(player_start)
    .insert_resource(game_map)
    .insert_resource(sprites)
    .init_resource::<world::EditedChunks>()
    .init_resource::<player::Blockers>()
    .init_resource::<input::Intent>()
    .insert_resource(MapLabel {
        name: map_name,
        units_per_voxel: args.units_per_voxel,
        light: args.light.name(),
    })
    .add_systems(
        Startup,
        (
            camera::spawn_camera,
            hud::spawn_hud,
            hud::spawn_crosshair,
            sprites::setup_sprites,
        ),
    )
    .add_systems(
        Update,
        (
            input::read_devices,
            camera::grab_cursor,
            camera::look,
            camera::fly,
            world::track_edited_chunks,
            player::walk,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (sprites::draw_billboards, hud::update_hud, hud::draw_gun).after(player::walk),
    );
    if let Some(steps) = args.script.clone() {
        app.insert_resource(input::Script::new(steps)).add_systems(
            Update,
            input::run_script
                .after(input::read_devices)
                .before(camera::look)
                .after(capture::CaptureSet::Track)
                .before(capture::CaptureSet::Shoot),
        );
    }
    app.run()
}
