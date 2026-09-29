//! Making the match's chrs: `chr_init`'s defaults (`chr.c:1120`), a human
//! player's chr, and the simulants in PD's allocation order (`setup.c:1961`:
//! a random walk over the slots, `botmgr_allocate_bot`, `botmgr.c:30`).

use glam::Vec3;
use pd_core::anim::Anim;
use pd_core::ids::*;
use pd_core::model::{Bodies, Model, ModelStore};
use pd_core::mp::{MatchChr, MatchSetup, MAX_SIMULANTS};
use pd_core::rng::Rng;

use super::{body, Act, Chr, GoPos};
use crate::bot::{Aibot, BotConfig};

/// `VOICEBOX_FEMALE` (`constants.h:4527`).
pub const VOICEBOX_FEMALE: u8 = 3;

/// `mp_get_body_id` / `mp_get_head_id` (`mplayer.c`): the `BODY_*` and `HEAD_*`
/// of an MP config (an out-of-range head falls back to the last MP head).
pub fn mp_chr_body_head(bodies: &Bodies, chr: &MatchChr) -> (usize, Option<usize>) {
    let bodynum = bodies.mpbodies.get(chr.mpbodynum as usize).map_or(0, |b| b.bodynum.max(0) as usize);
    let headnum = bodies.mpheads.get((chr.mpheadnum as usize).min(bodies.mpheads.len().saturating_sub(1))).map(|h| h.headnum).filter(|&h| h > 0).map(|h| h as usize);
    (bodynum, headnum)
}

impl Chr {
    /// `chr_init` (`chr.c:1120`) for a chr with this body: standing at the
    /// origin, `radius` 20, `height` 185, `maxdamage` 4 (a simulant's AI list
    /// makes it 8).
    #[allow(clippy::too_many_arguments)]
    pub fn new(chrnum: usize, player: Option<usize>, name: String, bodynum: usize, headnum: Option<usize>, model: Model, animscale: f32, bodyheight: f32, ismale: bool) -> Chr {
        Chr {
            chrnum,
            player,
            name,
            bodynum,
            headnum,
            ismale,
            voicebox: 0,
            bodyheight,
            model,
            anim: Anim { animscale, ..Anim::default() },
            held: [None, None],
            pos: Vec3::ZERO,
            prevpos: Vec3::ZERO,
            rooms: Vec::new(),
            manground: 0.0,
            ground: 0.0,
            sumground: 0.0,
            fallspeed: Vec3::ZERO,
            radius: 20.0,
            height: 185.0,
            floorroom: None,
            floortype: FLOORTYPE_DEFAULT,
            inlift: false,
            lift: None,
            liftaction: 0,
            onladder: false,
            invalidmove: 0,
            lastmoveok60: 0,
            forcetoground: false,
            actiontype: if player.is_some() { Act::BondMulti } else { Act::Stand },
            act_gopos: GoPos::default(),
            fadetimer60: -1,
            fadealpha: -1.0,
            damage: 0.0,
            maxdamage: 4.0,
            flinchcnt: -1,
            flinchtype: 0,
            headshotted: false,
            autoanim: false,
            aimendlshoulder: 0.0,
            aimendrshoulder: 0.0,
            aimendback: 0.0,
            aimendsideback: 0.0,
            aimuplshoulder: 0.0,
            aimuprshoulder: 0.0,
            aimupback: 0.0,
            aimsideback: 0.0,
            aimendcount: 0,
            hand_firing: [false; 2],
            firecount: [0; 2],
            unk32c_12: 0,
            fireslots: Default::default(),
            firesounddone: false,
            target: None,
            cloak: Default::default(),
            drugheadcount: 0,
            drugheadsway: 0.0,
            oldframe: 0.0,
            lastfootsample: 0,
            onscreen: false,
            lastshooter: None,
            timeshooter: 0,
            aibot: None,
            playertheta: 0.0,
            eyeheight: 0.0,
            blurdrugamount: 0,
            blurnumtimesdied: 0,
            sleep: 0,
            timeextra: 0.0,
            elapseextra: 0.0,
            extraspeed: Vec3::ZERO,
            footstep: 0,
            magicanim: None,
            magicframe: 0.0,
            magicspeed: 0.0,
            onanyscreen: false,
            onanyscreenprev: false,
            team: 1,
            mpslot: 0,
            cshield: 0.0,
            shielddamaged: false,
        }
    }
}

/// A human player's chr (`playermgr`'s `body_instantiate_model_to_addr` with
/// `isplayer`, no height variation): its perimeter is the player's (radius 30).
pub fn player_chr(store: &ModelStore, bodies: &Bodies, playernum: usize, chr: &MatchChr) -> Result<Chr, String> {
    let (bodynum, headnum) = mp_chr_body_head(bodies, chr);
    let (model, animscale, height, ismale) = body::chr_body(store, bodies, bodynum, headnum, None)?;
    let mut c = Chr::new(playernum, Some(playernum), chr.name.clone(), bodynum, headnum, model, animscale, height, ismale);
    c.radius = 30.0;
    c.team = 1 << (chr.team & 7);
    c.voicebox = if ismale { 0 } else { VOICEBOX_FEMALE };
    Ok(c)
}

/// The simulants in PD's allocation order (`setup.c:1961`): `maxsimulants`
/// slots walked from `random() % maxsimulants`, skipping ones done, and each
/// configured slot allocated (`botmgr_allocate_bot`: the body, with its
/// `RANDOMFRAC` height; `voicebox = random() % 3`, female 3; `random1`,
/// `random2`, `randomfrac`). `nchrs` is the match's chr count (players +
/// simulants). Returns the chrs with their setup index.
///
/// `// SUBST:` PD walks 4 slots until `MPFEATURE_8BOTS` is unlocked / the
/// menus unlock everything, so always 8.
pub fn botmgr_allocate_bots(store: &ModelStore, bodies: &Bodies, setup: &MatchSetup, nchrs: usize, rng: &mut Rng) -> Result<Vec<(usize, Chr)>, String> {
    let maxsimulants = MAX_SIMULANTS;
    let mut slotsdone = [false; MAX_SIMULANTS];
    let mut out = Vec::new();
    for _ in 0..maxsimulants {
        let mut slotnum = (rng.random() % maxsimulants as u32) as usize;
        while slotsdone[slotnum] {
            slotnum = (slotnum + 1) % maxsimulants;
        }
        if let Some(si) = setup.simulants.iter().position(|s| s.slot as usize == slotnum + 4) {
            let chrnum = out.len();
            let sim = &setup.simulants[si];
            let (bodynum, headnum) = mp_chr_body_head(bodies, &sim.chr);
            let (model, animscale, height, ismale) = body::chr_body(store, bodies, bodynum, headnum, Some(rng))?;
            let mut c = Chr::new(chrnum, None, sim.chr.name.clone(), bodynum, headnum, model, animscale, height, ismale);
            c.team = 1 << (sim.chr.team & 7);
            c.voicebox = (rng.random() % 3) as u8;
            if !ismale {
                c.voicebox = VOICEBOX_FEMALE;
            }
            let random1 = rng.random();
            let mut a = Aibot::new(BotConfig { difficulty: sim.difficulty, bottype: sim.bottype }, si, nchrs, random1);
            a.random2 = rng.random();
            a.randomfrac = rng.randomfrac();
            c.aibot = Some(Box::new(a));
            // GAILIST_AIBOT_INIT: set_chr_maxdamage(CHR_SELF, 80).
            c.maxdamage = 8.0;
            out.push((si, c));
        }
        slotsdone[slotnum] = true;
    }
    Ok(out)
}
