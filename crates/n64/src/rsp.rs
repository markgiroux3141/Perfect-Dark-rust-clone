//! RSP semantics the renderers must reproduce: fixed-point matrix conventions,
//! vertex lighting (directional + ambient), texgen (the chrome `guLookAtReflect`
//! path), culling and the gun's 1.5/1000 near/far. Pure maths, shared by the CPU
//! rasteriser and the GPU pipelines.
