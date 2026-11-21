# bevy_voxel_DOOM

This repo holds a "vibe-coded" experiment, using [Bevy](https://bevy.org/) and [bevy_voxel_world](https://github.com/splashdust/bevy_voxel_world) to generate DOOM maps from WADs and render them as a voxel world. The game plays with monsters, weapons and sound. BVW sub-grids are used for doors and moving platforms. The world is destructible, and shooting walls will take chunks of voxels out.

<img alt src="https://github.com/user-attachments/assets/8fa690b4-ed10-443b-b35d-1afa82292743" />

## Documentation

Instructions and documentation in the wall-of-slop below :)

----

The workspace has three parts:

| | |
|---|---|
| `crates/wad` (`doom_wad`) | reads WAD files: map lumps, the palette, COLORMAP, flats, wall textures, sprites and sounds |
| `crates/voxelize` (`doom_voxelize`) | turns one map into voxels: `VoxelMap` |
| `src/` (`doom_voxels`) | the Bevy app: the voxel world, the palette material, the player, monsters and things, doors and lifts, the weapons, items and keys, and the debris |

## Running

```sh
cargo run --release -- --wad path/to/freedoom1.wad
cargo run --release -- --wad path/to/DOOM.WAD --map E1M3
DOOM_WAD=path/to/freedoom2.wad cargo run --release -- --map MAP07
```

### Where to get a WAD

- [Freedoom](https://github.com/freedoom/freedoom/releases): free maps and
  art under a BSD-style licence. `freedoom1.wad` has E1M1-style maps,
  `freedoom2.wad` MAP01-style ones.
- Your own `DOOM.WAD` or `DOOM2.WAD`, read where it is.

Never commit a WAD: id's are copyrighted, and `.gitignore` keeps `*.wad` out.

## Controls

| | |
|---|---|
| mouse | look (the cursor is captured) |
| W A S D | walk forward, back, left, right; in fly mode, fly |
| Shift | run; in fly mode, three times faster |
| Space | use: open a door, press a switch; in fly mode, also up |
| C | jump (DOOM has none) |
| Ctrl | in fly mode, down |
| left click | fire; hold to keep firing. The click that captures the cursor doesn't fire |
| 1 to 7 | pick a weapon, as DOOM's keys do: 1 the fist or chainsaw, 2 the pistol, 3 the shotgun or super shotgun, 4 the chaingun, 5 the rocket launcher, 6 the plasma rifle, 7 the BFG9000 |
| mouse wheel | the next or previous weapon with ammo |
| V | switch between walking and the noclip fly camera |
| R or click | when dead, respawn at the start with full health |
| Esc | free the cursor; click to capture it again |

## Command line

| flag | |
|---|---|
| `--wad PATH` | the WAD; falls back to the `DOOM_WAD` environment variable |
| `--map NAME` | the map, like `E1M1` or `MAP01`; the WAD's first map by default |
| `--list-maps` | print the WAD's maps and exit |
| `--units-per-voxel N` | DOOM map units per voxel, 2 by default |
| `--light MODE` | `sector` (default): DOOM's sector light baked in, no propagation. `propagated`: full-bright colours times propagated light. `both`: sector light baked in, times propagated light. `sector-glow`: full-bright colours, every surface glowing at its sector's light. `flat`: full-bright, no light at all |
| `--contrast K` | stretches baked sector light away from a middle light (about light 128), 1.25 by default; 1 is DOOM's own. Dark sectors take darker COLORMAP tables; bright ones get brighter through a gain on every colour, since no table is brighter than the palette |
| `--min-light B` | the least brightness propagated light leaves, from 0 to 1, 0.08 by default and 0 in `sector-glow` |
| `--falloff BASE` | each light level is `BASE` times as bright as the one above, 0.9 by default; 1 is linear. In `sector-glow` lamp light follows COLORMAP instead |
| `--sky on\|off` | sky light, on by default and off in `sector-glow` |
| `--sky-reach N` | how many voxels sky light spreads past where it falls, 1 to 32, 32 by default |
| `--voxel-reach N` | how many voxels a lamp's light spreads, 1 to 32, 32 by default and 14 in `sector-glow` |
| `--lamp-gain G` | how bright lamp light is at level 15, before the cap at full brightness, 1.5 by default and 1 in `sector-glow` |
| `--precision 16\|256` | light levels each voxel holds, 16 by default and 256 in `sector-glow` |
| `--sky-fill on\|off` | carry sky light through openings into the rooms beside the sky, on by default |
| `--lamp-level full\|sector` | light flats and walls glow at level 15, or at their sector's light level / 16 |
| `--stepped` | show each voxel's light as a flat step instead of smoothing it |
| `--flash-time S` | how long a shot's flash glows, 0.12 s by default. A long time keeps flashes on for a screenshot |
| `--no-vsync` | don't wait for the display, so the HUD's frame rate shows what a frame costs |
| `--closed-doors` | keep closed the door sectors that nothing usable opens; they are baked open by default. Doors that Space opens start closed either way |
| `--fly` | start in the noclip fly camera instead of walking; V switches |
| `--god` | take no damage |
| `--health N` | start with N health, 1 to 200 (default 100); an exit switch starts the next map with the health you left with |
| `--weapons LIST` | start with these weapons besides the fist and pistol, comma-separated from `chainsaw`, `shotgun`, `ssg`, `chaingun`, `rocket`, `plasma`, `bfg`, or `all`. Weapons the WAD has no sprites for (the super shotgun in DOOM) are left out |
| `--ammo B,S,R,C` | start with this many bullets, shells, rockets and cells, up to the limits, or `full` (default 50,0,0,0) |
| `--backpack` | start with the backpack, which doubles the ammo limits |
| `--weapon NAME` | start with this weapon in hand. An exit switch starts the next map with the weapons, ammo, backpack and weapon you left with |
| `--awake` | monsters start awake, chasing the player |
| `--freeze` | things stand still and don't think, for screenshots |
| `--mute` | no sound |
| `--list-things` | print where each monster stands, in voxels |
| `--frame-times` | every 5 seconds print frame time statistics and where the monster tics spend their time |
| `--script STEPS` | once the world has settled, play comma-separated steps: `w:S`, `s:S`, `a:S`, `d:S` walk for S seconds (capitals run), `jw:S` jump while walking, `jump`, `use`, `turn:DEG` (positive left), `pitch:DEG`, `fire:N` hold the trigger as long as N chaingun shots take, `weapon:N` press number key N, `wait:S`, `shot:PATH` save a screenshot, `respawn`. Each step prints the player's position. The cursor isn't captured, and `--screenshot` waits for the script |
| `--pos X,Y,Z` | start the camera (the eye, when walking) here, in voxels, instead of at player 1's start |
| `--yaw DEG` | start yaw: 0 faces DOOM's north, positive turns left |
| `--pitch DEG` | start pitch, positive looks up |
| `--blast-radius R` | the radius in voxels of the ball a bullet carves, 4 by default; the other weapons carve in proportion (see the weapons below) |
| `--fire N` | once the world is ready, fire N chaingun bullets in a small spiral around the view centre, without ammo |
| `--debris N` | debris cubes a shot throws out, 16 by default; 0 turns debris off |
| `--screenshot PATH` | wait until the world in view has been generated and meshed, and its light has settled, save a PNG and exit. With `--fire`, it waits for the carved chunks too, and for debris to settle |
| `--screenshot-delay S` | take the screenshot S seconds after the last scripted shot, settled or not, to catch debris in the air |
| `--quit-after S` | exit after S seconds |

At start it prints how long building the voxel map took and the map's voxel
bounds. Voxel `[x, y, z]` with `s` units per voxel covers DOOM's x from
`x*s`, height from `y*s` and DOOM's y from `-(z+1)*s`: DOOM's east is +x and
its north is −z.

## How it works

- **The world.** The DOOM map turned into voxels, loaded in chunks around you.
- **Colour.** Every voxel uses DOOM's own 256-colour palette.
- **Light.** Lamps, light flats and the sky light up the voxels around them.
- **Sector glow.** Every surface glows as bright as its DOOM room.
- **Screenshots.** Taken once the world has finished loading and lighting.
- **The player** (`src/player.rs`, `src/collide.rs`). DOOM's size, speed and step height, colliding with voxels.
- **Doors, lifts and moving floors** (`src/specials.rs`, `src/movers.rs`). Slide up and down as in DOOM; keyed ones need their key.
- **Exits** (`src/levels.rs`). An exit switch restarts the game on the next map, keeping your weapons and ammo.
- **Sprites** (`crates/wad/src/sprite.rs`, `src/sprites.rs`). Monsters and items are flat pictures that face the camera.
- **Monsters** (`src/info.rs`, `src/actors.rs`). Wake, chase and attack as DOOM's do.
- **Weapons** (`src/weapon.rs`, `src/inventory.rs`). All nine DOOM weapons, with limited ammo; shots carve holes in the world.
- **Barrels.** Explode when shot, hurting what's near and blowing a big crater.
- **Items and keys** (`src/inventory.rs`). Walk over health, weapons, ammo and keys to pick them up.
- **Sound** (`crates/wad/src/sound.rs`, `src/sound.rs`). DOOM's sounds, quieter with distance.
- **Debris** (`src/debris.rs`). Carved voxels fly out as cubes and settle back into the world.
- **The voxelizer** (`crates/voxelize`). Turns floors, ceilings and walls into solid and empty voxels.
- **Inside the solid.** Holes show the nearby texture going on inside, getting darker.
- **The WAD reader** (`crates/wad`). Reads maps, textures and colours from the WAD file.

## Licence

GPL-2.0-or-later; the full text is in [LICENSE](LICENSE). The weapons,
monsters and pickups follow the logic and numbers of id Software's DOOM
source code, which is released under the GPL. WAD files are not included and
carry their own licences.
