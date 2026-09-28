//! `pd_snapshot <outdir> menu [--scale <n>] [--fresh] [--combat] <steps...>`: the
//! menus under a scripted controller, one PNG per `shot:<name>` (the RGBA5551
//! framebuffer, scaled `n`× nearest-neighbour, default 3). The script format is
//! `pd_menu::script`'s, the old `pd_combat_sim_snapshot`'s. For example, the
//! character select:
//!
//! ```text
//! pd_snapshot out menu --combat w40 down down down a w50 right w30 down a w80 shot:char
//! ```

use std::path::{Path, PathBuf};

use pd_menu::script::Script;

pub fn run(outdir: &Path, args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut scale = 3usize;
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--scale" {
            scale = it.next().ok_or("--scale needs a value")?.parse::<usize>().map_err(|e| format!("--scale: {e}"))?.clamp(1, 8);
        } else {
            words.push(a.clone());
        }
    }
    let script = Script::parse(&words)?;
    if !script.steps.iter().any(|s| matches!(s, pd_menu::script::Step::Shot(_))) {
        return Err("menu: the script takes no shot (add shot:<name>)".into());
    }
    let mut pd = script.start(&crate::assets())?;
    let mut paths = Vec::new();
    script.run(&mut pd, |name, pd| {
        let (w, h) = (pd.draw.gfx.w, pd.draw.gfx.h);
        let rgba = pd.draw.gfx.rgba8(true);
        let mut big = vec![0u8; w * scale * h * scale * 4];
        for y in 0..h * scale {
            for x in 0..w * scale {
                let s = ((y / scale) * w + x / scale) * 4;
                let d = (y * w * scale + x) * 4;
                big[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
            }
        }
        let path = outdir.join(format!("{name}.png"));
        crate::write_png(&path, w * scale, h * scale, &big)?;
        paths.push(path);
        Ok(())
    })?;
    if let Some(e) = &pd.draw.error {
        eprintln!("pd_snapshot menu: {e}");
    }
    Ok(paths)
}
