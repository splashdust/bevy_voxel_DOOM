//! What the player wants this frame, from the keyboard and mouse or from a
//! `--script`, so the controller and the weapon don't read devices
//! themselves and a script can play the game for a test.
//!
//! A script is a comma-separated list of steps run one after another once
//! the world has settled. Each prints the player's position as it starts:
//!
//! - `w:S`, `s:S`, `a:S`, `d:S`: walk forward, back, left or right for S
//!   seconds; `W:S` and the other capitals run;
//! - `jump`: jump, and `jw:S` jump while walking forward for S seconds;
//! - `turn:DEG`: turn left by DEG degrees (right when negative);
//! - `pitch:DEG`: look DEG degrees up from level;
//! - `fire:N`: hold the trigger for as long as N chaingun shots take;
//! - `weapon:N`: press number key N to switch weapons;
//! - `use`: press use once;
//! - `wait:S`: stand still for S seconds;
//! - `shot:PATH`: save a screenshot;
//! - `respawn`: respawn, when dead.

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use crate::capture::WorldSettled;

/// Radians per pixel of mouse motion.
const MOUSE_SENSITIVITY: f32 = 0.0022;
/// Pixels of scrolling that step one weapon, on a touchpad.
const SCROLL_PIXELS: f32 = 40.0;
/// The number keys that pick weapons, 1 to 7.
const WEAPON_KEYS: [KeyCode; 7] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
];

/// This frame's wishes.
#[derive(Resource, Default, Debug, Clone)]
pub struct Intent {
    /// −1 to 1, forward positive.
    pub forward: f32,
    /// −1 to 1, right positive.
    pub strafe: f32,
    /// −1 to 1, up positive: fly mode only.
    pub up: f32,
    pub run: bool,
    /// Jump pressed (held counts: it jumps again on landing).
    pub jump: bool,
    /// Use pressed this frame: open a door, press a switch.
    pub use_: bool,
    /// The trigger is held.
    pub fire: bool,
    /// Radians to turn left and to look up.
    pub turn: f32,
    pub look_up: f32,
    /// Set the pitch to this, in radians.
    pub set_pitch: Option<f32>,
    /// V pressed: switch between walking and flying.
    pub toggle_fly: bool,
    /// R or a click while dead: respawn.
    pub respawn: bool,
    /// A number key pressed this frame, 1 to 7: pick that weapon.
    pub weapon_slot: Option<u8>,
    /// Mouse wheel steps this frame: positive for the next weapon.
    pub weapon_step: i32,
}

/// One step of a script.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Walk with (forward, strafe) for seconds, running or not, jumping.
    Move {
        forward: f32,
        strafe: f32,
        run: bool,
        jump: bool,
        seconds: f32,
    },
    Turn(f32),
    Pitch(f32),
    Fire(u32),
    Wait(f32),
    Shot(String),
    Respawn,
    Use,
    /// Press a weapon number key.
    Weapon(u8),
}

/// A scripted run; see the module docs.
#[derive(Resource, Debug)]
pub struct Script {
    steps: Vec<Step>,
    /// The step running, and seconds it has run.
    at: usize,
    elapsed: f32,
    started: bool,
}

impl Script {
    pub fn new(steps: Vec<Step>) -> Self {
        Script {
            steps,
            at: 0,
            elapsed: 0.0,
            started: false,
        }
    }

    pub fn done(&self) -> bool {
        self.at >= self.steps.len()
    }
}

/// Parses a script; see the module docs.
pub fn parse_script(text: &str) -> Result<Vec<Step>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|step| {
            let (name, arg) = step.split_once(':').unwrap_or((step, ""));
            let number = || {
                arg.trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| format!("script step {step:?} needs a number"))
            };
            let walk = |forward, strafe, run, jump| -> Result<Step, String> {
                Ok(Step::Move {
                    forward,
                    strafe,
                    run,
                    jump,
                    seconds: number()?,
                })
            };
            match name {
                "w" | "W" => walk(1.0, 0.0, name == "W", false),
                "s" | "S" => walk(-1.0, 0.0, name == "S", false),
                "a" | "A" => walk(0.0, -1.0, name == "A", false),
                "d" | "D" => walk(0.0, 1.0, name == "D", false),
                "jw" => walk(1.0, 0.0, false, true),
                "jump" => Ok(Step::Move {
                    forward: 0.0,
                    strafe: 0.0,
                    run: false,
                    jump: true,
                    seconds: 0.1,
                }),
                "turn" => Ok(Step::Turn(number()?.to_radians())),
                "pitch" => Ok(Step::Pitch(number()?.to_radians())),
                "fire" => Ok(Step::Fire(number()? as u32)),
                "wait" => Ok(Step::Wait(number()?)),
                "shot" if !arg.is_empty() => Ok(Step::Shot(arg.to_string())),
                "respawn" => Ok(Step::Respawn),
                "use" => Ok(Step::Use),
                "weapon" => match number()? as u8 {
                    n @ 1..=7 => Ok(Step::Weapon(n)),
                    _ => {
                        Err(format!("script step {step:?} needs a weapon number 1 to 7"))
                    }
                },
                _ => Err(format!("unknown script step {step:?}")),
            }
        })
        .collect()
}

/// Reads the keyboard and mouse into [`Intent`]. The click that captures
/// the cursor doesn't fire: the trigger counts only once pressed with the
/// cursor captured.
#[allow(clippy::too_many_arguments)]
pub fn read_devices(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut intent: ResMut<Intent>,
    mut trigger: Local<bool>,
    mut scrolled: Local<f32>,
) {
    let captured = cursor.grab_mode != CursorGrabMode::None;
    let axis = |plus: KeyCode, minus: KeyCode| {
        keys.pressed(plus) as i32 as f32 - keys.pressed(minus) as i32 as f32
    };
    if !captured
        || keys.just_pressed(KeyCode::Escape)
        || !buttons.pressed(MouseButton::Left)
    {
        *trigger = false;
    } else if buttons.just_pressed(MouseButton::Left) {
        *trigger = true;
    }
    let (turn, look_up) = if captured {
        (
            -motion.delta.x * MOUSE_SENSITIVITY,
            -motion.delta.y * MOUSE_SENSITIVITY,
        )
    } else {
        (0.0, 0.0)
    };
    // A wheel's line is a step; a touchpad's pixels add up to steps.
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / SCROLL_PIXELS,
    };
    *scrolled = if captured { *scrolled + lines } else { 0.0 };
    let steps = scrolled.trunc();
    *scrolled -= steps;
    *intent = Intent {
        forward: axis(KeyCode::KeyW, KeyCode::KeyS),
        strafe: axis(KeyCode::KeyD, KeyCode::KeyA),
        up: keys.pressed(KeyCode::Space) as i32 as f32
            - (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
                as i32 as f32,
        run: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        jump: keys.pressed(KeyCode::KeyC),
        use_: keys.just_pressed(KeyCode::Space),
        fire: *trigger,
        turn,
        look_up,
        set_pitch: None,
        toggle_fly: keys.just_pressed(KeyCode::KeyV),
        respawn: keys.just_pressed(KeyCode::KeyR)
            || buttons.just_pressed(MouseButton::Left),
        weapon_slot: (1..)
            .zip(WEAPON_KEYS)
            .find(|(_, key)| keys.just_pressed(*key))
            .map(|(n, _)| n),
        // Scrolling down, toward you, is the next weapon.
        weapon_step: -steps as i32,
    };
}

/// Replaces [`Intent`] with the script's step, once the world has settled,
/// and holds the screenshot back until the script is done.
pub fn run_script(
    time: Res<Time>,
    mut commands: Commands,
    mut script: ResMut<Script>,
    mut settled: ResMut<WorldSettled>,
    mut intent: ResMut<Intent>,
    player: Single<&crate::player::Player>,
) {
    if script.done() {
        return;
    }
    if !script.started {
        if !settled.is_settled() {
            return;
        }
        script.started = true;
        script.elapsed = 0.0;
        println!("script: starting at {}", player.describe());
    }
    settled.unsettle();
    let mut fresh = Intent {
        respawn: false,
        ..Intent::default()
    };
    let step = script.steps[script.at].clone();
    let first = script.elapsed == 0.0;
    if first {
        println!(
            "script step {}: {step:?} from {}",
            script.at + 1,
            player.describe()
        );
    }
    script.elapsed += time.delta_secs();
    let finished = match step {
        Step::Move {
            forward,
            strafe,
            run,
            jump,
            seconds,
        } => {
            fresh.forward = forward;
            fresh.strafe = strafe;
            fresh.run = run;
            fresh.jump = jump;
            script.elapsed >= seconds
        }
        Step::Turn(radians) => {
            fresh.turn = radians;
            true
        }
        Step::Pitch(radians) => {
            fresh.set_pitch = Some(radians);
            true
        }
        Step::Fire(shots) => {
            fresh.fire = true;
            script.elapsed >= shots as f32 * crate::weapon::SHOT_INTERVAL
        }
        Step::Wait(seconds) => script.elapsed >= seconds,
        Step::Respawn => {
            fresh.respawn = true;
            true
        }
        Step::Use => {
            fresh.use_ = true;
            true
        }
        Step::Weapon(n) => {
            fresh.weapon_slot = Some(n);
            true
        }
        Step::Shot(path) => {
            if first {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            // A frame for the screenshot to be taken.
            script.elapsed > 0.05
        }
    };
    *intent = fresh;
    if finished {
        script.at += 1;
        script.elapsed = 0.0;
        if script.done() {
            println!("script: done at {}", player.describe());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse() {
        let steps = parse_script(
            "w:1.5, W:2,jump,turn:90,fire:3,shot:/tmp/a.png,wait:1,respawn,weapon:3",
        )
        .unwrap();
        assert_eq!(steps.len(), 9);
        assert_eq!(steps[8], Step::Weapon(3));
        assert!(parse_script("weapon:8").is_err());
        assert_eq!(steps[7], Step::Respawn);
        assert_eq!(
            steps[1],
            Step::Move {
                forward: 1.0,
                strafe: 0.0,
                run: true,
                jump: false,
                seconds: 2.0
            }
        );
        assert_eq!(steps[3], Step::Turn(90f32.to_radians()));
        assert_eq!(steps[4], Step::Fire(3));
        assert_eq!(steps[5], Step::Shot("/tmp/a.png".into()));
        assert!(parse_script("w").is_err());
        assert!(parse_script("fly:2").is_err());
    }
}
