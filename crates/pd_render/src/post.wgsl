// PD's framebuffer effects (`bondview.c`), each a full-screen pass over a copy
// of a frame. PD redraws every screen line with `bview_copy_pixels`: a
// one-line textured rectangle, zoomed horizontally about the centre by
// `scale`, times the env colour, blended by `G_RM_CLD_SURF`. Here each pixel
// works out which of the view's `lines` it is on and samples the copy the same
// way. The target is display-space (not sRGB), as the N64's framebuffer is.
//
// mode 0: `bview_draw_slayer_rocket_interlace` (`bondview.c:331`): this frame,
//         line y at scale 2 - sin(angle), angle 30°..150° down the view; env
//         (ff,ff,00) / (ff,ff,bf) in 8-line bands scrolled by `offset`.
// mode 1: `bview_draw_static` (`:303`): a random row of RDRAM per line, read as
//         I8, × env (4f,ff,ff) at alpha.
// mode 2: `bview_draw_zoom_blur` (`:429`): the last frame, line
//         (lines − lines/sy)/2 + i/sy for line i, zoomed by sx, at alpha.
// mode 3: `player_draw_fade` (`player.c:2261`): a flat colour at alpha.

struct Post {
    params: vec4<f32>, // mode, lines, offset / seed, alpha
    zoom: vec4<f32>,   // sx, sy, width (PD pixels), _
    col: vec4<f32>,    // the fade's colour
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    var o: VOut;
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    o.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// A copied line zoomed about the centre: s starts at (w − w/scale)/2 and steps 1/scale.
fn zoomed_u(u: f32, scale: f32) -> f32 {
    return 0.5 + (u - 0.5) / scale;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let mode = u32(post.params.x);
    let lines = post.params.y;
    let line = floor(in.uv.y * lines);
    if (mode == 0u) {
        let angle = radians(30.0) + (radians(150.0) - radians(30.0)) * line / lines;
        let u = zoomed_u(in.uv.x, 2.0 - sin(angle));
        let texel = textureSample(src, samp, vec2<f32>(u, (line + 0.5) / lines)).rgb;
        // The env changes on every 8th line of y − offset; the lines before the
        // first change keep the first line's yellow (C's % goes negative).
        let offsety = line - post.params.z;
        var env = vec3<f32>(1.0, 1.0, 0.0);
        if (offsety >= 0.0 && (offsety - 16.0 * floor(offsety / 16.0)) >= 8.0) {
            env = vec3<f32>(1.0, 1.0, 191.0 / 255.0);
        }
        return vec4<f32>(texel * env, 1.0);
    }
    if (mode == 1u) {
        let row = floor(hash(vec2<f32>(line, post.params.z)) * 4096.0);
        let x = floor(in.uv.x * post.zoom.z);
        let n = hash(vec2<f32>(x + row * 0.37, row));
        return vec4<f32>(vec3<f32>(n) * vec3<f32>(79.0 / 255.0, 1.0, 1.0), post.params.w);
    }
    if (mode == 3u) {
        return vec4<f32>(post.col.rgb, post.params.w);
    }
    let sy = post.zoom.y;
    let srcline = floor((lines - lines / sy) * 0.5 + line / sy);
    let texel = textureSample(src, samp, vec2<f32>(zoomed_u(in.uv.x, post.zoom.x), (srcline + 0.5) / lines)).rgb;
    return vec4<f32>(texel, post.params.w);
}
