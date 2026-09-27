# Handoff — Perfect Dark's arsenal (SHIPPED) and what remains

State: branch `feat/pd-hunters-ship` @ `21ffef0`, **378 tests green**, release built.

The five build items in the original recon are **done**. PD's 33 multiplayer guns are
playable by the player and the hunters, with two firing functions each, both models per
gun, and PD's explosion model. Everything below the "What shipped" section is what is
*left*, in the order it is worth doing.

**Nothing here has been playtested yet.** See the playtest brief at the bottom — that is
the first thing to do, before building anything further on top.

---

## How to work on this track

Two rules the user has stated explicitly, both learned the hard way:

1. **Always go back to the decomp** (`reference/pd-decomp`, gitignored — `reference/README.md`
   says how to re-clone). Do not invent a plausible adjustment. This track produced five more
   examples where the obvious answer was wrong and the decomp had the real one — see
   "Five things measured" below.
2. **Do not launch and drive the game yourself.** It steals focus and makes the user's machine
   unusable. Hand them a specific brief instead.
   *PowerShell note:* env vars need `$env:NAME = "value"` as a separate statement.

```
cargo test --release      # 378 green today
cargo build --release     # the user tests target/release/build-and-hide.exe
```

Regenerate, never hand-edit, the transcribed tables:

```
python tools/pd-assets/pd_weapons.py list                       # summary, one line per weapon
python tools/pd-assets/pd_weapons.py json  tools/pd-assets/pd_weapons.json
python tools/pd-assets/pd_weapons.py rust  native/crates/game/src/combat/pd_weapons.rs
python tools/pd-assets/pd_gltf.py guns tools/pd-assets/pd_weapons.json native/assets/weapons/pd
```

---

## What shipped

| | before | now |
|---|---|---|
| weapons | 23 GoldenEye | +33 Perfect Dark, coexisting (`ARSENAL=ge\|pd\|both`) |
| fire modes | one per weapon | **two**, for player and hunters |
| barrel axis | measured convention | authored `CHRGUNFIRE`, or PD's own grip fallback |
| explosions | one linear sphere | PD's per-axis box, squared, propagating |
| enemy standoff | guessed range × 0.6 | PD's authored per-function engagement bands |
| viewmodel placement | hand-tuned JSON | PD's `posx/posy/posz` (PD guns only) |

**Controls.** `E` (keyboard) or **hold B ~0.42 s** (N64 pad) switches firing function. Both are
PD's own model: a *remembered per-weapon mode bit*, not a second trigger
(`bgun_is_using_secondary_function`, `bondgun.c:9043`). The pad threshold is exact —
`bondmove.c:931` only toggles once `usedowntime > TICKS(25)`. A short B tap still reloads,
which is why **reload now fires on B release rather than on the press**: until B comes up
there is no way to know which press it was.

**Files.** `combat/pd_weapons.rs` (generated table), `combat/arsenal.rs` (the family bridge
+ the AI's function choice), `combat/config.rs` (`SecondaryFire`), `combat/mod.rs` (the
two-function runtime), `combat/explosives.rs` (PD falloff + `Blast`), `combat/enemy_weapons.rs`
(`EnemySecondary`), `gamepad.rs` (the B hold/tap split), `world/combat.rs`, `app.rs`.

### Five things measured, each contradicting a plausible assumption

1. **`WEAPONFLAG_AICANUSE` is not a gun filter.** The original recon said it "says exactly which
   guns an enemy may hold". It is set on all 64 real weapons and absent only from the 20
   non-weapons, so it gates *items*. The real per-weapon AI data is `g_BotWeaponConfigs`
   (`botinv.c:21`) — a score per function plus an engagement band each.
2. **1 PD damage unit = 25.0 of our HP.** Derived from two independent facts agreeing: a PD guard
   has `maxdamage = 4` and the Falcon 2 does `damage = 1` (4 shots); our hunters have 100 HP and
   the PP7 does 25 (also 4).
3. **The barrel axis *is* recoverable from the asset.** `DESIGN_PD_SIMULANT_AI.md` §15 measured a
   convention because it "was not recoverable from the gun mesh". `CHRGUNFIRE` authors it, −X on
   all 27 models that carry one. For the 17 (of 33) with none, `chr_get_gun_pos`
   (`chraction.c:9640`) falls back to `MODELPART_0001` — **fire from the grip**.
4. **PD's explosion falloff is not a sphere.** Per-axis box minimum, squared for characters,
   peaking at `damage * 8.0`. Characters and scenery use **different formulas** (the object path
   has a 30% floor; the character path does not).
5. **Propagation does not fix the grenade self-kill.** The recon hoped it might. PD applies the
   **full** damage radius on the blast's first frame, so PD is *less* forgiving. PD's real answer
   is structural — a grenade bot stands at an authored 4.5–7.0 m. Fixed here with a predictive
   guard instead (refuse the throw if a hunter could be inside the blast after ~1 s of flight).

---

## Playtest brief — do this first

Nothing below is worth starting until the arsenal has been seen on screen. **CPU-side green
does not mean it works**: a `MAX_JOINTS` truncation once drew every PD body as a black fan
while every headless check passed.

```powershell
$env:ARSENAL = "pd"
.\native\target\release\build-and-hide.exe
```

In-game, in HUNT:

1. **Do the guns look right in your hands?** Cycle with `Q` through all 33. The viewmodel
   placement is PD's authored `posx/posy/posz` rather than the hand-tuned JSON, and it has
   never been looked at — expect this to be the thing that needs tuning. Screenshot any gun
   that sits wrong (too big, clipping the camera, off-centre, pointing the wrong way).
2. **Press `E`.** The log names the function it switched to. Try it on the **Falcon 2**
   (shot ↔ pistol whip) and the **SuperDragon** (rifle ↔ grenade launcher, its own 6 rounds).
   Does the HUD ammo change to the right pool? Does firing feel like a different weapon?
3. **Hold B on the N64 pad** for about half a second — same switch. Then **tap B** — that
   should still reload, and this is the risky one, because reload moved from press to release.
   Does reloading feel late or unreliable?
4. **Do the hunters hold their guns properly?** PD's `chr*` models are handless by construction,
   so the "James Bond's hand" artifact should be gone. Watch for the gun sitting in the fist at
   a wrong angle instead.
5. **Do hunters hit you?** The barrel axis is authored now. If they consistently miss, that is
   the aim path and worth a screenshot of the aim overlay.
6. **Explosives.** Fire the Rocket Launcher (PD) at a wall near a hunter. PD's blast is much
   more lethal at the centre and much weaker at the rim than before. A/B it with
   `$env:PD_EXPLOSIONS = "0"` for the old linear feel.

Also worth one run of `$env:ARSENAL = "both"` to confirm the GoldenEye 23 are untouched and the
`(PD)`-suffixed duplicates read sensibly in the shop.

---

## What remains, in order

### 1. Tune the PD viewmodel placement (expect this to be needed)

`combat/arsenal.rs` converts `weapondef.posx/posy/posz` from PD centimetres into our view
space, negating z. That conversion is *reasoned* but unverified — PD's first-person camera is
not ours, and `PD_VIEW_SCALE` is inherited from the character pipeline's measured unit
equivalence rather than measured for guns. If the guns sit wrong, this is the one place to fix
it, and the fix is per-weapon data, not code.

### 2. The gun's own animation (`guncmd`)

A 12-opcode bytecode, four scripts per weapon, keyframed sound / part visibility / "you may act
again now". **The exports are deliberately static** — PD's first-person models articulate (43
matrices on the Falcon 2) but our viewmodel is one mesh with a recoil kick, so those parts had
nowhere to land. The part offsets are baked into the geometry rather than dropped, so adding
articulation is a **re-export, not a re-rip**. Assess whether the viewmodel wants it before
committing; most of what these scripts express still has nowhere to go.

### 3. PD weapon audio

Every PD gun currently borrows the closest GoldenEye sound (`combat/arsenal.rs`,
`fire_sound_for`). The real `funcdef_shoot.shootsound` SFX ids **are** transcribed, so this is
a substitution waiting on an asset, not a guess. PD's audio is in `sfx.ctl`/`sfx.tbl` under
`reference/pd-decomp/src/assets/ntsc-final/` and is not yet extracted. See
[goldeneye-soundpack](memory) for the AIFF→WAV precedent.

### 4. Shop prices for the PD arsenal

`shop::weapon_price` falls back to 1000 credits for anything unlisted, so all 33 PD guns cost
the same. `listed_price` needs PD entries. PD authors no prices, so this is a design call —
but `g_BotWeaponConfigs.score1` is a ready-made desirability ranking to derive them from.

### 5. The special cases, in rising cost

Cheap now that the two-function scaffolding exists (data plus a flag):
* **Phoenix** `EXPLOSIVESHELLS`, **Crossbow** two damage modes, **DY357LX** — all already
  bridged; they need their flags *consumed*, not transcribed.
* **Devastator** `wallhugger` — `STICKTOWALL` is our existing mine behaviour.
* **Reaper** `grind` — the melee funcdef path exists (the pistol whip uses it).

Real features, one system each:
* **Laptop Gun** `deploy` — a deployable sentry. Still the largest single item. `crate::ecs` is
  the natural home, `funcdef_shootauto` already carries `turretaccel`/`turretdecel`, and
  `EngageTarget` means a turret can pick a target the way a hunter does.
* **FarSight** `targetlocator` — sees and shoots through walls. `penetration` is already a
  transcribed `funcdef` field so the *shot* half may be nearly free; the targeting half is the
  work.
* **Slayer** `flybywire` — a player-steered rocket. Camera takeover + controllable projectile.
* **Psychosis Gun** — much cheaper than it looks: §14 gave the FSM an arbitrary `EngageTarget`
  and §16.1 a team predicate, so this is close to "flip the target's team for N seconds".
* **Tranquilizer** `MAKEDIZZY` — the simulant model has the `dizzyamount` hook; nothing drives it.

Also unported, and each needs a system we do not have:
* **B+Z temporary alt-fire.** PD's third B-button behaviour: `invertgunfunc` uses the other
  function *only while held*, versus the tap's permanent toggle. Also two per-weapon exceptions
  in `bgun_consider_toggle_gun_function` — the Sniper Rifle wants a longer 50-tick hold, and the
  RC-P120 / Laptop / Dragon / Remote Mine treat B as a one-shot **action** (deploy,
  self-destruct, detonate) rather than a mode.
* **Cloaking Device / Shield / Combat Boost / X-Ray** — the four `equipment_only` rows,
  excluded by the scope decision. They are transcribed and tagged, not dropped.

### 6. Re-enable the grenade flush

`World::grenades` is still **default-OFF**. The self-kill it was disabled for now has a
predictive fix (and a regression test pinning both sides of a 1 m discrimination), but the
default was set from a playtest, so flipping it back is the user's call. Turn it on and watch
whether hunters still blow each other up.

---

## Traps this track hit, carried forward

1. **CPU-side green does not mean it works.** Still true; hence the playtest brief.
2. **A test can pass because of the bug you are about to fix — or fail because of the fix.**
   Three fired here. `blast_falloff_is_linear` called the dispatcher and so silently changed
   meaning when the default moved to PD. `every_weapon_aims_down_its_barrel` caught a real bug
   (GoldenEye's Shotgun picking up PD's −X barrel through a name collision). And the
   camper-flush test encoded the old non-predictive grenade rule.
3. **Name-keyed lookups collide across families.** Seven PD weapons share a GoldenEye name.
   Shop prices and enemy defs are name-keyed *on purpose*; resolve by **display** name, never
   source name.
4. **Assert token counts when transcribing.** Four parser defects surfaced only because the
   generator refused a column-count mismatch instead of shrugging: a bitfield pair, two
   different `functions[2]` initializer shapes, `#if VERSION` inside an enum (which
   mis-resolved every weapon past 0x4f into a *plausible wrong answer*), and trailing `//`
   comments eating 7 flag defines.
5. **Measure the invariant you actually have.** The barrel-axis test asserts "the muzzle's
   dominant component is −X" — what was measured — rather than an angular tolerance. The Reaper
   sits 36° off because it is a minigun with the barrel slung under the grip; a 25° bound
   invented for tidiness would have failed on real data.

## Context worth reading

* `DESIGN_PD_WEAPON_MECHANICS.md` — the weapon-side recon. §2 (no attach offset to tune),
  §3 (two models per gun), §5 (`attackanimconfig`), §8 (`guncmd`).
* `DESIGN_PD_SIMULANT_AI.md` §13 (transcribing PD data with provenance), §15 (the barrel axis —
  now superseded, see above), §19 (the two asset conventions).
* `combat/pd_weapons.rs` module docs — the conversion constants and why each is derived.
* Memory: `pd-arsenal-decisions`, `pd-weapon-mechanics`, `pd-direction-table`, `explosives-port`.
