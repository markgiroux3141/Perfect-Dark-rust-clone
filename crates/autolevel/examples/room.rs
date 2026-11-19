//! One room of a level, storey by storey: its catwalk, its ramps, its doors, and whether each
//! storey's catwalk can be walked to from a door (`Interior::nav`). Which room: by key, or the one
//! over a plan point in cells.
//! `cargo run --release -p autolevel --example room -- RECIPE.json (KEY | X,Y)`

use autolevel::furnish::Interior;
use std::collections::{HashSet, VecDeque};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).expect("a recipe file");
    let lv = autolevel::recipe::load(&text).expect("a recipe").generate().expect("a level");
    if args[2] == "orphans" {
        // every room's storeys with catwalk nobody can walk to from a door
        for (key, it) in &lv.interiors {
            let g = it.nav();
            let mut seen: HashSet<(i32, i32, i32)> = HashSet::new();
            let mut todo: VecDeque<(i32, i32, i32)> = it.door_nodes().into_iter().flatten().collect();
            seen.extend(todo.iter().copied());
            while let Some(n) = todo.pop_front() {
                for &m in g.get(&n).into_iter().flatten() {
                    if seen.insert(m) {
                        todo.push_back(m);
                    }
                }
            }
            for &z in &it.upper {
                // (catwalk only: a pod's roof beside it is walkable too, but may not be reached)
                let deck = &it.deck[&z];
                let blocks: Vec<_> =
                    it.blocks(z).into_iter().filter(|c| [(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|(dx, dy)| deck.contains(&(c.0 + dx, c.1 + dy)))).collect();
                let lost = blocks.iter().filter(|c| !seen.contains(&(c.0, c.1, z))).count();
                if lost > 0 {
                    println!("{key}@{z}: {lost} of {} catwalk blocks not reached ({} {:?})", blocks.len(), it.s.kind, it.layout);
                }
            }
        }
        return;
    }
    if args[2] == "list" {
        for s in lv.spaces().iter().filter(|s| s.host.is_empty() && s.storeys().len() >= 3) {
            println!("{} {} cells ({},{})..({},{}) floor {} top {}", s.key, s.kind, s.x0, s.y0, s.x1, s.y1, s.floor, s.top);
        }
        return;
    }
    let key = match args[2].split_once(',') {
        Some((x, y)) => {
            let (x, y): (f64, f64) = (x.parse().unwrap(), y.parse().unwrap());
            let s = lv
                .spaces()
                .iter()
                .filter(|s| s.host.is_empty() && (s.x0 as f64) <= x && x < s.x1 as f64 && (s.y0 as f64) <= y && y < s.y1 as f64)
                .max_by_key(|s| s.top - s.floor);
            s.expect("a room there").key.clone()
        }
        None => args[2].clone(),
    };
    let s = lv.space(&key);
    let it: &Interior = lv.interior(&key);
    println!(
        "{key}: {} {}x{} cells, floor {} top {}, layout {:?}, tower {:?}, high {:?}, galleries {:?}, door levels {:?}",
        s.kind,
        s.x1 - s.x0,
        s.y1 - s.y0,
        s.floor,
        s.top,
        it.layout,
        it.tower,
        s.high,
        s.galleries,
        s.door_levels()
    );
    if !it.shapes.is_empty() {
        println!("  shapes (attempt {}): {:?}", it.attempt, it.shapes);
    }
    // walk the nav graph from every door
    let g = it.nav();
    let mut seen: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut todo: VecDeque<(i32, i32, i32)> = it.door_nodes().into_iter().flatten().collect();
    seen.extend(todo.iter().copied());
    while let Some(n) = todo.pop_front() {
        for &m in g.get(&n).into_iter().flatten() {
            if seen.insert(m) {
                todo.push_back(m);
            }
        }
    }
    for &z in it.levels.iter().rev() {
        let blocks = it.blocks(z);
        let reached = blocks.iter().filter(|c| seen.contains(&(c.0, c.1, z))).count();
        let doors = it.doors.iter().filter(|d| d.level == z).count();
        let up: Vec<String> = it.ramps.iter().filter(|r| r.lo == z).map(|r| format!("->{}", r.hi)).collect();
        let deck = if z == it.levels[0] { "floor".to_string() } else { format!("{} deck cells", it.deck[&z].len() / 4) };
        println!(
            "  {z:>5}: {deck:<16} {:>4} blocks, {:>4} reached from a door, {doors} doors, ramps up {:?}, band {}",
            blocks.len(),
            reached,
            up,
            if it.band_of(z) == i32::MIN { "room".to_string() } else { z.to_string() }
        );
    }
}
