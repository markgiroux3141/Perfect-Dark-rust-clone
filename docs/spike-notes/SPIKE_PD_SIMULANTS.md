# SPIKE: Perfect Dark simulants, replicated literally

Branch `spike/pd-simulant`. Code: `native/crates/game/src/pd_spike/`. Binary: `pd_arena`.

## Why a spike

Judging the hunter AI inside the full game was too hard, because every symptom had several possible causes. This spike isolates one thing: Perfect Dark's Combat Simulator bots, ported function by function from the decomp. They use PD bodies and PD animations, in a bare room, with every internal variable on screen.

It shares **no code** with the game's enemy stack on purpose (see "What the decomp overturned" below).

## Run it

```powershell
cd native
cargo build --release --bin pd_arena
./target/release/pd_arena.exe
```

Optional environment variables. In PowerShell, set each one separately first, e.g. `$env:PD_BOTS = "6"`.

| var | default | meaning |
|---|---|---|
| `PD_BOTS` | 4 | 1–8 bots |
| `PD_DIFF` | normal | meat / easy / normal / hard / perfect / dark |
| `PD_WEAPON` | mixed | a weapon name (`ar34`, `cmp150`, `falcon2`, `dragon`, `k7avenger`, `dy357magnum`, `magsec4`, `laptopgun`) or `unarmed` |
| `PD_DUAL` | 0 | `1` = dual-wield one-handed guns |
| `PD_RATE` | 60 | simulated N64 frame rate: 60 / 30 / 20 / 15 |
| `PD_SEED` | PD's seed | RNG seed; runs are deterministic for a given seed |
| `PD_WAYPOINTS` | 250 | the arena's waypoint grid spacing, cm |
| `PD_LEVEL` | arena | `arena` or `complex` (PD's Complex; see `SPIKE_PD_COMPLEX.md`) |
| `PD_DECOMP_DIR` | `reference/pd-decomp` | where Complex's JSON is read from |

**Controls:**
- Camera:
  - RMB drag: orbit
  - MMB or Shift+RMB drag: pan
  - wheel: zoom
  - WASD: slide
  - F: follow the selected bot
- Time:
  - Space: pause
  - `.`: step one frame
  - Speed slider: slow motion
- Match:
  - LMB: select a bot
  - R: reset the match

The inspector on the right shows the selected bot's aibot fields. Difficulty and weapon can be changed live.

**Overlays:**
- cyan arrow: travel heading (`roty`)
- yellow arrow: body facing (`theta`)
- red line: aim ray (bright while the trigger is held)
- orange arc: the ±63° trigger cone
- green/grey line: target link (in sight / not in sight)
- red/green rings: the distance band around the target (the ring for the current mode is lit)
- magenta: the go-to steering point
- tracers: red where a round hit a chr
- optional: the waypoint graph and spawn pads

## What is ported (all verified against `reference/pd-decomp`)

- **Body:** `player_choose_third_person_animation` and its `var80070ba4` table.
  - These are the only animations a bot plays: idle, walk and run rows per wield mode.
  - The legs twist up to ±60° at 6°/tick. The model is drawn at `theta − angleoffset` and the waist counter-rotates by `+angleoffset`.
  - The run clip plays **backwards** when the bot moves more than 93.6° away from where it faces.
- **Animation engine:** PD's `struct anim` (`model.c`).
  - Frame advance per 240 Hz sub-tick.
  - Lazy wrap and clamp by `ANIMFLAG_LOOP`.
  - Explicit loop windows for the idle breathing.
  - The 16-tick merge (a slerp of the old pose into the new), which blocks re-configuration while it runs.
  - Speed tweens and negative speed.
- **Joint callback:** `chr_handle_joint_positioned`. Aim pitch, the leg-twist counter-rotation and the body flinch, applied in world space about each joint (`Ry(aim)·Rz·Rx(−x)·Ry(y−aim)`).
- **Movement:**
  - Velocity-driven. `bot_update_lateral` smoothing and `bot_calculate_max_speed` (Normal 7.6 u/tick, ×0.5 on the last 2 m).
  - Root motion is discarded except its height.
  - `roty` snaps to the steering point; facing turns at most 3.53°/tick.
- **Brain:**
  - `bot_tick`, the free-for-all slice of `bot_tick_unpaused`.
  - `bot_choose_general_target`: round-robin 360° sight; omniscient fallback to the nearest bot.
  - `bot_update_zero_angle` and `g_BotDifficulties`.
  - `botcmd_tick_dist_mode` with `g_BotDistConfigs`.
  - The per-hand trigger logic, reload scheduling and punching.
- **Shooting:**
  - The bot path of `chr_shoot`: `firecount` / ticks-per-shot and `bgun_calculate_bot_shot_spread`.
  - A geometric raycast: the first thing hit takes the damage.
  - The muzzle comes from the rendered gun's `CHRGUNFIRE` node; headless runs use PD's off-screen fallback.
- **Damage:** the bot branch of `chr_damage`. Shove, flinch and death at 8.0 — no stun, no hit clip.
- **Death and respawn:** a random `g_DeathAnimations` clip, a 90-tick fade, then `player_choose_spawn_location`. In one room every pad counts as "very bad", so the bot spawns on one of the 4 pads farthest from the nearest enemy. Spawn fade-in lasts 120 ticks.
- **Timing:** `lv.c` frame timing (`lvupdate240`/`60`/`60f`), plus PD's RNG (`lib/rng_c.c`) and seed.

The verbatim C for all of it is in `reference/pd_bot_port_sheet.md` (gitignored, like the decomp).

## Substitutions (marked at the call site)

- **Pickups:** bots get their configured gun on every spawn, instead of spawning unarmed and running to pickups.
- **Waypoints (arena only):** a regular grid replaces PD's hand-placed pads, routed by the ported `nav_find_route`. Complex uses PD's real graph. A go-to fails only when no route exists. (`nav_find_route`'s count includes its NULL terminator, so PD's `numwaypoints > 1` means at least one waypoint; the first build misread it as two.) Next step: `SPIKE_PD_COMPLEX.md`.
- **Collision:** since step 2, PD's own `chr_calculate_push_pos` over the arena converted to polygons (`SPIKE_PD_COMPLEX.md`, stage 2).
- **Rooms:** the arena is one room, so every room test in `bot_is_about_to_attack` is true. From EasySim up, a bot with a target always faces it.
- **Not ported:** duck/squat rows, cloak, dizziness, the shotgun, explosives, the rest of the arsenal, and a human player.

## What the decomp overturned

These contradict the game's hunter code and `DESIGN_PD_SIMULANT_AI.md`:

1. **Bots never use guard attack animations.**
   - No `ACT_ATTACK`, no `attackanimconfig` fire windows, no 32-slot direction tables.
   - They shoot from their locomotion row.
2. **The trigger cone is ±63.3°, not 45°.** The argument is 45/256 of a turn (`D256TOR`).
3. **Horizontal aim equals body facing** (`holdturn = false` for bots). No aim offset pulls the barrel onto the target. The zeroing error is the whole accuracy model, and a zeroed NormalSim still carries up to 5° of it.
4. **Automatics have no burst pause for bots.** `FUNCFLAG_BURST3` is only read on the single-shot path, so an AR34 fires every 4 ticks (15 rds/s) while its conditions hold.
5. **Position is velocity, not root motion.** The clip's playback speed is fixed (0.5, ~realtime) whatever the bot's speed.
6. **`animscale` lands every body's feet** within ±4 cm with no measured offset. The game uses a per-body foot offset instead.

## Headless checks

`cargo test --release -p game pd_spike` runs 21 tests covering the animation engine, the chooser, leg-twist signs, feet on the floor, fire cadence, the zeroing residual, and that a match produces kills.

For a readable per-second timeline:

```
cargo test --release -p game pd_spike::tests::probe -- --ignored --nocapture
```

## A human player (branch `spike/pd-complex`, 2026-09-27)

`SPIKE_PD_COMPLEX_FIGHT.md` adds a player chr (`SimConfig::humans`, `Chr::player`). `bot_tick` skips it and a host moves it. Also ported for it:

- `chr_calculate_aimend`'s player branch: bots aim at eye − 0.4 × eye height;
- `chr_damage` with hit parts (`chr_damage_hitpart`), with player victims handed to the host (`Sim::player_hits`);
- `bot_is_target_invisible`, `canseecloaked`, `targetcloaktimer60` and `zerocloakspeed` (the player can cloak);
- `footstep_check_default`, with floor types on `GeomPoly` from the tiles' `floortype`;
- the effects half of `chr_shoot`, whose records now carry the hand, the weapon, `makebeam` and wall hits.

Spawn uses the chr's own radius (30 for a player). The bot-only matches and their seeded A/B tests are unchanged.
