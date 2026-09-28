// PD's effects: beams (G_CC_BLENDIA), sparks (G_CC_CUSTOM_04), wallhits and
// smoke (IA texel × shade), explosion flares (two textures × shade) and flat
// vertex-coloured geometry. Positions are world centimetres; `mode` picks the
// combiner. The target is not sRGB: values are written as the RDP writes them.

struct Pass {
    view_proj: mat4x4<f32>,
    env: vec4<f32>,
};

struct Draw {
    tex_size: vec4<f32>, // w, h, mode, _
};

@group(0) @binding(0) var<uniform> pass_u: Pass;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(1) @binding(1) var tex0: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var tex1: texture_2d<f32>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) st: vec2<f32>,
    @location(2) col: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) col: vec4<f32>,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.clip = pass_u.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv = v.st / draw.tex_size.xy;
    o.col = v.col;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex0, samp, in.uv);
    let t1 = textureSample(tex1, samp, in.uv);
    let mode = u32(draw.tex_size.z);
    var c: vec4<f32>;
    switch mode {
        // BLENDIA: (ENV − SHADE)·TEXEL0 + SHADE; alpha TEXEL0·SHADE.
        case 0u: { c = vec4<f32>((pass_u.env.rgb - in.col.rgb) * t.rgb + in.col.rgb, t.a * in.col.a); }
        // CUSTOM_04: colour SHADE; alpha TEXEL0·SHADE.
        case 1u: { c = vec4<f32>(in.col.rgb, t.a * in.col.a); }
        // Wallhit / smoke: MODULATEIA.
        case 2u: { c = t * in.col; }
        // Explosion: INTERFERENCE (TEXEL0·TEXEL1) then MODULATEIA2 (× SHADE).
        case 4u: { c = t * t1 * in.col; }
        default: { c = in.col; }
    }
    if (c.a <= 0.0) {
        discard;
    }
    return c;
}
