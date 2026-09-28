//! RSP semantics the renderers must reproduce: the vertex stage PD's microcode
//! runs per vertex (modelview transform, directional + ambient lighting,
//! `G_TEXTURE_GEN` / `_LINEAR` from the `LookAt` vectors), shared by the CPU
//! rasteriser ([`crate::rdp`]) and, later, the GPU pipelines.
//!
//! A lit vertex's colour bytes are its normal (signed, /127), not a colour: the
//! geometry mode at `G_VTX` time decides, which is why the model exporter records
//! `lit`/`texgen` per vertex. Light directions are eye space, taken through the
//! transposed modelview (the microcode's inverse-transpose for a rigid matrix).
//!
//! Source: the vertex stage of the old repo's `pd_menu/pdmodel.rs` `render_node`
//! (the CPU copy of `pdgun.wgsl`'s vertex shader).

use glam::{Mat3, Mat4, Vec3};

use crate::rdp::PV;

/// `Lights1` + `LookAt` for a model pass.
#[derive(Clone, Copy, Debug)]
pub struct Lights {
    /// Ambient intensity, 0..255.
    pub ambient: f32,
    /// Directional intensity, 0..255.
    pub diffuse: f32,
    /// Raw light direction / 127, eye space.
    pub dir: Vec3,
    pub lookat_x: Vec3,
    pub lookat_y: Vec3,
}

/// One model vertex through the RSP: `mtx` is its matrix (the model's matrix
/// segment slot the display list loaded), `st` its texture coordinates in texels
/// (the texgen scale for a texgen vertex), `c` its colour or normal bytes.
#[allow(clippy::too_many_arguments)]
pub fn vertex(mtx: &Mat4, proj: &Mat4, pos: Vec3, st: [f32; 2], c: [u8; 4], lit: bool, texgen: bool, texgen_linear: bool, lights: &Lights) -> PV {
    let eye = *mtx * pos.extend(1.0);
    let mut st = st;
    let shade = if lit {
        let nrm = Vec3::new(c[0] as i8 as f32, c[1] as i8 as f32, c[2] as i8 as f32);
        let m3t = Mat3::from_mat4(*mtx).transpose();
        let coeffs = (m3t * lights.dir).normalize_or_zero();
        let inten = nrm.dot(coeffs) / 127.0;
        let k = lights.ambient + if inten > 0.0 { inten * lights.diffuse } else { 0.0 };
        let k = k.min(255.0) / 255.0;
        if texgen {
            let cx = (m3t * lights.lookat_x).normalize_or_zero();
            let cy = (m3t * lights.lookat_y).normalize_or_zero();
            let mut dx = (nrm.dot(cx) / 127.0).clamp(-1.0, 1.0);
            let mut dy = (nrm.dot(cy) / 127.0).clamp(-1.0, 1.0);
            if texgen_linear {
                dx = (-dx).acos() / 4.0;
                dy = (-dy).acos() / 4.0;
            } else {
                dx = (dx + 1.0) / 4.0;
                dy = (dy + 1.0) / 4.0;
            }
            st = [dx * st[0], dy * st[1]];
        }
        [k, k, k, c[3] as f32 / 255.0]
    } else {
        [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]
    };
    PV { eye, clip: *proj * eye, st, shade }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lights() -> Lights {
        Lights { ambient: 100.0, diffuse: 155.0, dir: Vec3::Z, lookat_x: Vec3::X, lookat_y: Vec3::Y }
    }

    #[test]
    fn a_normal_facing_the_light_is_fully_lit_and_one_facing_away_is_ambient() {
        let m = Mat4::IDENTITY;
        let lit = vertex(&m, &m, Vec3::ZERO, [0.0; 2], [0, 0, 127, 200], true, false, false, &lights());
        assert!((lit.shade[0] - 1.0).abs() < 1e-6 && (lit.shade[3] - 200.0 / 255.0).abs() < 1e-6);
        let away = vertex(&m, &m, Vec3::ZERO, [0.0; 2], [0, 0, 0x81, 255], true, false, false, &lights());
        assert!((away.shade[0] - 100.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn an_unlit_vertex_keeps_its_colour() {
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let v = vertex(&m, &Mat4::IDENTITY, Vec3::ONE, [3.0, 4.0], [255, 0, 51, 255], false, false, false, &lights());
        assert_eq!(v.eye.truncate(), Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(v.st, [3.0, 4.0]);
        assert_eq!(v.shade, [1.0, 0.0, 0.2, 1.0]);
    }
}
