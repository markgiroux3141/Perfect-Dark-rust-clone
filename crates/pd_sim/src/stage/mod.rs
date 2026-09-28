//! A stage as the simulation needs it, loaded from `assets/stages/<code>/`
//! (`tools/pd-assets/pd_stage.py`):
//!
//! - the level geometry ([`LevelGeom`]/[`GeomPoly`]: floors, walls, sight/shot
//!   blockers, ladders, crouch zones, floor types, rooms) from `tiles.json`;
//! - collision on it ([`TileLevel`]: the `lib/collision.c` primitives);
//! - pads, PD's waypoint graph (waypoints + waygroups) and cover from `pads.json`;
//! - the MP setup's spawn pads (`intro[]`'s `spawn()`s) and props from `setup.json`;
//! - what shots hit ([`BgHitMesh`]): the textured BG's triangles from `bg.json`.
//!
//! The box [`fixtures`] (the simulant spike's arena, the gun spike's range) are
//! test stages built from the same polygons.
//!
//! Sources: the old repo's `pd_spike/level_geom.rs`, `tile_level.rs` and
//! `pd_tiles.rs`. The spike read the decomp's JSON and `mp_setup<code>.c` at run
//! time; the stage exporter owns that now.

pub mod bghit;
mod collision;
pub mod fixtures;
mod geom;

pub use bghit::{BgHit, BgHitMesh, TexSurface};
pub use collision::*;
pub use geom::*;

use glam::Vec3;
use pd_core::assets::AssetDir;
use pd_core::ids::*;
use serde::Deserialize;

/// `WPSEGFLAG_OUTWARDSONLY` (`padhalllv.c:44`): the link may only be used leaving
/// this waypoint ("eg. top of ledge").
pub const WPSEGFLAG_OUTWARDSONLY: i32 = 0x4000;
/// `WPSEGFLAG_INWARDSONLY` (`padhalllv.c:45`): only arriving ("eg. bottom of ledge").
pub const WPSEGFLAG_INWARDSONLY: i32 = 0x8000;

/// `WPSEG_GET_ID(seg)` (`padhalllv.c:51`).
pub fn wpseg_get_id(seg: i32) -> usize {
    (seg & (0xffff & !(WPSEGFLAG_OUTWARDSONLY | WPSEGFLAG_INWARDSONLY))) as usize
}

/// `struct pad`. `pos` is where PD put the pad, which is **not** ground height:
/// on Complex most pads sit 52–53 cm above their floor, some over a railing or
/// a walkway edge.
#[derive(Clone, Debug, Deserialize)]
pub struct Pad {
    #[serde(deserialize_with = "vec3")]
    pub pos: Vec3,
    #[serde(deserialize_with = "vec3")]
    pub look: Vec3,
    #[serde(deserialize_with = "vec3")]
    pub up: Vec3,
    /// `PADFLAG_*`.
    pub flags: u32,
    /// `[xmin, xmax, ymin, ymax, zmin, zmax]`.
    pub bbox: [f32; 6],
    pub liftnum: i32,
}

impl Pad {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// The facing a chr spawned here takes: `atan2f(pad.look.x, pad.look.z)`
    /// (`player.c:326`), true radians from +z towards +x.
    pub fn look_angle(&self) -> f32 {
        pd_core::math::atan2f(self.look.x, self.look.z)
    }
}

/// `struct waypoint`: `neighbours` are PD's encoded segments (neighbour id |
/// `WPSEGFLAG_*`), without the `-1` terminator.
#[derive(Clone, Debug)]
pub struct Waypoint {
    pub padnum: usize,
    pub neighbours: Vec<i32>,
    pub groupnum: usize,
}

/// `struct waygroup`: `neighbours` encoded like [`Waypoint::neighbours`];
/// `waypoints` lists the member waypoints in index order, which is PD's order
/// (`tools/assetmgr/mkpads:40-46` builds it that way).
#[derive(Clone, Debug)]
pub struct Waygroup {
    pub neighbours: Vec<i32>,
    pub waypoints: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Cover {
    #[serde(deserialize_with = "vec3")]
    pub pos: Vec3,
    #[serde(deserialize_with = "vec3")]
    pub look: Vec3,
    pub special: i32,
}

/// Everything the world loads for one stage.
#[derive(Clone, Debug)]
pub struct Stage {
    /// PD's stage code, the name of its asset directory (`ref` is Complex).
    pub code: String,
    /// `STAGE_*`.
    pub stagenum: u8,
    pub geom: LevelGeom,
    pub pads: Vec<Pad>,
    pub waypoints: Vec<Waypoint>,
    pub waygroups: Vec<Waygroup>,
    pub cover: Vec<Cover>,
    /// The pads of the `spawn()` entries in `intro[]`, in file order
    /// (`g_SpawnPoints`, `setup.c`).
    pub spawn_pads: Vec<usize>,
    /// The rest of `intro[]` and `props[]`, one JSON object per setup macro
    /// (`{type, <param>: value}`), for the pickups (M8) and scenarios (M10).
    pub intro: Vec<serde_json::Value>,
    pub props: Vec<serde_json::Value>,
    /// The BG triangles shots hit, with their textures' surface types.
    pub bghit: BgHitMesh,
}

// ─── the files ───────────────────────────────────────────────────────────────

fn vec3<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec3, D::Error> {
    <[f32; 3]>::deserialize(d).map(Vec3::from)
}

#[derive(Deserialize)]
struct TilesFile {
    format: String,
    rooms: Vec<u16>,
    tiles: Vec<TileRow>,
}

#[derive(Deserialize)]
struct TileRow {
    room: u16,
    flags: u32,
    floortype: u8,
    verts: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
struct PadsFile {
    format: String,
    pads: Vec<Pad>,
    waypoints: Vec<WaypointRow>,
    waygroups: Vec<WaygroupRow>,
    cover: Vec<Cover>,
}

#[derive(Deserialize)]
struct WaypointRow {
    pad: usize,
    group: usize,
    neighbours: Vec<i32>,
}

#[derive(Deserialize)]
struct WaygroupRow {
    neighbours: Vec<i32>,
}

#[derive(Deserialize)]
struct SetupFile {
    format: String,
    stage: SetupStage,
    intro: Vec<serde_json::Value>,
    props: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SetupStage {
    num: u8,
}

fn check_format(what: &str, got: &str, want: &str) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: format {got:?}, expected {want}"))
    }
}

/// One tile as the collision code sees it. Only the flags the chr code reads
/// survive; `GEOFLAG_STEP`/`SLOPE`/`DIE`/`RAMPWALL`/lifts arrive with M9.
fn tile_poly(t: &TileRow) -> Result<GeomPoly, String> {
    if t.verts.len() < 3 {
        return Err(format!("tiles: room {:#x} has a tile with {} vertices", t.room, t.verts.len()));
    }
    let f = |bit: u32| t.flags & bit != 0;
    let mut p = GeomPoly::new(
        t.verts.iter().map(|&v| Vec3::from(v)).collect(),
        f(GEOFLAG_FLOOR1 | GEOFLAG_FLOOR2),
        f(GEOFLAG_WALL),
        f(GEOFLAG_BLOCK_SIGHT),
        f(GEOFLAG_BLOCK_SHOOT),
        Some(t.room),
    );
    p.ladder = f(GEOFLAG_LADDER);
    p.crouch = f(GEOFLAG_AIBOTCROUCH);
    p.duck = f(GEOFLAG_AIBOTDUCK);
    p.floortype = t.floortype;
    Ok(p)
}

impl Stage {
    /// Load `stages/<code>/`.
    pub fn load(assets: &AssetDir, code: &str) -> Result<Stage, String> {
        let dir = assets.stage(code);
        let tiles: TilesFile = assets.read_json(&dir.join("tiles.json"))?;
        check_format("tiles.json", &tiles.format, "pd-tiles/1")?;
        let pads: PadsFile = assets.read_json(&dir.join("pads.json"))?;
        check_format("pads.json", &pads.format, "pd-pads/1")?;
        let setup: SetupFile = assets.read_json(&dir.join("setup.json"))?;
        check_format("setup.json", &setup.format, "pd-setup/1")?;

        let geom = LevelGeom { polys: tiles.tiles.iter().map(tile_poly).collect::<Result<_, _>>()?, rooms: tiles.rooms };
        let waypoints: Vec<Waypoint> =
            pads.waypoints.into_iter().map(|w| Waypoint { padnum: w.pad, neighbours: w.neighbours, groupnum: w.group }).collect();
        let mut waygroups: Vec<Waygroup> = pads.waygroups.into_iter().map(|g| Waygroup { neighbours: g.neighbours, waypoints: Vec::new() }).collect();
        for (i, w) in waypoints.iter().enumerate() {
            if w.padnum >= pads.pads.len() {
                return Err(format!("waypoint {i:#x}: pad {:#x} out of range", w.padnum));
            }
            waygroups.get_mut(w.groupnum).ok_or_else(|| format!("waypoint {i:#x}: bad group"))?.waypoints.push(i);
        }
        let spawn_pads: Vec<usize> = setup
            .intro
            .iter()
            .filter(|e| e["type"] == "spawn")
            .map(|e| e["pad"].as_u64().map(|p| p as usize).ok_or_else(|| format!("setup: spawn without a pad: {e}")))
            .collect::<Result<_, _>>()?;
        if let Some(&bad) = spawn_pads.iter().find(|&&p| p >= pads.pads.len()) {
            return Err(format!("spawn pad {bad:#x} out of range"));
        }
        Ok(Stage {
            code: code.to_owned(),
            stagenum: setup.stage.num,
            geom,
            pads: pads.pads,
            waypoints,
            waygroups,
            cover: pads.cover,
            spawn_pads,
            intro: setup.intro,
            props: setup.props,
            bghit: BgHitMesh::load(assets, code)?,
        })
    }

    /// A test stage from a box fixture: its polygons, shots hitting them as the
    /// default surface, and a spawn pad at each `(pos, look)`.
    pub fn fixture(code: &str, geom: LevelGeom, spawns: &[(Vec3, Vec3)]) -> Stage {
        let pads: Vec<Pad> = spawns.iter().map(|&(pos, look)| Pad { pos, look, up: Vec3::Y, flags: 0, bbox: [0.0; 6], liftnum: -1 }).collect();
        Stage {
            code: code.to_owned(),
            stagenum: 0,
            bghit: BgHitMesh::from_geom(&geom),
            geom,
            spawn_pads: (0..pads.len()).collect(),
            pads,
            waypoints: Vec::new(),
            waygroups: Vec::new(),
            cover: Vec::new(),
            intro: Vec::new(),
            props: Vec::new(),
        }
    }

    pub fn waypoint_pos(&self, w: usize) -> Vec3 {
        self.pads[self.waypoints[w].padnum].pos
    }

    /// Every directed link of the waypoint graph a chr may walk: `a → b` is out
    /// if `a`'s entry for `b` is `WPSEGFLAG_INWARDSONLY` ("only arrive at `a` this
    /// way"), or if `b`'s entry for `a` is `WPSEGFLAG_OUTWARDSONLY`. Returns
    /// `(a, b, one_way)`.
    pub fn waypoint_links(&self) -> Vec<(usize, usize, bool)> {
        let wps = &self.waypoints;
        let seg = |a: usize, b: usize| wps[a].neighbours.iter().copied().find(|&s| wpseg_get_id(s) == b);
        let mut out = Vec::new();
        for (a, w) in wps.iter().enumerate() {
            for &s in &w.neighbours {
                let b = wpseg_get_id(s);
                let back = seg(b, a);
                let forward_ok = s & WPSEGFLAG_INWARDSONLY == 0;
                let back_ok = back.is_none_or(|t| t & WPSEGFLAG_OUTWARDSONLY == 0);
                if forward_ok && back_ok {
                    let one_way = s & WPSEGFLAG_OUTWARDSONLY != 0 || back.is_none_or(|t| t & WPSEGFLAG_INWARDSONLY != 0);
                    out.push((a, b, one_way));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn complex() -> Stage {
        Stage::load(&AssetDir::from_manifest_dir(env!("CARGO_MANIFEST_DIR")), "ref").expect("assets/stages/ref")
    }

    #[test]
    fn segments_keep_pds_encoding() {
        assert_eq!(wpseg_get_id(0x8a | WPSEGFLAG_OUTWARDSONLY), 0x8a);
        assert_eq!(wpseg_get_id(0x13 | WPSEGFLAG_INWARDSONLY), 0x13);
    }

    /// Complex as the simulant spike's recon counted it (SPIKE_PD_COMPLEX.md).
    #[test]
    fn complex_loads_with_the_documented_counts() {
        let s = complex();
        assert_eq!(s.stagenum, STAGE_MP_COMPLEX);
        let g = &s.geom;
        assert_eq!(g.polys.len(), 1208);
        assert_eq!(g.rooms.len(), 45);
        assert_eq!(g.polys.iter().filter(|p| p.floor).count(), 350);
        assert_eq!(g.polys.iter().filter(|p| p.wall).count(), 858);
        assert_eq!(g.polys.iter().filter(|p| p.wall && !p.blocks_sight).count(), 41);
        assert_eq!(g.polys.iter().filter(|p| p.ladder).count(), 2);
        assert_eq!(g.polys.iter().filter(|p| p.crouch).count(), 3);
        let (lo, hi) = g.bounds();
        assert_eq!((lo.x, lo.y, lo.z), (-5053.0, -276.0, -1956.0));
        assert_eq!((hi.x, hi.y, hi.z), (-643.0, 748.0, 1943.0));
        assert_eq!(s.pads.len(), 226);
        assert_eq!(s.waypoints.len(), 144);
        assert_eq!(s.waygroups.len(), 20);
        assert_eq!(s.cover.len(), 101);
        assert_eq!(s.spawn_pads, (0x1c..=0x2e).collect::<Vec<_>>());
        assert_eq!(s.waygroups.iter().map(|g| g.waypoints.len()).sum::<usize>(), 144);
        assert_eq!(s.waypoint_links().len(), 401);
        // mp_setupref.c: ten weapon locations, 20 ammo crates, five crates.
        let count = |t: &str| s.props.iter().filter(|p| p["type"] == t).count();
        assert_eq!((count("weapon"), count("ammocratemulti"), count("stdobject")), (10, 20, 5));
    }

    /// Pads and tiles share one frame: every pad has a floor tile under it, and
    /// spawn pads sit 52–63 cm above theirs (what a spawning chr drops by).
    #[test]
    fn every_pad_stands_over_a_floor() {
        let s = complex();
        let under = |pad: usize| {
            let p = s.pads[pad].pos;
            s.geom.floor_below(p.x, p.z, p.y + 10.0).map(|(y, _)| p.y - y)
        };
        for pad in 0..s.pads.len() {
            assert!(under(pad).is_some(), "pad {pad:#x} has no floor under it");
        }
        for &pad in &s.spawn_pads {
            let h = under(pad).unwrap_or_else(|| panic!("spawn pad {pad:#x} has no floor under it"));
            assert!((50.0..=65.0).contains(&h), "spawn pad {pad:#x} is {h} cm above its floor");
        }
    }
}
