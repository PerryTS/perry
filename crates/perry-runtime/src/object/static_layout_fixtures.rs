//! Explicit request fixtures: the input scalar is a template word, never a global identity.
use super::*;

pub(crate) const fn fixture_word(content: u32) -> u32 {
    0x8000_0000 | (512 + (content & 0x7ff))
}
fn request(word: u32) -> crate::codegen_abi::PerryStaticLayoutRequestV1 {
    crate::codegen_abi::PerryStaticLayoutRequestV1 {
        shape_id: word.wrapping_add(0x10000),
        layout_word: word,
    }
}

pub(crate) fn fixture_shape_seed_plain(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
) -> u32 {
    if requested == 0 {
        let id = super::js_shape_seed_plain(requested, packed, packed_len, count, live, rep);
        return shapes::handle_by_identity(crate::codegen_abi::ShapeId(id)).map_or(0, |w| w.0);
    }
    let mut request = request(requested);
    super::js_layout_seed_plain(&mut request, packed, packed_len, count, live, rep)
}

pub(crate) fn fixture_shape_seed_plain_constfn(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u32 {
    if requested == 0 {
        let id = super::js_shape_seed_plain_constfn(
            requested,
            packed,
            packed_len,
            count,
            live,
            rep,
            entries,
            entry_count,
        );
        return shapes::handle_by_identity(crate::codegen_abi::ShapeId(id)).map_or(0, |w| w.0);
    }
    let mut request = request(requested);
    super::js_layout_seed_plain_constfn(
        &mut request,
        packed,
        packed_len,
        count,
        live,
        rep,
        entries,
        entry_count,
    )
}

pub(crate) fn fixture_object_shape_id_for_class_keys_static(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
    rep: u64,
) -> u32 {
    if requested == 0 {
        let id = super::js_object_shape_id_for_class_keys_static(
            keys, key_count, live, class_id, requested, rep,
        );
        return shapes::handle_by_identity(crate::codegen_abi::ShapeId(id)).map_or(0, |w| w.0);
    }
    let mut request = request(requested);
    super::js_object_layout_for_class_keys_static(
        keys,
        key_count,
        live,
        class_id,
        &mut request,
        rep,
    )
}

pub(crate) fn fixture_object_final_shape_id_for_class_keys_static_constfn(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u32 {
    if requested == 0 {
        let id = super::js_object_final_shape_id_for_class_keys_static_constfn(
            keys,
            key_count,
            live,
            class_id,
            requested,
            rep,
            entries,
            entry_count,
        );
        return shapes::handle_by_identity(crate::codegen_abi::ShapeId(id)).map_or(0, |w| w.0);
    }
    let mut request = request(requested);
    super::js_object_final_layout_for_class_keys_static_constfn(
        keys,
        key_count,
        live,
        class_id,
        &mut request,
        rep,
        entries,
        entry_count,
    )
}

pub(crate) fn fixture_object_final_shape_id_for_class_keys_static_private(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    requested: u32,
    rep: u64,
    private_packed: *const u8,
    private_len: u32,
    brands: *const u64,
    brand_count: u32,
) -> u32 {
    if requested == 0 {
        let id = super::js_object_final_shape_id_for_class_keys_static_private(
            keys,
            key_count,
            live,
            class_id,
            requested,
            rep,
            private_packed,
            private_len,
            brands,
            brand_count,
        );
        return shapes::handle_by_identity(crate::codegen_abi::ShapeId(id)).map_or(0, |w| w.0);
    }
    let mut request = request(requested);
    super::js_object_final_layout_for_class_keys_static_private(
        keys,
        key_count,
        live,
        class_id,
        &mut request,
        rep,
        private_packed,
        private_len,
        brands,
        brand_count,
    )
}
