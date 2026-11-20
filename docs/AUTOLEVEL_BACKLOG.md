# Autolevel: what is left

Everything planned for the level generator and its editor is built (editor milestones 1 to 5,
the importer's source for generated levels, simulant navigation, views drawn by the game, the
editor inside `pd_edit`; see [AUTOLEVEL.md](AUTOLEVEL.md) and M21 in
[MILESTONES.md](MILESTONES.md)). This is what remains, as of 2026-11-17, in the order it
should be taken: the user's own look first, since it is the most likely source of real work.

## 1. Waiting on the user

- **The first look.** No window built for the generator has been seen by a person: the editor's
  Rooms tool and per-room panel, the generator inside `pd_edit` with its 3D view and Walk, and a
  generated arena in the game. The keys, mouse and drawing code for `pd_edit`'s 3D view has no
  test at all. The playtest brief is in MILESTONES.md, M21's notes. Problems found there come
  before anything else on this list.
- **Hall brightness.** The importer's source for generated levels gives PD real rooms and
  portals, so PD's room lighting settles halls at 210 of 255, about 18% dimmer than the glTF
  import drew them, with a step at some doorways (Complex's own rooms do the same). Kept PD's
  way for now. The alternative is to scale each room's colours by 255 over its settled
  brightness at import.
- **Simulants meet less often** on the new source: 28 kills against 69 over ten 5-minute matches
  on seed 40, target in sight about 45% of the time against 70%. The same drop shows without the
  waypoint repair, so it comes from the rooms. The suspect is PD's respawn rule (no respawn in a
  room an opponent is in, and a hall is now one room), read in the code but not isolated by
  experiment. Worth finding for certain if bot matches on generated levels matter.
- **The ground probe** (`pd_sim::stage::collision`, `cd_find_ground_at_cyl_ctfril`, marked
  `// SUBST`): of more than 20 floor tiles under the player it keeps the 20 nearest, not the
  first 20. Generated levels need it (stacked decks). It also changes Car Park at 60 of 684
  probed spots. Keep it for PD's arenas, or gate it to custom stages?
- **Stage numbers.** (2026-11-19: the custom numbers run from 96 to 127, and 103 to 121 were free;
  `atrium1` and `trek1` took 103 and 104.) Before: 124 to 127 are the only free custom numbers. 124, 125 and 126 hold
  `autolevel40`, `autolevel1` and `autolevel7` (the last two imported while the new source was
  checked). Remove those two (`crates/pd_import/levels/autolevel1.json`, `autolevel7.json` and
  their `custom/stages/` folders) if the numbers are wanted. More numbers would mean widening
  `CUSTOM_STAGENUMS`.

## 2. Known rough edges (small; none blocks anything)

Each is noted under "Rough" in AUTOLEVEL.md's "As built" sections.

- **No edit changes an existing room's height.** Only `add` gives one. Today it means deleting
  the room and adding another.
- **The 3D view is brighter than the game**: it draws every room at full baked brightness with
  no portal pass, as `pd_edit`'s ordinary fly-through does.
- **Keys in the 3D view:** Delete or Backspace still deletes the selected room while the 3D view
  is up (Ctrl+Z undoes it), and there is no key for Plan | 3D. The mouse turns a walker at PD's
  rate, a quarter of the fly camera's (`Walker.p.mouse_sens`).
- **The panel shifts under the pointer:** the first choice of a kind adds a row to the Edits list
  above, so everything below drops one row. With a room selected the list of rooms is below the
  fold of a 950-high window.
- **The level is made twice an edit** in the 3D view (once by the generator's editor, once for
  the preview: about 0.4 s wasted), and `autolevel build` makes it again for its walker (0.6 s
  to open where the glTF took 0.2). Handing the made level on needs proof that the editor's level
  equals `Recipe::generate()`'s.
- **`--choices` says a room "could take" a layout by its size alone**: seed 40's basin with a
  ring is listed as possible, and no inside can be furnished with it.
- **The waypoint sampler's own fault** (`pd_sim::nav::gen`, `sample()`: of two floors within 30
  cm at one point it keeps the first met, where the higher should be kept) is only repaired
  around for generated levels (`pd_import::autonav`). Fixing it at the source would change PD's
  own arenas' graphs and pinned tests: the user's call.
- **A room's translucent layer has a fixed order** (no BSP tree as PD's rooms have): a whole
  hall's rails and glass are one layer. No artefact seen in 47 views.
- **A walker put back after a remake** is not checked for standing room, and "nearest floor" may
  be somewhere odd (a tank's top).
- **`--size 4` levels** were walked (270 of 270) but never imported as an arena.

## 3. Bigger directions (not started; the user chooses)

- **Atriums and the trek** (the user's, 2026-11-18; started 2026-11-19): a hall fifteen storeys
  tall whose top walkway and base are reached through a sequence of other rooms, planned
  goal-first. Storeys to 4134, the atrium's rules, and the planner (`autolevel trek SEED`) are
  built; arenas `atrium1` and `trek1`. Then (the user's direction) made general: `--scale
  towering`, whose tall halls' high pods are reached only from outside, through whatever rooms
  the scatter put at that height (arena `towering9`). Next: climbs in the score, longer climbs.
- **Empty air and platforms** (2026-11-19): measured (`empty_air`, `largest_void` in the report)
  and filled with big platforms off the catwalks (`--platforms N`, arena `towering9p`).
  AUTOLEVEL.md, "Platforms". Then galleries (`--galleries N`): high storeys with no base given a
  catwalk of their own, climbed to from outside (arena `towering9g`). Then drops (`--holes P`):
  holes and spans through platforms, and more drops between rooms (arena `towering9h`). Then
  cellars (`--cellars P`): a room under a hall, a hatch down and a ladder up (arena `cellars40`).
  AUTOLEVEL.md, "Drops". Next asked about: a large base kind, bases shared by two halls.
  AUTOLEVEL.md, "Atriums". Then angled catwalks (`--angled P`, 2026-11-19): an island reached only
  by angled spokes, and shortcuts across a hall's air between catwalks and platforms (arena
  `angled9`); AUTOLEVEL.md, "Angled catwalks". Next: angled ramps between storeys, platforms left
  floating for them. Then flooded rooms (`--flooded P`, on by default at 0.1, the user's call): the
  floor water under railed walkways joining the doors, rafts, angled catwalks and tanks (arena
  `flooded13`); AUTOLEVEL.md, "Flooded rooms". Then a first try at shaped rooms (`--shaped P`,
  0.1 by default, the user's call): any room big enough an L or a U cut from its lot, its catwalks
  round the cut (arena `shaped1`); AUTOLEVEL.md, "Shaped rooms". Then the pillars (the user's,
  2026-11-19): they stop at the first storey, and tall rooms take fewer (a rule of the Rust's own);
  and a pillar with a clear top has a ladder up one side (AUTOLEVEL.md, Rules of the Rust's own).

- **Blank levels: your edit wins** (the user's, 2026-11-18; designed, chosen as the next step):
  start empty, draw rooms, and the generator remakes the rest round each edit, offering
  alternatives where an edit can be met in several ways; a side view for heights. Design, the
  user's decisions and the order of work: AUTOLEVEL.md, "Blank levels: your edit wins".

- **The generator in other levels' styles** (Felicity, G5): the user's stated next step before
  the port. Do the same methods transfer, or does each style need rules of its own? The procgen
  experiments in the Blender MCP repo already measured themes for many PD arenas
  (`addon/ge_pd_tools/procgen/theme_data/*.json`; Felicity and G5 are `fel` and `g5` in
  `procgen/themes.py`, format in `docs/experiments/procgen_modules/THEME_FORMAT.md`). Started
  2026-11-17: the dressing is a `kit::Kit` (Complex is `complex::KIT`), and `--style felicity` makes a
  Felicity level with its own planner, kit and doors (AUTOLEVEL.md, "Felicity"). Next for Felicity:
  the user's playtest of `felicity7`; then the hall's bays, windows and rails, the beam ceilings,
  the editor. G5 would be a third planner or a variant of Felicity's (rooms off halls, no doors).
- **Sightlines in the score.** The Python's `los` took 30 to 60 s a plan, so it was left out; the
  Rust may make it affordable.
- **Spots, ladders, half levels**, and the user's **wave-function-collapse** idea for room
  interiors (the furnisher's half-cell grid is ready for it).
- **"Lock a room, re-roll the rest"**, in the editor's original plan. Not asked for again, not
  built.
- **Retiring the frozen Python** in the Blender MCP repo. Nothing new depends on it but the answer
  keys; when it goes, the keys become a record re-written from the Rust's own output, and a tool
  for that is needed.
