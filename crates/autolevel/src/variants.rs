//! Re-rolls side by side: the editing roadmap's item 5 (docs/AUTOLEVEL.md, "Re-rolls side by
//! side"). New here; the Python has none.
//!
//! A room's whole inside, or one part of it (`level::PARTS`: catwalks, ramps, cover, windows), is
//! drawn from another stream of random numbers by a `reroll` edit. `variants` is the recipes of
//! the next few streams after the one the room has, each with that edit made as a choice
//! (`choices::set`: the room's one edit of its kind), and `made` makes their levels at once, on
//! threads, so an editor can show a strip of small pictures and keep one with a click. `part_of`
//! is a part as text, so a re-roll that came out the same as another can be told.

use crate::choices;
use crate::level::{Level, PARTS};
use crate::recipe::{Edit, Recipe};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// How many re-rolls a strip shows at once.
pub const SHOWN: usize = 5;

/// One re-roll: which stream, the edit, and the recipe with it made.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub n: u32,
    pub edit: Edit,
    pub recipe: Recipe,
}

/// The edit that draws `room`'s part (None: its whole inside) from stream `n`.
pub fn edit(room: &str, part: Option<&str>, n: u32) -> Edit {
    Edit::Reroll { room: room.to_string(), n, part: part.map(str::to_string) }
}

/// Which stream `room`'s part (None: its whole inside) is drawn from in `recipe`: 0 is the seed's.
pub fn now(recipe: &Recipe, room: &str, part: Option<&str>) -> u32 {
    let c = choices::chosen(recipe, room);
    match part {
        None => c.reroll.unwrap_or(0),
        Some(p) => c.parts.iter().find(|(q, _)| *q == p).map_or(0, |(_, n)| *n),
    }
}

/// The recipes of `count` re-rolls of `room`'s part (None: its whole inside), streams `from`,
/// `from + 1`, ..., skipping the one it has now. Err for a part that is none of `PARTS`, or a
/// re-roll that can't be told to be wrong without a level (`choices::refusal`); `lv`, the
/// recipe's level, also refuses a pod and a room it does not have.
pub fn variants(recipe: &Recipe, lv: Option<&Level>, room: &str, part: Option<&str>, from: u32, count: usize) -> Result<Vec<Variant>, String> {
    if let Some(p) = part.filter(|p| !PARTS.contains(p)) {
        return Err(format!("reroll: a part is one of {}, not {p:?}", PARTS.join(", ")));
    }
    let had = now(recipe, room, part);
    let mut out = Vec::new();
    for n in (from.max(1)..).filter(|n| *n != had).take(count) {
        let e = edit(room, part, n);
        if let Some(why) = choices::refusal(lv, &e) {
            return Err(why);
        }
        out.push(Variant { n, recipe: choices::set(recipe, e.clone())?, edit: e });
    }
    Ok(out)
}

/// Each variant's level, or why it makes none: made side by side on threads.
pub fn made(variants: &[Variant]) -> Vec<Result<Level, String>> {
    side_by_side(variants, |v| catch_unwind(AssertUnwindSafe(|| v.recipe.generate())).unwrap_or_else(|_| Err("the generator stopped on it".to_string())))
}

/// `f` of each item, in order, worked out on as many threads as there are cores (or items).
pub fn side_by_side<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let n = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(n) else { break };
                let r = f(item);
                out.lock().expect("no worker stops with the list held")[n] = Some(r);
            });
        }
    });
    out.into_inner().expect("the workers are done").into_iter().map(|r| r.expect("every one was worked out")).collect()
}

/// `room`'s part (None: its whole inside) in `lv` as text: two re-rolls that read the same came
/// out the same. Empty for a room `lv` does not have.
pub fn part_of(lv: &Level, room: &str, part: Option<&str>) -> String {
    let Some(it) = lv.interiors.iter().find(|(k, _)| k == room).map(|(_, it)| it) else { return String::new() };
    match part {
        Some("catwalks") => format!("{:?}", (&it.deck, &it.bridge, it.layout, it.balcony, &it.shapes)),
        Some("ramps") => format!("{:?}", it.ramps),
        Some("cover") => format!("{:?}", (&it.cover, it.shaped)),
        Some("windows") => format!("{:?}", lv.windows.iter().filter(|w| w.hall == room).collect::<Vec<_>>()),
        _ => format!("{:?}", (&it.deck, &it.ramps, &it.cover, it.layout, it.shaped, &it.rails)),
    }
}

/// What a re-roll is called on a strip: "catwalks 3", "inside 2"; 0 is "the seed's own".
pub fn label(part: Option<&str>, n: u32) -> String {
    match n {
        0 => "the seed's own".to_string(),
        n => format!("{} {n}", part.unwrap_or("inside")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_streams_are_offered_and_made_side_by_side() {
        let r = Recipe::new(40);
        let lv = r.generate().unwrap();
        // the streams after the one it has, that one skipped
        let v = variants(&r, Some(&lv), "vast", Some("ramps"), 1, 3).unwrap();
        assert_eq!(v.iter().map(|v| v.n).collect::<Vec<_>>(), [1, 2, 3]);
        let had = choices::set(&r, edit("vast", Some("ramps"), 2)).unwrap();
        assert_eq!(now(&had, "vast", Some("ramps")), 2);
        assert_eq!(now(&had, "vast", None), 0);
        let v = variants(&had, Some(&lv), "vast", Some("ramps"), 1, 3).unwrap();
        assert_eq!(v.iter().map(|v| v.n).collect::<Vec<_>>(), [1, 3, 4]);
        // each is the room's one edit of its kind
        assert!(v.iter().all(|v| v.recipe.edits == [v.edit.clone()]));
        // made at once, each the level its recipe makes
        let made = made(&v);
        for (v, lv) in v.iter().zip(&made) {
            let one = v.recipe.generate().unwrap();
            assert_eq!(part_of(lv.as_ref().unwrap(), "vast", Some("ramps")), part_of(&one, "vast", Some("ramps")));
        }
        // the errors
        assert_eq!(
            variants(&r, Some(&lv), "vast", Some("pillars"), 1, 3).unwrap_err(),
            "reroll: a part is one of catwalks, ramps, cover, windows, not \"pillars\""
        );
        assert_eq!(variants(&r, Some(&lv), "small8", None, 1, 3).unwrap_err(), "reroll: small8 is a pod of vast: it is not furnished by itself");
        assert_eq!(variants(&r, Some(&lv), "attic", None, 1, 3).unwrap_err(), "reroll: no room attic");
        assert_eq!(
            (label(None, 2), label(Some("cover"), 1), label(Some("cover"), 0)),
            ("inside 2".to_string(), "cover 1".to_string(), "the seed's own".to_string())
        );
    }
}
