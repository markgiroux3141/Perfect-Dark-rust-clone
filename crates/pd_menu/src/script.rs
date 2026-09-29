//! Headless runs of the menus with a scripted controller: the way to look at
//! the menus without a window (`pd_snapshot <outdir> menu <script...>`) and the
//! way the golden images replay (`tests/golden.rs`).
//!
//! A script is whitespace-separated words, the old repo's
//! `pd_combat_sim_snapshot` format:
//!
//! * `--fresh`: the save files decide, on a blank Game Pak (a new file:
//!   challenges and unlockables locked); else every challenge counts as done
//!   ([`Profile::Complete`]);
//! * `--combat`: start in the Combat Simulator; `--boot`: at power on's agent
//!   select; else on the Perfect Menu;
//! * `w<N>`: run N frames with nothing held (`w30`);
//! * `a` `b` `z` `start` `up` `down` `left` `right` `l` `r` `cu` `cd` `cl` `cr`:
//!   tap that button on controller 1 (held one frame, released the next, then
//!   6 more frames);
//! * `p2start` `p3start` `p4start`: plug in controller 2-4 and tap START on it;
//! * `sx<V>` / `sy<V>`: hold controller 1's stick at V (−80..80) until changed;
//! * `bs`: one frame of the name keyboard's delete;
//! * `shot:<name>`: hand the current frame to the caller.
//!
//! Every frame is one 60 Hz tick (`diffframe60` 1).

use n64::pad::*;
use pd_core::assets::AssetDir;
use pd_core::lv::Lv;

use super::mpstate::Profile;
use super::MenuSystem;

/// One step of a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Wait(u32),
    Tap { pad: usize, button: u16 },
    Back2,
    StickX(i8),
    StickY(i8),
    Shot(String),
}

/// A parsed script: where it starts and what it does.
#[derive(Clone, Debug)]
pub struct Script {
    pub profile: Profile,
    pub combat: bool,
    pub boot: bool,
    pub steps: Vec<Step>,
}

fn button(word: &str) -> Option<(usize, u16)> {
    Some(match word {
        "a" => (0, A_BUTTON),
        "b" => (0, B_BUTTON),
        "z" => (0, Z_TRIG),
        "start" => (0, START_BUTTON),
        "up" => (0, U_JPAD),
        "down" => (0, D_JPAD),
        "left" => (0, L_JPAD),
        "right" => (0, R_JPAD),
        "l" => (0, L_TRIG),
        "r" => (0, R_TRIG),
        "cu" => (0, U_CBUTTONS),
        "cd" => (0, D_CBUTTONS),
        "cl" => (0, L_CBUTTONS),
        "cr" => (0, R_CBUTTONS),
        "p2start" => (1, START_BUTTON),
        "p3start" => (2, START_BUTTON),
        "p4start" => (3, START_BUTTON),
        _ => return None,
    })
}

impl Script {
    pub fn parse<S: AsRef<str>>(words: &[S]) -> Result<Script, String> {
        let mut s = Script { profile: Profile::Complete, combat: false, boot: false, steps: Vec::new() };
        for w in words {
            let w = w.as_ref();
            let stick = |v: &str| v.parse::<i8>().map_err(|_| format!("bad stick {w}"));
            let step = if w == "--fresh" {
                s.profile = Profile::Files;
                continue;
            } else if w == "--combat" {
                s.combat = true;
                continue;
            } else if w == "--boot" {
                s.boot = true;
                continue;
            } else if let Some((pad, button)) = button(w) {
                Step::Tap { pad, button }
            } else if w == "bs" {
                Step::Back2
            } else if let Some(name) = w.strip_prefix("shot:") {
                Step::Shot(name.to_string())
            } else if let Some(v) = w.strip_prefix("sx") {
                Step::StickX(stick(v)?)
            } else if let Some(v) = w.strip_prefix("sy") {
                Step::StickY(stick(v)?)
            } else if let Some(n) = w.strip_prefix('w') {
                Step::Wait(n.parse().map_err(|_| format!("bad wait {w}"))?)
            } else {
                return Err(format!("unknown step {w}"));
            };
            s.steps.push(step);
        }
        Ok(s)
    }

    /// A fresh menu system, opened where the script starts.
    pub fn start(&self, assets: &AssetDir) -> Result<MenuSystem, String> {
        let mut pd = MenuSystem::new(assets, self.profile)?;
        if self.boot {
            pd.open_file_select();
        } else if self.combat {
            pd.open_combat_simulator();
        } else {
            pd.open_main_menu();
        }
        Ok(pd)
    }

    /// Run the steps on `pd`, calling `shot(name, pd)` at every `shot:`.
    pub fn run(&self, pd: &mut MenuSystem, mut shot: impl FnMut(&str, &MenuSystem) -> Result<(), String>) -> Result<(), String> {
        let mut c = Controller::default();
        for step in &self.steps {
            match step {
                Step::Wait(n) => {
                    for _ in 0..*n {
                        c.frame(pd);
                    }
                }
                Step::Tap { pad, button } => {
                    pd.pads[*pad].connected = true;
                    c.held[*pad] |= button;
                    c.frame(pd);
                    c.held[*pad] &= !button;
                    for _ in 0..6 {
                        c.frame(pd);
                    }
                }
                Step::Back2 => {
                    c.back2 = true;
                    c.frame(pd);
                }
                Step::StickX(v) => c.stick.0 = *v,
                Step::StickY(v) => c.stick.1 = *v,
                Step::Shot(name) => shot(name, pd)?,
            }
        }
        Ok(())
    }
}

/// The scripted controllers and the frame clock.
#[derive(Default)]
struct Controller {
    held: [u16; MAX_PADS],
    stick: (i8, i8),
    back2: bool,
    lv: Lv,
}

impl Controller {
    fn frame(&mut self, pd: &mut MenuSystem) {
        for (i, pad) in pd.pads.iter_mut().enumerate() {
            let (sx, sy) = if i == 0 { self.stick } else { (0, 0) };
            pad.next_frame(self.held[i], sx, sy);
        }
        pd.back2[0] = std::mem::take(&mut self.back2);
        self.lv.frametime_apply(1, 4);
        pd.frame(&self.lv);
    }
}
