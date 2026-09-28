//! The menus against the golden frames captured from the old repo's spike
//! (`tests/golden/`, see `capture_from_spike.py`): every `shot:` of every
//! script in `scripts.txt` must match its PNG pixel for pixel, in the
//! RGBA5551-quantised output the spike wrote.
//!
//! On a mismatch the frame and a diff (red where they differ) go to
//! `target/golden-diff/`, and the test lists every shot that differed.

use std::path::{Path, PathBuf};

use pd_core::assets::AssetDir;
use pd_menu::script::Script;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

fn runs() -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(golden_dir().join("scripts.txt")).unwrap();
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| l.split_whitespace().map(String::from).collect()).collect()
}

#[test]
fn menus_match_the_spike_goldens() {
    let assets = AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR"));
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("golden-diff");
    let mut failures = Vec::new();
    let mut shots = 0;
    for words in runs() {
        let script = Script::parse(&words).unwrap();
        let mut pd = script.start(&assets).unwrap();
        script
            .run(&mut pd, |name, pd| {
                shots += 1;
                let got = pd.draw.gfx.rgba8(true);
                let (w, h) = (pd.draw.gfx.w, pd.draw.gfx.h);
                let want = image::open(golden_dir().join(format!("{name}.png"))).map_err(|e| format!("{name}: {e}"))?.to_rgba8();
                assert_eq!((want.width() as usize, want.height() as usize), (w, h), "{name}: size");
                let want = want.into_raw();
                let mut diff = vec![0u8; w * h * 4];
                let (mut n, mut worst) = (0usize, 0u8);
                for i in 0..w * h {
                    let d = (0..3).map(|c| got[i * 4 + c].abs_diff(want[i * 4 + c])).max().unwrap();
                    let o = &mut diff[i * 4..i * 4 + 4];
                    if d > 0 {
                        n += 1;
                        worst = worst.max(d);
                        o.copy_from_slice(&[255, 0, 0, 255]);
                    } else {
                        let g = got[i * 4] / 4 + got[i * 4 + 1] / 4 + got[i * 4 + 2] / 4;
                        o.copy_from_slice(&[g, g, g, 255]);
                    }
                }
                if n > 0 {
                    std::fs::create_dir_all(&out).unwrap();
                    image::save_buffer(out.join(format!("{name}.png")), &got, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
                    image::save_buffer(out.join(format!("{name}_diff.png")), &diff, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
                    failures.push(format!("{name}: {n} pixels differ (max {worst})"));
                }
                Ok(())
            })
            .unwrap();
    }
    assert!(shots >= 25, "only {shots} shots ran");
    assert!(failures.is_empty(), "{} of {shots} frames differ from the spike (see target/golden-diff/):\n{}", failures.len(), failures.join("\n"));
}
