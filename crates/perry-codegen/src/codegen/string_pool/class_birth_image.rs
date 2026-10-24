//! Shared birth images are compile-time constants. This preserves only the
//! publication call for a deferred empty-key birth before exposing its image.
use crate::block::LlBlock;
use crate::types::{I32, I64, PTR};

pub(super) fn emit(
    blk: &mut LlBlock,
    keys_global: &str,
    class_id: u32,
    _gc_packed: u64,
    _canonical_shape: &str,
    live: u32,
    deferred_keys: bool,
) {
    if !deferred_keys {
        return;
    }
    // A class the driver gave no static identity has no constant header image
    // (its births mint through the runtime), so there is nothing to publish.
    let Some(birth) = crate::codegen::static_shape_ids::static_birth_content(keys_global) else {
        return;
    };
    let (identity, _) = crate::layout_ordinals::deferred_word(&birth, live);
    blk.call(
        I32,
        "js_object_layout_for_class_keys_static",
        &[
            (I64, "0"),
            (I32, "0"),
            (I32, &live.to_string()),
            (I32, &class_id.to_string()),
            (
                PTR,
                &format!("@{}", crate::layout_ordinals::request_symbol(identity)),
            ),
            (I64, "0"),
        ],
    );
}
