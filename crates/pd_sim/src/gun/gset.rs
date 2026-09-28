//! The weapon table (`g_Weapons`, `invitems.c`) and the gun scripts, typed, with
//! the `gset_*` accessors (`gset.c`) the hand code calls. One table for players
//! and simulants alike, read from `assets/data/weapons.json`.
//!
//! The export covers every weapon the Combat Simulator hands out (`g_MpWeapons`)
//! plus `invitem_unarmed`. Scripts are compiled once into [`GunCmd`] lists; a
//! script reference is a [`ScriptId`] and a running command position a
//! [`CmdPtr`], PD's `struct guncmd *`.
//!
//! Source: the old repo's `pd_guns/gset.rs` and the weapon half of `data.rs`.

use std::collections::HashMap;

use pd_core::assets::AssetDir;
use pd_core::ids::*;
use serde::Deserialize;
use serde_json::Value;

/// Index into [`Gset::scripts`].
pub type ScriptId = usize;
/// PD's `struct guncmd *`: a script and an index into it.
pub type CmdPtr = (ScriptId, usize);

/// `struct guncmd` (`gunscript.h`), decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum GunCmd {
    End,
    PlayAnimation { condition: u8, anim: u16, params: i32 },
    ShowPart { keyframe: i32, part: i32 },
    HidePart { keyframe: i32, part: i32 },
    WaitForZReleased { keyframe: i32 },
    AllowFeature { keyframe: i32, feature: i32 },
    PlaySound { keyframe: i32, sound: u16 },
    Include { condition: u8, target: ScriptId },
    Random { probability: i32, target: ScriptId },
    RepeatUntilFull { keyframe: i32, gotokeyframe: i32 },
    PopOutSackOfPills { keyframe: i32 },
    SetSoundSpeed { keyframe: i32, speed: i32 },
}

impl GunCmd {
    /// The `keyframe` field (the union's first u16), for the commands that have one.
    pub fn keyframe(&self) -> i32 {
        match *self {
            GunCmd::ShowPart { keyframe, .. }
            | GunCmd::HidePart { keyframe, .. }
            | GunCmd::WaitForZReleased { keyframe }
            | GunCmd::AllowFeature { keyframe, .. }
            | GunCmd::PlaySound { keyframe, .. }
            | GunCmd::RepeatUntilFull { keyframe, .. }
            | GunCmd::PopOutSackOfPills { keyframe }
            | GunCmd::SetSoundSpeed { keyframe, .. } => keyframe,
            _ => 0,
        }
    }
}

/// `struct recoilsettings`.
#[derive(Deserialize, Debug, Clone, Copy, Default)]
pub struct RecoilSettings {
    pub xrange: f32,
    pub yrange: f32,
    pub zrange: f32,
}

/// `struct noisesettings`.
#[derive(Deserialize, Debug, Clone, Copy, Default)]
pub struct NoiseSettings {
    pub minradius: f32,
    pub maxradius: f32,
    pub incradius: f32,
    pub decbasespeed: f32,
    pub decremspeed: f32,
}

/// `struct funcdef_shoot` beyond the base.
#[derive(Clone, Debug, Default)]
pub struct ShootDef {
    pub recoil: Option<RecoilSettings>,
    pub recoverytime60: i32,
    pub damage: f32,
    pub spread: f32,
    /// `unk24..unk27`: recoil rise ticks, recoil fall ticks, earliest refire
    /// tick, refire blend ticks (`bgun_tick_recoil`, `bondgun.c:1851`).
    pub unk24: i32,
    pub unk25: i32,
    pub unk26: i32,
    pub unk27: i32,
    pub recoildist: f32,
    pub recoilangle: f32,
    pub slidemax: f32,
    pub impactforce: f32,
    pub duration60: i32,
    pub shootsound: u16,
    pub penetration: i32,
    // funcdef_shootauto
    pub initialrpm: f32,
    pub maxrpm: f32,
    pub turretaccel: f32,
    pub turretdecel: f32,
}

/// The projectile half of `struct funcdef_throw` / `funcdef_shootprojectile`.
#[derive(Clone, Debug, Default)]
pub struct ProjDef {
    pub projectilemodelnum: i32,
    /// The model file `projectilemodelnum` names through `g_ModelStates`
    /// (`modeldata/general.c:404`), by stem.
    pub model: Option<String>,
    /// `g_ModelStates[projectilemodelnum].scale / 4096` (`obj_init`, `propobj.c:2098`).
    pub modelscale: f32,
    pub scale: f32,
    pub speed: f32,
    pub speeddecel: f32,
    pub traveldist: f32,
    pub timer60: i32,
    pub hitspeedpreservationfrac: f32,
}

/// `struct funcdef` and its subtypes, flattened.
#[derive(Clone, Debug, Default)]
pub struct FuncDef {
    pub symbol: String,
    /// The function's name in the HUD (`name_text`, from the `L_GUN` bank).
    pub name: String,
    /// `funcdef.type`: `INVENTORYFUNCTYPE_*`.
    pub ftype: u32,
    pub ammoindex: i32,
    pub noise: NoiseSettings,
    pub fire_animation: Option<ScriptId>,
    pub flags: u32,
    pub shoot: Option<ShootDef>,
    /// Melee / throw / special fields.
    pub damage: f32,
    pub range: f32,
    pub recoverytime60: i32,
    pub activatetime60: i32,
    /// `HANDATTACKTYPE_*` for specials.
    pub specialfunc: i32,
    pub proj: Option<ProjDef>,
    /// `funcdef_special.soundnum` / `funcdef_shootprojectile.soundnum`.
    pub soundnum: u16,
}

impl FuncDef {
    /// The low byte of the type: `INVENTORYFUNCTYPE_SHOOT`, `_THROW`, `_MELEE`, ...
    pub fn kind(&self) -> u32 {
        self.ftype & 0xff
    }

    /// `INVENTORYFUNCTYPE_SHOOT_AUTOMATIC`.
    pub fn is_auto(&self) -> bool {
        self.ftype & 0xff00 == 0x100
    }
}

/// `struct inventory_ammo`.
#[derive(Clone, Debug, Default)]
pub struct AmmoDef {
    pub ammotype: i32,
    pub casingeject: i32,
    pub clipsize: i32,
    pub reload_animation: Option<ScriptId>,
    pub flags: u32,
}

/// `struct gunviscmd`.
#[derive(Clone, Debug)]
pub struct GunVisCmd {
    /// `GUNVISCMD_*`: 1 always, 4 upgrade, 5 in the left hand, 6 in the right.
    pub ctype: i32,
    pub param: i32,
    /// `GUNVISOP_*`.
    pub op: i32,
    pub partnum: i32,
}

/// `struct invaimsettings`.
#[derive(Clone, Debug)]
pub struct AimDef {
    pub zoomfov: f32,
    pub guntransup: f32,
    pub guntransdown: f32,
    pub guntransside: f32,
    pub aimdamp: f32,
    pub flags: u32,
}

impl Default for AimDef {
    /// `invaimsettings_default` (`invitems.c:90`).
    fn default() -> Self {
        AimDef { zoomfov: 0.0, guntransup: 3.0, guntransdown: 8.0, guntransside: 15.0, aimdamp: 0.9767, flags: INVAIMFLAG_AUTOAIM }
    }
}

/// `struct weapondef` (`types.h:3023`).
#[derive(Clone, Debug)]
pub struct WeaponDef {
    pub weaponnum: u8,
    pub name: String,
    pub short_name: String,
    /// The first-person model's stem under `models/` (`hi_model`).
    pub model: Option<String>,
    pub equip_animation: Option<ScriptId>,
    pub unequip_animation: Option<ScriptId>,
    pub pritosec_animation: Option<ScriptId>,
    pub sectopri_animation: Option<ScriptId>,
    pub functions: [Option<FuncDef>; 2],
    pub ammos: [Option<AmmoDef>; 2],
    pub aim: AimDef,
    pub muzzlez: f32,
    pub posx: f32,
    pub posy: f32,
    pub posz: f32,
    pub sway: f32,
    pub gunviscmds: Vec<GunVisCmd>,
    pub flags: u32,
    /// `g_MpWeapons[]` starting ammo: (type, quantity) per function.
    pub mp_ammo: [(i32, i32); 2],
}

/// The whole table plus the scripts (`g_Weapons` and every `invanim_*`).
pub struct Gset {
    pub weapons: HashMap<u8, WeaponDef>,
    /// Weapon numbers in the export's order (the MP menu's, then unarmed).
    pub order: Vec<u8>,
    pub scripts: Vec<Vec<GunCmd>>,
    pub script_names: Vec<String>,
    /// `var80070200` (`bondgun.c:6391`): the detonator press, a script built in C.
    pub detonate_script: Option<ScriptId>,
}

// ─── weapons.json ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RawFile {
    weapons: Vec<RawWeapon>,
    scripts: HashMap<String, RawScript>,
    aimsettings: HashMap<String, RawAim>,
    recoilsettings: HashMap<String, RecoilSettings>,
    noisesettings: HashMap<String, NoiseSettings>,
    gunviscmds: HashMap<String, Vec<RawGunVis>>,
    code_anims: HashMap<String, u16>,
    /// `enum sfxnum` + `enum sfxmap` names to ids.
    sfx: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct RawWeapon {
    weapon: String,
    weaponnum: u8,
    name_text: Option<String>,
    short_text: Option<String>,
    muzzlez: f32,
    posx: f32,
    posy: f32,
    posz: f32,
    sway: f32,
    weapon_flags: u32,
    assets: Option<RawAssets>,
    mp: Option<RawMp>,
    #[serde(default)]
    functions: Vec<Option<Value>>,
    #[serde(default)]
    ammo: Vec<Option<RawAmmo>>,
    equip_animation: Option<String>,
    unequip_animation: Option<String>,
    pritosec_animation: Option<String>,
    sectopri_animation: Option<String>,
    aimsettings: Option<String>,
    gunviscmds_symbol: Option<String>,
}

#[derive(Deserialize)]
struct RawAssets {
    fp_model: Option<String>,
}

#[derive(Deserialize)]
struct RawMp {
    pri_ammo_type: i32,
    pri_ammo_qty: i32,
    sec_ammo_type: i32,
    sec_ammo_qty: i32,
}

#[derive(Deserialize)]
struct RawAmmo {
    #[serde(rename = "type")]
    ammotype: i32,
    casingeject: i32,
    clipsize: i32,
    reload_animation: Option<String>,
    flags: u32,
}

#[derive(Deserialize)]
struct RawScript {
    cmds: Vec<Value>,
}

#[derive(Deserialize)]
struct RawAim {
    zoomfov: f32,
    guntransup: f32,
    guntransdown: f32,
    guntransside: f32,
    aimdamp: f32,
    flags: Value,
}

#[derive(Deserialize)]
struct RawGunVis {
    op: String,
    args: Vec<Value>,
}

fn num(v: &Value, key: &str) -> f32 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

fn int(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl Gset {
    /// `assets/data/weapons.json`.
    pub fn load(assets: &AssetDir) -> Result<Gset, String> {
        let raw: RawFile = assets.read_json(&assets.data("weapons.json"))?;
        Ok(Gset::from_raw(&raw))
    }

    fn from_raw(w: &RawFile) -> Gset {
        // Scripts first, so references resolve to indices.
        let mut names: Vec<String> = w.scripts.keys().cloned().collect();
        names.sort();
        let index: HashMap<String, usize> = names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let mut scripts: Vec<Vec<GunCmd>> = names.iter().map(|n| w.scripts[n].cmds.iter().map(|c| decode_cmd(c, &index)).collect()).collect();
        let script = |s: &Option<String>| -> Option<ScriptId> { s.as_ref().and_then(|n| index.get(n).copied()) };

        let mut weapons = HashMap::new();
        let mut order = Vec::new();
        for rw in &w.weapons {
            let aim = rw
                .aimsettings
                .as_ref()
                .and_then(|n| w.aimsettings.get(n))
                .map(|a| AimDef {
                    zoomfov: a.zoomfov,
                    guntransup: a.guntransup,
                    guntransdown: a.guntransdown,
                    guntransside: a.guntransside,
                    aimdamp: a.aimdamp,
                    flags: a.flags.as_u64().unwrap_or(0) as u32,
                })
                .unwrap_or_default();
            let mut functions: [Option<FuncDef>; 2] = [None, None];
            for (i, f) in rw.functions.iter().enumerate().take(2) {
                let Some(f) = f else { continue };
                let noise = string(f, "noisesettings").and_then(|n| w.noisesettings.get(&n).copied()).unwrap_or_default();
                let ftype = int(f, "type") as u32;
                let mut fd = FuncDef {
                    symbol: string(f, "symbol").unwrap_or_default(),
                    name: string(f, "name_text").unwrap_or_default(),
                    ftype,
                    ammoindex: int(f, "ammoindex") as i32,
                    noise,
                    fire_animation: script(&string(f, "fire_animation")),
                    flags: int(f, "flags") as u32,
                    shoot: None,
                    damage: num(f, "damage"),
                    range: num(f, "range"),
                    recoverytime60: int(f, "recoverytime60") as i32,
                    activatetime60: int(f, "activatetime60") as i32,
                    specialfunc: handattacktype(f.get("specialfunc")),
                    proj: f.get("projectilemodelnum").and_then(Value::as_i64).map(|m| ProjDef {
                        projectilemodelnum: m as i32,
                        model: string(f, "projectile_model"),
                        modelscale: int(f, "projectile_model_scale") as f32 * (1.0 / 4096.0),
                        scale: if f.get("scale").is_some() { num(f, "scale") } else { 1.0 },
                        speed: num(f, "speed"),
                        speeddecel: num(f, "speeddecel"),
                        traveldist: num(f, "traveldist"),
                        timer60: int(f, "timer60") as i32,
                        hitspeedpreservationfrac: num(f, "hitspeedpreservationfrac"),
                    }),
                    soundnum: match f.get("soundnum") {
                        Some(Value::Number(n)) if n.as_i64().unwrap_or(0) > 0 => n.as_i64().unwrap_or(0) as u16,
                        Some(v @ Value::String(_)) => resolve_sfx(Some(v), w),
                        _ => 0,
                    },
                };
                if ftype & 0xff == INVENTORYFUNCTYPE_SHOOT {
                    fd.shoot = Some(ShootDef {
                        recoil: string(f, "recoilsettings").and_then(|n| w.recoilsettings.get(&n).copied()),
                        recoverytime60: int(f, "recoverytime60") as i32,
                        damage: num(f, "damage"),
                        spread: num(f, "spread"),
                        unk24: int(f, "unk24") as i32,
                        unk25: int(f, "unk25") as i32,
                        unk26: int(f, "unk26") as i32,
                        unk27: int(f, "unk27") as i32,
                        recoildist: num(f, "recoildist"),
                        recoilangle: num(f, "recoilangle"),
                        slidemax: num(f, "slidemax"),
                        impactforce: num(f, "impactforce"),
                        duration60: int(f, "duration60") as i32,
                        shootsound: resolve_sfx(f.get("shootsound"), w),
                        penetration: int(f, "penetration") as i32,
                        initialrpm: num(f, "initialrpm"),
                        maxrpm: num(f, "maxrpm"),
                        turretaccel: num(f, "turretaccel"),
                        turretdecel: num(f, "turretdecel"),
                    });
                }
                functions[i] = Some(fd);
            }
            let mut ammos: [Option<AmmoDef>; 2] = [None, None];
            for (i, a) in rw.ammo.iter().enumerate().take(2) {
                if let Some(a) = a {
                    ammos[i] = Some(AmmoDef { ammotype: a.ammotype, casingeject: a.casingeject, clipsize: a.clipsize, reload_animation: script(&a.reload_animation), flags: a.flags });
                }
            }
            let gunviscmds = rw.gunviscmds_symbol.as_ref().and_then(|n| w.gunviscmds.get(n)).map(|cmds| cmds.iter().map(decode_gunvis).collect()).unwrap_or_default();
            // "guns/falcon2.bin" -> "falcon2".
            let model = rw.assets.as_ref().and_then(|a| a.fp_model.as_ref()).map(|p| {
                let file = p.rsplit('/').next().unwrap_or(p);
                file.strip_suffix(".bin").unwrap_or(file).to_owned()
            });
            let mp_ammo = rw.mp.as_ref().map_or([(0, 0); 2], |m| [(m.pri_ammo_type, m.pri_ammo_qty), (m.sec_ammo_type, m.sec_ammo_qty)]);
            let def = WeaponDef {
                weaponnum: rw.weaponnum,
                name: rw.name_text.clone().unwrap_or_else(|| rw.weapon.clone()),
                short_name: rw.short_text.clone().unwrap_or_else(|| rw.weapon.clone()),
                model,
                equip_animation: script(&rw.equip_animation),
                unequip_animation: script(&rw.unequip_animation),
                pritosec_animation: script(&rw.pritosec_animation),
                sectopri_animation: script(&rw.sectopri_animation),
                functions,
                ammos,
                aim,
                muzzlez: rw.muzzlez,
                posx: rw.posx,
                posy: rw.posy,
                posz: rw.posz,
                sway: rw.sway,
                gunviscmds,
                flags: rw.weapon_flags,
                mp_ammo,
            };
            order.push(rw.weaponnum);
            weapons.insert(rw.weaponnum, def);
        }
        // var80070200 = { PLAYANIMATION(ANIM_0434, 10000), END }.
        let detonate_script = w.code_anims.get("ANIM_0434").map(|&anim| {
            scripts.push(vec![GunCmd::PlayAnimation { condition: 0, anim, params: 10000 }, GunCmd::End]);
            names.push("var80070200".into());
            scripts.len() - 1
        });
        Gset { weapons, order, scripts, script_names: names, detonate_script }
    }

    /// `gset_get_weapondef`.
    pub fn weapon(&self, weaponnum: u8) -> Option<&WeaponDef> {
        self.weapons.get(&weaponnum)
    }

    /// `gset_get_funcdef_by_weaponnum_funcnum`.
    pub fn func(&self, weaponnum: u8, which: usize) -> Option<&FuncDef> {
        self.weapon(weaponnum).and_then(|w| w.functions.get(which)).and_then(|f| f.as_ref())
    }

    /// `gset_has_weapon_flag`.
    pub fn has_flag(&self, weaponnum: u8, flag: u32) -> bool {
        self.weapon(weaponnum).is_some_and(|w| w.flags & flag != 0)
    }

    /// `gset_has_aim_flag`.
    pub fn has_aim_flag(&self, weaponnum: u8, flag: u32) -> bool {
        self.weapon(weaponnum).is_some_and(|w| w.aim.flags & flag != 0)
    }

    /// `gset_get_filenum2(weaponnum) != 0`: the weapon has a first-person model.
    pub fn has_model(&self, weaponnum: u8) -> bool {
        self.weapon(weaponnum).is_some_and(|w| w.model.is_some())
    }

    pub fn cmd(&self, p: CmdPtr) -> &GunCmd {
        self.scripts[p.0].get(p.1).unwrap_or(&GunCmd::End)
    }

    /// `gset_get_sight` (`gset.c:586`) for the right hand's weapon and function,
    /// without cheats (`CHEAT_CLASSICSIGHT` is solo-only).
    pub fn gset_get_sight(&self, weaponnum: u8, weaponfunc: usize) -> i32 {
        if self.func(weaponnum, weaponfunc).is_some_and(|f| f.kind() == INVENTORYFUNCTYPE_MELEE) {
            return SIGHT_NONE;
        }
        match weaponnum {
            WEAPON_HORIZONSCANNER => SIGHT_NONE,
            WEAPON_FALCON2_SCOPE | WEAPON_MAGSEC4 | WEAPON_SNIPERRIFLE | WEAPON_LAPTOPGUN | WEAPON_DRAGON | WEAPON_K7AVENGER | WEAPON_AR34 | WEAPON_SUPERDRAGON => SIGHT_ZOOM,
            WEAPON_MAULER | WEAPON_REAPER => SIGHT_SKEDAR,
            WEAPON_PHOENIX | WEAPON_CALLISTO | WEAPON_FARSIGHT => SIGHT_MAIAN,
            WEAPON_PP9I | WEAPON_CC13 | WEAPON_KL01313 | WEAPON_KF7SPECIAL | WEAPON_ZZT | WEAPON_DMC | WEAPON_AR53 | WEAPON_RCP45 => SIGHT_CLASSIC,
            _ => SIGHT_DEFAULT,
        }
    }
}

/// `HANDATTACKTYPE_*` by name or number.
fn handattacktype(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) as i32,
        Some(Value::String(s)) => match s.as_str() {
            "HANDATTACKTYPE_SHOOT" => HANDATTACKTYPE_SHOOT,
            "HANDATTACKTYPE_SHOOTPROJECTILE" => HANDATTACKTYPE_SHOOTPROJECTILE,
            "HANDATTACKTYPE_THROWPROJECTILE" => HANDATTACKTYPE_THROWPROJECTILE,
            "HANDATTACKTYPE_MELEE" => HANDATTACKTYPE_MELEE,
            "HANDATTACKTYPE_DETONATE" => HANDATTACKTYPE_DETONATE,
            "HANDATTACKTYPE_BOOST" => HANDATTACKTYPE_BOOST,
            "HANDATTACKTYPE_REVERTBOOST" => HANDATTACKTYPE_REVERTBOOST,
            "HANDATTACKTYPE_CROUCH" => HANDATTACKTYPE_CROUCH,
            "HANDATTACKTYPE_RCP120CLOAK" => HANDATTACKTYPE_RCP120CLOAK,
            "HANDATTACKTYPE_MELEENOUNCLOAK" => HANDATTACKTYPE_MELEENOUNCLOAK,
            "HANDATTACKTYPE_UPLINK" => HANDATTACKTYPE_UPLINK,
            _ => 0,
        },
        _ => 0,
    }
}

fn resolve_sfx(v: Option<&Value>, w: &RawFile) -> u16 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u16,
        Some(Value::String(s)) => w.sfx.get(s).copied().unwrap_or(0) as u16,
        _ => 0,
    }
}

fn decode_gunvis(c: &RawGunVis) -> GunVisCmd {
    // The operator arrives by name (`GUNVISOP_*`); read as a number,
    // SETVISIBILITY became IFTRUE_SETVISIBLE and the in-left/in-right hand
    // checks never hid anything (the spike's remote-mine hands drew both).
    let arg = |i: usize| match c.args.get(i) {
        Some(Value::String(s)) => match s.as_str() {
            "GUNVISOP_IFTRUE_SETVISIBLE" => GUNVISOP_IFTRUE_SETVISIBLE,
            "GUNVISOP_IFTRUE_SETHIDDEN" => GUNVISOP_IFTRUE_SETHIDDEN,
            "GUNVISOP_SETVISIBILITY" => GUNVISOP_SETVISIBILITY,
            _ => 0,
        },
        Some(v) => v.as_i64().unwrap_or(0) as i32,
        None => 0,
    };
    match c.op.as_str() {
        // gunviscmd_sethidden(part) = { ALWAYSTRUE, 0, IFTRUE_SETHIDDEN, part }
        "sethidden" => GunVisCmd { ctype: 1, param: 0, op: GUNVISOP_IFTRUE_SETHIDDEN, partnum: arg(0) },
        // gunviscmd_checkupgrade(upgrade, op, part)
        "checkupgrade" => GunVisCmd { ctype: 4, param: arg(0), op: arg(1), partnum: arg(2) },
        "checkinlefthand" => GunVisCmd { ctype: 5, param: 0, op: arg(0), partnum: arg(1) },
        "checkinrighthand" => GunVisCmd { ctype: 6, param: 0, op: arg(0), partnum: arg(1) },
        _ => GunVisCmd { ctype: 0, param: 0, op: 0, partnum: 0 },
    }
}

fn decode_cmd(c: &Value, index: &HashMap<String, usize>) -> GunCmd {
    let op = c.get("op").and_then(Value::as_str).unwrap_or("end");
    let i = |k: &str| int(c, k) as i32;
    let target = |k: &str| -> ScriptId { c.get(k).and_then(Value::as_str).and_then(|n| index.get(n).copied()).unwrap_or(usize::MAX) };
    match op {
        "playanimation" => GunCmd::PlayAnimation { condition: int(c, "condition") as u8, anim: int(c, "anim") as u16, params: int(c, "params") as i32 },
        "showpart" => GunCmd::ShowPart { keyframe: i("keyframe"), part: i("part") },
        "hidepart" => GunCmd::HidePart { keyframe: i("keyframe"), part: i("part") },
        "waitforzreleased" => GunCmd::WaitForZReleased { keyframe: i("keyframe") },
        "allowfeature" => GunCmd::AllowFeature { keyframe: i("keyframe"), feature: i("feature") },
        "playsound" => GunCmd::PlaySound { keyframe: i("keyframe"), sound: int(c, "sound") as u16 },
        "include" => GunCmd::Include { condition: int(c, "condition") as u8, target: target("target") },
        "random" => GunCmd::Random { probability: i("probability"), target: target("target") },
        "repeatuntilfull" => GunCmd::RepeatUntilFull { keyframe: i("keyframe"), gotokeyframe: i("gotokeyframe") },
        "popoutsackofpills" => GunCmd::PopOutSackOfPills { keyframe: i("keyframe") },
        "setsoundspeed" => GunCmd::SetSoundSpeed { keyframe: i("keyframe"), speed: i("speed") },
        _ => GunCmd::End,
    }
}
