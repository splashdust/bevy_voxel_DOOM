//! Sound effects from the WAD's `DS` lumps.
//!
//! Game logic writes [`PlaySound`]. Each DMX lump is turned into an
//! in-memory WAV at start and played through bevy_audio, at a volume that
//! falls off with distance from the player as DOOM's does: full within 200
//! map units, silent past 1200. At most `VOICES` play at once, as DOOM
//! mixes a handful of channels. `--mute` leaves sound out.

use std::collections::HashMap;

use bevy::{
    audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume},
    prelude::*,
};
use doom_wad::Wad;

use crate::player::Player;
use crate::world::GameMap;

/// Plays the WAD's `DS` + `name` lump, from `at` in voxels or everywhere.
#[derive(Message, Clone, Debug)]
pub struct PlaySound {
    pub name: &'static str,
    pub at: Option<Vec3>,
}

impl PlaySound {
    pub fn global(name: &'static str) -> Self {
        PlaySound { name, at: None }
    }

    pub fn at(name: &'static str, at: Vec3) -> Self {
        PlaySound { name, at: Some(at) }
    }
}

/// s_sound.c: full volume within this many units, none past the other.
const CLOSE: f32 = 200.0;
const CLIPPING: f32 = 1200.0;
/// Sounds at once.
const VOICES: usize = 12;
/// The overall volume.
const VOLUME: f32 = 0.5;

/// The WAD's sounds as WAV files, by name without `DS`.
#[derive(Resource)]
pub struct SoundBank {
    wavs: HashMap<String, Vec<u8>>,
    handles: HashMap<String, Handle<AudioSource>>,
}

impl SoundBank {
    /// Decodes every `DS` lump; ones that don't decode are left out.
    pub fn load(wad: &Wad) -> Self {
        let names: Vec<String> = wad
            .lump_names()
            .filter_map(|n| n.strip_prefix("DS"))
            .map(str::to_string)
            .collect();
        let wavs = names
            .into_iter()
            .filter_map(|name| Some((name.clone(), wad.sound(&name).ok()?.to_wav())))
            .collect();
        SoundBank {
            wavs,
            handles: HashMap::new(),
        }
    }
}

pub struct SoundPlugin {
    pub bank: Option<SoundBank>,
}

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>();
        if let Some(bank) = &self.bank {
            app.insert_resource(SoundBank {
                wavs: bank.wavs.clone(),
                handles: HashMap::new(),
            })
            .add_systems(Startup, make_sources)
            .add_systems(PostUpdate, play_sounds);
        }
    }
}

/// A sound playing.
#[derive(Component)]
struct Voice;

fn make_sources(mut bank: ResMut<SoundBank>, mut sources: ResMut<Assets<AudioSource>>) {
    let wavs = std::mem::take(&mut bank.wavs);
    for (name, wav) in wavs {
        let handle = sources.add(AudioSource { bytes: wav.into() });
        bank.handles.insert(name, handle);
    }
    println!("{} sounds", bank.handles.len());
}

fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    bank: Res<SoundBank>,
    map: Res<GameMap>,
    player: Single<&Player>,
    voices: Query<(), With<Voice>>,
) {
    let mut playing = voices.iter().count();
    let ear = player.eye();
    for request in requests.read() {
        let Some(handle) = bank.handles.get(request.name) else {
            continue;
        };
        let volume = request.at.map_or(1.0, |at| {
            let units = at.distance(ear) * map.units;
            ((CLIPPING - units) / (CLIPPING - CLOSE)).clamp(0.0, 1.0)
        });
        if volume <= 0.0 || playing >= VOICES {
            continue;
        }
        playing += 1;
        commands.spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume * VOLUME)),
            Voice,
        ));
    }
}
