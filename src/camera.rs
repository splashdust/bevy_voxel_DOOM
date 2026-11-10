//! The camera: mouse look with the cursor captured, and the noclip fly
//! mode, WASD, Space and Ctrl up and down, Shift faster. Esc frees the
//! cursor; a click captures it again. Walking is `player`'s.

use bevy::{
    prelude::*,
    render::view::{DebandDither, Tonemapping},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_voxel_world::prelude::*;

use crate::input::Intent;
use crate::player::{Mode, Player};
use crate::world::DoomWorld;

/// Flying speed in DOOM map units per second, about DOOM's running speed.
const SPEED_UNITS: f32 = 300.0;
const SHIFT_FACTOR: f32 = 3.0;
/// Vertical field of view: about 90° across in a 16:10 window.
const FOV_DEGREES: f32 = 64.0;

#[derive(Component)]
pub struct FlyCamera {
    /// Radians around +y, 0 facing −z.
    pub yaw: f32,
    /// Radians, positive looking up.
    pub pitch: f32,
    /// Voxels per second.
    pub speed: f32,
}

impl FlyCamera {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

/// Where the camera starts and how fast it flies.
#[derive(Resource, Clone, Copy)]
pub struct CameraStart {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub view_distance: f32,
}

impl CameraStart {
    pub fn speed_for(units_per_voxel: i32) -> f32 {
        SPEED_UNITS / units_per_voxel as f32
    }
}

pub fn spawn_camera(
    mut commands: Commands,
    start: Res<CameraStart>,
    player: Res<crate::player::PlayerStart>,
) {
    let fly = FlyCamera {
        yaw: start.yaw,
        pitch: start.pitch,
        speed: start.speed,
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            near: 0.05,
            far: start.view_distance * 2.0,
            ..default()
        }),
        // The palette shader writes final colours: nothing may change them.
        Tonemapping::None,
        DebandDither::Disabled,
        Transform::from_translation(start.position).with_rotation(fly.rotation()),
        fly,
        Player::new(&player),
        VoxelWorldCamera::<DoomWorld>::default(),
    ));
}

pub fn captured(on: bool) -> CursorOptions {
    CursorOptions {
        visible: !on,
        grab_mode: if on {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        },
        ..default()
    }
}

pub fn grab_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        **cursor = captured(false);
    } else if buttons.just_pressed(MouseButton::Left) {
        **cursor = captured(true);
    }
}

/// Turns the camera as the mouse or script says.
pub fn look(intent: Res<Intent>, camera: Single<(&mut FlyCamera, &mut Transform)>) {
    let (mut fly, mut transform) = camera.into_inner();
    fly.yaw += intent.turn;
    fly.pitch = intent
        .set_pitch
        .unwrap_or(fly.pitch + intent.look_up)
        .clamp(-1.55, 1.55);
    transform.rotation = fly.rotation();
}

/// Flies the camera through walls, in fly mode.
pub fn fly(
    time: Res<Time>,
    intent: Res<Intent>,
    camera: Single<(&FlyCamera, &mut Transform, &Player)>,
) {
    let (fly, mut transform, player) = camera.into_inner();
    if player.mode != Mode::Fly {
        return;
    }
    let direction = *transform.forward() * intent.forward
        + *transform.right() * intent.strafe
        + Vec3::Y * intent.up;
    let mut speed = fly.speed;
    if intent.run {
        speed *= SHIFT_FACTOR;
    }
    transform.translation += direction.normalize_or_zero() * speed * time.delta_secs();
}
