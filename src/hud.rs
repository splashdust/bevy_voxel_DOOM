//! The overlay: a line of status text (map, scale, light mode, position,
//! the propagated light there, monsters awake, frame rate), a crosshair,
//! the weapon in hand at the bottom centre, health, the ammo of the weapon
//! in hand, every ammo count and limit, the weapons owned by number key,
//! the keys held, messages such as pickups', and the red of damage and
//! death and yellow of a pickup.
//!
//! The weapon is drawn as DOOM draws its weapon sprites on a 320×200
//! screen: the patch's top-left at `(1 − left_offset, sy − top_offset)`,
//! `sy` 32 in hand and lower while it raises or lowers, moved by the
//! walking bob while it waits to fire, scaled to the window's height with
//! DOOM's tall pixels (200 lines across the height, 240 columns' width per
//! height). It takes the light of the sector the player stands in; the
//! muzzle flash is full-bright.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    window::PrimaryWindow,
};

use bevy_voxel_world::prelude::*;

use crate::actors::{census, Actor};
use crate::camera::FlyCamera;
use crate::inventory::{AmmoKind, Key, WeaponKind};
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

/// hu_stuff.c `HU_MSGTIMEOUT`: seconds a message shows.
const MESSAGE_SECONDS: f32 = 4.0;

/// The message line: a pickup, a locked door. Messages are printed too.
#[derive(Resource, Default)]
pub struct HudMessage {
    text: String,
    left: f32,
}

impl HudMessage {
    pub fn say(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.left = MESSAGE_SECONDS;
        println!("message: {}", self.text);
    }
}

#[derive(Component)]
pub struct HudText;

/// The parts of the status panel that show the inventory.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Message,
    /// The ammo of the weapon in hand.
    Ammo,
    /// Every ammo count and limit.
    AmmoTable,
    /// Number key `n`: lit when a weapon on it is owned.
    Arms(u8),
}

/// A key's icon, shown while it is held.
#[derive(Component)]
pub struct KeyIcon(Key);

/// The height of a key icon, in logical pixels.
const KEY_ICON: f32 = 22.0;

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
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        TextShadow::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(44.0),
            left: Val::Px(8.0),
            ..default()
        },
        GlobalZIndex(4),
        Status::Message,
    ));
    // Bottom right: the ammo in hand, every ammo, the arms and the keys.
    let label = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(10.0),
                right: Val::Px(16.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(4.0),
                ..default()
            },
            GlobalZIndex(3),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|keys| {
                    for key in Key::ALL {
                        keys.spawn((
                            ImageNode::default(),
                            Node::default(),
                            Visibility::Hidden,
                            KeyIcon(key),
                        ));
                    }
                });
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|arms| {
                    for n in 1..=7 {
                        arms.spawn((
                            Text::new(n.to_string()),
                            label(16.0),
                            TextColor(Color::srgb(0.35, 0.35, 0.35)),
                            TextShadow::default(),
                            Status::Arms(n),
                        ));
                    }
                });
            panel.spawn((
                Text::new(""),
                label(13.0),
                TextColor(Color::srgb(0.9, 0.85, 0.6)),
                TextShadow::default(),
                TextLayout::justify(Justify::Right),
                Status::AmmoTable,
            ));
            panel.spawn((
                Text::new(""),
                label(36.0),
                TextColor(Color::srgb(0.85, 0.1, 0.1)),
                TextShadow::default(),
                Status::Ammo,
            ));
        });
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
    red.0 = if dead {
        Color::srgba(0.8, 0.0, 0.0, 0.55)
    } else if player.hurt >= player.bonus {
        Color::srgba(0.8, 0.0, 0.0, player.hurt * 0.6)
    } else {
        Color::srgba(0.85, 0.7, 0.2, player.bonus * 0.4)
    };
}

/// Fills in the inventory panel and the message line.
pub fn update_status(
    time: Res<Time>,
    mut message: ResMut<HudMessage>,
    player: Single<&Player, With<FlyCamera>>,
    mut texts: Query<(&Status, &mut Text, &mut TextColor)>,
) {
    message.left -= time.delta_secs();
    let inventory = &player.inventory;
    for (status, mut text, mut colour) in &mut texts {
        let content = match *status {
            Status::Message => {
                if message.left > 0.0 {
                    message.text.clone()
                } else {
                    String::new()
                }
            }
            Status::Ammo => inventory
                .ready
                .ammo()
                .map_or(String::new(), |kind| inventory.ammo(kind).to_string()),
            Status::AmmoTable => AmmoKind::ALL
                .iter()
                .map(|&kind| {
                    format!(
                        "{} {:>3} / {}",
                        kind.label(),
                        inventory.ammo(kind),
                        inventory.max(kind)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Status::Arms(n) => {
                let owned = WeaponKind::ALL
                    .into_iter()
                    .any(|w| w.slot() == n && inventory.has(w));
                let wanted = Color::srgb(0.95, 0.8, 0.2);
                let in_hand = inventory.pending.unwrap_or(inventory.ready).slot() == n;
                let want = match (owned, in_hand) {
                    (_, true) => Color::WHITE,
                    (true, false) => wanted,
                    (false, false) => Color::srgb(0.35, 0.35, 0.35),
                };
                if colour.0 != want {
                    colour.0 = want;
                }
                continue;
            }
        };
        if text.0 != content {
            text.0 = content;
        }
    }
}

/// Shows the keys held, as their items' sprites.
pub fn draw_keys(
    map: Res<GameMap>,
    player: Single<&Player, With<FlyCamera>>,
    mut bank: ResMut<SpriteBank>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Query<(&KeyIcon, &mut ImageNode, &mut Node, &mut Visibility)>,
) {
    for (icon, mut image, mut node, mut visibility) in &mut icons {
        let held = player.inventory.holds(icon.0);
        let found = held
            .then(|| bank.ui_image(icon.0.sprite(), b'A', 0, &map, &mut images))
            .flatten();
        let Some((handle, patch)) = found else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        if image.image != handle {
            image.image = handle;
        }
        let pic = &patch.picture;
        let scale = KEY_ICON / pic.height.max(1) as f32;
        node.width = Val::Px(pic.width as f32 * scale * 1.2);
        node.height = Val::Px(KEY_ICON);
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// Places the weapon in hand and its flash.
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
    // DOOM's bob: up to 16 units, a turn every 64 tics, while it waits.
    *phase += time.delta_secs() * 35.0 / 64.0 * std::f32::consts::TAU;
    let speed_units = player.ground_speed * map.units / 35.0;
    let bob = if !weapon.bobbing() || !player.grounded {
        0.0
    } else {
        (speed_units * speed_units / 4.0).min(16.0)
    };
    let (sx, sy) = (
        1.0 + bob * phase.cos(),
        weapon.sy + bob * (*phase % std::f32::consts::PI).sin(),
    );
    let table = if map.sector_light {
        map.map
            .sprite_table(player.feet.x.floor() as i32, player.feet.z.floor() as i32)
    } else {
        0
    };
    let height = window.height();
    let (px_w, px_h) = (height / 240.0, height / 200.0);
    let inventory = &player.inventory;
    for (gun, mut image, mut node, mut visibility) in &mut sprites {
        let frame = if gun.flash {
            weapon
                .flash_frame(inventory)
                .map(|(sprite, letter)| (sprite, letter, 0))
        } else {
            let (sprite, letter, bright) = weapon.frame(inventory);
            Some((sprite, letter, if bright { 0 } else { table }))
        };
        let show = player.mode == Mode::Walk && weapon.sy < 128.0;
        let found = frame.filter(|_| show).and_then(|(sprite, letter, table)| {
            bank.ui_image(sprite, letter, table, &map, &mut images)
        });
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
