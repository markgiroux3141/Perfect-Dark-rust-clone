# Handoff — PD fidelity: DONE, and the track is out of the spike

State: `main`, 330 tests green, release built. **All four items of this handoff are
complete, plus the roster widening that followed.** Perfect Dark is no longer behind `PD_LAB=1` — it is the game's
hunter AI. Full write-ups in `DESIGN_PD_SIMULANT_AI.md`:

| item | § | outcome |
|---|---|---|
| 1. The 32-slot direction table | §13 | 15 clips exported at slots 36–50, tables transcribed, per-burst selection wired. Live-confirmed. |
| 2. The free-for-all seam | §14 | `Enemy::update` takes an `EngageTarget`; the whole chase / standoff / cover chain runs against an arbitrary target. Live-confirmed. |
| 3. The barrel-axis fallback | §15 | Not a cleverer estimator — the axis is a convention of the asset set, and `BARREL_MODEL_AXIS` had the wrong sign. |
| 4. Promote out of the spike | §17 | Done, with the three decisions below taken. |
| 5. Widen the roster | §18 | 44 GoldenEye + 6 Perfect Dark bodies, all on PD's animations. The "cost" in §17 was a wrong doc comment. |

Playtest also turned up three engagement defects, all with decomp answers — §16.

## How to work on this track

Two rules the user has stated explicitly, both learned the hard way, and both still true:

1. **Always go back to the decomp.** Do not invent a plausible adjustment to make
   something look right. Every bug on this track has had a decomp answer, and several
   times the *obvious* fix was demonstrably wrong: the barrel axis was not recoverable
   from the mesh (§15), the pack fighting itself was a missing team check rather than a
   targeting-score problem (§16.1), and packmates blocking fire was our sight cast rather
   than a crowding problem (§17).
2. **Do not launch and drive the game yourself.** It steals focus and makes the user's
   machine unusable. When something has to be seen on screen, stop and hand them a
   specific brief: exact command, what to do in-game, what to look at, which screenshot.

What you *can* do without them:
* `tools/pd-assets/pd_preview.py <model.glb> <out.png> --clip <clip.glb> --frames 8 --yaw 90 --highlight Bone_9`
  — CPU render, no GPU, no engine.
* Headless measurement through the real engine types. `cargo test -p game fire_animation --
  --nocapture` prints every fire animation's measured barrel yaw against the `angleoffset`
  read out of `chraction.c` — that is the pattern: assert the asset against the decomp,
  not against itself.
* The AI lab: `cargo test -p game -- world::ai_testbed` runs the jank scenarios headlessly
  and the `JankMonitor` reports stalls, thrash, overlap and packmate occlusion.

```
cargo test --release                  # 330 green
cargo build --release                 # the user tests target/release/build-and-hide.exe
```

## The decisions that were taken

Put to the user rather than assumed (this handoff's predecessor flagged all three):

* **Scope — everything, Perfect Dark bodies included.** Hunters run PD clips, the simulant
  AI and zeroing shots. *(The stated reason — "the directional fire table cannot run on a
  GoldenEye body" — turned out to be false, which is what §18 then exploited: the roster is
  both families now, all of them on PD's animations.)*
* **The zeroing model replaces the hit roll for all hunters**, retiring `MAX_HIT_RATE`, the
  per-weapon `accuracy` and the `accuracy_mult` / `falloff_ease` difficulty levers together.
* **The radar is always up in HUNT**, promoted from lab aid to player-facing feature.

## What is left

**The body-roster cost turned out not to exist** — §18. A GoldenEye body plays the Perfect
Dark clip set correctly (identical bind orientations; only bone lengths differ, which
rotation clips ignore), so the roster is all 44 GoldenEye bodies plus the 6 Perfect Dark
ones with every hunter on PD's animations. `PD_TEMPLATE_CLIPS` is bound twice, once per rig.

The remaining asset opportunity is the other direction: `pd_roster.json` establishes that
any of the **65** shared-rig PD bodies exports fully textured from both texture storage
paths, and only 6 are exported. Adding more is: extend `characters` in `pd_roster.json`,
re-run `pd_gltf.py batch`, extend `PD_BODY_CATALOG`. Head/body pairings are an authoring
choice (PD mixes them freely via `g_HeadsAndBodies`), not a derivation. Worth doing because
the families appear in proportion to the catalog — at 38:6 a Perfect Dark body is the rarer
sight in a wave.

**Personality is per-wave, not per-hunter.** `PdHunters::bot_type` is `General` for the
whole squad. Half of `BotType` would stop a hunter hunting (`Peace` never fires, `Coward`
flees unless it out-guns you), so a varied squad needs the type chosen per hunter with the
non-hunting ones excluded — at which point Prey / Judge / Venge / Feud become real flavour,
since §14 gave them a target they can actually manoeuvre against.

**§11's roadmap is otherwise unchanged** and item 1 there is now closed. The next-best
items by behaviour-per-unit-work: authored cover points with a reservation lock (ours will
hand the same nav cell to the whole pack), room/portal awareness for the search fan-out,
and the one-line deceleration ramp. PD's "waypoints" are still not worth porting — see the
note there and `ai-navmesh-attempt-reverted`.

## Kill-switches and flags

Behaviour switches (all default ON unless noted): `set_pd_omniscience`, `set_utility_ai`,
`set_local_avoidance`, `set_head_look`, `set_wall_clearance`, `set_authored_reactions`,
`set_ragdoll`, `set_hit_reactions` (**default OFF** — GoldenEye-family flinches; a PD
hunter uses the authored tables instead), and `set_grenades` (**default OFF** — hunters
were catching their own blast; the safe-distance check tests the moment of release, but the
round flies ~1 s while the pack keeps closing).

Boot environment. **PowerShell has no inline env-var prefix**, so it is two statements and
the value needs quoting — `$env:BODIES = "ge"` then `.	arget
eleaseuild-and-hide.exe`.
Bare `BODIES=ge cmd` is bash-only and PowerShell tries to run `ge`. They persist for the
session; clear one with `Remove-Item Env:\BODIES`.

`GE_CLIPS` reads its value (`0`/`false`/`no`/`off` = off). The older `PD_LAB*` flags key off
mere presence, so `PD_LAB=0` still turns the lab **on** — unset them rather than zeroing them.

The boot log prints the resolved roster (`HUNTERS: 38 GoldenEye + 0 Perfect Dark bodies,
Perfect Dark clips`). Check it rather than guessing: env vars persist for a shell session, so
a `PD_LAB` set an hour ago is still set, and the roster block deliberately runs after every
mode default so an explicit `BODIES=` cannot be silently overridden again.


| var | effect |
|---|---|
| `PD_LAB=1` | the bare test room + the per-simulant debug overlay. **Not** a gate on the AI any more. |
| `PD_LAB_COUNT=n` | wave size (1 = duel) |
| `PD_LAB_DIFFICULTY=meat…dark` | pin a tier instead of following the dial |
| `PD_LAB_TYPE=general\|peace\|coward\|…` | the personality axis |
| `PD_LAB_FFA=1` | **teams off** — hunters fight each other as well as the player (§16.1). Off by default. |
| `BODIES=ge\|pd` | one body family only. Still Perfect Dark's animations either way — this is an aesthetic/debug switch, not a fidelity one. |
| `GE_CLIPS=1` | the pre-promotion **GoldenEye clip set** on GoldenEye bodies: hand-set fire windows, height-zone hit picks, canned flinches, no directional table. The A/B, kept reachable. Narrows the bodies too, since a PD body cannot take a GoldenEye clip. |

## Traps this track has hit

1. **CPU-side green does not mean it works.** A `MAX_JOINTS` truncation drew every PD
   body as a black fan while all headless checks passed. Hand off a playtest brief.
2. **Measure through the real stack, not the raw asset.** The aim bug was invisible in
   the clip and only appeared once composed — clip-alone `+1.6°`, through the layer
   stack `−78.2°`.
3. **The obvious fix has been wrong more often than not.** See rule 1 above.
4. **A test can pass because of the bug you are about to fix.** `orca_a_pack_funnels_through_a_doorway`
   crossed its doorway only because a firing hunter could not plant, and
   `utility_engages_holds_standoff_and_fires` only passed because it happened to spawn a
   GoldenEye hunter that fired in short bursts. The two were in direct tension and the
   family split was hiding it. When a promotion makes one default universal, expect the
   tests that encoded the old default to be load-bearing in ways nobody wrote down.
5. **Porting a selection algorithm means porting its filters too.** We took
   `bot_choose_general_target`'s distance walk and its personality vetoes and left out one
   line of team check, which is the whole difference between a squad and a free-for-all.
6. **A doc comment asserting an incompatibility is not evidence of one.** "PD's bind pose is
   not GoldenEye's" cost the roster 38 bodies and was wrong — the bind orientations are
   identical to 0.0°, and one headless measurement settled it. When a claim is load-bearing
   for a decision, measure it; the engine can pose any clip against any skeleton on the CPU
   in milliseconds.
