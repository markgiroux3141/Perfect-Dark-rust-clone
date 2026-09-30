//! The glTF importer on a file written here, and the layout: a level's
//! placement adopted, edited and placed again.

use glam::{Vec2, Vec3};
use pd_core::ids::*;
use serde_json::{json, Value};

use crate::layout::{Ammo, Layout, Weapon};
use crate::recipe::{GltfSource, Recipe, Source};

/// A glTF 2.0 binary being written: meshes into one BIN chunk.
#[derive(Default)]
struct GlbOut {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
}

impl GlbOut {
    fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.views.push(json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()}));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn floats(&mut self, v: &[f32], ty: &str) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        let comps = match ty {
            "VEC2" => 2,
            "VEC3" => 3,
            _ => 4,
        };
        let view = self.view(&bytes);
        self.accessors.push(json!({"bufferView": view, "componentType": 5126, "count": v.len() / comps, "type": ty}));
        self.accessors.len() - 1
    }

    /// A mesh of quads (4 corners each, wound counter-clockwise seen from
    /// the side they face), indexed as u16 triangle lists.
    fn quads(&mut self, quads: &[[Vec3; 4]], material: usize, colour: Option<[f32; 4]>) -> usize {
        let pos: Vec<f32> = quads.iter().flat_map(|q| q.iter().flat_map(|p| p.to_array())).collect();
        let uv: Vec<f32> = quads.iter().flat_map(|_| [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]).collect();
        let idx: Vec<u8> = (0..quads.len() as u16).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|k| q * 4 + k)).flat_map(|i| i.to_le_bytes()).collect();
        let mut attrs = json!({"POSITION": self.floats(&pos, "VEC3"), "TEXCOORD_0": self.floats(&uv, "VEC2")});
        if let Some(c) = colour {
            let cols: Vec<f32> = (0..quads.len() * 4).flat_map(|_| c).collect();
            attrs["COLOR_0"] = json!(self.floats(&cols, "VEC4"));
        }
        let iv = self.view(&idx);
        self.accessors.push(json!({"bufferView": iv, "componentType": 5123, "count": quads.len() * 6, "type": "SCALAR"}));
        self.meshes.push(json!({"primitives": [{"attributes": attrs, "indices": self.accessors.len() - 1, "material": material}]}));
        self.meshes.len() - 1
    }

    fn node(&mut self, n: Value) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn write(mut self, path: &std::path::Path, materials: Value, png: &[u8]) {
        let img = self.view(png);
        let children: Vec<u64> = self.nodes.iter().flat_map(|n| n["children"].as_array().cloned().unwrap_or_default()).filter_map(|c| c.as_u64()).collect();
        let roots: Vec<usize> = (0..self.nodes.len()).filter(|i| !children.contains(&(*i as u64))).collect();
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let json = json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": roots}],
            "nodes": self.nodes, "meshes": self.meshes, "accessors": self.accessors, "bufferViews": self.views,
            "buffers": [{"byteLength": self.bin.len()}], "materials": materials,
            "images": [{"bufferView": img, "mimeType": "image/png"}], "textures": [{"source": 0, "sampler": 0}],
            "samplers": [{"wrapS": 33071, "wrapT": 10497}],
        });
        let mut j = serde_json::to_vec(&json).unwrap();
        while !j.len().is_multiple_of(4) {
            j.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + j.len() + 8 + self.bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(j.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        out.extend_from_slice(&j);
        out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E4942u32.to_le_bytes());
        out.extend_from_slice(&self.bin);
        std::fs::write(path, out).unwrap();
    }
}

/// A floor quad over `lo..hi` (x, z) at `y`, facing up.
fn floor(lo: Vec2, hi: Vec2, y: f32) -> [Vec3; 4] {
    [Vec3::new(lo.x, y, lo.y), Vec3::new(lo.x, y, hi.y), Vec3::new(hi.x, y, hi.y), Vec3::new(hi.x, y, lo.y)]
}

/// A wall from `a` to `b` (x, z), `h` high, facing left of a → b.
fn wall(a: Vec2, b: Vec2, h: f32) -> [Vec3; 4] {
    [Vec3::new(a.x, 0.0, a.y), Vec3::new(b.x, 0.0, b.y), Vec3::new(b.x, h, b.y), Vec3::new(a.x, h, a.y)]
}

/// A 24 × 24 m yard in metres: a textured floor lit by the sun, 3 m walls
/// facing in, a floor pad under a mirrored node (its winding must come back
/// the right way), an invisible barrier (`-colonly`), a floating decal
/// (`-nocol`) with vertex colours, and a spawn marker turned to face +x.
fn write_yard(path: &std::path::Path) {
    let mut g = GlbOut::default();
    let (lo, hi) = (Vec2::splat(-12.0), Vec2::splat(12.0));
    let m = g.quads(&[floor(lo, hi, 0.0)], 0, None);
    g.node(json!({"name": "Floor", "mesh": m}));
    let (a, b, c, d) = (lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y));
    let m = g.quads(&[wall(b, a, 3.0), wall(c, b, 3.0), wall(d, c, 3.0), wall(a, d, 3.0)], 0, None);
    // Under a parent: the two translations cancel.
    let child = g.node(json!({"name": "wall mesh", "mesh": m, "translation": [-1.0, 0.0, 0.0]}));
    g.node(json!({"name": "Walls", "translation": [1.0, 0.0, 0.0], "children": [child]}));
    // A 2 × 2 m plate 0.4 m up at x 6..8 (mirrored in x from -8..-6).
    let m = g.quads(&[floor(Vec2::new(6.0, 6.0), Vec2::new(8.0, 8.0), 0.4)], 0, None);
    g.node(json!({"name": "Plate", "mesh": m, "scale": [-1.0, 1.0, 1.0]}));
    let m = g.quads(&[wall(Vec2::new(-5.0, -12.0), Vec2::new(-5.0, 0.0), 3.0)], 0, None);
    g.node(json!({"name": "Barrier-colonly", "mesh": m}));
    let m = g.quads(&[floor(Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), 2.5)], 1, Some([0.2159, 0.2159, 0.2159, 1.0]));
    g.node(json!({"name": "Leaves-nocol", "mesh": m}));
    // Turned 90° about y: its +z faces +x.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    g.node(json!({"name": "pd_spawn.001", "translation": [-8.0, 0.0, 8.0], "rotation": [0.0, s, 0.0, s]}));
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba([200, 180, 160, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let materials = json!([
        {"name": "ground", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}},
        {"name": "leaves", "doubleSided": true, "alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0]}},
    ]);
    g.write(path, materials, png.get_ref());
}

fn recipe(path: &std::path::Path) -> Recipe {
    Recipe {
        format: "pd-import-recipe/1".into(),
        code: "test_gltf".into(),
        name: "Test glTF".into(),
        stagenum: 0x7e,
        scale: 100.0,
        source: Source::Gltf(GltfSource { path: path.to_string_lossy().into_owned(), sky: None, sun: None, climb_ledges: false, max_texture: 8 }),
        rooms: 4,
        spawns: 4,
        weapons: 3,
        hills: 1,
        marker_share: 1.0,
    }
}

/// The file's geometry in centimetres and PD's terms: the drawn triangles
/// (not the barrier), the tiles (not the decal), the mirrored plate a floor,
/// the sun baked in where there are no vertex colours and linear colours
/// made display ones where there are, the texture halved to the recipe's
/// limit, the clamp wrap kept, the marker facing +x.
#[test]
fn a_gltf_becomes_a_level_source() {
    let dir = std::env::temp_dir().join(format!("pd_import_gltf_src_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yard.glb");
    write_yard(&path);
    let r = recipe(&path);
    let Source::Gltf(g) = &r.source else { unreachable!() };
    let mut report = Vec::new();
    let src = crate::gltf::load(&r, g, &path, &mut report).unwrap();
    // Floor 2 + walls 8 + plate 2 + leaves 2 drawn; the barrier's 2 not.
    assert_eq!(src.tris.len(), 14);
    // Floor 2 + walls 8 + plate 2 + barrier 2 collide; the leaves don't.
    assert_eq!(src.collision.len(), 14);
    let (lo, hi) = src.bounds();
    assert_eq!((lo.x, hi.x, hi.y), (-1200.0, 1200.0, 300.0));
    let floors: Vec<_> = src.collision.iter().filter(|p| p.flags & GEOFLAG_FLOOR1 != 0).collect();
    assert_eq!(floors.len(), 4, "the ground's two and the mirrored plate's two");
    assert!(floors.iter().any(|p| p.verts.iter().all(|v| v.y == 40.0 && (-800.0..=-600.0).contains(&v.x))), "the plate, mirrored to x -8..-6 m");
    let walls = src.collision.iter().filter(|p| p.flags == GEOFLAG_WALL | GEOFLAG_BLOCK_SIGHT | GEOFLAG_BLOCK_SHOOT).count();
    assert_eq!(walls, 10, "the yard's and the barrier's");
    // The floor faces the sun more than the walls: brighter.
    let bright = |y: bool| src.tris.iter().filter(|t| t.material == 0 && (t.v[0].pos.y == t.v[1].pos.y && t.v[1].pos.y == t.v[2].pos.y) == y).map(|t| t.v[0].col[0]).max().unwrap();
    assert!(bright(true) > bright(false));
    let leaves = src.tris.iter().find(|t| t.material == 1).unwrap();
    assert_eq!(leaves.v[0].col, [128, 128, 128, 255], "linear 0.2159 is display 128");
    assert!(leaves.xlu && !src.materials[1].cull_back && src.materials[1].blend == crate::source::Blend::Translucent);
    assert_eq!((src.textures[0].w, src.textures[0].h), (8, 8));
    assert_eq!(src.materials[0].wrap, [crate::source::Wrap::Clamp, crate::source::Wrap::Repeat]);
    assert_eq!(src.markers.len(), 1);
    assert!((src.markers[0].facing - std::f32::consts::FRAC_PI_2).abs() < 1e-4 && src.markers[0].pos == Vec3::new(-800.0, 0.0, 800.0));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The glTF through the whole pipeline into a stage the game loads; then its
/// placement adopted as a layout, one weapon moved and one taken away (with
/// its crates), a crate added to the first, and the level placed again from
/// `source.json` and the cached waypoints: the setup is the layout's, each
/// crate after its weapon.
#[test]
fn a_gltf_level_is_placed_by_its_layout() {
    let dir = std::env::temp_dir().join(format!("pd_import_gltf_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yard.glb");
    write_yard(&path);
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("assets");
    let paths = crate::Paths { assets: assets.clone(), custom: dir.join("custom"), src: None };
    let r = recipe(&path);
    let report = crate::import(&r, &paths, &Layout::new()).unwrap();
    assert!(report.iter().any(|l| l.starts_with("check: a minute")), "{report:#?}");
    let a = pd_core::assets::AssetDir::new(&assets).with_custom_dir(&paths.custom);
    let stage = pd_sim::stage::Stage::load(&a, "test_gltf").unwrap();
    assert_eq!(stage.spawn_pads.len(), 4);
    // A spawn at the marker (snapped to a waypoint within 3 m).
    assert!(stage.spawn_pads.iter().any(|&p| stage.pads[p].pos.distance(Vec3::new(-800.0, 53.0, 800.0)) < 300.0), "a spawn at the marker");

    let (mut layout, skipped) = Layout::adopt(&paths.stage_dir("test_gltf")).unwrap();
    assert!(skipped.is_empty());
    let weapons = layout.weapons.clone().unwrap();
    assert_eq!(weapons.len(), 3);
    assert_eq!(layout.ammo.as_ref().unwrap().len(), 6);
    // Weapon 1 moved to (500, 53, -500), weapon 2 and its crates gone, a
    // third crate for weapon 0.
    let moved = Weapon { pos: [500.0, 53.0, -500.0], location: 7 };
    layout.weapons = Some(vec![weapons[0], moved]);
    let mut ammo: Vec<Ammo> = layout.ammo.clone().unwrap().into_iter().filter(|c| c.weapon < 2).collect();
    ammo.push(Ammo { pos: [-300.0, 53.0, -900.0], weapon: 0 });
    layout.ammo = Some(ammo);
    let lp = dir.join("test_gltf.layout.json");
    layout.save(&lp).unwrap();
    let layout = Layout::load(&lp).unwrap().unwrap();
    let report = crate::replace(&r, &paths, &layout, crate::Finish { check: false, fresh_graph: false }).unwrap();
    assert!(report.iter().any(|l| l.starts_with("waypoints:") && l.ends_with("(cached)")), "{report:#?}");
    assert!(report.iter().any(|l| l == "weapons: 2 locations (the layout's)"), "{report:#?}");

    let stage = pd_sim::stage::Stage::load(&a, "test_gltf").unwrap();
    let rows: Vec<&Value> = stage.props.iter().filter(|p| matches!(p["type"].as_str(), Some("weapon" | "ammocratemulti"))).collect();
    let kinds: String = rows.iter().map(|p| if p["type"] == "weapon" { 'W' } else { 'a' }).collect();
    assert_eq!(kinds, "WaaaWaa", "each crate after its weapon");
    assert_eq!(rows[4]["weapon"].as_u64(), Some(WEAPON_MPLOCATION00 as u64 + 7));
    assert_eq!(stage.pads[rows[4]["chr"].as_u64().unwrap() as usize].pos, Vec3::new(500.0, 53.0, -500.0));
    assert_eq!(stage.pads[rows[3]["pad"].as_u64().unwrap() as usize].pos, Vec3::new(-300.0, 53.0, -900.0));
    // Everything else as adopted: the same spawns.
    let (again, _) = Layout::adopt(&paths.stage_dir("test_gltf")).unwrap();
    assert_eq!(again.spawns, layout.spawns);
    assert_eq!(again.weapons, layout.weapons);
    // The crates as the setup lists them: grouped by weapon.
    let by_weapon = |l: &Layout| {
        let mut v = l.ammo.clone().unwrap();
        v.sort_by_key(|c| c.weapon);
        v
    };
    assert_eq!(by_weapon(&again), by_weapon(&layout));

    // Waypoint edits: one added in the open and linked both ways to the
    // nearest, one generated waypoint removed. The stage has them.
    let before = stage.waypoints.len();
    let p = |w: usize| stage.waypoint_pos(w);
    let spot = Vec3::new(300.0, 53.0, 300.0);
    let near = (0..before).min_by(|&a, &b| p(a).distance(spot).total_cmp(&p(b).distance(spot))).unwrap();
    let gone = (0..before).find(|&w| w != near && p(w).distance(p(near)) > 500.0).unwrap();
    let mut layout = layout;
    layout.waypoints = Some(crate::layout::WaypointEdits {
        removed: vec![p(gone).to_array()],
        added: vec![spot.to_array()],
        linked: vec![crate::layout::Link { a: spot.to_array(), b: p(near).to_array(), one_way: false }],
        ..Default::default()
    });
    let report = crate::replace(&r, &paths, &layout, crate::Finish { check: false, fresh_graph: false }).unwrap();
    assert!(report.iter().any(|l| l.starts_with("waypoint edits: 1 removed, 0 moved, 1 added, 0 links cut, 1 made") && !l.contains("found no")), "{report:#?}");
    let stage = pd_sim::stage::Stage::load(&a, "test_gltf").unwrap();
    assert_eq!(stage.waypoints.len(), before);
    let added = (0..stage.waypoints.len()).find(|&w| stage.waypoint_pos(w).distance(spot) < 1.0).expect("the added waypoint");
    let links = stage.waypoint_links();
    let near_now = (0..stage.waypoints.len()).find(|&w| stage.waypoint_pos(w).distance(p(near)) < 1.0).unwrap();
    assert!(links.iter().any(|&(x, y, _)| x == added && y == near_now) && links.iter().any(|&(x, y, _)| x == near_now && y == added));
    std::fs::remove_dir_all(&dir).unwrap();
}
