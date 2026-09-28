// The N64's vertex stage and colour combiner for textured, lit or vertex-coloured
// triangles, drawn the way fast3d (pd-pcport/port/fast3d/gfx_pc.cpp) draws a
// display list:
//   * vertices transformed by the matrix slot the display list loaded
//     (`joints[mtx]`, model -> eye space), then the frame's projection;
//   * RSP lighting in the vertex stage: ambient + one directional light whose
//     direction is taken into model space by the transposed modelview, and
//     G_TEXTURE_GEN from the LookAt vectors (gfx_pc.cpp:1075-1150);
//   * the RDP colour combiner, one or two cycles, evaluated from its 16 mux ids;
//   * the cycle-1 FOG_PRIM_A blender tint, the alpha compares.
// All maths runs on the raw 0..1 values the N64 puts on screen, and the result
// is written as it is: the target is not sRGB, so blending also happens on
// display-space values, as the RDP's blender does.
//
// Source: the old repo's pd_guns/pdgun.wgsl, with its final sRGB-to-linear
// step removed (the spike drew into an sRGB swapchain).

struct Frame {
    proj: mat4x4<f32>,
    ambient: vec4<f32>,   // 0..255
    diffuse: vec4<f32>,   // 0..255
    light_dir: vec4<f32>, // raw dir / 127, eye space
    lookat_x: vec4<f32>,
    lookat_y: vec4<f32>,
    envcol: vec4<f32>,    // the render context's env colour (0..1)
    flat: vec4<f32>,      // flat colour override + alpha (alpha 0 = off)
    misc: vec4<f32>,      // x: env-alpha override (0 = off), y: 3-point filter
};

struct Material {
    cc0: vec4<u32>,  // a0 b0 c0 d0
    ac0: vec4<u32>,  // Aa0 Ab0 Ac0 Ad0
    cc1: vec4<u32>,
    ac1: vec4<u32>,
    prim: vec4<f32>,
    env: vec4<f32>,
    fog: vec4<f32>,
    tex: vec4<f32>,      // width, height, uls, ult
    shift: vec4<f32>,    // shift scale s, t, has_texture, two_cycle
    flags: vec4<u32>,    // alpha_test (0 none, 1 edge, 2 threshold), fog_tint, env_from_frame, fog_from_frame
    flags2: vec4<u32>,   // texgen_linear, unused, translucent material, bilerp (G_TF_BILERP)
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> joints: array<mat4x4<f32>>;
@group(1) @binding(0) var<uniform> mat: Material;
@group(1) @binding(1) var tex0: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) col: vec4<f32>,
    @location(3) mtx: u32,
    @location(4) vflags: u32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) st: vec2<f32>,
    @location(1) shade: vec4<f32>,
};

fn signed_byte(b: f32) -> f32 {
    return select(b, b - 256.0, b > 127.0);
}

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let m = joints[v.mtx];
    let eye = m * vec4<f32>(v.pos, 1.0);
    o.clip = frame.proj * eye;
    var st = v.uv;
    if ((v.vflags & 1u) != 0u) {
        // G_LIGHTING: the colour bytes are a normal (signed), alpha stays.
        let n = vec3<f32>(signed_byte(v.col.x), signed_byte(v.col.y), signed_byte(v.col.z));
        let m3t = transpose(mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz));
        let coeffs = normalize(m3t * frame.light_dir.xyz);
        var c = frame.ambient.rgb;
        let inten = dot(n, coeffs) / 127.0;
        if (inten > 0.0) {
            c = c + inten * frame.diffuse.rgb;
        }
        c = min(c, vec3<f32>(255.0));
        o.shade = vec4<f32>(c / 255.0, v.col.w / 255.0);
        if ((v.vflags & 2u) != 0u) {
            let cx = normalize(m3t * frame.lookat_x.xyz);
            let cy = normalize(m3t * frame.lookat_y.xyz);
            var dx = clamp(dot(n, cx) / 127.0, -1.0, 1.0);
            var dy = clamp(dot(n, cy) / 127.0, -1.0, 1.0);
            if (mat.flags2.x != 0u) {
                dx = acos(-dx) / 4.0;
                dy = acos(-dy) / 4.0;
            } else {
                dx = (dx + 1.0) / 4.0;
                dy = (dy + 1.0) / 4.0;
            }
            // v.uv holds the texture scale / 32 → texels (U = dot * scale, S10.5).
            st = vec2<f32>(dx * v.uv.x, dy * v.uv.y);
        }
    } else {
        o.shade = v.col / 255.0;
    }
    o.st = st;
    return o;
}

struct Inputs {
    combined: vec4<f32>,
    t0: vec4<f32>,
    t1: vec4<f32>,
    shade: vec4<f32>,
    prim: vec4<f32>,
    env: vec4<f32>,
    lod: f32,
};

fn cc_a(s: u32, i: Inputs) -> vec3<f32> {
    switch s {
        case 0u: { return i.combined.rgb; }
        case 1u: { return i.t0.rgb; }
        case 2u: { return i.t1.rgb; }
        case 3u: { return i.prim.rgb; }
        case 4u: { return i.shade.rgb; }
        case 5u: { return i.env.rgb; }
        case 6u: { return vec3<f32>(1.0); }
        default: { return vec3<f32>(0.0); }
    }
}

fn cc_b(s: u32, i: Inputs) -> vec3<f32> {
    switch s {
        case 0u: { return i.combined.rgb; }
        case 1u: { return i.t0.rgb; }
        case 2u: { return i.t1.rgb; }
        case 3u: { return i.prim.rgb; }
        case 4u: { return i.shade.rgb; }
        case 5u: { return i.env.rgb; }
        default: { return vec3<f32>(0.0); }
    }
}

fn cc_c(s: u32, i: Inputs) -> vec3<f32> {
    switch s {
        case 0u: { return i.combined.rgb; }
        case 1u: { return i.t0.rgb; }
        case 2u: { return i.t1.rgb; }
        case 3u: { return i.prim.rgb; }
        case 4u: { return i.shade.rgb; }
        case 5u: { return i.env.rgb; }
        case 7u: { return vec3<f32>(i.combined.a); }
        case 8u: { return vec3<f32>(i.t0.a); }
        case 9u: { return vec3<f32>(i.t1.a); }
        case 10u: { return vec3<f32>(i.prim.a); }
        case 11u: { return vec3<f32>(i.shade.a); }
        case 12u: { return vec3<f32>(i.env.a); }
        case 13u: { return vec3<f32>(i.lod); }
        default: { return vec3<f32>(0.0); }
    }
}

fn cc_d(s: u32, i: Inputs) -> vec3<f32> {
    switch s {
        case 0u: { return i.combined.rgb; }
        case 1u: { return i.t0.rgb; }
        case 2u: { return i.t1.rgb; }
        case 3u: { return i.prim.rgb; }
        case 4u: { return i.shade.rgb; }
        case 5u: { return i.env.rgb; }
        case 6u: { return vec3<f32>(1.0); }
        default: { return vec3<f32>(0.0); }
    }
}

fn ac_abd(s: u32, i: Inputs) -> f32 {
    switch s {
        case 0u: { return i.combined.a; }
        case 1u: { return i.t0.a; }
        case 2u: { return i.t1.a; }
        case 3u: { return i.prim.a; }
        case 4u: { return i.shade.a; }
        case 5u: { return i.env.a; }
        case 6u: { return 1.0; }
        default: { return 0.0; }
    }
}

fn ac_c(s: u32, i: Inputs) -> f32 {
    switch s {
        case 0u: { return i.lod; }
        case 1u: { return i.t0.a; }
        case 2u: { return i.t1.a; }
        case 3u: { return i.prim.a; }
        case 4u: { return i.shade.a; }
        case 5u: { return i.env.a; }
        default: { return 0.0; }
    }
}

fn combine(cc: vec4<u32>, ac: vec4<u32>, i: Inputs) -> vec4<f32> {
    let rgb = (cc_a(cc.x, i) - cc_b(cc.y, i)) * cc_c(cc.z, i) + cc_d(cc.w, i);
    let a = (ac_abd(ac.x, i) - ac_abd(ac.y, i)) * ac_c(ac.z, i) + ac_abd(ac.w, i);
    return clamp(vec4<f32>(rgb, a), vec4<f32>(0.0), vec4<f32>(1.0));
}

// The RDP's 3-point filter on one mip level (see the engine's
// `shader_textured.wgsl`): every tap is a texel centre, so the sampler only
// contributes its wrap/clamp/mirror mode.
fn three_point_level(uv: vec2<f32>, level: u32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(tex0, level));
    let lf = f32(level);
    var off = fract(uv * size - vec2<f32>(0.5));
    off = off - step(1.0, off.x + off.y);
    let c0 = textureSampleLevel(tex0, samp, uv - off / size, lf);
    let c1 = textureSampleLevel(tex0, samp, uv - vec2<f32>(off.x - sign(off.x), off.y) / size, lf);
    let c2 = textureSampleLevel(tex0, samp, uv - vec2<f32>(off.x, off.y - sign(off.y)) / size, lf);
    return c0 + abs(off.x) * (c1 - c0) + abs(off.y) * (c2 - c0);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var t0 = vec4<f32>(1.0);
    if (mat.shift.z != 0.0) {
        // Tile coordinates: texels × 2^-shift − (uls, ult), over the tile size.
        let st = in.st * mat.shift.xy - mat.tex.zw;
        let uv = st / mat.tex.xy;
        t0 = textureSample(tex0, samp, uv);
        if (frame.misc.y != 0.0 && mat.flags2.w != 0u) {
            // N64 video: 3-point per level; mipmapped textures blend the two
            // nearest levels by the hardware's LOD, as TRILERP does.
            let size0 = vec2<f32>(textureDimensions(tex0, 0u));
            let texels = uv * size0;
            let rho = max(length(dpdx(texels)), length(dpdy(texels)));
            let top = f32(textureNumLevels(tex0) - 1u);
            let lod = clamp(log2(max(rho, 1e-6)), 0.0, top);
            let l0 = u32(floor(lod));
            let l1 = min(l0 + 1u, u32(top));
            t0 = mix(three_point_level(uv, l0), three_point_level(uv, l1), fract(lod));
        }
    }
    var i: Inputs;
    i.combined = vec4<f32>(0.0);
    i.t0 = t0;
    // TRILERP's TEXEL1 is the next LOD; the sampler's trilinear filter already
    // blends the levels, so TEXEL1 = TEXEL0 and LOD_FRACTION is moot.
    i.t1 = t0;
    i.shade = in.shade;
    i.prim = mat.prim;
    i.env = select(mat.env, frame.envcol, mat.flags.z != 0u);
    i.lod = 0.0;
    var c = combine(mat.cc0, mat.ac0, i);
    if (mat.shift.w != 0.0) {
        i.combined = c;
        c = combine(mat.cc1, mat.ac1, i);
    }
    // Texture-edge / threshold alpha compare (fast3d's rules).
    if (mat.flags.x == 1u) {
        if (c.a > 0.19) { c.a = 1.0; } else { discard; }
    } else if (mat.flags.x == 2u) {
        if (c.a < 8.0 / 256.0) { discard; }
    }
    // Cycle-1 blender G_RM_FOG_PRIM_A: CLR_FOG·A_FOG + CLR_IN·(1−A_FOG).
    if (mat.flags.y != 0u) {
        let fog = select(mat.fog, frame.envcol, mat.flags.w != 0u);
        c = vec4<f32>(mix(c.rgb, fog.rgb, fog.a), c.a);
    }
    // Env-alpha override: a render context that swaps every combiner's alpha
    // for the env alpha (opaque materials) or texel alpha x env alpha
    // (translucent ones), times shade alpha in cycle 2.
    if (frame.misc.x > 0.0) {
        let a = select(frame.misc.x, t0.a * frame.misc.x, mat.flags2.z != 0u);
        c = vec4<f32>(c.rgb, a * in.shade.a);
    }
    // Flat-colour override: the blender's fog colour at full weight replaces
    // the colour, and its alpha scales the coverage.
    if (frame.flat.a > 0.0) {
        c = vec4<f32>(frame.flat.rgb, c.a * frame.flat.a);
    }
    return c;
}
