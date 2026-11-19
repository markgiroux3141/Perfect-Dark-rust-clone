//! The climbs of towering levels (`autolevel::climbs`): for each seed, every pod on a towering
//! hall's high storey (and every gallery) and the way up to it from the hall's floor.
//! `cargo run --release -p autolevel --example climbs -- 1 40 [GALLERIES]`

use autolevel::climbs::climbs;
use autolevel::level::{generate, Options};
use autolevel::place::Scale;

fn main() {
    let arg = |n: usize, or: u64| std::env::args().nth(n).and_then(|s| s.parse().ok()).unwrap_or(or);
    let (lo, hi) = (arg(1, 1), arg(2, 40));
    let mut o = Options::default();
    o.siting.scale = Scale::Towering;
    o.galleries = arg(3, 0) as usize;
    for seed in lo..=hi {
        match generate(seed, &o) {
            Ok(lv) => {
                let cs = climbs(&lv);
                let pods = lv.spaces().iter().filter(|s| !s.host.is_empty()).count();
                let high = lv.spaces().iter().filter(|s| !s.high.is_empty()).map(|s| format!("{} {:?}", s.key, s.high)).collect::<Vec<_>>();
                println!("seed {seed}: {pods} pods, high {high:?}");
                for c in cs {
                    let doors =
                        if c.gallery { format!(" ({} doors)", lv.net.doors(&c.hall).iter().filter(|d| d.level == c.storey).count()) } else { String::new() };
                    println!("    {} on {} in {}{doors}: {} rooms out: {}", c.pod, c.storey, c.hall, c.rooms(), c.way.join(" > "));
                }
            }
            Err(why) => println!("seed {seed}: {why}"),
        }
    }
}
