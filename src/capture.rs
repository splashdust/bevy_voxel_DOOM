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
//!
//! `--screenshot-delay S` takes the screenshot S seconds after the last such
//! change instead, settled or not, or S seconds after the world first settles
//! if nothing changes it.

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
    /// Seconds after the last change to take the screenshot at, settled or
    /// not.
    pub delay: Option<f32>,
    pub quit_after: Option<f32>,
    /// Track [`WorldSettled`] even without a screenshot.
    pub track_settling: bool,
    /// Print frame time statistics every few seconds.
    pub frame_times: bool,
}

/// Frame times since the last report, in milliseconds.
#[derive(Resource, Default)]
struct FrameTimes {
    frames: Vec<f32>,
    since: f32,
}

/// Seconds between frame time reports.
const FRAME_TIME_REPORT: f32 = 5.0;

fn record_frame_times(
    time: Res<Time<Real>>,
    mut times: ResMut<FrameTimes>,
    actors: Query<&crate::actors::Actor>,
    mut profile: ResMut<crate::actors::TicProfile>,
) {
    let dt = time.delta_secs();
    times.frames.push(dt * 1000.0);
    times.since += dt;
    if times.since < FRAME_TIME_REPORT {
        return;
    }
    let (alive, awake) = crate::actors::census(&actors);
    let mut frames = std::mem::take(&mut times.frames);
    times.since = 0.0;
    frames.sort_by(f32::total_cmp);
    let at = |q: f32| frames[((frames.len() - 1) as f32 * q) as usize];
    let mean = frames.iter().sum::<f32>() / frames.len() as f32;
    println!(
        "frame times at {:.0} s over {} frames, {alive} monsters, {awake} awake: mean {mean:.1} ms, p50 {:.1}, p95 {:.1}, p99 {:.1}, worst {:.1}",
        time.elapsed_secs(),
        frames.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0)
    );
    println!("  {}", profile.report());
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
    /// When `unsettle` was last called.
    changed_at: Option<Instant>,
}

impl WorldSettled {
    pub fn is_settled(&self) -> bool {
        self.quiet >= QUIET_FRAMES
    }

    /// Starts the count again, after changing the world.
    pub fn unsettle(&mut self) {
        self.quiet = 0;
        self.changed_at = Some(Instant::now());
    }

    /// Starts the count again, for something still moving that will change
    /// the world, like debris in flight. Unlike `unsettle` it doesn't move
    /// the clock of `--screenshot-delay`.
    pub fn hold(&mut self) {
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
    delay: Option<f32>,
    /// When the world first settled.
    settled_at: Option<Instant>,
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
                delay: self.delay,
                settled_at: None,
            })
            .init_resource::<ShotSaved>()
            .add_systems(Update, take_screenshot.in_set(CaptureSet::Shoot));
        }
        if self.frame_times {
            app.init_resource::<FrameTimes>()
                .add_systems(Last, record_frame_times);
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
        // A sub-grid's chunk keys are on its own grid.
        .filter(|chunk| {
            voxel_world
                .grid(chunk.grid())
                .and_then(|grid| grid.chunk_state(chunk.key()))
                != Some(ChunkStatus::Ready)
        })
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
    if settled.is_settled() && plan.settled_at.is_none() {
        plan.settled_at = Some(Instant::now());
    }
    let ready = match plan.delay {
        Some(delay) => settled
            .changed_at
            .or(plan.settled_at)
            .is_some_and(|t| t.elapsed().as_secs_f32() >= delay),
        None => settled.is_settled(),
    };
    if !ready && !timed_out {
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
