//! PD models: the file format our exporter writes (node tree, display-list draws,
//! materials and N64 draw state, part boxes) and the walker that poses it:
//! `model_set_matrices_with_anim` with every node type, including the
//! `MODELNODETYPE_0100`/`0200` elbow and knee helper matrices, heads taking matrix 0
//! from the body's headspot, gun toggles, and `model_test_for_hit` (first box in tree
//! order wins).
//!
//! One walker for guns, hands, bodies, heads, props and the menu hudpiece.
//! Sources: `pd_menu/pdmodel.rs` (the superset, with helper joints) and
//! `pd_guns/model.rs`. Retires the glTF bodies and the engine skeletal path that
//! `pd_spike` and `pd_complex` used for simulants.
