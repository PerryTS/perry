//! The existing allocation image describes capacity and actual birth membership.
//! Canonical class keys remain a final-layout template for guarded field access.
use crate::block::LlBlock;
use crate::types::{I32, I64};

pub(super) fn emit(
    blk: &mut LlBlock,
    keys_global: &str,
    class_id: u32,
    gc_packed: u64,
    canonical_shape: &str,
    live: u32,
    deferred_keys: bool,
) {
    let shape = if deferred_keys {
        // Capacity is traceable, but no declared key exists before DefineField.
        // Reuse the shape interner and the allocation image, without a cache.
        blk.call(
            I32,
            "js_object_shape_id_for_class_keys_live",
            &[
                (I64, "0"),
                (I32, "0"),
                (I32, &live.to_string()),
                (I32, &class_id.to_string()),
                (I64, "0"),
            ],
        )
    } else {
        canonical_shape.to_string()
    };
    let shape_i64 = blk.zext(I32, &shape, I64);
    let shifted = blk.shl(I64, &shape_i64, "32");
    let header_word = blk.or(I64, &shifted, &class_id.to_string());
    let image = blk.fresh_reg();
    blk.emit_raw(format!(
        "{image} = insertelement <2 x i64> <i64 {gc_packed}, i64 0>, i64 {header_word}, i32 1"
    ));
    let global = crate::typed_shape::header_image_global_name_from_keys_global(keys_global);
    blk.emit_raw(format!("store <2 x i64> {image}, ptr @{global}, align 8"));
}
