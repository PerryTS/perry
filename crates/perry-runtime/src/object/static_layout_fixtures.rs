//! Explicit request fixtures: the input scalar is a template word, never a global identity.
use super::*;
use crate::codegen_abi::ShapeId;

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
        let id = dynamic_plain(requested, packed, packed_len, count, live, rep);
        return shapes::handle_by_identity(ShapeId(id)).map_or(0, |w| w.0);
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
        let id = dynamic_plain_constfn(
            requested,
            packed,
            packed_len,
            count,
            live,
            rep,
            entries,
            entry_count,
        );
        return shapes::handle_by_identity(ShapeId(id)).map_or(0, |w| w.0);
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
        return shapes::handle_by_identity(ShapeId(id)).map_or(0, |w| w.0);
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
        return shapes::handle_by_identity(ShapeId(id)).map_or(0, |w| w.0);
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
        return shapes::handle_by_identity(ShapeId(id)).map_or(0, |w| w.0);
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

// Dynamic fixtures retain the former seed path only for unit tests.
fn dynamic_plain(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
) -> u32 {
    if count == 0 || packed.is_null() || packed_len == 0 {
        return 0;
    }
    // SAFETY: compiler-owned static data of `packed_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let names: Vec<&[u8]> = crate::object::packed_key_names(bytes);
    if names.len() != count as usize {
        return 0;
    }
    let keys = unsafe { canonical_keys_for_names(&names) };
    let id = shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_rep(
        keys.arr(),
        keys.count(),
        live,
        0,
        shapes::ShapeObjectKind::Ordinary,
        0,
        shapes::PROTO_ID_DEFAULT,
        shapes::ReceiverFacts::NONE,
        rep,
        Some(ShapeId(requested)),
    ));
    // SAFETY: as above.
    unsafe { shapes::note_external_carrier_of_word((id).0) };
    let id = shapes::shape_record_for_word(id).map_or(0, |r| r.identity().0);
    super::note_static_request("seed", requested, id);
    id
}

fn dynamic_plain_constfn(
    requested: u32,
    packed: *const u8,
    packed_len: u32,
    count: u32,
    live: u32,
    rep: u64,
    entries: *const ConstFnStaticEntry,
    entry_count: u32,
) -> u32 {
    if requested == 0 || count == 0 || packed.is_null() || packed_len == 0 {
        super::invalid_constfn_static_seed();
    }
    // SAFETY: compiler-owned static data of `packed_len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(packed, packed_len as usize) };
    let names: Vec<&[u8]> = crate::object::packed_key_names(bytes);
    if names.len() != count as usize {
        super::invalid_constfn_static_seed();
    }
    let infos = super::checked_constfn_static_entries(entries, entry_count);
    let keys = unsafe { canonical_keys_for_names(&names) };
    let id = shapes::publish_shape_result(shapes::final_shape_ensure_constfn(
        keys.arr(),
        keys.count(),
        live,
        0,
        rep,
        &infos,
        Some(ShapeId(requested)),
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { shapes::note_external_carrier_of_word((id).0) };
    let id = shapes::shape_record_for_word(id).map_or(0, |r| r.identity().0);
    super::note_static_request("seed-constfn", requested, id);
    id
}
