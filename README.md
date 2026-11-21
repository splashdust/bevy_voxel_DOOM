# bevy_voxel_DOOM

This repo holds a "vibe-coded" experiment, using [Bevy](https://bevy.org/) and [bevy_voxel_world](https://github.com/splashdust/bevy_voxel_world) to generate DOOM maps from WADs. The game plays with monsters, weapons and sound. BVW sub-grids are used for doors and moving platforms. The world is destructible, and shooting walls will take chunks of voxels out.

<img alt src="https://github.com/user-attachments/assets/8fa690b4-ed10-443b-b35d-1afa82292743" />


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

- **The world.** One `VoxelWorldConfig`, `DoomWorld`, holds the `VoxelMap` in
  an `Arc`. Its voxel lookup calls `VoxelMap::voxel`, and its fill hint asks
  `VoxelMap::uniform_in` about the chunk's box, so the solid below the map and
  the air above it are never generated. Chunks spawn out to about 2048 map
  units and despawn only when far away, so turning round doesn't rebuild the
  room behind you. There is no LOD: a coarse level samples every second voxel
  and would lose one-voxel walls.
- **Colour.** The material index is a `u16`: the palette index in its low
  byte and, for a voxel that gives off light, its level in bits 8 to 11. The
  texture index mapper passes the palette index on, and a custom material
  (`PaletteMaterial`, with the crate's vertex shader and `src/palette.wesl` as
  fragment shader) looks it up in a 256-colour uniform built from the map's
  palette at runtime. The colour is multiplied by the mesher's ambient
  occlusion and, in the light modes that propagate light, by the face's
  light. The camera has no tonemapping or dithering, so the window shows the
  palette's bytes.
- **Light.** In `propagated`, `both` and `sector-glow` the world turns on the crate's light
  propagation, and the crate wraps `PaletteMaterial` in its light bindings.
  The shader stays unlit: it scales the colour by the face's sky light, the
  lamp light around it and its own glow if it is a lamp, capped at full
  brightness and floored at `--min-light`. Sky light enters columns with no
  solid up to the map top, so it falls into sky sectors only: every other
  column is solid from its ceiling up to the top. Sky fill carries it on
  through windows and doors into the rooms beside the sky. What glows, all
  worked out at import:
  - the bright texels of DOOM's light flats (`CEIL1_2`, `TLITE6_4`,
    `FLOOR1_7` and the rest) and of wall textures whose name holds `LITE`,
    or `BRICKLIT`. Bright means at least 85% of the brightest texel of its
    texture, so a strip light glows and its frame doesn't;
  - light-giving things (candles, candelabras, torches, tech lamps, floor
    lamps, burning barrels), each a 2×2-voxel pillar about its sprite's
    height, a plain stand under a top in the sprite's colour that glows.
    Nothing else of things is drawn.

  A glowing voxel keeps its full-bright palette colour in `both` too, since
  a lamp dimmed by its own sector reads as off. Every source glows at level
  15 by default: light reaches only 32 voxels at most, a fraction of a DOOM
  room, and a lower level only shortens that. `--lamp-level sector` ties
  light flats and walls to their sector's level instead. A shot also lights
  a flash: the voxel in front of the face it hits glows for `--flash-time`
  and goes back to air.
- **Sector glow.** `--light sector-glow` is a hack that stands in for
  seeding open voxels with a base light level, which the crate can't do:
  every surface is a light source. Every solid voxel beside air glows at a
  level standing for the light DOOM draws it in, which is its sector's
  light through the floor, ceiling or wall table, fake contrast included.
  The level of a floor or ceiling comes from its own column's sector, a
  wall's from the sector of the air it faces, a grate's from its side, and
  a pillar's stand from the sector it stands in. Lamps keep level 15, and
  voxels inside the solid don't glow, so a crater shows only the light that
  reaches into it. At a voxel reach of 14 light loses one level per voxel,
  so the face of a surface glowing at level `E` reads `E − 1` from the air
  in front of it. The voxelizer measures how much each COLORMAP table
  darkens the palette, as the mean share of sRGB luma it leaves, picks each
  table's level from that share, and gives the app the brightness each
  level should show. The shader scales the colour's sRGB value, as COLORMAP
  does, rather than its linear one, and only faces of level-15 lamps glow
  as lamps: every other surface shows the light in front of it. Sky light
  is off, since sky sectors glow at their own light. At rest a view looks
  much as in `sector`, and it reacts to the world: light leaks through a
  hole shot between two rooms, crater insides darken inward, and sector
  edges soften over a few voxels.
- **Screenshots.** `--screenshot` waits until every spawned chunk has been
  ready, and `VoxelWorld::light_is_settled` true, for 60 frames in a row with
  no chunk spawning or despawning, or two minutes at most. `--fire` waits
  for the same signal before its first shot, and each shot starts the count
  again. In a lit world each shot prints how long its light took to land.
- **The player** (`src/player.rs`, `src/collide.rs`). The player is
  DOOM's: a box 32 map units across and 56 tall, the eye at 41, stepping up
  ledges of up to 24 units, so 16 × 28 voxels with a 12-voxel step at 2
  units per voxel. The collision is kinematic code, no physics engine. A
  move goes in substeps of under half a voxel, x, then z, then y. A
  horizontal substep looks only at the slab of voxels the box's leading
  face enters: if that slab is solid only within a step of the feet and the
  box fits on top, the player steps up, otherwise it stops at the voxel
  boundary and the other axes carry on, so it slides along walls. A
  vertical substep looks at the row under the feet or over the head. So no
  speed tunnels through a one-voxel wall, and voxels that end up inside the
  box are ignored, so the player can walk out of them. Voxels come from
  `VoxelMap::is_solid` in chunks without edits, and from
  `VoxelWorld::get_voxel` in chunks with edits, falling back to the map
  where it has nothing, so craters and anything else that edits the world
  collide. Shot flashes count as air. Things that block (monsters, solid
  decorations) stop a move along an axis that would end overlapping them
  and no farther away. Movement uses DOOM's numbers: walking and running
  speeds of 8.3 and 16.7 units a tic, reached and lost at the rate of
  DOOM's friction, and its gravity of 1 unit a tic a tic. The player barely
  steers in the air. DOOM has no jump; C jumps about 32 units, which
  with the step climbs out of most craters. After a step up the eye catches
  up over a few frames, as DOOM's view height does. Health is 100; at 0 the
  view sinks and turns red until R or a click respawns at the start.
- **Doors, lifts and moving floors** (`src/specials.rs`, `src/movers.rs`).
  Every sector a use special moves is a mover: manual doors (DR and D1,
  keyed ones without the key), and the sectors tagged by switches and
  buttons (S1 and SR) that open or close doors, lower lifts, raise or lower
  floors, or lower a ceiling to the floor. The map is built twice, in
  parallel: the world with each mover's plane at the end of its travel that
  leaves the most air (a door open, a lift down, a floor at its lowest),
  and a second build with it at the other end. The voxels of that second
  build between the two ends, only their shell, go into a sub-grid per
  mover, which slides along y as the plane moves: a door rises into the
  solid above its frame, a lift sinks into the solid below. At the world's
  end a sub-grid is hidden, since its face would lie on the world's.
  Movers run at DOOM's 35 tics a second with DOOM's speeds, waits and
  sounds, drawn between tics. Collision, sight and shots see a mover as the
  whole rows of its sector's columns between the world's plane and the
  moving one, so a body stands up to half a voxel off a lift's drawn top
  while it moves. A rising floor carries the player and monsters standing
  on it; a lift rising into a ceiling with a body on it goes back down, and
  a closing door goes back up from a body in its way, as DOOM's do. Space
  traces DOOM's 64 units from the player along the view's yaw, through
  two-sided lines with an opening, as `P_UseLines` does: the first line
  with a use special is used from its front side, and a wall says "oof".
  Keyed doors and switches (DR, D1, SR and S1 blue, yellow and red) do
  nothing without a key of their colour, key card or skull key, and say
  which they need. A
  switch flips between its `SW1` and `SW2` textures by editing the world's
  wall voxels (`VoxelMap::switch_voxels`), and a button flips back after a
  second.
- **Exits** (`src/levels.rs`). An exit switch picks the next map as
  `G_DoCompleted` does: the next map of the episode, `E?M9` from a secret
  exit and back from it to where it was found (`MAP31` and `MAP32` in
  DOOM II), and the next episode's first map after `E?M8` when the WAD has
  it. The world, its sub-grids, monsters and light are all built for one
  map at startup, so a second after the switch the app exits and `main`
  starts the program again for the next map, on Unix in the same process
  through `exec`. The new run keeps the command line's settings, drops the
  first map's start, script and screenshot, and starts with `--health`,
  `--weapons`, `--ammo`, `--backpack` and `--weapon` set to what the player
  left with. Keys and berserk stay behind, as in DOOM.
- **Sprites** (`crates/wad/src/sprite.rs`, `src/sprites.rs`). The WAD
  reader lists the sprite lumps between `S_START` and `S_END` (and the
  `SS_` markers) by prefix, frame letter and rotation, a second
  letter-and-rotation pair naming the same lump mirrored, and the patch
  decoder decodes them. Every thing is a quad turned about y to face the
  camera. Its rotation comes from r_things.c: the angle from the camera to
  the thing less the thing's facing picks one of eight 45° sectors. The
  quad sits where DOOM draws the patch, from its left and top offsets, one
  patch pixel to a map unit. Colours are worked out on the CPU: each
  texel's palette index goes through the COLORMAP table that walls of the
  thing's sector take at the distance the map is baked for, stretched by
  `--contrast` (`VoxelMap::sprite_table`), then the colour gain, so things
  are as dark as the room they stand in. Full-bright frames skip the table.
  Each lump, table and mirroring is one image and one unlit, alpha-masked
  material, made the first time it shows. The spectre is drawn dark and
  see-through instead of DOOM's fuzz. The weapon in hand is drawn at the
  bottom centre as DOOM draws weapon sprites on a 320×200 screen, scaled
  to the window with DOOM's tall pixels, bobbing as the player walks, in
  the light of the player's sector, its flash full-bright.
- **Monsters** (`src/info.rs`, `src/actors.rs`). Things spawn from THINGS
  at skill 3 (flag 2), single player only. Monsters and the barrel run at
  DOOM's 35 tics a second in `FixedUpdate`, drawn between tics. Each
  monster type carries DOOM's numbers from info.c (health, speed, radius,
  height, mass, pain chance) and its state sequences (spawn, see, melee,
  missile, pain, death, gib) with DOOM's frame letters, durations and
  actions, which follow p_enemy.c:
  - a monster in its spawn frames looks for the player: it wakes when it
    sees them in front of it, or close behind, or hears them fire within
    1024 units (an ambush monster must see them too). Sight is a voxel line
    trace from three quarters up the monster to the player's eye, so a hole
    shot through a wall lets monsters see through it. Traces run in
    parallel, every few tics per monster;
  - awake, it chases as A_Chase does: it turns 45° a step toward the
    direction it moves in, keeps a direction for a random count of steps,
    and picks a new one as P_NewChaseDir does, the diagonal toward the
    player first, then the straight directions, its old one, then any,
    trying each with a short trial move through the same collision as the
    player's. Monsters step up 24 units, fall where the floor is gone,
    and block each other and the player, so they don't stack;
  - it attacks with DOOM's odds (P_CheckMeleeRange, P_CheckMissileRange):
    hitscanners fire DOOM's bullets, spread as DOOM spreads them, tested
    against the player's and other monsters' bodies before the world, and
    a bullet that misses digs a pit 1 voxel across in what it hits; melee
    attackers claw and bite in DOOM's range; the imp, cacodemon, baron,
    hell knight, revenant, mancubus and arachnotron fire DOOM's
    projectiles at DOOM's speeds and damage, which burst on things and dig
    a crater 3 voxels across in the world;
  - damage pushes things away as DOOM's thrust does, sends them into pain
    at their pain chance, wakes them when the player did it, and kills
    them: death or gib frames, a corpse that isn't solid and falls if the
    floor goes.
- **Weapons** (`src/weapon.rs`, `src/inventory.rs`). All nine of DOOM's
  weapons, run as DOOM's player sprite states (p_pspr.c) at 35 tics a
  second: a weapon raises from the bottom of the screen, waits, fires
  through its attack frames at DOOM's rates, refires while the trigger is
  held, and lowers to switch. The player starts with the fist, the pistol
  and 50 bullets, and each shot takes DOOM's ammo; out of ammo, the weapon
  lowers and the best one with ammo comes up, as `P_CheckAmmo` picks it.
  Hitscans are rays from the camera along its forward vector, so they go
  where the crosshair is, for DOOM's damage and spread: the pistol and
  chaingun spread after the first shot of a burst, the shotgun fires 7
  spread pellets and the super shotgun 20, spread up and down too, and the
  fist and chainsaw reach DOOM's 64 units, the fist ten times as hard with
  berserk. A ray hits the first monster or barrel whose body it crosses,
  leaving blood, or else carves the world within the view distance,
  leaving a puff. The rocket launcher, plasma rifle and BFG9000 fire
  DOOM's projectiles: a rocket blasts what it can see within 128 units, as
  P_RadiusAttack does, the player too, and the BFG ball's burst sprays 40
  tracers from the player across 90°, each for 15 rolls of 1 to 8 on the
  first thing within 1024 units the player can see. Every attack wakes
  monsters within 1024 units. How much a weapon carves grows with
  `--blast-radius` R: a bullet R, a pellet or chainsaw bite R/2, a punch 1
  (R/2 with berserk), a plasma ball 3R/4, a rocket 2R and the BFG's ball
  4R; rockets and BFG balls carve where they burst, against a monster too.
  `Carver::carve` does every carve in
  the app, the player's shots, monsters' bullets and projectiles and
  barrels alike: around the voxel hit, `set_voxels_in` carves a ball whose
  edge is jittered by a hash of each voxel's position, and darkens solid
  voxels in a shell 1.5 voxels thick outside it by a few COLORMAP tables.
  It returns the voxels it emptied with their materials, for anything that
  wants to react to destruction. The darkening starts from the colour
  `VoxelMap::voxel` gives, so carving the same spot again doesn't darken
  it further, and a scorched lamp goes out. Air is never made solid,
  except for a hitscan's flash, which rays and bodies pass through. The
  scripted shots of `--fire` are spread so the craters sit side by side at
  the distance the centre ray hits.
- **Barrels.** A barrel is a thing with 20 health that never wakes. Shot
  dead, it plays DOOM's explosion and blasts everything it can see within
  128 units for 128 less the distance, as P_RadiusAttack does, which can
  set off other barrels, then carves a crater 11 voxels in radius.
- **Items and keys** (`src/inventory.rs`). Decorations and items stand as
  billboards, animated and lit like monsters; solid ones block. The
  player picks up an item by touching it, as p_inter.c
  `P_TouchSpecialThing` does, with DOOM's message and sound and a yellow
  flash: health (the stimpack, the medikit and the health bonus), the
  weapons, ammo (clips, shells, rockets, cells and their boxes), the
  backpack, the six keys and berserk, with DOOM's amounts and limits. An
  item the player has no use for stays. A new weapon is switched to, and
  ammo of a kind the player had none of switches from the fist or pistol to
  a weapon that fires it, as `P_GiveAmmo` does. Zombies drop a clip,
  shotgun guys a shotgun and chaingunners a chaingun when they die, worth
  half a clip or one clip of ammo. The HUD shows the ammo in hand, every
  ammo count and its limit, the number keys of the weapons owned and the
  keys held. A dead player who respawns keeps what they found, without
  berserk, and with at least 50 bullets. Armour, the soul sphere,
  megasphere and the other powerups are still only shown.
- **Sound** (`crates/wad/src/sound.rs`, `src/sound.rs`). The `DS` lumps are
  DMX sounds, turned into in-memory WAVs at start and played through
  bevy_audio: the weapons, monsters' sight, attack, pain, death and active
  sounds, projectiles and their bursts, barrels, the player's pain and
  death, and pickups. Volume falls off with distance as DOOM's does, full
  within 200 units and silent past 1200, with at most a dozen sounds at
  once.
- **Debris** (`src/debris.rs`). `Carver::carve` sends `SprayDebris` with
  the voxels it carved, whatever did the carving, and some of them fly out
  as cubes 0.9 voxels across: `--debris` for a player's shot, fewer for a
  bullet pit, dozens for a barrel's crater (the count grows with the square
  root of the carved volume). They fly along the hit normal in a cone, or
  every way from a barrel, with some lift and spin, under
  avian3d physics at DOOM's scale (32 map units to the metre). A cube shows
  its voxel's palette colour through an unlit material shared by every cube
  of that colour, with the map's contrast gain, as the world draws it. The
  world gets colliders only where cubes fly: regions of 16³ voxels, each a
  parry voxel shape (`Collider::voxels`) of its solid voxels, read with
  `VoxelWorld::get_voxel` and from the map where that is unset. Every solid
  voxel goes in, not only those open to air: with a shell one voxel thick, a
  cube pushed into it by others could come out inside the solid. The shape
  only collides on its open faces, so the rest costs memory, not contacts.
  A carve rebuilds the regions around it at once, and a settling cube a
  tenth of a second later, so a burst costs one rebuild. Any other edit
  reaches them through `EditsCommitted`, a frame late, and rebuilds them a
  tenth of a second after that. A region no cube has come near for two
  seconds is dropped. A cube settles when avian puts it to sleep, after
  0.3 s slow, or at 8 s, or at once when more than 500 are in flight, oldest
  first. It becomes a world voxel in the cell holding its centre, or one of
  the two above if that is solid, and a copy slides and turns into the cell
  and stays until the chunk has remeshed, so nothing blinks. A cube with
  nothing solid under or beside its cell, stopped in mid-air or on a cube
  still in flight, waits, and is dropped if its time runs out, as is a cube
  that falls out of the bottom of the map. Once all
  debris has settled, a line says how many cubes flew, settled and were
  dropped, the longest frames meanwhile, and how long collider builds and
  physics steps took.
- **The voxelizer** (`crates/voxelize`). At import every voxel column gets its
  sector from the map's BSP, with a nearest-linedef test for the void outside
  the map. A voxel is solid below its sector's floor, air up to the ceiling
  (or to the map top under a sky ceiling) and solid above. The solid voxel
  under the air takes the floor flat, the one over it the ceiling flat, and
  solid voxels beside air take the wall texture of the linedef between them,
  with DOOM's pegging rules. Masked middle textures (grates) are one-voxel
  slabs whose transparent texels are air. Doors are baked open, and
  `Options::heights` builds any sector at other heights, which the movers
  use. Sector light
  goes through COLORMAP at a fixed viewing distance.
- **Inside the solid.** Hidden solid voxels carry the nearest surface on
  into the solid, so a crater shows material rather than a hole: the floor
  flat goes on down under a floor, the ceiling flat up over a ceiling, and
  the wall texture of the nearest linedef back into walls and the void. Each
  column knows its nearest column of another sector with air within 10
  columns, found at import. The colour darkens through COLORMAP with depth,
  slowly at first and reaching the darkest table at 10 voxels; from there on
  it is palette index 0. `uniform_in` calls a box all interior only when the
  box grown by 11 voxels holds no surface, so it agrees with `voxel`.
- **The WAD reader** (`crates/wad`) has no dependencies and reads the map
  lumps, PLAYPAL, COLORMAP, flats and composite wall textures.
