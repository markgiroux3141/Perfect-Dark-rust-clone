//! Posed PD models on the GPU (guns, hands, bodies, heads, props, weapon objects),
//! from the matrices `pd_core::model` computes. The gun and hands draw in their own
//! depth range (near/far 1.5/1000). Source: `pd_guns/render.rs` `PdRenderer`.
