//! `--screenshot PATH`: waits until the world around the camera has finished
//! generating and meshing, saves the window as a PNG and exits. Also
//! `--quit-after S`.
//!
//! The world counts as finished once every spawned chunk has been `Ready` for
//! `QUIET_FRAMES` frames in a row with no chunk spawned or despawned. Spawning
//! rays only find chunks behind ones already generated, so a pause in new
//! chunks is the sign that streaming has reached everything in view.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::{
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
};
use bevy_voxel_world::prelude::*;

use crate::world::DoomWorld;

/// Frames in a row with every chunk ready and none coming or going. Long
/// enough for meshes to reach the GPU and their pipeline to compile.
const QUIET_FRAMES: u32 = 60;
/// Taken anyway after this long, with a warning.
const GIVE_UP: Duration = Duration::from_secs(120);

pub struct CapturePlugin {
    pub screenshot: Option<PathBuf>,
    pub quit_after: Option<f32>,
}

#[derive(Resource)]
struct ShotPlan {
    path: PathBuf,
    started: Instant,
    quiet: u32,
    last_count: usize,
    requested: bool,
}

/// Set once the screenshot is on disk.
#[derive(Resource, Default)]
struct ShotSaved(bool);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Some(path) = &self.screenshot {
            app.insert_resource(ShotPlan {
                path: path.clone(),
                started: Instant::now(),
                quiet: 0,
                last_count: 0,
                requested: false,
            })
            .init_resource::<ShotSaved>()
            .add_systems(Update, take_screenshot);
        }
        if let Some(seconds) = self.quit_after {
            app.add_systems(
                Update,
                move |time: Res<Time<Real>>, mut exit: MessageWriter<AppExit>| {
                    if time.elapsed_secs() >= seconds {
                        exit.write(AppExit::Success);
                    }
                },
            );
        }
    }
}

fn take_screenshot(
    mut commands: Commands,
    mut plan: ResMut<ShotPlan>,
    saved: Res<ShotSaved>,
    voxel_world: VoxelWorld<DoomWorld>,
    chunks: Query<&Chunk<DoomWorld>>,
    mut exit: MessageWriter<AppExit>,
) {
    if saved.0 {
        exit.write(AppExit::Success);
        return;
    }
    if plan.requested {
        return;
    }
    let count = chunks.iter().count();
    let busy = chunks
        .iter()
        .filter(|chunk| voxel_world.chunk_state(chunk.key()) != Some(ChunkStatus::Ready))
        .count();
    if busy == 0 && count > 0 && count == plan.last_count {
        plan.quiet += 1;
    } else {
        plan.quiet = 0;
    }
    plan.last_count = count;

    let elapsed = plan.started.elapsed();
    let timed_out = elapsed > GIVE_UP;
    if plan.quiet < QUIET_FRAMES && !timed_out {
        return;
    }
    if timed_out {
        warn!("the world didn't settle in {GIVE_UP:?}: {busy} of {count} chunks busy");
    }
    println!(
        "screenshot after {:.2} s with {count} chunks: {}",
        elapsed.as_secs_f32(),
        plan.path.display()
    );
    plan.requested = true;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(plan.path.clone()))
        .observe(|_: On<ScreenshotCaptured>, mut saved: ResMut<ShotSaved>| {
            saved.0 = true;
        });
}
