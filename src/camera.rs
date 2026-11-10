//! The fly camera: mouse look with the cursor captured, WASD, Space and Ctrl
//! up and down, Shift faster. Esc frees the cursor; a click captures it again.

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    render::view::{DebandDither, Tonemapping},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_voxel_world::prelude::*;

use crate::world::DoomWorld;

/// Walking speed in DOOM map units per second, about DOOM's running speed.
const SPEED_UNITS: f32 = 300.0;
const SHIFT_FACTOR: f32 = 3.0;
/// Radians per pixel of mouse motion.
const MOUSE_SENSITIVITY: f32 = 0.0022;
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
    fn rotation(&self) -> Quat {
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

pub fn spawn_camera(mut commands: Commands, start: Res<CameraStart>) {
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

pub fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    camera: Single<(&mut FlyCamera, &mut Transform)>,
) {
    let (mut fly, mut transform) = camera.into_inner();
    if cursor.grab_mode != CursorGrabMode::None {
        fly.yaw -= motion.delta.x * MOUSE_SENSITIVITY;
        fly.pitch = (fly.pitch - motion.delta.y * MOUSE_SENSITIVITY).clamp(-1.55, 1.55);
    }
    transform.rotation = fly.rotation();

    let mut direction = Vec3::ZERO;
    let pressed = |key| keys.pressed(key);
    if pressed(KeyCode::KeyW) {
        direction += *transform.forward();
    }
    if pressed(KeyCode::KeyS) {
        direction -= *transform.forward();
    }
    if pressed(KeyCode::KeyD) {
        direction += *transform.right();
    }
    if pressed(KeyCode::KeyA) {
        direction -= *transform.right();
    }
    if pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if pressed(KeyCode::ControlLeft) || pressed(KeyCode::ControlRight) {
        direction -= Vec3::Y;
    }
    let mut speed = fly.speed;
    if pressed(KeyCode::ShiftLeft) || pressed(KeyCode::ShiftRight) {
        speed *= SHIFT_FACTOR;
    }
    transform.translation += direction.normalize_or_zero() * speed * time.delta_secs();
}
