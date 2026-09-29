//! Offline renders of PD's music, through the game's own synth and queue
//! (`pd_core::music`), to 16-bit stereo WAVs at the N64's 22018 Hz.
//!
//! ```text
//! pd_music --list
//! pd_music <out.wav> <tune> [seconds]      a tune as the menus play one (MUSIC_* name or number)
//! pd_music <out.wav> --flow [seconds]      the Combat Simulator menu, a match on Dark Combat,
//!                                          a death, the end screens (pd_game::music's calls)
//! ```

use std::sync::Arc;

use pd_core::assets::AssetDir;
use pd_core::ids::*;
use pd_core::music::{Music, MusicCall, MusicData, MusicVolume};

const RATE: u32 = 22018;

fn write_wav(path: &str, frames: &[[i16; 2]]) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + frames.len() * 4);
    let data_len = (frames.len() * 4) as u32;
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(RATE.to_le_bytes());
    b.extend((RATE * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for f in frames {
        b.extend(f[0].to_le_bytes());
        b.extend(f[1].to_le_bytes());
    }
    std::fs::write(path, b)
}

fn stats(frames: &[[i16; 2]]) -> String {
    let n = frames.len().max(1) as f64;
    let peak = frames.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
    let clipped = frames.iter().filter(|f| f[0].unsigned_abs() >= 0x7fff || f[1].unsigned_abs() >= 0x7fff).count();
    let rms = |c: usize| (frames.iter().map(|f| (f[c] as f64).powi(2)).sum::<f64>() / n).sqrt();
    format!("{:.1} s, peak {peak} ({:.1} dBFS), rms L {:.0} R {:.0}, {clipped} clipped", n / RATE as f64, 20.0 * (peak.max(1) as f64 / 32768.0).log10(), rms(0), rms(1))
}

/// Frames at 60 Hz: each game frame applies its calls and ticks the queue
/// (`diffframe240` 4), and audio frames are made as 22018 Hz time passes.
struct Clock {
    music: Music,
    out: Vec<[i16; 2]>,
    due: f64,
}

impl Clock {
    fn frame(&mut self, calls: &[MusicCall]) {
        for &c in calls {
            self.music.apply(c);
        }
        self.music.apply(MusicCall::Tick { diffframe240: 4 });
        self.due += RATE as f64 / 60.0;
        while self.due >= 736.0 {
            self.music.render_frame(&mut self.out);
            self.due -= 736.0;
        }
    }

    fn run(&mut self, secs: f64) {
        for _ in 0..(secs * 60.0) as usize {
            self.frame(&[]);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assets = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
    let data = Arc::new(MusicData::load(&assets).unwrap_or_else(|e| panic!("music: {e}")));
    if args.first().map(String::as_str) == Some("--list") || args.len() < 2 {
        for (i, n) in data.names.iter().enumerate() {
            println!("{i:3} {n}");
        }
        if args.len() < 2 && args.first().map(String::as_str) != Some("--list") {
            eprintln!("usage: pd_music <out.wav> <tune|--flow> [seconds]");
        }
        return;
    }
    let out = &args[0];
    let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let mut clock = Clock { music: Music::new(data.clone()), out: Vec::new(), due: 0.0 };
    if args[1] == "--flow" {
        // The Combat Simulator menu (menutick.c:598), then lv_stop + lv_reset
        // for a match on Dark Combat, a death at 10 s (its timer ends 5 s
        // later), and the end screens (mp_end_match's music_start_menu).
        let seg = secs / 4.0;
        clock.frame(&[MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_MENU)]);
        clock.run(seg);
        clock.music.music_stop();
        clock.frame(&[MusicCall::Reset, MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_DARK_COMBAT, fadesecs: 0.0, volume: MusicVolume::Music }]);
        clock.run(seg);
        clock.frame(&[
            MusicCall::SaveInterval,
            MusicCall::Stop { tracktype: TRACKTYPE_MENU },
            MusicCall::Stop { tracktype: TRACKTYPE_DEATH },
            MusicCall::Stop { tracktype: TRACKTYPE_AMBIENT },
            MusicCall::Fade { tracktype: TRACKTYPE_PRIMARY, secs: 0.1, keep: true },
            MusicCall::SaveInterval,
            MusicCall::Start { tracktype: TRACKTYPE_DEATH, tracknum: MUSIC_DEATH_MP, fadesecs: 0.0, volume: MusicVolume::MaxSfxMusic },
            MusicCall::RestoreInterval,
            MusicCall::RestoreInterval,
        ]);
        clock.run(5.0);
        clock.frame(&[MusicCall::Fade { tracktype: TRACKTYPE_DEATH, secs: 2.0, keep: false }, MusicCall::Start { tracktype: TRACKTYPE_PRIMARY, tracknum: MUSIC_DARK_COMBAT, fadesecs: 2.0, volume: MusicVolume::Music }]);
        clock.run(seg);
        clock.frame(&[MusicCall::StartTrackAsMenu(MUSIC_COMBATSIM_COMPLETE)]);
        clock.run(seg);
    } else {
        let tune = args[1].parse::<i32>().ok().or_else(|| data.names.iter().position(|n| *n == args[1] || n.trim_start_matches("MUSIC_") == args[1]).map(|i| i as i32));
        let Some(tune) = tune else {
            eprintln!("no tune {} (pd_music --list)", args[1]);
            std::process::exit(2);
        };
        clock.frame(&[MusicCall::StartTrackAsMenu(tune)]);
        clock.run(secs);
    }
    write_wav(out, &clock.out).unwrap_or_else(|e| panic!("{out}: {e}"));
    let notes = clock.music.audio.players.iter().fold([0; 3], |a, p| [a[0] + p.notes[0], a[1] + p.notes[1], a[2] + p.notes[2]]);
    println!("{out}: {}, notes {} played, {} no sound, {} no voice, {} physical voices busy at the end", stats(&clock.out), notes[0], notes[1], notes[2], clock.music.audio.syn.voices_playing());
}
