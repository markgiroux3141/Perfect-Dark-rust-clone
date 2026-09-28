//! PD's maths, bugs included: the `M_BADPI` angle constants and the `BADDTOR`
//! family, the `lib/mtx.c` matrix helpers on glam's `Mat4`, PD's `atan2f`, and
//! the euler → quaternion conversion the model code uses.
//!
//! PD's `M_BADPI` is `3.141092641`, wrong in the fourth decimal, and most of the
//! game wraps angles at `BADDTOR(360)` (≈ 6.28218) rather than 2π, while `atan2f`
//! returns true radians. That mismatch is part of how PD behaves (a 0.03° seam;
//! it moves the bots' trigger cone), so every constant here is PD's.
//!
//! PD's `Mtxf` is `f32 m[4][4]` with `m[3]` the translation column: the same
//! column-major layout as glam, so `m[i]` is `col(i)`. `mtx00015be4(a, b, dst)`
//! and `mtx4_mult_mtx4(a, b, dst)` are both `dst = a · b` (checked against the C:
//! the former just forces the bottom row to (0,0,0,1)). The anonymous
//! `mtx000xxxxx` helpers are named by what they do, with the address in the doc.
//!
//! Sources: the old repo's `pd_spike/pdmath.rs` (angles), `pd_guns/pdmtx.rs`
//! (matrices), `pd_guns/props.rs` `pd_atan2f` and `pd_guns/model.rs` `euler_quat`.

use glam::{Mat4, Quat, Vec3, Vec4};

// ─── angles ──────────────────────────────────────────────────────────────────

/// `M_BADPI` (`math.h:5`).
pub const M_BADPI: f32 = 3.141_092_641;
/// `M_BADTAU`.
pub const M_BADTAU: f32 = M_BADPI * 2.0;

/// `BADDTOR(deg)` = `deg * M_BADPI / 180`.
#[inline]
pub fn baddtor(deg: f32) -> f32 {
    deg * M_BADPI / 180.0
}

/// `BADDTOR2(deg)` = `deg * (M_BADPI / 180)`: the same value, rounded differently in C.
#[inline]
pub fn baddtor2(deg: f32) -> f32 {
    deg * (M_BADPI / 180.0)
}

/// `BADRTOD4(rad)` = `rad * 360 / M_BADTAU` (`math.h:25`): how the walk turns
/// radians back into `vv_theta` degrees.
#[inline]
pub fn badrtod4(rad: f32) -> f32 {
    rad * 360.0 / M_BADTAU
}

/// `DTOR(deg)`: a true degree conversion.
#[inline]
pub fn dtor(deg: f32) -> f32 {
    deg * std::f32::consts::PI / 180.0
}

/// A full turn in PD's wrapping unit, `BADDTOR(360)`.
#[inline]
pub fn turn() -> f32 {
    baddtor(360.0)
}

/// Wrap into `[0, BADDTOR(360))` the way PD's `while` loops do.
pub fn wrap_pos(mut a: f32) -> f32 {
    let t = turn();
    while a >= t {
        a -= t;
    }
    while a < 0.0 {
        a += t;
    }
    a
}

/// PD's `atan2f(x, z)` (`atan2f.c`): the angle from +z towards +x, in `[0, 2π)`
/// true radians.
pub fn atan2f(x: f32, z: f32) -> f32 {
    let a = x.atan2(z);
    if a < 0.0 {
        a + std::f32::consts::TAU
    } else {
        a
    }
}

// ─── matrices ────────────────────────────────────────────────────────────────

/// `mtx4_load_rotation` (`mtx.c:187`): `Rz(z) · Ry(y) · Rx(x)`.
pub fn load_rotation(rot: Vec3) -> Mat4 {
    let (xs, xc) = rot.x.sin_cos();
    let (ys, yc) = rot.y.sin_cos();
    let (zs, zc) = rot.z.sin_cos();
    let a = xs * zs;
    let b = xc * zs;
    let c = xs * zc;
    let d = xc * zc;
    Mat4::from_cols(
        Vec4::new(yc * zc, yc * zs, -ys, 0.0),
        Vec4::new(c * ys - xc * zs, a * ys + xc * zc, xs * yc, 0.0),
        Vec4::new(d * ys + xs * zs, b * ys - xs * zc, xc * yc, 0.0),
        Vec4::W,
    )
}

/// `mtx4_load_rotation_and_translation`.
pub fn load_rotation_translation(pos: Vec3, rot: Vec3) -> Mat4 {
    let mut m = load_rotation(rot);
    m.w_axis = pos.extend(1.0);
    m
}

/// `mtx4_load_x_rotation`: identical to glam's.
pub fn load_x_rotation(a: f32) -> Mat4 {
    Mat4::from_rotation_x(a)
}

/// `mtx4_load_y_rotation`.
pub fn load_y_rotation(a: f32) -> Mat4 {
    Mat4::from_rotation_y(a)
}

/// `mtx4_load_z_rotation`.
pub fn load_z_rotation(a: f32) -> Mat4 {
    Mat4::from_rotation_z(a)
}

/// `mtx4_set_translation`.
pub fn set_translation(m: &mut Mat4, pos: Vec3) {
    m.w_axis = Vec4::new(pos.x, pos.y, pos.z, m.w_axis.w);
}

/// `mtx00015be4(a, b, dst)`: `dst = a · b` with the affine bottom row.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = *a * *b;
    r.x_axis.w = 0.0;
    r.y_axis.w = 0.0;
    r.z_axis.w = 0.0;
    r.w_axis.w = 1.0;
    r
}

/// `mtx00015f04(mult, m)`: scale the three basis columns (incl. their w).
pub fn scale3(m: &mut Mat4, s: f32) {
    m.x_axis *= s;
    m.y_axis *= s;
    m.z_axis *= s;
}

/// `mtx00015f4c(mult, m)`: scale the 3×3 only (the basis columns' xyz).
pub fn scale3x3(m: &mut Mat4, s: f32) {
    for c in [&mut m.x_axis, &mut m.y_axis, &mut m.z_axis] {
        c.x *= s;
        c.y *= s;
        c.z *= s;
    }
}

/// `mtx00015e24(mult, m)`: scale column 0's xyz (the DUALFLIP mirror).
pub fn scale_col0_xyz(m: &mut Mat4, s: f32) {
    m.x_axis.x *= s;
    m.x_axis.y *= s;
    m.x_axis.z *= s;
}

/// `mtx00015df0`: column 0 incl. w.
pub fn scale_col0(m: &mut Mat4, s: f32) {
    m.x_axis *= s;
}

/// `mtx00015e4c`: column 1 incl. w.
pub fn scale_col1(m: &mut Mat4, s: f32) {
    m.y_axis *= s;
}

/// `mtx00015ea8`: column 2 incl. w.
pub fn scale_col2(m: &mut Mat4, s: f32) {
    m.z_axis *= s;
}

/// `mtx00015e80`: column 1 xyz.
pub fn scale_col1_xyz(m: &mut Mat4, s: f32) {
    m.y_axis.x *= s;
    m.y_axis.y *= s;
    m.y_axis.z *= s;
}

/// `mtx4_get_rotation` (`mtx.c:223`): the euler angles `load_rotation` would
/// need to build `m`'s rotation.
pub fn mtx4_get_rotation(m: &Mat4) -> Vec3 {
    const EPSILON: f32 = 0.000_001_907_348_6;
    let e = |c: usize, r: usize| m.col(c)[r];
    let (sin_x_cos_y, cos_x_cos_y) = (e(1, 2), e(2, 2));
    let norm = (sin_x_cos_y * sin_x_cos_y + cos_x_cos_y * cos_x_cos_y).sqrt();
    if EPSILON < norm {
        Vec3::new(atan2f(e(1, 2), e(2, 2)), atan2f(-e(0, 2), norm), atan2f(e(0, 1), e(0, 0)))
    } else {
        Vec3::new(0.0, atan2f(-e(0, 2), norm), atan2f(-e(1, 0), e(1, 1)))
    }
}

/// `mtx4_load_rotation_from` (`mtx.c:514`): the 3×3's transpose (a rotation's
/// inverse), no translation.
pub fn load_rotation_from(m: &Mat4) -> Mat4 {
    let mut r = Mat4::from_mat3(glam::Mat3::from_mat4(*m).transpose());
    r.w_axis = Vec4::W;
    r
}

/// `mtx00015edc`: column 2 xyz.
pub fn scale_col2_xyz(m: &mut Mat4, s: f32) {
    m.z_axis.x *= s;
    m.z_axis.y *= s;
    m.z_axis.z *= s;
}

/// `mtx00016710(mult, m)`: scale ROW 2 (every column's z), i.e. `diag(1,1,s,1) · m`.
pub fn scale_row2(m: &mut Mat4, s: f32) {
    m.x_axis.z *= s;
    m.y_axis.z *= s;
    m.z_axis.z *= s;
    m.w_axis.z *= s;
}

/// `mtx4_transform_vec`.
pub fn transform(m: &Mat4, v: Vec3) -> Vec3 {
    m.transform_point3(v)
}

/// `mtx4_rotate_vec`.
pub fn rotate(m: &Mat4, v: Vec3) -> Vec3 {
    m.transform_vector3(v)
}

/// `mtx00016b58` (`mtx.c:354`): a camera-style basis from a look direction and
/// an up vector, with `pos` as the translation. Column 2 is the NEGATED,
/// normalised look; column 0 is `up × look'`, column 1 re-orthogonalised up.
pub fn look_basis(pos: Vec3, look: Vec3, up: Vec3) -> Mat4 {
    let mut look = look;
    let tmp = -1.0 / look.length();
    look *= tmp;
    let mut a = up.y * look.z - up.z * look.y;
    let mut b = up.z * look.x - up.x * look.z;
    let mut c = up.x * look.y - up.y * look.x;
    let tmp = 1.0 / (a * a + b * b + c * c).sqrt();
    a *= tmp;
    b *= tmp;
    c *= tmp;
    let mut ux = look.y * c - look.z * b;
    let mut uy = look.z * a - look.x * c;
    let mut uz = look.x * b - look.y * a;
    let tmp = 1.0 / (ux * ux + uy * uy + uz * uz).sqrt();
    ux *= tmp;
    uy *= tmp;
    uz *= tmp;
    Mat4::from_cols(
        Vec4::new(a, b, c, 0.0),
        Vec4::new(ux, uy, uz, 0.0),
        Vec4::new(look.x, look.y, look.z, 0.0),
        Vec4::new(pos.x, pos.y, pos.z, 1.0),
    )
}

/// `mtx00016d58`: `look_basis` with the look given as a target point.
pub fn look_at_basis(pos: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    look_basis(pos, target - pos, up)
}

/// `mtx00016874` (`mtx.c:285`): the *inverse* of `look_basis`, a world-to-camera
/// view matrix (rows are the basis, translation is `-basis · pos`).
pub fn view_matrix(pos: Vec3, look: Vec3, up: Vec3) -> Mat4 {
    let basis = look_basis(Vec3::ZERO, look, up);
    let a = basis.x_axis.truncate();
    let u = basis.y_axis.truncate();
    let l = basis.z_axis.truncate();
    Mat4::from_cols(
        Vec4::new(a.x, u.x, l.x, 0.0),
        Vec4::new(a.y, u.y, l.y, 0.0),
        Vec4::new(a.z, u.z, l.z, 0.0),
        Vec4::new(-pos.dot(a), -pos.dot(u), -pos.dot(l), 1.0),
    )
}

/// `guRotateF` (`ultra/gu/rotate.c`): `a` degrees about the axis `(x, y, z)`.
pub fn gu_rotate_f(a: f32, x: f32, y: f32, z: f32) -> Mat4 {
    let len = (x * x + y * y + z * z).sqrt();
    let (x, y, z) = if len > 0.0 { (x / len, y / len, z / len) } else { (x, y, z) };
    let a = a * (3.141_592_6 / 180.0);
    let (sine, cosine) = a.sin_cos();
    let t = 1.0 - cosine;
    let (ab, bc, ca) = (x * y * t, y * z * t, z * x * t);
    let mut m = Mat4::IDENTITY;
    let (xx, yy, zz) = (x * x, y * y, z * z);
    m.x_axis.x = xx + cosine * (1.0 - xx);
    m.z_axis.y = bc - x * sine;
    m.y_axis.z = bc + x * sine;
    m.y_axis.y = yy + cosine * (1.0 - yy);
    m.z_axis.x = ca + y * sine;
    m.x_axis.z = ca - y * sine;
    m.z_axis.z = zz + cosine * (1.0 - zz);
    m.y_axis.x = ab - z * sine;
    m.x_axis.y = ab + z * sine;
    m
}

/// `guAlignF` (`ultra/gu/align.c`), `angle` in degrees, as `mtx4_align` passes
/// `RTOD2(angle)`.
pub fn align(angle_deg: f32, x: f32, y: f32, z: f32) -> Mat4 {
    let len = (x * x + y * y + z * z).sqrt();
    let (x, y, z) = if len > 0.0 { (x / len, y / len, z / len) } else { (x, y, z) };
    let a = angle_deg * (3.141_592_6 / 180.0);
    let (s, c) = a.sin_cos();
    let h = (x * x + z * z).sqrt();
    if h == 0.0 {
        return Mat4::IDENTITY;
    }
    let hinv = 1.0 / h;
    Mat4::from_cols(
        Vec4::new((-z * c - s * y * x) * hinv, s * h, (c * x - s * y * z) * hinv, 0.0),
        Vec4::new((z * s - c * y * x) * hinv, c * h, (-s * x - c * y * z) * hinv, 0.0),
        Vec4::new(-x, -y, -z, 0.0),
        Vec4::W,
    )
}

/// `mtx4_align(m, angle, x, y, z)`: radians in, via `RTOD2`.
pub fn mtx4_align(angle_rad: f32, x: f32, y: f32, z: f32) -> Mat4 {
    align(angle_rad * (180.0 / std::f32::consts::PI), x, y, z)
}

/// `mtx00016e98` (`mtx.c:444`): a basis whose −z is the normalised `(x,y,z)`,
/// rolled by `angle`; used by the muzzle-flare billboards.
pub fn mtx00016e98(angle: f32, x: f32, y: f32, z: f32) -> Mat4 {
    let len = (x * x + y * y + z * z).sqrt();
    let (x, y, z) = if len > 0.0 { (x / len, y / len, z / len) } else { (x, y, z) };
    let (sine, cosine) = angle.sin_cos();
    let norm = (x * x + z * z).sqrt();
    if norm == 0.0 {
        return Mat4::IDENTITY;
    }
    let cos_x = x * cosine;
    let sin_x = x * sine;
    let cos_z = z * cosine;
    let sin_z = z * sine;
    let invnorm = 1.0 / norm;
    Mat4::from_cols(
        Vec4::new((-cos_z - y * sin_x) * invnorm, (sin_z - y * cos_x) * invnorm, -x, 0.0),
        Vec4::new(sine * norm, cosine * norm, -y, 0.0),
        Vec4::new((cos_x - y * sin_z) * invnorm, (-sin_x - y * cos_z) * invnorm, -z, 0.0),
        Vec4::W,
    )
}

/// `model_tween_rot_axis` (`model.c:597`): shortest-way tween of one PD euler angle.
pub fn tween_rot_axis(curangle: f32, goalangle: f32, mult: f32) -> f32 {
    let full = turn();
    let mut cur = curangle;
    let mut diff = goalangle - curangle;
    if goalangle < curangle {
        diff += full;
    }
    if diff < std::f32::consts::PI {
        cur += diff * mult;
        if cur >= full {
            cur -= full;
        }
    } else {
        cur -= (full - diff) * mult;
        if cur < 0.0 {
            cur += full;
        }
    }
    cur
}

/// `model_tween_rot`.
pub fn tween_rot(cur: Vec3, goal: Vec3, mult: f32) -> Vec3 {
    Vec3::new(tween_rot_axis(cur.x, goal.x, mult), tween_rot_axis(cur.y, goal.y, mult), tween_rot_axis(cur.z, goal.z, mult))
}

// ─── quaternions (`game/quaternion.c`) ───────────────────────────────────────

/// PD's quaternion layout: `[w, x, y, z]`.
pub type Quatf = [f32; 4];

/// `quaternion0f096ca0` (`quaternion.c:9`): PD euler angles (applied
/// `Rz · Ry · Rx`) → quaternion.
pub fn quaternion0f096ca0(angle: Vec3) -> Quatf {
    let (sinx, cosx) = (angle.x * 0.5).sin_cos();
    let (siny, cosy) = (angle.y * 0.5).sin_cos();
    let (sinz, cosz) = (angle.z * 0.5).sin_cos();
    let cosx_cosy = cosx * cosy;
    let cosx_siny = cosx * siny;
    let sinx_cosy = sinx * cosy;
    let sinx_siny = sinx * siny;
    [
        cosx_cosy * cosz + sinx_siny * sinz,
        sinx_cosy * cosz - cosx_siny * sinz,
        cosx_siny * cosz + sinx_cosy * sinz,
        cosx_cosy * sinz - sinx_siny * cosz,
    ]
}

/// `quaternion_to_mtx` (`quaternion.c:57`). Divides by the norm, so a
/// non-unit quaternion still yields a rotation.
pub fn quaternion_to_mtx(q: Quatf) -> Mat4 {
    let mult = 2.0 / (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    let a = q[1] * mult;
    let b = q[2] * mult;
    let c = q[3] * mult;
    let sp34 = q[0] * a;
    let sp30 = q[0] * b;
    let sp2c = q[0] * c;
    let sp28 = q[1] * a;
    let sp24 = q[1] * b;
    let sp20 = q[1] * c;
    let sp1c = q[2] * b;
    let sp18 = q[2] * c;
    let sp14 = q[3] * c;
    Mat4::from_cols(
        Vec4::new(1.0 - (sp1c + sp14), sp24 + sp2c, sp20 - sp30, 0.0),
        Vec4::new(sp24 - sp2c, 1.0 - (sp28 + sp14), sp18 + sp34, 0.0),
        Vec4::new(sp20 + sp30, sp18 - sp34, 1.0 - (sp28 + sp1c), 0.0),
        Vec4::W,
    )
}

/// `quaternion_to_transform_mtx` (`quaternion.c:136`).
pub fn quaternion_to_transform_mtx(pos: Vec3, q: Quatf) -> Mat4 {
    let mut m = quaternion_to_mtx(q);
    m.w_axis = Vec4::new(pos.x, pos.y, pos.z, 1.0);
    m
}

/// `quaternion0f097044` (`quaternion.c:88`): a rotation matrix → quaternion.
pub fn quaternion0f097044(m: &Mat4) -> Quatf {
    let e = |c: usize, r: usize| m.col(c)[r];
    let trace = e(0, 0) + e(1, 1) + e(2, 2) + 1.0;
    let mut q = [0.0f32; 4];
    if trace > 0.01 {
        let var1 = trace.sqrt();
        let var2 = 0.5 / var1;
        q[0] = var1 * 0.5;
        q[1] = (e(1, 2) - e(2, 1)) * var2;
        q[2] = (e(2, 0) - e(0, 2)) * var2;
        q[3] = (e(0, 1) - e(1, 0)) * var2;
    } else {
        let indices = [1usize, 2, 0];
        let mut i = 0;
        if e(0, 0) < e(1, 1) {
            i = 1;
        }
        if e(i, i) < e(2, 2) {
            i = 2;
        }
        let j = indices[i];
        let k = indices[j];
        let var1 = (e(i, i) - (e(j, j) + e(k, k)) + 1.0).sqrt();
        let var2 = 0.5 / var1;
        q[i + 1] = var1 * 0.5;
        q[0] = (e(j, k) - e(k, j)) * var2;
        q[j + 1] = (e(i, j) + e(j, i)) * var2;
        q[k + 1] = (e(i, k) + e(k, i)) * var2;
    }
    q
}

/// `quaternion_slerp` (`quaternion.c:147`), PD's epsilon.
pub fn quaternion_slerp(q1: Quatf, q2: Quatf, t: f32) -> Quatf {
    const EPSILON: f32 = 0.000_010_01;
    let dot = q1[0] * q2[0] + q1[1] * q2[1] + q1[2] * q2[2] + q1[3] * q2[3];
    let mut r = [0.0f32; 4];
    if dot < -1.0 + EPSILON {
        for i in 0..4 {
            r[i] = (1.0 - t) * q1[i] - q2[i] * t;
        }
    } else if dot <= 1.0 - EPSILON {
        let theta = dot.acos();
        let theta_q1 = (1.0 - t) * theta;
        let theta_q2 = t * theta;
        let sine = theta.sin();
        let coeff_q1 = theta_q1.sin() / sine;
        let coeff_q2 = theta_q2.sin() / sine;
        for i in 0..4 {
            r[i] = coeff_q1 * q1[i] + q2[i] * coeff_q2;
        }
    } else {
        for i in 0..4 {
            r[i] = (1.0 - t) * q1[i] + q2[i] * t;
        }
    }
    r
}

/// `quaternion0f097518` (`quaternion.c:181`): slerp from the identity (sign
/// matched) to `q` by `t`. Not normalised, as in PD.
pub fn quaternion0f097518(q: Quatf, t: f32) -> Quatf {
    let mut sp34 = q[0];
    let mut sp30 = 1.0f32;
    if q[0] < 0.0 {
        sp34 = -sp34;
        sp30 = -sp30;
    }
    if sp34 < -0.999_989_99 {
        [q[0] * t - (1.0 - t) * sp30, q[1] * t, q[2] * t, q[3] * t]
    } else if sp34 <= 0.999_989_99 {
        let sp2c = sp34.acos();
        let sp28 = t * sp2c;
        let sp24 = (1.0 - t) * sp2c;
        let sp20 = sp2c.sin();
        let sp1c = sp28.sin() / sp20;
        let sp18 = sp24.sin() / sp20;
        [q[0] * sp1c + sp18 * sp30, q[1] * sp1c, q[2] * sp1c, q[3] * sp1c]
    } else {
        [q[0] * t + (1.0 - t) * sp30, q[1] * t, q[2] * t, q[3] * t]
    }
}

/// `quaternion_mult_quaternion` (`quaternion.c:234`): `a · b`.
pub fn quaternion_mult_quaternion(a: Quatf, b: Quatf) -> Quatf {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + b[0] * a[1] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] + b[0] * a[2] + a[3] * b[1] - a[1] * b[3],
        a[0] * b[3] + b[0] * a[3] + a[1] * b[2] - a[2] * b[1],
    ]
}

/// `quaternion0f0976c0` (`quaternion.c:224`): flip `q2` into `q1`'s hemisphere.
pub fn quaternion0f0976c0(q1: Quatf, q2: &mut Quatf) {
    let dot = q1[0] * q2[0] + q1[1] * q2[1] + q1[2] * q2[2] + q1[3] * q2[3];
    if dot < 0.0 {
        for v in q2.iter_mut() {
            *v = -*v;
        }
    }
}

/// `func0f096700` (`game_096700.c:8`): `sqrt(tan(x) + 1)`, the knee/elbow
/// helper's stretch.
pub fn func0f096700(value: f32) -> f32 {
    (value.sin() / value.cos() + 1.0).sqrt()
}

/// A PD quaternion as glam's, for callers outside the model code.
pub fn quat_to_glam(q: Quatf) -> Quat {
    Quat::from_xyzw(q[1], q[2], q[3], q[0])
}

/// `func0002f560` (`lib_2f490_c.c:205`): where the line from `from` along
/// `dir` (the whole segment to `to`) crosses triangle `p0 p1 p2`, and the
/// triangle's (unnormalised) normal `(p1 - p0) × (p2 - p1)`. `None` when it
/// misses, lies parallel, or crosses outside `from..to`. PD's arithmetic, in
/// its order: the BG hit test (`bg_test_hit_in_vtx_batch`) depends on it.
pub fn func0002f560(p0: Vec3, p1: Vec3, p2: Vec3, from: Vec3, to: Vec3, dir: Vec3) -> Option<(Vec3, Vec3)> {
    let (f0, f1, f2) = (p0.x, p0.y, p0.z);
    let f3 = p1.x - f0;
    let f4 = p1.y - f1;
    let f5 = p1.z - f2;
    let f6 = p2.x - p1.x;
    let f7 = p2.y - p1.y;
    let f8 = p2.z - p1.z;
    let f9 = p2.x - f0;
    let f10 = p2.y - f1;
    let f11 = p2.z - f2;
    let f12 = f4 * f8 - f7 * f5;
    let f13 = f5 * f6 - f8 * f3;
    let f14 = f3 * f7 - f6 * f4;
    let f15 = f12 * f0 + f13 * f1 + f14 * f2;
    let (f16, f17, f18) = (dir.x, dir.y, dir.z);
    let f19 = f12 * f16 + f13 * f17 + f14 * f18;
    if f19 == 0.0 {
        return None;
    }
    let (f20, f21, f22) = (to.x, to.y, to.z);
    let t = (f15 - f12 * f20 - f13 * f21 - f14 * f22) / f19;
    let (f23, f24, f25) = (f20 + t * f16, f21 + t * f17, f22 + t * f18);
    // Past `to`.
    if f16 * (f23 - f20) + f17 * (f24 - f21) + f18 * (f25 - f22) > 0.0 {
        return None;
    }
    // Before `from`.
    let (g20, g21, g22) = (from.x, from.y, from.z);
    if f16 * (f23 - g20) + f17 * (f24 - g21) + f18 * (f25 - g22) < 0.0 {
        return None;
    }
    let (d0, d1, d2) = (f23 - f0, f24 - f1, f25 - f2);
    let (f26, f27, f28);
    let c = f6 * f4 - f3 * f7;
    if c != 0.0 {
        f26 = c;
        f27 = d0 * f4;
        f28 = d1 * f3;
    } else {
        let c = f7 * f5 - f4 * f8;
        if c != 0.0 {
            f26 = c;
            f27 = d1 * f5;
            f28 = d2 * f4;
        } else {
            f26 = f8 * f3 - f5 * f6;
            f27 = d2 * f3;
            f28 = d0 * f5;
        }
    }
    let v = (f27 - f28) / f26;
    if v < 0.0 {
        return None;
    }
    let u = if f3 != 0.0 {
        (d0 - v * f9) / f3
    } else if f4 != 0.0 {
        (d1 - v * f10) / f4
    } else {
        (d2 - v * f11) / f5
    };
    if u >= 0.0 && u + v <= 1.0 {
        Some((Vec3::new(f23, f24, f25), Vec3::new(f12, f13, f14)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_rotation_is_z_y_x() {
        let r = Vec3::new(0.3, -0.7, 1.1);
        let m = load_rotation(r);
        let g = Mat4::from_euler(glam::EulerRot::ZYX, r.z, r.y, r.x);
        assert!(m.abs_diff_eq(g, 1e-5), "{m:?}\n{g:?}");
        assert!(quaternion_to_mtx(quaternion0f096ca0(r)).abs_diff_eq(m, 1e-5));
    }

    #[test]
    fn matrix_to_quaternion_round_trips() {
        for r in [Vec3::new(0.3, -0.7, 1.1), Vec3::new(3.0, 0.1, -2.9), Vec3::new(0.0, 3.1, 0.0)] {
            let m = load_rotation(r);
            let q = quaternion0f097044(&m);
            assert!(quaternion_to_mtx(q).abs_diff_eq(m, 1e-5), "{r:?}");
        }
    }

    #[test]
    fn slerp_and_the_half_turn_agree_with_glam() {
        let a = quaternion0f096ca0(Vec3::new(0.2, 0.5, -0.4));
        let b = quaternion0f096ca0(Vec3::new(-0.6, 1.2, 0.3));
        let s = quat_to_glam(quaternion_slerp(a, b, 0.3));
        let g = quat_to_glam(a).slerp(quat_to_glam(b), 0.3);
        assert!(s.abs_diff_eq(g, 1e-5), "{s:?} {g:?}");
        let h = quat_to_glam(quaternion0f097518(b, 0.5));
        assert!(h.abs_diff_eq(Quat::IDENTITY.slerp(quat_to_glam(b), 0.5), 1e-5));
    }

    #[test]
    fn view_matrix_inverts_look_basis() {
        let pos = Vec3::new(10.0, 2.0, -5.0);
        let look = Vec3::new(0.3, -0.2, 1.0);
        let up = Vec3::Y;
        let b = look_basis(pos, look, up);
        let v = view_matrix(pos, look, up);
        assert!((v * b).abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }

    #[test]
    fn tween_takes_the_short_way_round() {
        let full = turn();
        let a = tween_rot_axis(full - 0.1, 0.1, 0.5);
        assert!(a < 0.01 || a > full - 0.01, "{a}");
    }

    #[test]
    fn badpi_is_pds_and_not_pi() {
        assert_eq!(M_BADPI, 3.141_092_641);
        assert!((turn() - 6.282_185).abs() < 1e-5);
        assert!(wrap_pos(-0.5) > 5.7 && wrap_pos(turn()) == 0.0);
    }

    #[test]
    fn a_line_crosses_a_triangle_inside_the_segment_only() {
        let (a, b, c) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 10.0), Vec3::new(0.0, 10.0, 10.0));
        let from = Vec3::new(2.0, 2.0, 0.0);
        let to = Vec3::new(2.0, 2.0, 20.0);
        let (hit, n) = func0002f560(a, b, c, from, to, to - from).unwrap();
        assert!((hit - Vec3::new(2.0, 2.0, 10.0)).length() < 1e-5, "{hit}");
        assert!(n.cross(Vec3::Z).length() < 1e-5, "normal along z: {n}");
        // Short of the triangle, beyond it, or beside it: no hit.
        let near = Vec3::new(2.0, 2.0, 5.0);
        assert!(func0002f560(a, b, c, from, near, near - from).is_none());
        let past = Vec3::new(2.0, 2.0, 15.0);
        assert!(func0002f560(a, b, c, past, to, to - past).is_none());
        let off = Vec3::new(9.0, 9.0, 0.0);
        assert!(func0002f560(a, b, c, off, off + Vec3::Z * 20.0, Vec3::Z * 20.0).is_none());
    }

    #[test]
    fn atan2f_runs_from_plus_z_towards_plus_x() {
        assert_eq!(atan2f(0.0, 1.0), 0.0);
        assert!((atan2f(1.0, 0.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((atan2f(-1.0, 0.0) - 3.0 * std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }
}
