//! Moving on from a map: an exit switch picks the next map as DOOM does
//! (g_game.c `G_DoCompleted`), and the game starts it with the health the
//! player left with.
//!
//! The world, its sub-grids, the monsters and the light are all built for
//! one map at startup, so the next map starts as a fresh run of this
//! program: the app exits a second after the switch, once its sound has
//! played, and `main` starts the program again in its place with `--map`
//! and `--health` set (see [`relaunch`]).

use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::player::Player;

/// Seconds between the exit switch and leaving the map.
const EXIT_DELAY: f32 = 1.0;

/// Sent when the player uses an exit line.
#[derive(Message, Clone, Copy, Debug)]
pub struct LevelExit {
    pub secret: bool,
}

/// The map to start once the app has exited, and the player's health,
/// shared with `main`.
#[derive(Resource, Clone, Default)]
pub struct NextLevel(pub Arc<Mutex<Option<(String, i32)>>>);

pub struct LevelsPlugin {
    pub map: String,
    /// The WAD's maps.
    pub maps: Vec<String>,
    pub next: NextLevel,
}

#[derive(Resource)]
struct Maps {
    current: String,
    all: Vec<String>,
    /// The map being left for, and seconds since.
    leaving: Option<(String, f32)>,
}

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LevelExit>()
            .insert_resource(self.next.clone())
            .insert_resource(Maps {
                current: self.map.clone(),
                all: self.maps.clone(),
                leaving: None,
            })
            .add_systems(Update, leave_level);
    }
}

fn leave_level(
    mut exits: MessageReader<LevelExit>,
    mut maps: ResMut<Maps>,
    next: Res<NextLevel>,
    player: Single<&Player>,
    time: Res<Time<Real>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if let Some((to, since)) = &mut maps.leaving {
        *since += time.delta_secs();
        if *since >= EXIT_DELAY {
            *next.0.lock().expect("nothing panics holding it") =
                Some((to.clone(), player.health));
            app_exit.write(AppExit::Success);
        }
        exits.clear();
        return;
    }
    let Some(exit) = exits.read().last().copied() else {
        return;
    };
    let kind = if exit.secret { "secret exit" } else { "exit" };
    match next_map(&maps.current, exit.secret, &maps.all) {
        Some(to) => {
            println!("{kind}: {} is done, next is {to}", maps.current);
            maps.leaving = Some((to, 0.0));
        }
        None => println!(
            "{kind}: {} is done, and the WAD has no map after it: that's the end",
            maps.current
        ),
    }
}

/// The map after `map` in `maps`, by the way out: DOOM's order, with its
/// secret levels. The end of an episode goes on to the next episode's first
/// map when the WAD has it. A secret exit with no secret map to go to counts
/// as a normal exit.
pub fn next_map(map: &str, secret: bool, maps: &[String]) -> Option<String> {
    let has = |name: String| maps.contains(&name).then_some(name);
    if let Some((e, m)) = episode_map(map) {
        let name = |e: u32, m: u32| has(format!("E{e}M{m}"));
        let normal = || match m {
            8 => name(e + 1, 1),
            // Back from the secret level to where its exit was found.
            9 => match e {
                1 => name(e, 4),
                2 => name(e, 6),
                3 => name(e, 7),
                4 => name(e, 3),
                _ => None,
            },
            _ => name(e, m + 1),
        };
        return if secret && m != 9 {
            name(e, 9).or_else(normal)
        } else {
            normal()
        };
    }
    let n: u32 = map.strip_prefix("MAP")?.parse().ok()?;
    let name = |n: u32| has(format!("MAP{n:02}"));
    let normal = || match n {
        30 => None,
        31 | 32 => name(16),
        _ => name(n + 1),
    };
    match (n, secret) {
        (15, true) => name(31).or_else(normal),
        (31, true) => name(32).or_else(normal),
        _ => normal(),
    }
}

/// `ExMy` as (x, y).
fn episode_map(map: &str) -> Option<(u32, u32)> {
    let rest = map.strip_prefix('E')?;
    let (e, m) = rest.split_once('M')?;
    Some((e.parse().ok()?, m.parse().ok()?))
}

/// Starts this program again for `map` with `health`, keeping the command
/// line's other settings: see [`crate::args::next_level_words`]. On Unix the
/// new run replaces this process.
pub fn relaunch(map: &str, health: i32) -> AppExit {
    let words = crate::args::next_level_words(std::env::args().skip(1), map, health);
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("error: can't find this program to start {map}: {e}");
            return AppExit::error();
        }
    };
    println!("starting {map} with {health} health");
    let mut command = std::process::Command::new(exe);
    command.args(words);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = command.exec();
        eprintln!("error: can't start {map}: {e}");
        AppExit::error()
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) if status.success() => AppExit::Success,
        Ok(_) => AppExit::error(),
        Err(e) => {
            eprintln!("error: can't start {map}: {e}");
            AppExit::error()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &str) -> Vec<String> {
        list.split(' ').map(String::from).collect()
    }

    #[test]
    fn doom_order() {
        let shareware = names("E1M1 E1M2 E1M3 E1M4 E1M5 E1M6 E1M7 E1M8 E1M9");
        let next = |map, secret| next_map(map, secret, &shareware);
        assert_eq!(next("E1M1", false).as_deref(), Some("E1M2"));
        assert_eq!(next("E1M3", true).as_deref(), Some("E1M9"));
        assert_eq!(next("E1M9", false).as_deref(), Some("E1M4"));
        assert_eq!(next("E1M9", true).as_deref(), Some("E1M4"));
        assert_eq!(next("E1M8", false), None);
        let registered = names("E1M8 E2M1 E2M5 E2M6 E2M9");
        assert_eq!(
            next_map("E1M8", false, &registered).as_deref(),
            Some("E2M1")
        );
        assert_eq!(
            next_map("E2M9", false, &registered).as_deref(),
            Some("E2M6")
        );
        // No secret map: the secret exit goes on as the normal one does.
        assert_eq!(
            next_map("E2M5", true, &names("E2M5 E2M6")).as_deref(),
            Some("E2M6")
        );
    }

    #[test]
    fn doom2_order() {
        let all: Vec<String> = (1..=32).map(|n| format!("MAP{n:02}")).collect();
        let next = |map, secret| next_map(map, secret, &all);
        assert_eq!(next("MAP01", false).as_deref(), Some("MAP02"));
        assert_eq!(next("MAP09", true).as_deref(), Some("MAP10"));
        assert_eq!(next("MAP15", true).as_deref(), Some("MAP31"));
        assert_eq!(next("MAP31", true).as_deref(), Some("MAP32"));
        assert_eq!(next("MAP31", false).as_deref(), Some("MAP16"));
        assert_eq!(next("MAP32", false).as_deref(), Some("MAP16"));
        assert_eq!(next("MAP30", false), None);
        assert_eq!(next_map("MAP07", false, &names("MAP07")), None);
        assert_eq!(next_map("START", false, &all), None);
    }
}
