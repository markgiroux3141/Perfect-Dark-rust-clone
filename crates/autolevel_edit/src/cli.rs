//! The command line, read into what to do: which recipe (a seed and options, or a file), what
//! to change in it (move, size, pin, add or delete a room's box; choose a room's layout, cover,
//! inside, brightness or ceiling; take a route out; join a room to another), and whether to
//! write a snapshot, a recipe or a list instead of opening the window. Here, not in `main.rs`,
//! so tests reach it; `headless::run` does what it asks for.

use autolevel::boxes::{self, Change};
use autolevel::candidates::Asked;
use autolevel::level::PARTS;
use autolevel::recipe::{self, Doorway, Edit, Recipe};
use autolevel::storeys::LEVELS;

pub const USAGE: &str = "autolevel_edit: the level editor for autolevel

  autolevel_edit --seed 40 --shot plan.png          the plan as a PNG, and the dead-end list as text
  autolevel_edit --seed 40                          the window
  autolevel_edit --seed 40 --pick tower@551 --shot plan.png
                                                    what could be joined to the tower's storey at 551:
                                                    a numbered list, and the same numbers on the plan
  autolevel_edit --seed 40 --pick tower@551 --join 3 --save-recipe r.json --shot plan.png
                                                    make number 3; the recipe and the level with it
  autolevel_edit --recipe r.json --routes           the level's routes, by the keys --remove takes
  autolevel_edit --recipe r.json --remove c12 --save-recipe r.json --shot plan.png
  autolevel_edit --seed 40 --lots                   the rooms' boxes, by the keys the room flags take
  autolevel_edit --seed 40 --move medium1@48,41 --try --shot plan.png
                                                    what moving a room would come to: the boxes as
                                                    they would lie, or in red why it can't be done
  autolevel_edit --seed 40 --rect medium1@50,29,59,36 --add small@70,40,74,44@276@512 --save-recipe r.json
                                                    a room resized and one made; the recipe with both

  --seed N              the level's seed (default 40)
  --empty               an empty start: no rooms but those --add makes; the seed only decorates them
  --recipe FILE         a recipe (seed, options, edits) instead; --seed and options then change it
  --NAME VALUE          an option, by its name in the recipe: scale size site cover clutter walkway
                        crates shapes risers second vents windows drops grates platforms galleries holes cellars angled flooded shaped sunken rescue
                        dead-ends dead-corridor dead-walk dead-climb loops shift level-change tries
                        (also --no-shapes, --keep-dead-ends, --no-rescue, --no-bases, --no-big-pods, and --crates alone)
  --shot FILE.png       write the plan view and print the dead-end list; no window, no GPU
  --storey Z            the snapshot shows one storey, by its floor (-276 0 276 551 ...; default all)
  --width W             the snapshot's width in pixels (default 1600)
  --list                print the dead-end list only
  --routes              print the level's corridors, door frames and vents: each one's key, the
                        doors it joins, what it is, and whether it is the only way to a room
  --remove KEY          take out the route with this key (any of its pieces' keys). Refused if it
                        is the only way to a room. One the dead-end pass added is taken out by
                        keeping its dead end. May be given more than once; done before --pick
  --pick ROOM@STOREY    list the corridors that could join this room, by a door on this storey, to
                        the rooms near it (ROOM alone: its own floor). With --shot they are drawn
                        on the plan, numbered as listed: each room's cheapest, or with --all every one
  --join N              make number N of that list: a join added to the recipe. What is drawn is
                        what is made: the same cells, the same doors, the same key
  --door SIDE:AT        with --pick: only joins through this doorway on the room's wall (E N W or S,
                        and the cell along it of the door's low end: x on N or S, y on E or W),
                        as E:26. The plan draws it, and a --join made with it keeps it
  --to ROOM             with --pick: only joins to this room (near or not)
  --to-door ROOM@SIDE:AT
                        with --pick: only joins to this room, through this doorway on its wall
  --via WAY             with --pick: joins by a corridor (never a vent) or a vent (even where a
                        corridor fits); none: a corridor, failing that a vent
  --all                 the snapshot draws every corridor --pick lists, not only each room's cheapest
  --rooms               the snapshot draws the room boxes over the level, as the recipe places
                        them (any of the room flags below turns this on)
  --select ROOM         that room's box is drawn selected: its grips, its size and heights
  --lots                print the lots: each room's key, kind, box in cells, floor and top, and
                        whose pod it is or which pods it has
  --move ROOM@X,Y       the room's low (south-west) corner goes to this cell, its size kept. A
                        hall's pods go with it
  --rect ROOM@X0,Y0,X1,Y1
                        the room's box becomes this, low corner then high. A hall's pods stay
  --pin ROOM@FLOOR      the room's floor goes on this storey, its height kept
  --height ROOM@FLOOR@TOP
                        the room stands from this floor to this ceiling (a hall's pods go as many
                        storeys as its floor): the side view's edit
  --delete ROOM         the room goes: a hall's pods with it, and the edits that name them
  --add KIND@X0,Y0,X1,Y1@FLOOR@TOP
                        a new room of this kind (--lots shows the kinds in use), from this floor to
                        this ceiling (236 512 787 1063 1338 1614 1890). Its key is the next free
                        one (room1, room2, ...), and is printed
                        Room changes are made in the order given, before --remove and --pick, and
                        the level is made again after them. One that breaks a rule (rooms keep a
                        cell between them, a pod fits its hall, ...) is refused: the snapshot is
                        still written, with the boxes as they would lie, that box in red and the
                        reason by it; nothing after it is done, and the exit status is 1
  --try                 the last room change is tried, not made: the snapshot and the text show
                        what it would come to, and the recipe is left as it was
  --layout ROOM@ring|galleries|mezzanine|none
                        a hall's first upper storey: a ring of catwalk round its walls, galleries
                        down both long walls with bridges across, a mezzanine across one end, or none
  --cover ROOM@mixed|pillars|crates|none
                        what stands on the room's floor (--cover with no ROOM@ is the level's option)
  --inside ROOM@N       the room furnished from its Nth other stream of random numbers: another
                        inside, its doors as they were (0: the seed's own)
  --reroll ROOM@PART@N  one part of the room's inside from its Nth other stream, the rest as it
                        was: catwalks (and its layout), ramps (where they go), cover (where it
                        stands: crates, a notch or cross wall; pillars stand by rule) or windows
                        (which places along its walls, glass or rail). 0: the seed's own
  --variants ROOM[@PART]
                        the next five re-rolls of the room's part (PART as --reroll's, or inside,
                        the default: its whole inside), made side by side and listed, numbered;
                        with --shot also a strip of small pictures, FILE.variants.png, the room as
                        it is now first. Done after --pick
  --from N              with --variants: the re-rolls from stream N (default 1)
  --keep N              with --variants: keep number N (the recipe and the snapshot have it)
  --brightness ROOM@0..255
                        what the room's floor is baked to
  --ceiling ROOM@plain|ceiling_tile|ceiling_dark|grille
                        the room's ceiling. Brightness and ceiling are for the built level: the
                        plan only says that they were chosen
  --feature ROOM@FEATURE@on|off|auto
                        one of the level's features for this room alone: shaped (an L or a U),
                        flooded, sunken, cellar (a big hall on the ground), platforms, holes
                        (drops through them), angled (catwalks across its air). On: wherever it
                        fits; off: none; auto: the level's chance again
  --own ROOM@layout|cover|inside|brightness|ceiling|FEATURE|reroll-PART|all
                        the seed's own again for that choice, or for all of the room's
  --choices ROOM        print the room's choices: what it has, whether the seed or the recipe
                        chose it, and each layout it could take or why it can't. Printed for
                        the level as it is after every change on the line; may be repeated
                        A room's choices are made in the order given, after the room changes
                        and before --remove and --pick, and the level is made again after each.
                        What was chosen is written under the room's name on the plan. One that
                        can't be (a pod's cover, a layout the room is too small for) is refused,
                        and one no inside can be furnished with makes no level: either way the
                        snapshot is still written, of the level as it was, with the room's box
                        in red and the reason by it; nothing after it is done, the exit status is 1
  --side x@Y | y@X | x@ROOM | y@ROOM
                        the snapshot is the side view, not the plan: a section along x through the
                        row of cells y = Y (or along y through the column x = X, or through a
                        room's middle), height upwards, the storeys labelled up the side. With the
                        room flags the boxes the section cuts are drawn over it, --select's with grips
  --save-recipe FILE    write the recipe, with what the room flags, the choices, --remove and
                        --join changed
  --frames N            the window closes by itself after N frames of a made level (a smoke test)";

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub recipe: Recipe,
    /// The file the recipe came from.
    pub from: Option<String>,
    pub save_recipe: Option<String>,
    pub shot: Option<String>,
    /// The storey a snapshot shows (None: all).
    pub storey: Option<i32>,
    pub width: usize,
    pub list: bool,
    pub frames: Option<u32>,
    /// Print the level's routes.
    pub routes: bool,
    /// Routes to take out, by a piece's key, in this order.
    pub remove: Vec<String>,
    /// The room to join from, and the storey (None: its own floor).
    pub pick: Option<(String, Option<i32>)>,
    /// What is asked of the joins from the pick (`--door`, `--to`, `--to-door`, `--via`).
    pub asked: Asked,
    /// Which of the corridors found for the pick to make, counting from 1.
    pub join: Option<usize>,
    /// Draw every corridor found for the pick, not only each room's cheapest.
    pub all: bool,
    /// The snapshot draws the room boxes (`draws_rooms`: any room flag does as much).
    pub rooms: bool,
    /// The room whose box the snapshot draws selected.
    pub select: Option<String>,
    /// Print the lots.
    pub lots: bool,
    /// Changes to the room boxes, in the order given. (An `Add`'s key is empty here: it gets
    /// the next free one when it is made.)
    pub boxes: Vec<Change>,
    /// The last of them is tried, not made.
    pub trial: bool,
    /// Choices for single rooms, in the order given.
    pub choose: Vec<Choose>,
    /// Rooms whose choices to print.
    pub choices: Vec<String>,
    /// The snapshot is the side view along this section, not the plan.
    pub side: Option<Side>,
    /// Re-rolls of a room's part (None: its whole inside) to make side by side (`--variants`).
    pub variants: Option<(String, Option<&'static str>)>,
    /// The first stream they start from (`--from`; 1 by default).
    pub vary_from: u32,
    /// Which of them to keep, counting from 1 (`--keep`).
    pub keep: Option<usize>,
}

/// Where `--side` cuts: its axis, and a row or column of cells, or a room's middle.
#[derive(Clone, Debug, PartialEq)]
pub struct Side {
    pub axis: crate::sideview::Axis,
    pub at: SideAt,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SideAt {
    Cell(i32),
    Room(String),
}

impl Side {
    /// Read `x@12`, `y@30` or `x@vast`.
    pub fn parse(v: &str) -> Result<Side, String> {
        use crate::sideview::Axis;
        let bad = || format!("--side: x or y, then @ and a row or column of cells or a room, as x@12 or y@vast, not {v}");
        let (axis, at) = v.split_once('@').ok_or_else(bad)?;
        let axis = match axis {
            "x" => Axis::X,
            "y" => Axis::Y,
            _ => return Err(bad()),
        };
        let at = match at.parse::<i32>() {
            Ok(n) => SideAt::Cell(n),
            Err(_) if !at.is_empty() => SideAt::Room(at.to_string()),
            Err(_) => return Err(bad()),
        };
        Ok(Side { axis, at })
    }
}

/// A choice for one room, as the line asks for it.
#[derive(Clone, Debug, PartialEq)]
pub enum Choose {
    /// Make this choice: a `layout`, `cover`, `reroll`, `brightness` or `ceiling` edit.
    Set(Edit),
    /// The seed's own again in a room, for one kind of choice (`choices::KINDS`) or for "all".
    Own { room: String, what: String },
}

impl Args {
    /// Is there something to do that needs no window?
    pub fn headless(&self) -> bool {
        self.shot.is_some() || self.list || self.save_recipe.is_some() || self.routes || self.lots || !self.choices.is_empty() || self.changes()
    }

    /// Does it change the recipe by what the recipe makes: its lots (the room flags) or its
    /// level (a room's choices, `--remove`, `--pick`)?
    pub fn changes(&self) -> bool {
        !self.boxes.is_empty() || !self.choose.is_empty() || !self.remove.is_empty() || self.pick.is_some() || self.variants.is_some()
    }

    /// Does a snapshot draw the room boxes? Asked for (`--rooms`), or any room flag is given.
    pub fn draws_rooms(&self) -> bool {
        let empty = self.recipe.options.start == autolevel::level::Start::Empty;
        empty || self.rooms || self.select.is_some() || self.lots || !self.boxes.is_empty()
    }
}

/// A storey by its floor, as text.
fn floor(what: &str, v: &str) -> Result<i32, String> {
    let floors = LEVELS.map(|z| z.to_string()).join(" ");
    v.parse().ok().filter(|z| LEVELS.contains(z)).ok_or_else(|| format!("{what} ({floors}), not {v}"))
}

/// A room's key, then `@` and the rest: "tower@12,30" is ("tower", "12,30").
fn room_at(v: &str) -> Option<(&str, &str)> {
    v.split_once('@').filter(|(room, rest)| !room.is_empty() && !rest.is_empty())
}

/// `N` whole numbers with commas between: cells.
fn cells<const N: usize>(v: &str) -> Option<[i32; N]> {
    let got: Vec<i32> = v.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
    got.try_into().ok()
}

/// `--add`'s value: a kind, a box, a floor and a top, with `@` between. The key is left empty.
fn added(v: &str) -> Result<Change, String> {
    let shape = || format!("--add: a kind, a box, a floor and a top, as medium@3,4,9,8@0@236, not {v}");
    let parts: Vec<&str> = v.split('@').collect();
    let [kind, rect, z, top] = parts[..] else { return Err(shape()) };
    let [x0, y0, x1, y1] = cells::<4>(rect).filter(|_| !kind.is_empty()).ok_or_else(shape)?;
    let z = floor("--add: its floor", z)?;
    let tops = boxes::tops(z);
    let lines = || tops.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ");
    let top = top.parse().ok().filter(|t| tops.contains(t)).ok_or_else(|| format!("--add: its top, a ceiling over its floor ({}), not {top}", lines()))?;
    Ok(Change::Add { key: String::new(), kind: kind.to_string(), rect: (x0, y0, x1, y1), floor: z, top })
}

/// The options that are yes or no: named alone, they are yes.
const FLAGS: [&str; 6] = ["crates", "shapes", "dead_ends", "rescue", "bases", "big_pods"];

/// Read the arguments (the program's name left off). `read` is how a recipe file is read.
pub fn parse(args: &[String], read: &dyn Fn(&str) -> Result<String, String>) -> Result<Args, String> {
    let value = |n: usize| args.get(n + 1).map(String::as_str).ok_or_else(|| format!("{} needs a value", args[n]));
    let number = |n: usize| value(n).and_then(|v| v.parse::<u64>().map_err(|_| format!("{}: a whole number, not {v}", args[n])));
    // the recipe file first, wherever it is named: the rest changes it
    let mut out = Args {
        recipe: Recipe::new(40),
        from: None,
        save_recipe: None,
        shot: None,
        storey: None,
        width: 1600,
        list: false,
        frames: None,
        routes: false,
        remove: Vec::new(),
        pick: None,
        asked: Asked::default(),
        join: None,
        all: false,
        rooms: false,
        select: None,
        lots: false,
        boxes: Vec::new(),
        trial: false,
        choose: Vec::new(),
        choices: Vec::new(),
        side: None,
        variants: None,
        vary_from: 1,
        keep: None,
    };
    // a room, then @ and its choice: ("vast", "ring") of "--layout vast@ring"
    let choice = |n: usize, what: &str| {
        let v = value(n)?;
        room_at(v).map(|(room, v)| (room.to_string(), v)).ok_or_else(|| format!("{}: a room, then @ and {what}, not {v}", args[n]))
    };
    if let Some(n) = args.iter().position(|a| a == "--recipe") {
        let path = value(n)?;
        out.recipe = recipe::load(&read(path)?).map_err(|e| format!("{path}: {e}"))?;
        out.from = Some(path.to_string());
    }
    let mut n = 0;
    while n < args.len() {
        let arg = args[n].as_str();
        let Some(name) = arg.strip_prefix("--") else { return Err(format!("{arg}: not an option (see --help)")) };
        let name = name.replace('-', "_");
        let mut took = 2;
        match name.as_str() {
            "recipe" => {}
            "seed" => out.recipe.seed = number(n)?,
            "save_recipe" => out.save_recipe = Some(value(n)?.to_string()),
            "shot" => out.shot = Some(value(n)?.to_string()),
            "width" => out.width = number(n)? as usize,
            "frames" => out.frames = Some(number(n)? as u32),
            "storey" => {
                let v = value(n)?;
                out.storey = match v {
                    "all" => None,
                    _ => Some(floor("--storey: all, or a floor", v)?),
                };
            }
            "empty" => (out.recipe.options.start, took) = (autolevel::level::Start::Empty, 1),
            "list" => (out.list, took) = (true, 1),
            "routes" => (out.routes, took) = (true, 1),
            "all" => (out.all, took) = (true, 1),
            "remove" => out.remove.push(value(n)?.to_string()),
            "join" => out.join = Some(number(n)?.try_into().ok().filter(|k| *k > 0).ok_or("--join: a number of the --pick list, from 1")?),
            "pick" => {
                let v = value(n)?;
                out.pick = Some(match v.split_once('@') {
                    Some((room, z)) if !room.is_empty() => (room.to_string(), Some(floor("--pick: a room, then @ and a floor", z)?)),
                    None if !v.is_empty() => (v.to_string(), None),
                    _ => return Err(format!("--pick: a room's key and a storey, as tower@551, not {v}")),
                });
            }
            "door" => {
                let v = value(n)?;
                out.asked.door = Some(Doorway::parse(v).ok_or_else(|| format!("--door: a wall (E N W or S), then : and a cell along it, as E:26, not {v}"))?);
            }
            "to" => out.asked.to = Some(value(n)?.to_string()),
            "to_door" => {
                let v = value(n)?;
                let read = room_at(v).and_then(|(room, d)| Some((room.to_string(), Doorway::parse(d)?)));
                let (room, d) = read.ok_or_else(|| format!("--to-door: a room, then @ and a doorway on its wall, as medium1@W:27, not {v}"))?;
                (out.asked.to, out.asked.to_door) = (Some(room), Some(d));
            }
            "via" => {
                let v = value(n)?;
                if !recipe::VIAS.contains(&v) {
                    return Err(format!("--via: {} (none: a corridor, failing that a vent), not {v}", recipe::VIAS.join(" or ")));
                }
                out.asked.via = Some(v.to_string());
            }
            "rooms" => (out.rooms, took) = (true, 1),
            "lots" => (out.lots, took) = (true, 1),
            "try" => (out.trial, took) = (true, 1),
            "select" => out.select = Some(value(n)?.to_string()),
            "delete" => out.boxes.push(Change::Delete { room: value(n)?.to_string() }),
            "move" => {
                let v = value(n)?;
                let read = room_at(v).and_then(|(room, at)| Some((room, cells::<2>(at)?)));
                let (room, [x, y]) = read.ok_or_else(|| format!("--move: a room, then @ and the cell its low corner goes to, as tower@12,30, not {v}"))?;
                out.boxes.push(Change::Move { room: room.to_string(), to: (x, y) });
            }
            "rect" => {
                let v = value(n)?;
                let read = room_at(v).and_then(|(room, at)| Some((room, cells::<4>(at)?)));
                let shape = || format!("--rect: a room, then @ and its box, low corner then high, as tower@12,30,20,38, not {v}");
                let (room, [x0, y0, x1, y1]) = read.ok_or_else(shape)?;
                out.boxes.push(Change::Rect { room: room.to_string(), rect: (x0, y0, x1, y1) });
            }
            "pin" => {
                let v = value(n)?;
                let (room, z) = room_at(v).ok_or_else(|| format!("--pin: a room's key and a floor, as tower@276, not {v}"))?;
                out.boxes.push(Change::Pin { room: room.to_string(), floor: floor("--pin: a room, then @ and a floor", z)? });
            }
            "height" => {
                let v = value(n)?;
                let shape = || format!("--height: a room, its floor and its top, as tower@276@1063, not {v}");
                let (room, rest) = room_at(v).ok_or_else(shape)?;
                let (f, t) = rest.split_once('@').ok_or_else(shape)?;
                let (floor, top) = (floor("--height: a room, then @ and a floor", f)?, t.parse::<i32>().map_err(|_| shape())?);
                out.boxes.push(Change::Height { room: room.to_string(), floor, top });
            }
            "side" => out.side = Some(Side::parse(value(n)?)?),
            "add" => out.boxes.push(added(value(n)?)?),
            "choices" => out.choices.push(value(n)?.to_string()),
            "layout" => {
                let (room, layout) = choice(n, "its layout (ring galleries mezzanine none), as vast@ring")?;
                out.choose.push(Choose::Set(Edit::Layout { room, layout: layout.to_string() }));
            }
            // (with no room it is the level's option, below)
            "cover" if args.get(n + 1).is_some_and(|v| v.contains('@')) => {
                let (room, cover) = choice(n, "its cover (mixed pillars crates none), as vast@pillars")?;
                out.choose.push(Choose::Set(Edit::Cover { room, cover: cover.to_string() }));
            }
            "ceiling" => {
                let (room, ceiling) = choice(n, "its ceiling (plain ceiling_tile ceiling_dark grille), as vast@grille")?;
                out.choose.push(Choose::Set(Edit::Ceiling { room, ceiling: ceiling.to_string() }));
            }
            "reroll" => {
                let what = format!("a part ({}), @ and which of its streams (0: the seed's own), as vast@catwalks@2", PARTS.join(" "));
                let (room, v) = choice(n, &what)?;
                let bad = || format!("--reroll: a room, then @ and {what}, not {}", args[n + 1]);
                let (part, k) = v.split_once('@').ok_or_else(bad)?;
                let part = PARTS.iter().find(|p| **p == part).ok_or_else(bad)?;
                let k = k.parse().map_err(|_| bad())?;
                out.choose.push(Choose::Set(Edit::Reroll { room, n: k, part: Some(part.to_string()) }));
            }
            "variants" => {
                let v = value(n)?;
                let bad = || format!("--variants: a room, or a room then @ and a part ({} or inside), as vast@catwalks, not {v}", PARTS.join(" "));
                out.variants = Some(match v.split_once('@') {
                    Some((room, "inside")) if !room.is_empty() => (room.to_string(), None),
                    Some((room, part)) if !room.is_empty() => (room.to_string(), Some(*PARTS.iter().find(|p| **p == part).ok_or_else(bad)?)),
                    None if !v.is_empty() => (v.to_string(), None),
                    _ => return Err(bad()),
                });
            }
            "from" => out.vary_from = number(n)?.try_into().ok().filter(|k| *k > 0).ok_or("--from: a stream, from 1")?,
            "keep" => out.keep = Some(number(n)?.try_into().ok().filter(|k| *k > 0).ok_or("--keep: a number of the --variants strip, from 1")?),
            "inside" => {
                let what = "which inside, a whole number (0: the seed's own), as vast@2";
                let (room, v) = choice(n, what)?;
                let n = v.parse().map_err(|_| format!("--inside: a room, then @ and {what}, not {}", args[n + 1]))?;
                out.choose.push(Choose::Set(Edit::Reroll { room, n, part: None }));
            }
            "brightness" => {
                let what = "a number from 0 to 255, as vast@120";
                let (room, v) = choice(n, what)?;
                let value = v.parse().map_err(|_| format!("--brightness: a room, then @ and {what}, not {}", args[n + 1]))?;
                out.choose.push(Choose::Set(Edit::Brightness { room, value }));
            }
            "feature" => {
                let names: Vec<&str> = autolevel::level::FEATURES.iter().map(|(f, _)| *f).collect();
                let what = format!("a feature ({}), @ and on, off or auto, as vast@flooded@on", names.join(" "));
                let (room, v) = choice(n, &what)?;
                let bad = || format!("--feature: a room, then @ and {what}, not {}", args[n + 1]);
                let (name, on) = v.split_once('@').ok_or_else(bad)?;
                let name = names.iter().find(|f| **f == name).ok_or_else(bad)?;
                out.choose.push(match on {
                    "on" | "off" => Choose::Set(Edit::Feature { room, feature: name.to_string(), on: on == "on" }),
                    "auto" => Choose::Own { room, what: name.to_string() },
                    _ => return Err(bad()),
                });
            }
            "own" => {
                let what = "what is the seed's own again (layout cover inside brightness ceiling, a feature, or all), as vast@layout";
                let (room, v) = choice(n, what)?;
                let feature = autolevel::level::FEATURES.iter().any(|(f, _)| *f == v);
                // (a part's re-roll: reroll-catwalks, reroll-cover, ...)
                let part = v.strip_prefix("reroll-").and_then(crate::strip::part);
                let kind = match v {
                    _ if part.is_some() => crate::strip::kind(part),
                    "inside" => "reroll",
                    "layout" | "cover" | "brightness" | "ceiling" | "all" => v,
                    _ if feature => v,
                    _ => return Err(format!("--own: a room, then @ and {what}, not {}", args[n + 1])),
                };
                out.choose.push(Choose::Own { room, what: kind.to_string() });
            }
            "no_shapes" => (out.recipe.options.shapes, took) = (false, 1),
            "keep_dead_ends" => (out.recipe.options.dead_ends, took) = (false, 1),
            "no_rescue" => (out.recipe.options.rescue, took) = (false, 1),
            "no_bases" => (out.recipe.options.bases, took) = (false, 1),
            "no_big_pods" => (out.recipe.options.big_pods, took) = (false, 1),
            _ if FLAGS.contains(&name.as_str()) && !matches!(args.get(n + 1).map(String::as_str), Some("true" | "false")) => {
                recipe::set_option(&mut out.recipe.options, &name, "true")?;
                took = 1;
            }
            _ if recipe::OPTIONS.contains(&name.as_str()) => recipe::set_option(&mut out.recipe.options, &name, value(n)?)?,
            _ => return Err(format!("{arg}: not an option (see --help)")),
        }
        n += took;
    }
    if out.join.is_some() && out.pick.is_none() {
        return Err("--join: of what? Name the room and storey with --pick".to_string());
    }
    if (out.keep.is_some() || out.vary_from != 1) && out.variants.is_none() {
        return Err("--keep, --from: of what? Name the room and part with --variants".to_string());
    }
    if !out.asked.plain() && out.pick.is_none() {
        return Err("--door, --to, --to-door, --via: of a join from what? Name the room and storey with --pick".to_string());
    }
    if out.trial && out.boxes.is_empty() {
        return Err("--try: of what? Name a change with --move, --rect, --pin, --delete or --add".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use autolevel::place::Scale;

    fn read(path: &str) -> Result<String, String> {
        match path {
            "seven.json" => {
                Ok("{\"format\": 1, \"seed\": 7, \"options\": {\"vents\": 0}, \"edits\": [{\"edit\": \"remove\", \"route\": \"c07\"}]}".to_string())
            }
            "old.json" => Ok("{\"format\": 9, \"seed\": 7}".to_string()),
            _ => Err(format!("{path}: no such file")),
        }
    }

    fn args(line: &str) -> Result<Args, String> {
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        parse(&words, &read)
    }

    #[test]
    fn a_seed_and_options_by_name() {
        let a = args("--seed 12 --cover pillars --dead-corridor 6 --site 70x50 --scale standard --no-shapes --crates --shot out.png --storey 276 --width 2000")
            .unwrap();
        let o = &a.recipe.options;
        assert_eq!((a.recipe.seed, o.cover, o.dead_corridor, o.siting.site, o.siting.scale), (12, "pillars", 6, Some((70, 50)), Scale::Standard));
        assert_eq!((o.shapes, o.crates, o.dead_ends), (false, true, true));
        assert_eq!((a.shot.as_deref(), a.storey, a.width, a.headless()), (Some("out.png"), Some(276), 2000, true));
        // nothing asked for: seed 40 in the window
        let a = args("").unwrap();
        assert_eq!((a.recipe.clone(), a.headless(), a.frames, a.width), (Recipe::new(40), false, None, 1600));
        let a = args("--frames 3 --keep-dead-ends --shapes false --dead_ends true --storey all").unwrap();
        assert_eq!((a.frames, a.recipe.options.shapes, a.recipe.options.dead_ends, a.storey), (Some(3), false, true, None));
    }

    #[test]
    fn a_recipe_file_that_the_line_then_changes() {
        let a = args("--vents 2 --recipe seven.json --save-recipe out.json").unwrap();
        assert_eq!((a.recipe.seed, a.recipe.options.vents, a.recipe.edits.len(), a.from.as_deref()), (7, 2, 1, Some("seven.json")));
        assert!(a.headless() && a.save_recipe.as_deref() == Some("out.json"));
        assert_eq!(args("--recipe old.json").unwrap_err(), "old.json: recipe: format 9 is not one this build reads (format 1)");
        assert_eq!(args("--recipe none.json").unwrap_err(), "none.json: no such file");
    }

    #[test]
    fn a_pick_a_join_and_routes_to_take_out() {
        let a = args("--seed 40 --remove c07 --pick tower@551 --join 3 --remove c12a --routes --all --shot out.png").unwrap();
        assert_eq!(
            (a.pick.clone(), a.join, a.remove.as_slice()),
            (Some(("tower".to_string(), Some(551))), Some(3), &["c07".to_string(), "c12a".to_string()][..])
        );
        assert!(a.routes && a.all && a.headless() && a.changes());
        // a room alone is picked on its own floor; a floor below ground is a floor
        assert_eq!(args("--pick medium4").unwrap().pick, Some(("medium4".to_string(), None)));
        assert_eq!(args("--pick small7@-276").unwrap().pick, Some(("small7".to_string(), Some(-276))));
        // each needs no window by itself, and none of them is asked for by default
        assert!(args("--routes").unwrap().headless() && args("--remove c07").unwrap().headless() && args("--pick tower").unwrap().headless());
        let plain = args("--seed 40").unwrap();
        assert!(!plain.changes() && !plain.routes && !plain.all && plain.join.is_none());
        assert_eq!(args("--join 2").unwrap_err(), "--join: of what? Name the room and storey with --pick");
        // what is asked of the joins: a doorway each end, the room at the other, the way
        let a = args("--pick medium3@0 --door w:26 --to-door medium1@E:30 --via vent --join 1").unwrap();
        let (w, e) = (Doorway { side: 'W', at: 26 }, Doorway { side: 'E', at: 30 });
        assert_eq!(a.asked, Asked { door: Some(w), to: Some("medium1".into()), to_door: Some(e), via: Some("vent".into()) });
        assert_eq!(args("--pick medium3 --to medium1").unwrap().asked.to.as_deref(), Some("medium1"));
        assert_eq!(args("--door E:26").unwrap_err(), "--door, --to, --to-door, --via: of a join from what? Name the room and storey with --pick");
        assert_eq!(args("--pick medium3 --door X:26").unwrap_err(), "--door: a wall (E N W or S), then : and a cell along it, as E:26, not X:26");
        assert_eq!(args("--pick medium3 --to-door medium1").unwrap_err(), "--to-door: a room, then @ and a doorway on its wall, as medium1@W:27, not medium1");
        assert_eq!(args("--pick medium3 --via tunnel").unwrap_err(), "--via: corridor or vent or straight (none: a corridor, failing that a vent), not tunnel");
        assert_eq!(args("--pick tower --join 0").unwrap_err(), "--join: a number of the --pick list, from 1");
        assert_eq!(
            args("--pick tower@550").unwrap_err(),
            "--pick: a room, then @ and a floor (-276 0 276 551 827 1102 1378 1654 1929 2205 2480 2756 3031 3307 3583 3858 4134), not 550"
        );
        assert_eq!(args("--pick @551").unwrap_err(), "--pick: a room's key and a storey, as tower@551, not @551");
        assert_eq!(args("--remove").unwrap_err(), "--remove needs a value");
    }

    #[test]
    fn room_boxes_to_draw_list_and_change() {
        let a = args("--seed 40 --move medium1@48,41 --rect vast@13,-2,24,34 --pin tower@276 --delete small3 --add small@2,3,6,7@0@512 --try --shot out.png")
            .unwrap();
        assert_eq!(
            a.boxes,
            [
                Change::Move { room: "medium1".into(), to: (48, 41) },
                Change::Rect { room: "vast".into(), rect: (13, -2, 24, 34) },
                Change::Pin { room: "tower".into(), floor: 276 },
                Change::Delete { room: "small3".into() },
                Change::Add { key: String::new(), kind: "small".into(), rect: (2, 3, 6, 7), floor: 0, top: 512 },
            ]
        );
        assert!(a.trial && a.changes() && a.headless() && a.draws_rooms() && !a.rooms && !a.lots && a.select.is_none());
        // the boxes are drawn when asked for, and when a box is selected or the lots are listed
        let a = args("--rooms --shot out.png").unwrap();
        assert!(a.rooms && a.draws_rooms() && !a.changes() && a.boxes.is_empty() && !a.trial);
        let a = args("--select tower --shot out.png").unwrap();
        assert_eq!((a.select.as_deref(), a.draws_rooms(), a.changes()), (Some("tower"), true, false));
        // the list needs no window; the boxes alone are no reason to do without it
        assert!(args("--lots").unwrap().headless() && args("--lots").unwrap().draws_rooms() && args("--delete tower").unwrap().headless());
        assert!(!args("--rooms").unwrap().headless() && !args("--select tower").unwrap().headless() && !args("--seed 40").unwrap().draws_rooms());
        // what each can't read
        assert_eq!(args("--move medium1").unwrap_err(), "--move: a room, then @ and the cell its low corner goes to, as tower@12,30, not medium1");
        assert_eq!(args("--move medium1@4").unwrap_err(), "--move: a room, then @ and the cell its low corner goes to, as tower@12,30, not medium1@4");
        assert_eq!(args("--move @4,5").unwrap_err(), "--move: a room, then @ and the cell its low corner goes to, as tower@12,30, not @4,5");
        assert_eq!(
            args("--rect medium1@4,5,6").unwrap_err(),
            "--rect: a room, then @ and its box, low corner then high, as tower@12,30,20,38, not medium1@4,5,6"
        );
        assert_eq!(
            args("--rect medium1@4,5,six,7").unwrap_err(),
            "--rect: a room, then @ and its box, low corner then high, as tower@12,30,20,38, not medium1@4,5,six,7"
        );
        assert_eq!(args("--pin tower").unwrap_err(), "--pin: a room's key and a floor, as tower@276, not tower");
        assert_eq!(
            args("--pin tower@100").unwrap_err(),
            "--pin: a room, then @ and a floor (-276 0 276 551 827 1102 1378 1654 1929 2205 2480 2756 3031 3307 3583 3858 4134), not 100"
        );
        assert_eq!(args("--add small@2,3,6,7@0").unwrap_err(), "--add: a kind, a box, a floor and a top, as medium@3,4,9,8@0@236, not small@2,3,6,7@0");
        assert_eq!(args("--add @2,3,6,7@0@236").unwrap_err(), "--add: a kind, a box, a floor and a top, as medium@3,4,9,8@0@236, not @2,3,6,7@0@236");
        assert_eq!(
            args("--add small@2,3,6,7@10@236").unwrap_err(),
            "--add: its floor (-276 0 276 551 827 1102 1378 1654 1929 2205 2480 2756 3031 3307 3583 3858 4134), not 10"
        );
        assert_eq!(
            args("--add small@2,3,6,7@276@300").unwrap_err(),
            "--add: its top, a ceiling over its floor (512 787 1063 1338 1614 1890 2165 2441 2716 2992 3267 3543 3819 4094 4370), not 300"
        );
        assert_eq!(
            args("--add small@2,3,6,7@276@236").unwrap_err(),
            "--add: its top, a ceiling over its floor (512 787 1063 1338 1614 1890 2165 2441 2716 2992 3267 3543 3819 4094 4370), not 236"
        );
        assert_eq!(args("--try --rooms").unwrap_err(), "--try: of what? Name a change with --move, --rect, --pin, --delete or --add");
        assert_eq!(args("--delete").unwrap_err(), "--delete needs a value");
        assert_eq!(args("--select").unwrap_err(), "--select needs a value");
    }

    #[test]
    fn a_rooms_choices_to_make_take_back_and_print() {
        let a = args("--seed 40 --layout colossal@ring --cover vast@none --inside vast@2 --brightness vast@90 --ceiling vast@grille --own tower@inside --own vast@all --choices vast --choices colossal")
            .unwrap();
        let set = |e: Edit| Choose::Set(e);
        assert_eq!(
            a.choose,
            [
                set(Edit::Layout { room: "colossal".into(), layout: "ring".into() }),
                set(Edit::Cover { room: "vast".into(), cover: "none".into() }),
                set(Edit::Reroll { room: "vast".into(), n: 2, part: None }),
                set(Edit::Brightness { room: "vast".into(), value: 90 }),
                set(Edit::Ceiling { room: "vast".into(), ceiling: "grille".into() }),
                Choose::Own { room: "tower".into(), what: "reroll".into() },
                Choose::Own { room: "vast".into(), what: "all".into() },
            ]
        );
        assert_eq!(a.choices, ["vast", "colossal"]);
        assert!(a.changes() && a.headless() && !a.draws_rooms() && a.recipe.options.cover == "mixed");
        // --cover with no room is the level's option, as it was; with one, the room's choice
        let a = args("--cover pillars --cover vast@crates").unwrap();
        assert_eq!((a.recipe.options.cover, a.choose.as_slice()), ("pillars", &[set(Edit::Cover { room: "vast".into(), cover: "crates".into() })][..]));
        // a list alone needs no window and changes nothing; none of it is asked for by default
        let a = args("--choices vast").unwrap();
        assert!(a.headless() && !a.changes() && a.choose.is_empty());
        assert!(args("--seed 40").unwrap().choices.is_empty() && !args("--seed 40").unwrap().changes());
        // a value that is none of those it may be is read here and refused when it is made, in
        // the generator's words; what is no room and value at all is an error
        assert_eq!(args("--layout vast@maze").unwrap().choose, [set(Edit::Layout { room: "vast".into(), layout: "maze".into() })]);
        assert_eq!(args("--brightness vast@300").unwrap().choose, [set(Edit::Brightness { room: "vast".into(), value: 300 })]);
        assert_eq!(args("--layout ring").unwrap_err(), "--layout: a room, then @ and its layout (ring galleries mezzanine none), as vast@ring, not ring");
        assert_eq!(args("--cover @none").unwrap_err(), "--cover: a room, then @ and its cover (mixed pillars crates none), as vast@pillars, not @none");
        assert_eq!(
            args("--ceiling vast@").unwrap_err(),
            "--ceiling: a room, then @ and its ceiling (plain ceiling_tile ceiling_dark grille), as vast@grille, not vast@"
        );
        assert_eq!(
            args("--inside vast@two").unwrap_err(),
            "--inside: a room, then @ and which inside, a whole number (0: the seed's own), as vast@2, not vast@two"
        );
        assert_eq!(
            args("--inside vast@-1").unwrap_err(),
            "--inside: a room, then @ and which inside, a whole number (0: the seed's own), as vast@2, not vast@-1"
        );
        assert_eq!(args("--brightness vast@dim").unwrap_err(), "--brightness: a room, then @ and a number from 0 to 255, as vast@120, not vast@dim");
        assert_eq!(args("--brightness 90").unwrap_err(), "--brightness: a room, then @ and a number from 0 to 255, as vast@120, not 90");
        let own = "--own: a room, then @ and what is the seed's own again (layout cover inside brightness ceiling, a feature, or all), as vast@layout";
        assert_eq!(args("--own vast@doors").unwrap_err(), format!("{own}, not vast@doors"));
        assert_eq!(args("--own vast").unwrap_err(), format!("{own}, not vast"));
        assert_eq!(args("--choices").unwrap_err(), "--choices needs a value");
        assert_eq!(args("--cover").unwrap_err(), "--cover needs a value");
    }

    #[test]
    fn what_it_cannot_read_it_says() {
        assert_eq!(args("--sead 4").unwrap_err(), "--sead: not an option (see --help)");
        assert_eq!(args("plan.png").unwrap_err(), "plan.png: not an option (see --help)");
        assert_eq!(args("--seed").unwrap_err(), "--seed needs a value");
        assert_eq!(args("--seed forty").unwrap_err(), "--seed: a whole number, not forty");
        assert_eq!(
            args("--storey 100").unwrap_err(),
            "--storey: all, or a floor (-276 0 276 551 827 1102 1378 1654 1929 2205 2480 2756 3031 3307 3583 3858 4134), not 100"
        );
        assert_eq!(args("--cover hedges").unwrap_err(), "cover: one of ('mixed', 'pillars', 'crates', 'none')");
    }
}
