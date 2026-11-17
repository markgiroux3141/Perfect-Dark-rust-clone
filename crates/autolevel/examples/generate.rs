//! One level from a seed, and how long it took: `cargo run --release --example generate -- 40`.
//! (A stand-in until the command-line tool is ported.)

use autolevel::level::{generate, Options};

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(40);
    let start = std::time::Instant::now();
    match generate(seed, &Options::default()) {
        Ok(lv) => {
            let r = &lv.report;
            println!(
                "seed {seed}: {} rooms, {} in the plan, {} links, {} walks, score {}, in {:.2?}",
                r.rooms,
                r.plan_rooms,
                r.links,
                r.walks,
                r.score,
                start.elapsed()
            );
        }
        Err(why) => println!("{why} (in {:.2?})", start.elapsed()),
    }
}
