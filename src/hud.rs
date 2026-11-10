//! A small text overlay: map, scale, camera voxel position and frame rate;
//! and a crosshair at the screen centre.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};

use crate::camera::FlyCamera;

/// What the HUD names the map by.
#[derive(Resource, Clone)]
pub struct MapLabel {
    pub name: String,
    pub units_per_voxel: i32,
}

#[derive(Component)]
pub struct HudText;

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
}

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

pub fn update_hud(
    label: Res<MapLabel>,
    diagnostics: Res<DiagnosticsStore>,
    camera: Single<&Transform, With<FlyCamera>>,
    mut text: Single<&mut Text, With<HudText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .map_or("--".to_string(), |fps| format!("{fps:.0}"));
    let p = camera.translation.floor().as_ivec3();
    text.0 = format!(
        "{}  {} u/vox\n{} {} {}  {fps} fps",
        label.name, label.units_per_voxel, p.x, p.y, p.z
    );
}
