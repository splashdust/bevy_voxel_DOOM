//! `--screenshot PATH`: waits until the world around the camera has finished
//! generating and meshing, saves the window as a PNG and exits. Also
//! `--quit-after S`.
//!
//! The world counts as settled once every spawned chunk has been `Ready`, and
//! the world's light settled, for `QUIET_FRAMES` frames in a row with no
//! chunk spawned or despawned. Spawning rays only find chunks behind ones
//! already generated, so a pause in new chunks is the sign that streaming has
//! reached everything in view. A world without light propagation always
//! reads its light as settled.
//! [`WorldSettled`] carries that signal; anything that changes the world on
//! the way to a screenshot, like a scripted shot, starts the count again.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
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
    /// Track [`WorldSettled`] even without a screenshot.
    pub track_settling: bool,
}

/// Whether the world around the camera has settled: every spawned chunk
/// ready, and none spawned or despawned, for `QUIET_FRAMES` frames.
#[derive(Resource, Default)]
pub struct WorldSettled {
    quiet: u32,
    count: usize,
    busy: usize,
    /// When a chunk was last busy or spawned or despawned, and when light
    /// was last unsettled, in seconds since start.
    chunks_busy_at: f32,
    light_busy_at: f32,
}

impl WorldSettled {
    pub fn is_settled(&self) -> bool {
        self.quiet >= QUIET_FRAMES
    }

    /// Starts the count again, after changing the world.
    pub fn unsettle(&mut self) {
        self.quiet = 0;
    }
}

/// `Track` updates [`WorldSettled`]; `Shoot` takes the screenshot. Systems
/// that change the world once it has settled go between them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CaptureSet {
    Track,
    Shoot,
}

#[derive(Resource)]
struct ShotPlan {
    path: PathBuf,
    started: Instant,
    requested: bool,
}

/// Set once the screenshot is on disk.
#[derive(Resource, Default)]
struct ShotSaved(bool);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if self.screenshot.is_some() || self.track_settling {
            app.init_resource::<WorldSettled>()
                .configure_sets(Update, CaptureSet::Track.before(CaptureSet::Shoot))
                .add_systems(Update, track_settling.in_set(CaptureSet::Track));
        }
        if let Some(path) = &self.screenshot {
            app.insert_resource(ShotPlan {
                path: path.clone(),
                started: Instant::now(),
                requested: false,
            })
            .init_resource::<ShotSaved>()
            .add_systems(Update, take_screenshot.in_set(CaptureSet::Shoot));
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

fn track_settling(
    time: Res<Time<Real>>,
    mut settled: ResMut<WorldSettled>,
    voxel_world: VoxelWorld<DoomWorld>,
    chunks: Query<&Chunk<DoomWorld>>,
) {
    let count = chunks.iter().count();
    let busy = chunks
        .iter()
        .filter(|chunk| voxel_world.chunk_state(chunk.key()) != Some(ChunkStatus::Ready))
        .count();
    let chunks_quiet = busy == 0 && count > 0 && count == settled.count;
    let light_settled = voxel_world.light_is_settled();
    let now = time.elapsed_secs();
    if !chunks_quiet {
        settled.chunks_busy_at = now;
    }
    if !light_settled {
        settled.light_busy_at = now;
    }
    if chunks_quiet && light_settled {
        settled.quiet += 1;
    } else {
        settled.quiet = 0;
    }
    settled.count = count;
    settled.busy = busy;
}

fn take_screenshot(
    mut commands: Commands,
    mut plan: ResMut<ShotPlan>,
    saved: Res<ShotSaved>,
    settled: Res<WorldSettled>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    if saved.0 {
        exit.write(AppExit::Success);
        return;
    }
    if plan.requested {
        return;
    }
    let elapsed = plan.started.elapsed();
    let timed_out = elapsed > GIVE_UP;
    if !settled.is_settled() && !timed_out {
        return;
    }
    let (busy, count) = (settled.busy, settled.count);
    if timed_out {
        warn!("the world didn't settle in {GIVE_UP:?}: {busy} of {count} chunks busy");
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
        .unwrap_or(f64::NAN);
    println!(
        "screenshot after {:.2} s with {count} chunks at {fps:.0} fps (chunks last busy at {:.2} s, light at {:.2} s): {}",
        elapsed.as_secs_f32(),
        settled.chunks_busy_at,
        settled.light_busy_at,
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
