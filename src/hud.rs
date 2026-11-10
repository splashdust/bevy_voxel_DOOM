//! The overlay: a line of status text (map, scale, light mode, position,
//! the propagated light there, monsters awake, frame rate), a crosshair,
//! the chaingun at the bottom centre, health, and the red of damage and
//! death.
//!
//! The chaingun is drawn as DOOM draws its weapon sprites on a 320×200
//! screen: the patch's top-left at `(1 − left_offset, 32 − top_offset)`,
//! moved by the walking bob, scaled to the window's height with DOOM's tall
//! pixels (200 lines across the height, 240 columns' width per height). It
//! takes the light of the sector the player stands in; the muzzle flash is
//! full-bright.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    window::PrimaryWindow,
};

use bevy_voxel_world::prelude::*;

use crate::actors::{census, Actor};
use crate::camera::FlyCamera;
use crate::info::{CHAINGUN_FLASH, CHAINGUN_SPRITE};
use crate::player::{Mode, Player};
use crate::sprites::SpriteBank;
use crate::weapon::Weapon;
use crate::world::{DoomWorld, GameMap};

/// What the HUD names the map by.
#[derive(Resource, Clone)]
pub struct MapLabel {
    pub name: String,
    pub units_per_voxel: i32,
    /// The light mode's name.
    pub light: &'static str,
}

#[derive(Component)]
pub struct HudText;

#[derive(Component)]
pub struct HealthText;

#[derive(Component)]
pub struct DeathText;

#[derive(Component)]
pub struct RedOverlay;

/// The weapon sprite, and its flash over it.
#[derive(Component)]
pub struct GunSprite {
    flash: bool,
}

pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(8.0),
            ..default()
        },
        HudText,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        GlobalZIndex(5),
        RedOverlay,
    ));
    for flash in [false, true] {
        commands.spawn((
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(1 + flash as i32),
            GunSprite { flash },
        ));
    }
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(36.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.1, 0.1)),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(10.0),
            left: Val::Px(16.0),
            ..default()
        },
        GlobalZIndex(3),
        HealthText,
    ));
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(28.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextShadow::default(),
        TextLayout::justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(40.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(6),
        DeathText,
    ));
}

type OnlyHud = (With<HudText>, Without<HealthText>, Without<DeathText>);
type OnlyHealth = (With<HealthText>, Without<HudText>, Without<DeathText>);
type OnlyDeath = (With<DeathText>, Without<HudText>, Without<HealthText>);

/// Crosshair arm length and thickness, in logical pixels.
const CROSSHAIR_SIZE: f32 = 14.0;
const CROSSHAIR_WIDTH: f32 = 2.0;

pub fn spawn_crosshair(mut commands: Commands) {
    for (w, h) in [
        (CROSSHAIR_SIZE, CROSSHAIR_WIDTH),
        (CROSSHAIR_WIDTH, CROSSHAIR_SIZE),
    ] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(w),
                height: Val::Px(h),
                margin: UiRect {
                    left: Val::Px(-w / 2.0),
                    top: Val::Px(-h / 2.0),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    label: Res<MapLabel>,
    diagnostics: Res<DiagnosticsStore>,
    camera: Single<(&Transform, &Player), With<FlyCamera>>,
    voxel_world: VoxelWorld<DoomWorld>,
    actors: Query<&Actor>,
    mut text: Single<&mut Text, OnlyHud>,
    mut health: Single<&mut Text, OnlyHealth>,
    mut death: Single<&mut Text, OnlyDeath>,
    mut red: Single<&mut BackgroundColor, With<RedOverlay>>,
) {
    let (transform, player) = camera.into_inner();
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .map_or("--".to_string(), |fps| format!("{fps:.0}"));
    let p = transform.translation.floor().as_ivec3();
    let levels = voxel_world.light_at(p).map_or(String::new(), |l| {
        format!("  sky {:.1} lamp {:.1}", l.sky, l.voxel)
    });
    let (alive, awake) = census(&actors);
    let mode = match (player.mode, player.god) {
        (Mode::Fly, _) => "  fly",
        (Mode::Walk, true) => "  god",
        (Mode::Walk, false) => "",
    };
    text.0 = format!(
        "{}  {} u/vox  light {}{mode}\n{} {} {}{levels}  monsters {alive}, {awake} awake  {fps} fps",
        label.name, label.units_per_voxel, label.light, p.x, p.y, p.z
    );
    health.0 = format!("{}%", player.health);
    let dead = !player.alive();
    death.0 = if dead {
        "You died.\nClick or press R to respawn.".into()
    } else {
        String::new()
    };
    let alpha = if dead { 0.55 } else { player.hurt * 0.6 };
    red.0 = Color::srgba(0.8, 0.0, 0.0, alpha);
}

/// Places the chaingun and its flash.
#[allow(clippy::too_many_arguments)]
pub fn draw_gun(
    time: Res<Time>,
    map: Res<GameMap>,
    weapon: Res<Weapon>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<&Player, With<FlyCamera>>,
    mut bank: ResMut<SpriteBank>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<(&GunSprite, &mut ImageNode, &mut Node, &mut Visibility)>,
    mut phase: Local<f32>,
) {
    let player = *camera;
    let firing = weapon.since_shot < crate::weapon::SHOT_INTERVAL * 1.5;
    // DOOM's bob: up to 16 units, a turn every 64 tics.
    *phase += time.delta_secs() * 35.0 / 64.0 * std::f32::consts::TAU;
    let speed_units = player.ground_speed * map.units / 35.0;
    let bob = if firing || !player.grounded {
        0.0
    } else {
        (speed_units * speed_units / 4.0).min(16.0)
    };
    let (sx, sy) = (
        1.0 + bob * phase.cos(),
        32.0 + bob * (*phase % std::f32::consts::PI).sin(),
    );
    let table = if map.sector_light {
        map.map
            .sprite_table(player.feet.x.floor() as i32, player.feet.z.floor() as i32)
    } else {
        0
    };
    let height = window.height();
    let (px_w, px_h) = (height / 240.0, height / 200.0);
    let flash_on = weapon.since_shot < 5.0 / 35.0;
    let odd = weapon.shots.is_multiple_of(2);
    for (gun, mut image, mut node, mut visibility) in &mut sprites {
        let (sprite, letter, table) = if gun.flash {
            (CHAINGUN_FLASH, if odd { b'B' } else { b'A' }, 0)
        } else {
            (
                CHAINGUN_SPRITE,
                if firing && odd { b'B' } else { b'A' },
                table,
            )
        };
        let show =
            player.alive() && player.mode == Mode::Walk && (!gun.flash || flash_on);
        let found = show
            .then(|| bank.ui_image(sprite, letter, table, &map, &mut images))
            .flatten();
        let Some((handle, patch)) = found else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        if image.image != handle {
            image.image = handle;
        }
        let x = sx - patch.left_offset as f32;
        let y = sy - patch.top_offset as f32;
        node.left = Val::Px(window.width() / 2.0 + (x - 160.0) * px_w);
        node.top = Val::Px(y * px_h);
        node.width = Val::Px(patch.picture.width as f32 * px_w);
        node.height = Val::Px(patch.picture.height as f32 * px_h);
        visibility.set_if_neq(Visibility::Inherited);
    }
}
