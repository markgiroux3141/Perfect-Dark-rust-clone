//! Quake 3 engine shader scripts (`shaders/*.shader`): what a surface's
//! texture name means. Only what a PD material can be is read: the surface
//! parameters, the culling, and each stage's image, blend, alpha test and
//! colour source; the rest (waves, scrolling, deforms) is skipped.

use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    /// The image (`map`, `clampMap`, `animMap`'s first frame), or `$lightmap`
    /// / `$whiteimage`.
    pub map: String,
    pub clamp: bool,
    /// `blendFunc`'s source and destination factors (`GL_ONE`, ...), the
    /// shorthands spelled out.
    pub blend: Option<(String, String)>,
    /// `alphaFunc` (`GT0`, `LT128`, `GE128`).
    pub alpha_func: Option<String>,
    /// `tcGen environment`: a reflection, not the surface's own picture.
    pub environment: bool,
    /// `rgbGen vertex` / `exactVertex`: lit by the vertex colours.
    pub vertex_colour: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shader {
    /// `surfaceparm`s, lower case (`nodraw`, `nonsolid`, `trans`, `sky`,
    /// `water`, `playerclip`, ...).
    pub parms: Vec<String>,
    /// `cull none` / `disable` / `twosided` (or `back`): both sides drawn.
    pub two_sided: bool,
    /// `q3map_material` (Jedi Academy: `Rock`, `SolidMetal`, ...).
    pub material: Option<String>,
    /// `skyParms`' far box (`textures/skies/x`: its `_ft` ... images).
    pub sky_box: Option<String>,
    pub stages: Vec<Stage>,
}

impl Shader {
    pub fn has(&self, parm: &str) -> bool {
        self.parms.iter().any(|p| p == parm)
    }

    /// The stage that is the surface's own picture: the first that is
    /// neither the lightmap nor a reflection.
    pub fn main_stage(&self) -> Option<(usize, &Stage)> {
        self.stages.iter().enumerate().find(|(_, s)| !s.environment && !s.map.starts_with('$') && !s.map.is_empty())
    }
}

/// `blendFunc`'s shorthands (`tr_shader.c`, `NameToSrcBlendMode`).
fn blend_of(args: &[&str]) -> Option<(String, String)> {
    let up = |s: &str| s.to_ascii_uppercase();
    match args {
        [a] if a.eq_ignore_ascii_case("add") => Some(("GL_ONE".into(), "GL_ONE".into())),
        [a] if a.eq_ignore_ascii_case("filter") => Some(("GL_DST_COLOR".into(), "GL_ZERO".into())),
        [a] if a.eq_ignore_ascii_case("blend") => Some(("GL_SRC_ALPHA".into(), "GL_ONE_MINUS_SRC_ALPHA".into())),
        [a, b, ..] => Some((up(a), up(b))),
        _ => None,
    }
}

/// The lines of `text` as tokens: comments dropped, braces their own tokens,
/// quotes stripped.
fn lines(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let mut line = String::new();
        let mut rest = raw;
        loop {
            if in_block {
                match rest.find("*/") {
                    Some(i) => {
                        rest = &rest[i + 2..];
                        in_block = false;
                    }
                    None => break,
                }
            } else {
                let (lc, bc) = (rest.find("//"), rest.find("/*"));
                match (lc, bc) {
                    (Some(l), b) if b.is_none_or(|b| l < b) => {
                        line.push_str(&rest[..l]);
                        break;
                    }
                    (_, Some(b)) => {
                        line.push_str(&rest[..b]);
                        line.push(' ');
                        rest = &rest[b + 2..];
                        in_block = true;
                    }
                    _ => {
                        line.push_str(rest);
                        break;
                    }
                }
            }
        }
        let spaced = line.replace('{', " { ").replace('}', " } ").replace('"', " ");
        let toks: Vec<String> = spaced.split_whitespace().map(str::to_owned).collect();
        if !toks.is_empty() {
            out.push(toks);
        }
    }
    out
}

/// Every shader defined in `text`, by lower-case name; a name defined twice
/// keeps its first definition.
pub fn parse(text: &str, into: &mut HashMap<String, Shader>) {
    let mut depth = 0;
    let mut name: Option<String> = None;
    let mut cur = Shader::default();
    let mut stage = Stage::default();
    for toks in lines(text) {
        let mut i = 0;
        while i < toks.len() {
            let t = toks[i].as_str();
            match t {
                "{" => {
                    depth += 1;
                    if depth == 2 {
                        stage = Stage::default();
                    }
                    i += 1;
                    continue;
                }
                "}" => {
                    if depth == 2 {
                        cur.stages.push(std::mem::take(&mut stage));
                    } else if depth == 1 {
                        if let Some(n) = name.take() {
                            into.entry(n).or_insert(std::mem::take(&mut cur));
                        }
                        cur = Shader::default();
                    }
                    depth = (depth - 1).max(0);
                    i += 1;
                    continue;
                }
                _ => {}
            }
            // A directive: the rest of the line up to a brace.
            let end = toks[i..].iter().position(|x| x == "{" || x == "}").map_or(toks.len(), |p| i + p);
            let args: Vec<&str> = toks[i + 1..end].iter().map(String::as_str).collect();
            let key = t.to_ascii_lowercase();
            match depth {
                0 => name = Some(t.replace('\\', "/").to_ascii_lowercase()),
                1 => match key.as_str() {
                    "surfaceparm" => cur.parms.extend(args.first().map(|a| a.to_ascii_lowercase())),
                    "cull" => cur.two_sided = args.first().is_some_and(|a| matches!(a.to_ascii_lowercase().as_str(), "none" | "disable" | "twosided" | "back" | "backside" | "backsided")),
                    "q3map_material" => cur.material = args.first().map(|a| a.to_string()),
                    "skyparms" => cur.sky_box = args.first().filter(|a| **a != "-").map(|a| a.replace('\\', "/").to_ascii_lowercase()),
                    _ => {}
                },
                2 => match key.as_str() {
                    "map" | "clampmap" => {
                        stage.map = args.first().map(|a| a.replace('\\', "/").to_ascii_lowercase()).unwrap_or_default();
                        stage.clamp = key == "clampmap";
                    }
                    // animMap <frequency> <frame> ...: the first frame.
                    "animmap" | "clampanimmap" => stage.map = args.get(1).map(|a| a.replace('\\', "/").to_ascii_lowercase()).unwrap_or_default(),
                    "blendfunc" => stage.blend = blend_of(&args),
                    "alphafunc" => stage.alpha_func = args.first().map(|a| a.to_ascii_uppercase()),
                    "tcgen" | "texgen" => stage.environment = args.first().is_some_and(|a| a.eq_ignore_ascii_case("environment")),
                    "rgbgen" => stage.vertex_colour = args.first().is_some_and(|a| a.eq_ignore_ascii_case("vertex") || a.eq_ignore_ascii_case("exactvertex")),
                    _ => {}
                },
                _ => {}
            }
            i = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_parse() {
        let text = r#"
// a comment
textures/korriban/k_basicfloor
{
	q3map_material	Rock
    {
        map $lightmap
    }
    {
        map textures/korriban/k_basicfloor  // trailing
        blendFunc GL_DST_COLOR GL_ZERO
    }
}
textures/korriban/os_basic_pillarb
{
	qer_editorimage	textures/korriban/os_basic_pillarb
    { map textures/common/etest4
        tcGen environment }
    {
        map textures/korriban/os_basic_pillarb
        blendFunc blend
    }
    /* a block
       comment { } */
    {
        map $lightmap
        blendFunc filter
    }
}
textures/system/caulk
{
	surfaceparm	nomarks
	surfaceparm	nodraw
	q3map_nolightmap
}
textures/x/vines
{
	surfaceparm nonsolid
	cull none
	{
		clampMap "textures/X/Vines.tga"
		alphaFunc GE128
		rgbGen vertex
	}
}
textures/korriban/k_basicfloor
{
	{ map textures/other }
}
"#;
        let mut m = HashMap::new();
        parse(text, &mut m);
        assert_eq!(m.len(), 4);
        let f = &m["textures/korriban/k_basicfloor"];
        assert_eq!(f.material.as_deref(), Some("Rock"));
        assert_eq!(f.stages.len(), 2, "the first definition kept");
        assert_eq!(f.main_stage().unwrap().0, 1);
        assert_eq!(f.stages[1].blend, Some(("GL_DST_COLOR".into(), "GL_ZERO".into())));
        let p = &m["textures/korriban/os_basic_pillarb"];
        assert_eq!(p.stages.len(), 3);
        assert!(p.stages[0].environment);
        let (i, s) = p.main_stage().unwrap();
        assert_eq!((i, s.map.as_str()), (1, "textures/korriban/os_basic_pillarb"));
        assert_eq!(s.blend, Some(("GL_SRC_ALPHA".into(), "GL_ONE_MINUS_SRC_ALPHA".into())));
        assert!(m["textures/system/caulk"].has("nodraw") && m["textures/system/caulk"].main_stage().is_none());
        let v = &m["textures/x/vines"];
        assert!(v.two_sided && v.has("nonsolid"));
        assert_eq!(v.stages[0], Stage { map: "textures/x/vines.tga".into(), clamp: true, blend: None, alpha_func: Some("GE128".into()), environment: false, vertex_colour: true });
    }
}
