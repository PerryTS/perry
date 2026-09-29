use super::*;

fn object_has_own_key_bytes(obj: *const ObjectHeader, key_bytes: &[u8]) -> bool {
    if obj.is_null() || key_bytes.is_empty() || key_bytes.len() > 4096 {
        return false;
    }
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_addr, _, heap_type) = object_shape(object_addr);
    if heap_type != crate::gc::GC_TYPE_OBJECT as u16 || shape_addr == 0 {
        return false;
    }
    unsafe {
        let obj = object_addr as *const ObjectHeader;
        let Some(descriptor) = crate::object::shapes::object_shape_descriptor(obj) else {
            return false;
        };
        let keys = descriptor.keys as usize as *const ArrayHeader;
        if keys.is_null() {
            return false;
        }
        // The shape's count: the keys array can be a longer shared backing.
        let key_count = (descriptor.logical_key_count as usize)
            .min(crate::array::js_array_length(keys) as usize);
        if key_count > 65_536 {
            return true;
        }
        for i in 0..key_count {
            let stored = crate::array::js_array_get(keys, i as u32);
            // #1781: SSO-aware shape match — pre-fix the `is_string()`
            // here returned false for a stored inline-SSO key, so the
            // typed-feedback hot-path guard fell back to the slow
            // generic dispatch on every read of an object whose shape
            // included a ≤5-byte key.
            if crate::string::js_string_key_matches_bytes(stored, key_bytes) {
                return true;
            }
        }
        false
    }
}

fn vtable_method_matches(class_id: u32, method_name: &str, expected_func_ptr: usize) -> bool {
    if class_id == 0 || expected_func_ptr == 0 {
        return false;
    }
    let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() else {
        return false;
    };
    let Some(registry) = registry.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    for _ in 0..32 {
        if let Some(vtable) = registry.get(&cid) {
            if let Some(entry) = vtable.methods.get(method_name) {
                return entry.func_ptr == expected_func_ptr;
            }
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

fn prototype_may_override_method(class_id: u32, method_name: &str, method_bytes: &[u8]) -> bool {
    if class_id == 0 {
        return false;
    }
    if crate::object::lookup_prototype_method(class_id, method_name).is_some() {
        return true;
    }
    let mut cid = class_id;
    for _ in 0..32 {
        let proto = crate::object::class_prototype_object(cid);
        if !proto.is_null() && object_has_own_key_bytes(proto, method_bytes) {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

fn method_direct_call_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    method_name_ptr: *const i8,
    method_name_len: usize,
    expected_func_ptr: *const u8,
) -> (usize, u32, u16, u64, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let Some(method_bytes) = method_name_bytes(method_name_ptr, method_name_len) else {
        return (shape_addr, class_id, gc_type, 0, false);
    };
    let Some(method_name) = method_name_str(method_name_ptr, method_name_len) else {
        return (
            shape_addr,
            class_id,
            gc_type,
            hash_bytes(method_bytes),
            false,
        );
    };
    let name_hash = hash_bytes(method_bytes);
    let method_guard_slot = crate::object::class_prototype_method_guard_slot(method_name);
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_shape_id(expected_shape_id)
        || expected_func_ptr.is_null()
        || crate::object::class_prototype_fast_guard_invalidated_for_method(method_guard_slot)
    {
        return (shape_addr, class_id, gc_type, name_hash, false);
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return (shape_addr, class_id, gc_type, name_hash, false);
    };
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (*gc_header)._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0
        {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
        let obj = object_addr as *const ObjectHeader;
        if !crate::object::object_is_regular(obj) {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
        if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
            || (*obj).class_id != expected_class_id
            || crate::object::shapes::object_shape_id(obj) != expected_shape_id
            || shape_addr != expected_shape_id as usize
        {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
        if object_has_own_key_bytes(obj, method_bytes) {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
    }

    let expected_func = expected_func_ptr as usize;
    let valid = vtable_method_matches(class_id, method_name, expected_func)
        && !prototype_may_override_method(class_id, method_name, method_bytes);
    (shape_addr, class_id, gc_type, name_hash, valid)
}

/// Borrow the key text for the guard's side-table lookups. Every consumer
/// (`class_getter_in_chain`, `descriptor_blocks_class_field_*`,
/// `get_accessor_descriptor`, `get_property_attrs`) reads Rust-side tables
/// and allocates nothing on the GC heap, so the payload cannot move while the
/// borrow is live; the `String` this used to return was one `malloc` + UTF-8
/// scan per guarded class-field access.
fn key_as_str<'a>(key: *const crate::StringHeader) -> Option<&'a str> {
    if !valid_string_key(key) {
        return None;
    }
    unsafe { crate::string::header_str_checked(key) }
}

fn class_setter_in_chain(class_id: u32, key_name: &str) -> bool {
    if class_id == 0 {
        return false;
    }
    let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() else {
        return true;
    };
    let Some(registry) = registry.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    for _ in 0..32 {
        if registry
            .get(&cid)
            .map(|vtable| vtable.declares_setter(key_name))
            .unwrap_or(false)
        {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

fn class_getter_in_chain(class_id: u32, key_name: &str) -> bool {
    if class_id == 0 {
        return false;
    }
    let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() else {
        return true;
    };
    let Some(registry) = registry.as_ref() else {
        return false;
    };
    let mut cid = class_id;
    for _ in 0..32 {
        if registry
            .get(&cid)
            .map(|vtable| vtable.declares_getter(key_name))
            .unwrap_or(false)
        {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

fn descriptor_blocks_class_field_get(obj_addr: usize, class_id: u32, key_name: &str) -> bool {
    if !crate::object::descriptors_in_use() {
        return false;
    }
    if crate::object::get_accessor_descriptor(obj_addr, key_name).is_some() {
        return true;
    }

    let mut cid = class_id;
    for _ in 0..32 {
        let proto = crate::object::class_prototype_object(cid);
        if !proto.is_null()
            && crate::object::get_accessor_descriptor(proto as usize, key_name).is_some()
        {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

/// Decide the raw-f64 half of a class-field guard after the caller has proven
/// the receiver's exact class/keys pair and that `field_index` is in bounds.
///
/// That shape proof ties the slot to the compile-time mask which made
/// `require_raw_f64` true. The per-object INTACT bit is therefore the complete
/// production answer: it is cleared before any representation downgrade and
/// is the same O(1) fact the codegen-inlined guard already trusts. Keep the
/// descriptor lookup only in `PERRY_VERIFY_TYPED_INTACT` mode, where doing the
/// expensive independent check is the feature's purpose.
#[inline]
fn class_field_raw_f64_layout_contract(
    object_addr: usize,
    field_index: u32,
    require_raw_f64: bool,
) -> bool {
    if !require_raw_f64 {
        return true;
    }
    if verify_typed_intact_enabled() {
        return crate::gc::layout_typed_raw_f64_slot_for_user(object_addr, field_index as usize);
    }
    crate::gc::layout_typed_intact_for_user(object_addr)
}

fn class_field_get_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
) -> (usize, u32, u16, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_shape_id(expected_shape_id)
    {
        return (0, 0, 0, false);
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return (0, 0, 0, false);
    };
    unsafe {
        let gc_type = (*gc_header).obj_type as u16;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return (0, 0, gc_type, false);
        }
        if (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (*gc_header)._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0
        {
            return (0, 0, gc_type, false);
        }

        let obj = object_addr as *mut ObjectHeader;
        let class_id = (*obj).class_id;
        let shape_id = crate::object::shapes::object_shape_id(obj);
        let shape_addr = shape_id as usize;
        let Some(descriptor) = crate::object::shapes::shape_descriptor_by_id(shape_id) else {
            return (shape_addr, class_id, gc_type, false);
        };
        let key_name = match key_as_str(key) {
            Some(name) => name,
            None => return (shape_addr, class_id, gc_type, false),
        };
        let keys = descriptor.keys as usize as *const ArrayHeader;
        let valid = crate::object::object_is_regular(obj)
            && class_id == expected_class_id
            && shape_id == expected_shape_id
            && expected_field_index < descriptor.live_inline_slot_count
            && expected_field_index < descriptor.logical_key_count
            && plain_array_index_guard(keys, expected_field_index, true)
            && object_key_matches_field(obj, key, expected_field_index)
            && class_field_raw_f64_layout_contract(
                object_addr,
                expected_field_index,
                require_raw_f64,
            )
            && !class_getter_in_chain(class_id, key_name)
            && !descriptor_blocks_class_field_get(object_addr, class_id, key_name);
        (shape_addr, class_id, gc_type, valid)
    }
}

fn class_field_fast_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    expected_field_index: u32,
    require_raw_f64: bool,
) -> bool {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_shape_id(expected_shape_id)
    {
        return false;
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return false;
    };
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (*gc_header)._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0
        {
            return false;
        }
        let obj = object_addr as *const ObjectHeader;
        let descriptor = crate::object::shapes::object_shape_descriptor(obj);
        let shape_id = crate::object::shapes::object_shape_stamp(obj);
        let shape_ok = (*obj).class_id == expected_class_id
            && shape_id == expected_shape_id
            && descriptor.is_some_and(|facts| {
                facts.object_kind == crate::object::shapes::ShapeObjectKind::Ordinary
                    && expected_field_index < facts.live_inline_slot_count
            });
        let layout_ok = shape_ok
            && class_field_raw_f64_layout_contract(
                object_addr,
                expected_field_index,
                require_raw_f64,
            );
        // #5093 self-check: the codegen-inlined fast path concludes "slot K is
        // raw-f64" purely from the per-object intact bit (plus a class_id/keys
        // match). Under PERRY_VERIFY_TYPED_INTACT=1, assert that whenever this
        // contract sees a shape match for a raw-f64 candidate field with the
        // intact bit set, the side table actually agrees the slot is raw-f64 —
        // i.e. the inline path could never read a NaN-boxed value as a raw
        // double. Any drift aborts loudly during the test sweep.
        if require_raw_f64 && shape_ok && verify_typed_intact_enabled() {
            let intact = crate::gc::layout_typed_intact_for_user(object_addr);
            if intact && !layout_ok {
                eprintln!(
                    "PERRY_VERIFY_TYPED_INTACT: intact bit set on class {} but slot {} is not raw-f64 in the side table (inline fast path would corrupt)",
                    expected_class_id, expected_field_index
                );
                std::process::abort();
            }
        }
        layout_ok
    }
}

#[cfg(not(test))]
fn verify_typed_intact_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        0 => {
            // Parse by value so `=0`/`=false`/`=off` don't enable the verifier,
            // matching `env_flag_enabled` in `gc/mod.rs` (which also disables the
            // inline fast path when this is on, so the verifier sees every access).
            let on = std::env::var("PERRY_VERIFY_TYPED_INTACT")
                .map(|v| {
                    matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "on" | "yes"
                    )
                })
                .unwrap_or(false);
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        2 => true,
        _ => false,
    }
}

#[cfg(test)]
fn verify_typed_intact_enabled() -> bool {
    false
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_class_field_get_guard(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
) -> i32 {
    if !typed_feedback_enabled() && !crate::object::descriptors_in_use() {
        return class_field_fast_contract(
            receiver,
            expected_class_id,
            expected_shape_id,
            expected_field_index,
            require_raw_f64 != 0,
        ) as i32;
    }
    let (shape_addr, class_id, gc_type, contract_valid) = class_field_get_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        key,
        expected_field_index,
        require_raw_f64 != 0,
    );
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    let observation = Observation {
        source: ObservationSource::Property,
        object_addr: shape_keyed_object_addr(ObservationSource::Property, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: expected_field_index as u64,
        value_tag: value_tag(receiver.to_bits()),
    };
    if guard_observe(
        site_id,
        TypedFeedbackSiteKind::PropertyGet,
        observation,
        contract_valid,
    ) {
        1
    } else {
        0
    }
}

fn class_field_set_fast_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    expected_field_index: u32,
    require_raw_f64: bool,
    value_bits: u64,
) -> bool {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if !class_field_fast_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        expected_field_index,
        require_raw_f64,
    ) {
        return false;
    }
    unsafe {
        let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
            return false;
        };
        if (*gc_header)._reserved
            & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF)
            != 0
        {
            return false;
        }
    }
    !require_raw_f64 || is_plain_number_bits(value_bits)
}

fn descriptor_blocks_class_field_set(obj_addr: usize, class_id: u32, key_name: &str) -> bool {
    if !crate::object::descriptors_in_use() {
        return false;
    }
    if crate::object::get_accessor_descriptor(obj_addr, key_name).is_some() {
        return true;
    }
    if crate::object::get_property_attrs(obj_addr, key_name)
        .map(|attrs| !attrs.writable())
        .unwrap_or(false)
    {
        return true;
    }

    let mut cid = class_id;
    for _ in 0..32 {
        let proto = crate::object::class_prototype_object(cid);
        if !proto.is_null() {
            let proto_addr = proto as usize;
            if crate::object::get_accessor_descriptor(proto_addr, key_name).is_some() {
                return true;
            }
            if crate::object::get_property_attrs(proto_addr, key_name)
                .map(|attrs| !attrs.writable())
                .unwrap_or(false)
            {
                return true;
            }
        }
        match crate::object::get_parent_class_id(cid) {
            Some(parent) if parent != 0 && parent != cid => cid = parent,
            _ => break,
        }
    }
    false
}

fn class_field_set_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
    value_bits: u64,
) -> (usize, u32, u16, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_shape_id(expected_shape_id)
    {
        return (0, 0, 0, false);
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return (0, 0, 0, false);
    };
    unsafe {
        let gc_type = (*gc_header).obj_type as u16;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return (0, 0, gc_type, false);
        }
        if (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return (0, 0, gc_type, false);
        }
        if (*gc_header)._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF
                | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
            != 0
        {
            let obj = object_addr as *mut ObjectHeader;
            return (
                crate::object::shapes::object_shape_id(obj) as usize,
                (*obj).class_id,
                gc_type,
                false,
            );
        }

        let obj = object_addr as *mut ObjectHeader;
        let class_id = (*obj).class_id;
        let shape_id = crate::object::shapes::object_shape_id(obj);
        let shape_addr = shape_id as usize;
        let Some(descriptor) = crate::object::shapes::shape_descriptor_by_id(shape_id) else {
            return (shape_addr, class_id, gc_type, false);
        };
        let key_name = match key_as_str(key) {
            Some(name) => name,
            None => return (shape_addr, class_id, gc_type, false),
        };
        let keys = descriptor.keys as usize as *const ArrayHeader;
        let valid = class_id == expected_class_id
            && crate::object::object_is_regular(obj)
            && shape_id == expected_shape_id
            && expected_field_index < descriptor.live_inline_slot_count
            && expected_field_index < descriptor.logical_key_count
            && plain_array_index_guard(keys, expected_field_index, true)
            && object_key_matches_field(obj, key, expected_field_index)
            && (!require_raw_f64
                || (is_plain_number_bits(value_bits)
                    && class_field_raw_f64_layout_contract(
                        object_addr,
                        expected_field_index,
                        true,
                    )))
            && !class_setter_in_chain(class_id, key_name)
            && !descriptor_blocks_class_field_set(object_addr, class_id, key_name);
        (shape_addr, class_id, gc_type, valid)
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_class_field_set_guard(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    value: f64,
    require_raw_f64: i32,
) -> i32 {
    let value_bits = value.to_bits();
    if !typed_feedback_enabled() && !crate::object::descriptors_in_use() {
        return class_field_set_fast_contract(
            receiver,
            expected_class_id,
            expected_shape_id,
            expected_field_index,
            require_raw_f64 != 0,
            value_bits,
        ) as i32;
    }
    let (shape_addr, class_id, gc_type, contract_valid) = class_field_set_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        key,
        expected_field_index,
        require_raw_f64 != 0,
        value_bits,
    );
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    let source = if require_raw_f64 != 0 {
        ObservationSource::NumericWrite
    } else {
        ObservationSource::Property
    };
    let observation = Observation {
        source,
        object_addr: shape_keyed_object_addr(source, object_addr),
        shape_addr,
        key_hash: key_hash(key),
        class_id,
        heap_type: gc_type,
        aux: expected_field_index as u64,
        value_tag: stable_value_kind(value_bits),
    };
    if guard_observe(
        site_id,
        if require_raw_f64 != 0 {
            TypedFeedbackSiteKind::NumericFieldWrite
        } else {
            TypedFeedbackSiteKind::PropertySet
        },
        observation,
        contract_valid,
    ) {
        1
    } else {
        0
    }
}

/// Class-field-SET guard-MISS fallback, outlined (#5334, lever A).
///
/// The default class-field-set diamond runs the inline
/// `js_typed_feedback_class_field_set_guard` in its entry block; on a guard
/// PASS it stores the slot inline, on a MISS it branches to the fallback arm.
/// That arm used to emit TWO inline calls per set site —
/// `js_typed_feedback_record_fallback_call` then `js_object_set_field_by_name`.
/// Since the guard has already run and FAILED (that failure is what branched
/// control here), nothing is left to decide: this helper just reproduces those
/// two operations, collapsed into ONE call so the cold arm costs a single
/// instruction per site instead of two.
///
/// Byte-identical semantics to the old inline pair:
///   1. record the miss for typed feedback, then
///   2. route the write by name (handles frozen / accessor / non-writable /
///      setter-in-chain). `obj_bits` keeps the full NaN-box tag (the by-name
///      setter inspects it for proxy/exotic dispatch before masking to the
///      heap address); `key_raw` is the POINTER_MASK-stripped key handle.
///
/// Cold-path only, so the extra call frame has zero hot-loop cost; the win is
/// purely in emitted IR size.
#[no_mangle]
pub extern "C" fn js_class_field_set_fallback(
    site_id: u64,
    obj_bits: u64,
    key_raw: u64,
    value: f64,
) {
    crate::typed_feedback::js_typed_feedback_record_fallback_call(site_id);
    crate::object::js_object_set_field_by_name(
        obj_bits as *mut ObjectHeader,
        key_raw as *const crate::StringHeader,
        value,
    );
}

/// Class-field-SET inline cache, FULLY OUTLINED (#5334, lever B).
///
/// For pathologically-large modules (which are forced to `clang -O0`, where the
/// inline IC diamond's ~15-line-per-site expansion is never optimized away),
/// codegen replaces the ENTIRE diamond — guard call, fast slot store, and
/// fallback arm — with a single `call @js_class_field_set_ic(...)`. This trades
/// a function-call frame on the (cold, startup-dominated) field-set path for a
/// large reduction in emitted IR, so clang can actually compile the module.
///
/// The body reproduces the diamond's exact semantics:
///   1. run the same `js_typed_feedback_class_field_set_guard`;
///   2. on a guard PASS, do the same slot store the inline fast block would —
///      a bare `f64` store for a `require_raw_f64` slot (pointer-free by typed
///      shape, no barrier), or `js_object_set_field` for a boxed slot (slot
///      write + layout note + write barrier);
///   3. on a guard FAIL, record the fallback and route the write by name
///      (handles frozen / accessor / non-writable / setter-in-chain).
///
/// Frozen/accessor/writable/setter handling all live behind the guard, so no
/// special-casing here. NB: the boxed store always emits the write barrier
/// (via `js_object_set_field`) — the compile-time non-pointer barrier elision
/// (#5334 lever D) does not apply on this path, an acceptable cost since the
/// full-outline path is gated to oversized, startup-dominated modules.
#[no_mangle]
pub extern "C" fn js_class_field_set_ic(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    value: f64,
    require_raw_f64: i32,
) {
    let guard_ok = js_typed_feedback_class_field_set_guard(
        site_id,
        receiver,
        expected_class_id,
        expected_shape_id,
        key,
        expected_field_index,
        value,
        require_raw_f64,
    );

    if guard_ok != 0 {
        let object_addr = normalize_raw_object_addr(receiver.to_bits());
        if require_raw_f64 != 0 {
            // Pointer-free raw-f64 slot: bare store, no GC barrier.
            unsafe {
                let fields_ptr =
                    (object_addr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
                let slot = fields_ptr.add(expected_field_index as usize);
                // GC_STORE_AUDIT(POINTER_FREE): a passing guard with
                // require_raw_f64 proved the slot is pointer-free by typed-shape
                // descriptor and the value is a plain number — identical to the
                // inline `class_field_set.fast` raw-f64 store, which is barrier-free.
                std::ptr::write(slot, value);
            }
        } else {
            // Boxed slot: slot write + layout note + write barrier.
            crate::object::js_object_set_field(
                object_addr as *mut ObjectHeader,
                expected_field_index,
                crate::value::JSValue::from_bits(value.to_bits()),
            );
        }
        return;
    }

    // Guard FAIL → identical to the cold guard-miss arm. Delegate to the shared
    // fallback helper so by-name routing (frozen / accessor / setter-in-chain)
    // stays defined in exactly one place.
    let obj_bits = receiver.to_bits();
    let key_raw = key as u64 & crate::value::POINTER_MASK;
    js_class_field_set_fallback(site_id, obj_bits, key_raw, value);
}

/// Class-field-GET inline cache, FULLY OUTLINED (#5391 path 2 — extends the
/// #5334 lever-B full-outline from field-SET to field-GET). For oversized
/// modules the entire `class_field_get` diamond (inline precheck + guard call +
/// fast slot load + by-name fallback + phi) collapses to this one call,
/// shrinking the large minified user functions enough for `clang -O0` to
/// compile them in practical time.
///
/// Reproduces the diamond's semantics: run the same
/// `js_typed_feedback_class_field_get_guard`; on a PASS read the field slot as
/// `f64` (a plain number is self-boxing in nan-boxing, so raw-f64 and boxed
/// slots read identically — matching the inline `class_field_get.fast` plain
/// `load double`); on a FAIL record the fallback and read by name. The full
/// outline drops the inline path's static raw-number type hint (the result is
/// treated as a general JS value), which is value-correct — acceptable on the
/// size-gated full-outline path.
///
/// `cache_slot` is the site's own read cache (the `PicCacheSlot` every
/// generic read site has). With typed feedback off, a read the inline
/// pre-check could not prove is the One Path read, exactly as for any other
/// receiver: the receiver's shape against the site's word, then the generic
/// ladder (own miss, the inherited-read cache, priming) — see
/// [`class_field_get_one_path`]. With feedback on, the guard runs and records
/// as before.
#[no_mangle]
pub extern "C" fn js_class_field_get_ic(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
    cache_slot: *mut crate::object::PicCacheSlot,
) -> f64 {
    if !typed_feedback_enabled() {
        return class_field_get_one_path(site_id, receiver, key, cache_slot, true);
    }
    let guard_ok = js_typed_feedback_class_field_get_guard(
        site_id,
        receiver,
        expected_class_id,
        expected_shape_id,
        key,
        expected_field_index,
        require_raw_f64,
    );

    if guard_ok != 0 {
        let object_addr = normalize_raw_object_addr(receiver.to_bits());
        unsafe {
            let fields_ptr =
                (object_addr as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const f64;
            return std::ptr::read(fields_ptr.add(expected_field_index as usize));
        }
    }

    class_field_get_after_guard_fail(site_id, receiver, key)
}

/// The class-field read's miss arm with typed feedback off: the One Path
/// read every other receiver takes, from the site's own cache.
///
/// The guard's answer is not needed first: a receiver its contract passes
/// holds the key in slot `expected_field_index` of a shape that lists it
/// there, which is the slot the shape read loads, so asking the shape answers
/// the same value for it and for every receiver the contract refuses. What
/// the shape read cannot serve inline (an inherited key, a getter, a Proxy,
/// a primitive, a nullish receiver's TypeError) the ladder handles exactly as
/// it does for `o.key` at a generic site. Before, a refused receiver took
/// `js_object_get_field_by_name_f64`, a walk with no site memo, on every
/// read: `this.pa` in a literal's method called on an `Object.create` child
/// of it cost ~1,300 instructions where the same read through a parameter
/// cost ~310.
///
/// `probe_mru` is false only when the S2 leaf entry already asked the word.
fn class_field_get_one_path(
    site_id: u64,
    receiver: f64,
    key: *const crate::StringHeader,
    cache_slot: *mut crate::object::PicCacheSlot,
    probe_mru: bool,
) -> f64 {
    let bits = receiver.to_bits();
    // `key` is always the class field's literal name, loaded from a
    // string-literal handle global (perry-codegen's property_get.rs), and a
    // literal handle is always heap-allocated regardless of length
    // (perry-codegen's strings.rs), so this can never be a SHORT_STRING_TAG
    // value in practice; checked explicitly so a short string here declines
    // to the generic ladder (which is SSO-aware) instead of being masked
    // into a garbage pointer.
    if crate::value::JSValue::from_bits(key as u64).is_short_string() {
        crate::hot_diag::recv_route_note_runtime(crate::hot_diag::RT_ROUTE_CLASS_MISS_LADDER);
        return crate::object::field_get_set::get_field_ic_dispatch(
            bits as i64,
            key,
            site_id,
            cache_slot,
            false,
        );
    }
    let key = (key as u64 & crate::value::POINTER_MASK) as *const crate::StringHeader;
    if probe_mru {
        if let Some(value) = unsafe { class_field_get_from_shape(bits, key, cache_slot, false) } {
            return value;
        }
    }
    crate::hot_diag::recv_route_note_runtime(crate::hot_diag::RT_ROUTE_CLASS_MISS_LADDER);
    crate::object::field_get_set::get_field_ic_dispatch(
        bits as i64,
        key,
        site_id,
        cache_slot,
        false,
    )
}

/// The two answers the receiver's shape gives without the ladder, in the
/// order the emitted generic read asks them: the site's own word, then (on
/// its declined edge) the inherited-read cache. `None` for everything else.
///
/// `leaf`: the caller is the S2 GC-leaf entry, so an inherited ACCESSOR entry
/// (which runs a getter) is declined, as `js_inherited_read_cache_hit_f64`
/// declines it for the emitted read. Otherwise the cache is asked once, as
/// the ladder's own first question (`get_field_ic_miss_impl`'s hook A) asks
/// it, getter included.
///
/// # Safety
/// `cache_slot` is null or the site's live read cache; `key` is the interned
/// key with its tag masked off.
#[inline(always)]
unsafe fn class_field_get_from_shape(
    bits: u64,
    key: *const crate::StringHeader,
    cache_slot: *mut crate::object::PicCacheSlot,
    leaf: bool,
) -> Option<f64> {
    // POINTER tag above the handle band: the receiver test every emitted
    // generic read makes before either lookup.
    if bits >> 48 != 0x7FFD {
        return None;
    }
    let handle = (bits & crate::value::POINTER_MASK) as usize as *const ObjectHeader;
    if !crate::value::addr_class::is_above_handle_band(handle as usize) {
        return None;
    }
    if let Some(value) = crate::object::field_get_set::pic_outlined_mru_hit(handle, cache_slot) {
        crate::hot_diag::recv_route_note_runtime(crate::hot_diag::RT_ROUTE_CLASS_MISS_SHAPE);
        return Some(value);
    }
    let value = if leaf {
        let value =
            crate::object::inherited_read_cache::js_inherited_read_cache_hit_f64(handle, key);
        (value.to_bits() != crate::value::TAG_HOLE).then_some(value)
    } else {
        match crate::object::inherited_read_cache::inherited_read_cache_lookup(handle, key) {
            crate::object::inherited_read_cache::Lookup::Hit(value) => {
                Some(f64::from_bits(value.bits()))
            }
            _ => None,
        }
    };
    if value.is_some() {
        crate::hot_diag::recv_route_note_runtime(crate::hot_diag::RT_ROUTE_CLASS_MISS_SHAPE);
    }
    value
}

/// `js_class_field_get_ic`'s guard-FAIL arm with typed feedback on.
fn class_field_get_after_guard_fail(
    site_id: u64,
    receiver: f64,
    key: *const crate::StringHeader,
) -> f64 {
    crate::typed_feedback::js_typed_feedback_record_fallback_call(site_id);
    let obj_bits = receiver.to_bits();
    // #7153: this function is the full-outline of the codegen class-field-get
    // diamond (#5391), so it must mirror the diamond's nullish-receiver check —
    // a field read on undefined/null throws TypeError instead of answering
    // `undefined` through the by-name lookup.
    let key_raw = key as u64 & crate::value::POINTER_MASK;
    if obj_bits == crate::value::TAG_UNDEFINED || obj_bits == crate::value::TAG_NULL {
        let name = unsafe {
            crate::object::has_own_helpers::str_from_string_header(
                key_raw as *const crate::StringHeader,
            )
        }
        .unwrap_or("");
        crate::error::js_throw_type_error_property_access(
            (obj_bits == crate::value::TAG_NULL) as u32,
            name.as_ptr(),
            name.len(),
        );
    }
    crate::object::js_object_get_field_by_name_f64(
        obj_bits as *const ObjectHeader,
        key_raw as *const crate::StringHeader,
    )
}

// ---------------------------------------------------------------------------
// S2 of the deferred-collection RFC: the full-outline class-field IC split
// into a GC-leaf hit and a collecting miss.
//
// `js_class_field_get_ic` / `js_class_field_set_ic` can run a getter or setter
// (the by-name fallback), so each full-outline site is a statepoint that
// spills and reloads every live GC value on every access. The `_fast` entries
// below serve the guard-PASS slot access; everything else is declined and the
// emitted cold arm calls the `_fast_miss` continuation, which does exactly
// what the full helper does from that point, so the guard's effects happen
// exactly once.
//
// They serve ONLY while typed feedback is off and no property descriptor is in
// use (`descriptors_in_use`). That is precisely when the class-field guards
// take `class_field_fast_contract` / `class_field_set_fast_contract`, which
// neither observe nor walk descriptors. The other arm is not a Perry-GC leaf
// on today's runtime, per the census call graph: the observe takes the
// feedback registry lock, a `GcRootRegistryGuard` whose drop can flush a
// deferred collection request (#11523), and the descriptor walk
// (`get_accessor_descriptor` / `get_property_attrs`) contains an indirect call.
// Under either condition the fast entry declines without evaluating anything
// and the continuation runs the whole helper.
//
// What the fast entries do reach (the S1 checker is the authority; this is
// what it must agree with): two static reads, the fast contract (GC-header and
// shape-descriptor reads, the typed-layout side table, and a verify-mode
// `abort`), then one slot load, a raw-f64 `ptr::write`, or
// `runtime_store_jsvalue_slot` (typed-slot canonicalization, `js_string_addref`,
// `layout_note_slot`, the slot write barrier). Excluded as well, because each
// can collect or re-enter: `js_object_set_field`'s two diagnostics (formatting
// is an indirect call; the census seeds `js_object_set_field` there) — a
// null-POINTER value is declined before anything runs — and
// `set_object_live_slot_count`, which mints a shape descriptor: a store that
// would widen the live bound is declined after the guard (status 3) and stored
// through `js_object_set_field` on the cold arm. The contract's
// `expected_field_index < live_inline_slot_count` makes that unreachable today;
// it is declined rather than assumed.
// ---------------------------------------------------------------------------

/// Status of [`js_class_field_set_ic_fast`]: the store is done.
pub const CLASS_FIELD_SET_FAST_DONE: i32 = 1;
/// The guard ran and FAILED: continue with the by-name fallback.
pub const CLASS_FIELD_SET_FAST_GUARD_FAILED: i32 = 0;
/// Nothing ran (feedback or descriptors in use, or a null-POINTER value):
/// replay the whole full-outline helper.
pub const CLASS_FIELD_SET_FAST_NOT_ATTEMPTED: i32 = 2;
/// The guard PASSED but the store would widen the live bound: store through
/// `js_object_set_field`.
pub const CLASS_FIELD_SET_FAST_STORE_SLOW: i32 = 3;

/// Whether the class-field guards run their side-effect-free, descriptor-free
/// contract — the only case the `_fast` entries serve. Both inputs are
/// set-only latches, so a decline observed by the fast entry is still a
/// decline when the continuation re-reads them.
#[inline(always)]
fn class_field_fast_entries_serve() -> bool {
    !typed_feedback_enabled() && !crate::object::descriptors_in_use()
}

/// GC-leaf hit of the full-outline class-field GET: the guard-PASS slot load,
/// or `TAG_HOLE` (the caller then calls [`js_class_field_get_ic_fast_miss`]).
/// A hole never sits in a slot the contract passes on (#10826: delete
/// transitions the ShapeId); if one did, the continuation's by-name read would
/// answer it correctly.
#[no_mangle]
pub extern "C" fn js_class_field_get_ic_fast(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
    cache_slot: *mut crate::object::PicCacheSlot,
) -> f64 {
    let _ = site_id;
    if !class_field_fast_entries_serve()
        || !class_field_fast_contract(
            receiver,
            expected_class_id,
            expected_shape_id,
            expected_field_index,
            require_raw_f64 != 0,
        )
    {
        // The receiver's shape (GC leaves, as in
        // `js_object_get_field_ic_fast`): the One Path hit for a receiver
        // the contract does not describe. Feedback on asks nothing here.
        //
        // `key` is always the class field's literal name, loaded from a
        // string-literal handle global (perry-codegen's property_get.rs),
        // and a literal handle is always heap-allocated regardless of
        // length (perry-codegen's strings.rs), so this can never be a
        // SHORT_STRING_TAG value in practice; checked explicitly so a short
        // string declines here (falls through to `TAG_HOLE`, and the miss
        // continuation's ladder) instead of being masked into a garbage
        // pointer.
        if !typed_feedback_enabled()
            && !crate::value::JSValue::from_bits(key as u64).is_short_string()
        {
            let key = (key as u64 & crate::value::POINTER_MASK) as *const crate::StringHeader;
            if let Some(value) =
                unsafe { class_field_get_from_shape(receiver.to_bits(), key, cache_slot, true) }
            {
                return value;
            }
        }
        return f64::from_bits(crate::value::TAG_HOLE);
    }
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    unsafe {
        let fields_ptr =
            (object_addr as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const f64;
        std::ptr::read(fields_ptr.add(expected_field_index as usize))
    }
}

/// Collecting continuation of [`js_class_field_get_ic_fast`]. With typed
/// feedback on, the fast entry served nothing and this is the whole
/// `js_class_field_get_ic`. With it off, the fast entry already asked the
/// contract (when descriptors allow) and the site's shape word, and this is
/// the rest of the One Path read: the generic ladder.
#[no_mangle]
pub extern "C" fn js_class_field_get_ic_fast_miss(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
    cache_slot: *mut crate::object::PicCacheSlot,
) -> f64 {
    if typed_feedback_enabled() {
        return js_class_field_get_ic(
            site_id,
            receiver,
            expected_class_id,
            expected_shape_id,
            key,
            expected_field_index,
            require_raw_f64,
            cache_slot,
        );
    }
    class_field_get_one_path(site_id, receiver, key, cache_slot, false)
}

/// GC-leaf hit of the full-outline class-field SET; returns one of the
/// `CLASS_FIELD_SET_FAST_*` statuses. On anything but `DONE` the caller calls
/// [`js_class_field_set_ic_fast_miss`] with the status and the same operands.
#[no_mangle]
pub extern "C" fn js_class_field_set_ic_fast(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    value: f64,
    require_raw_f64: i32,
) -> i32 {
    let _ = (site_id, key);
    let vbits = value.to_bits();
    if !class_field_fast_entries_serve()
        || ((vbits >> 48) == 0x7FFD && (vbits & crate::value::POINTER_MASK) == 0)
    {
        return CLASS_FIELD_SET_FAST_NOT_ATTEMPTED;
    }
    if !class_field_set_fast_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        expected_field_index,
        require_raw_f64 != 0,
        vbits,
    ) {
        return CLASS_FIELD_SET_FAST_GUARD_FAILED;
    }
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    unsafe {
        let fields_ptr =
            (object_addr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
        let slot = fields_ptr.add(expected_field_index as usize);
        if require_raw_f64 != 0 {
            // GC_STORE_AUDIT(POINTER_FREE): identical to `js_class_field_set_ic`'s
            // raw-f64 arm — a passing guard proved the slot pointer-free.
            std::ptr::write(slot, value);
            return CLASS_FIELD_SET_FAST_DONE;
        }
        let obj = object_addr as *mut ObjectHeader;
        if expected_field_index >= crate::object::object_live_slot_count(obj) {
            return CLASS_FIELD_SET_FAST_STORE_SLOW;
        }
        // `js_object_set_field`'s store for an in-bound index and a value that
        // is not a null POINTER (both established above).
        crate::gc::runtime_store_jsvalue_slot(
            object_addr,
            slot as usize,
            expected_field_index as usize,
            vbits,
        );
    }
    CLASS_FIELD_SET_FAST_DONE
}

/// Collecting continuation of [`js_class_field_set_ic_fast`], dispatched on
/// its status so that every path is what `js_class_field_set_ic` would have
/// done, with the guard evaluated once overall.
#[no_mangle]
pub extern "C" fn js_class_field_set_ic_fast_miss(
    status: i32,
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    value: f64,
    require_raw_f64: i32,
) {
    match status {
        CLASS_FIELD_SET_FAST_GUARD_FAILED => {
            let key_raw = key as u64 & crate::value::POINTER_MASK;
            js_class_field_set_fallback(site_id, receiver.to_bits(), key_raw, value);
        }
        CLASS_FIELD_SET_FAST_STORE_SLOW => crate::object::js_object_set_field(
            normalize_raw_object_addr(receiver.to_bits()) as *mut ObjectHeader,
            expected_field_index,
            crate::value::JSValue::from_bits(value.to_bits()),
        ),
        CLASS_FIELD_SET_FAST_DONE => {}
        _ => js_class_field_set_ic(
            site_id,
            receiver,
            expected_class_id,
            expected_shape_id,
            key,
            expected_field_index,
            value,
            require_raw_f64,
        ),
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_typed_feedback_native_call_method(
    site_id: u64,
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let bits = object.to_bits();
    let object_addr = normalize_raw_object_addr(bits);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let name_hash = if valid_method_name(method_name_ptr, method_name_len) {
        hash_bytes(std::slice::from_raw_parts(
            method_name_ptr as *const u8,
            method_name_len,
        ))
    } else {
        0
    };
    let observation = Observation {
        source: ObservationSource::Method,
        object_addr: shape_keyed_object_addr(ObservationSource::Method, object_addr),
        shape_addr,
        key_hash: name_hash,
        class_id,
        heap_type: gc_type,
        aux: 0,
        value_tag: value_tag(bits),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::MethodCall,
        observation,
        valid_method_name(method_name_ptr, method_name_len)
            && bits != TAG_NULL
            && bits != TAG_UNDEFINED,
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::object::js_native_call_method(
        object,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    )
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_typed_feedback_native_call_method_by_id(
    site_id: u64,
    object: f64,
    method_id: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    js_typed_feedback_native_call_method(
        site_id,
        object,
        name_ref.ptr as *const i8,
        name_ref.len,
        args_ptr,
        args_len,
    )
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_typed_feedback_native_call_method_apply(
    site_id: u64,
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_array: i64,
) -> f64 {
    let bits = object.to_bits();
    let object_addr = normalize_raw_object_addr(bits);
    let (shape_addr, class_id, gc_type) = object_shape(object_addr);
    let name_hash = if valid_method_name(method_name_ptr, method_name_len) {
        hash_bytes(std::slice::from_raw_parts(
            method_name_ptr as *const u8,
            method_name_len,
        ))
    } else {
        0
    };
    let observation = Observation {
        source: ObservationSource::Method,
        object_addr: shape_keyed_object_addr(ObservationSource::Method, object_addr),
        shape_addr,
        key_hash: name_hash,
        class_id,
        heap_type: gc_type,
        aux: 0,
        value_tag: value_tag(bits),
    };
    let pass = guard_observe(
        site_id,
        TypedFeedbackSiteKind::MethodCall,
        observation,
        valid_method_name(method_name_ptr, method_name_len)
            && bits != TAG_NULL
            && bits != TAG_UNDEFINED,
    );
    if !pass {
        record_fallback_call(site_id);
    }
    crate::object::js_native_call_method_apply(object, method_name_ptr, method_name_len, args_array)
}

#[no_mangle]
pub unsafe extern "C-unwind" fn js_typed_feedback_native_call_method_apply_by_id(
    site_id: u64,
    object: f64,
    method_id: i64,
    args_array: i64,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    js_typed_feedback_native_call_method_apply(
        site_id,
        object,
        name_ref.ptr as *const i8,
        name_ref.len,
        args_array,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_typed_feedback_method_direct_call_guard(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    method_name_ptr: *const i8,
    method_name_len: usize,
    expected_func_ptr: *const u8,
) -> i32 {
    let bits = receiver.to_bits();
    let (shape_addr, class_id, gc_type, name_hash, contract_valid) = method_direct_call_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        method_name_ptr,
        method_name_len,
        expected_func_ptr,
    );
    let object_addr = normalize_raw_object_addr(bits);
    let observation = Observation {
        source: ObservationSource::Method,
        object_addr: shape_keyed_object_addr(ObservationSource::Method, object_addr),
        shape_addr,
        key_hash: name_hash,
        class_id,
        heap_type: gc_type,
        aux: expected_func_ptr as u64,
        value_tag: value_tag(bits),
    };
    if guard_observe(
        site_id,
        TypedFeedbackSiteKind::MethodCall,
        observation,
        contract_valid,
    ) {
        1
    } else {
        0
    }
}

/// The class-id half of [`js_method_direct_shape_guard`], hoisted out so a
/// call site can test MORE than one (class id, ShapeId) pair per probe.
///
/// Returns the receiver's `class_id` when every precondition the guard checks
/// *other than* the class-id / shape comparison holds, and writes the
/// receiver's ShapeId through `out_shape_id`. The pair is an untrusted token:
/// callers must compare it to a compiler-published `(class_id, ShapeId)` pair
/// before using it as a layout proof. Returns 0 — never a valid user class id
/// — when any precondition fails, and then leaves output at 0 so a caller that
/// skips the return check still cannot match a real ShapeId.
///
/// This exists because the single-pair guard speculates the receiver's dynamic
/// class is exactly the *declared* class of the expression. For a receiver
/// typed as a base class in a hierarchy (`const n: Node2D = nodes[i]`) that
/// speculation is wrong for every subclass instance, so the guard misses 100%
/// of the time and every call pays the full `js_native_call_method` tower. One
/// probe plus an inline compare chain over the base's subclass closure turns
/// the same information into a direct call. See
/// `perry-codegen/src/lower_call/method_override.rs`.
///
/// Descriptor invalidation is deliberately scoped rather than process-wide:
/// an own descriptor sets `OBJ_FLAG_HAS_DESCRIPTORS` on this receiver, while a
/// user descriptor on a registered class/Object prototype flips the matching
/// method-name slot checked below. A descriptor on an unrelated object or for
/// an unrelated key can affect neither this method's resolution nor this exact
/// ShapeId proof and must not poison every direct-method site in the process.
#[no_mangle]
pub unsafe extern "C" fn js_method_direct_shape_class(
    receiver: f64,
    out_shape_id: *mut u32,
    method_guard_slot: u32,
) -> u32 {
    if !out_shape_id.is_null() {
        *out_shape_id = 0;
    }
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0 {
        return 0;
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return 0;
    };
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
        || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*gc_header)._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
                | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF)
            != 0
        || crate::object::class_prototype_fast_guard_invalidated_for_method(method_guard_slot)
    {
        return 0;
    }
    let obj = object_addr as *const ObjectHeader;
    let class_id = (*obj).class_id;
    if class_id == 0 {
        return 0;
    }
    // The emitted caller immediately compares BOTH words against one of its
    // compiler-published class-shape pairs. That exact ShapeId identity is the
    // descriptor proof: ids are process-unique, immutable and never reused.
    // Resolving the id through the thread-local descriptor HashMap here merely
    // repeated the proof before every virtual call. A class object cannot
    // alias an ordinary instance's expected id: the class-kind transition
    // mints its own semantic successor ShapeId.
    let shape_id = crate::object::shapes::object_shape_stamp(obj);
    if shape_id == 0 {
        return 0;
    }
    if !out_shape_id.is_null() {
        *out_shape_id = shape_id;
    }
    class_id
}

#[no_mangle]
pub unsafe extern "C" fn js_method_direct_shape_guard(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    method_guard_slot: u32,
) -> i32 {
    if expected_class_id == 0 || !crate::object::shapes::is_shape_id(expected_shape_id) {
        return 0;
    }
    let mut shape_id = 0;
    let class_id = js_method_direct_shape_class(receiver, &mut shape_id, method_guard_slot);
    (class_id == expected_class_id && shape_id == expected_shape_id) as i32
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_closure_direct_call_guard(
    site_id: u64,
    closure_value: f64,
    expected_info: *const crate::closure::JsFunctionInfo,
    expected_arity: u32,
    call_arity: u32,
) -> i32 {
    let bits = closure_value.to_bits();
    let raw_ptr = if (bits & TAG_MASK) == POINTER_TAG {
        (bits & POINTER_MASK) as *const crate::closure::ClosureHeader
    } else if (bits >> 48) == 0 && bits >= 0x10000 {
        bits as *const crate::closure::ClosureHeader
    } else {
        std::ptr::null()
    };
    let closure_ptr = crate::closure::clean_closure_ptr(raw_ptr);
    let info = crate::closure::closure_info(closure_ptr);
    let func_ptr = info.map_or(std::ptr::null(), |info| info.code);
    // The body's own info is the identity; its rest kind and parameter count
    // are the contract a direct call at `call_arity` relies on.
    let contract_valid = !expected_info.is_null()
        && info.is_some_and(|info| {
            std::ptr::eq(info, expected_info)
                && info.code != crate::closure::BOUND_METHOD_FUNC_PTR
                && crate::closure::info_rest(info).is_none()
                && u32::from(info.params) == expected_arity
        })
        && expected_arity == call_arity;
    let observation = Observation {
        source: ObservationSource::Closure,
        object_addr: 0,
        shape_addr: 0,
        key_hash: 0,
        class_id: 0,
        heap_type: if func_ptr.is_null() {
            0
        } else {
            crate::gc::GC_TYPE_CLOSURE as u16
        },
        aux: func_ptr as u64,
        value_tag: stable_value_kind(bits),
    };
    if guard_observe(
        site_id,
        TypedFeedbackSiteKind::ClosureCall,
        observation,
        contract_valid,
    ) {
        1
    } else {
        0
    }
}

/// Validate only the live closure function identity for a statically proven
/// direct call.
///
/// Whole-program object-literal capabilities already prove the target's
/// arity/rest contract at compile time. Repeating the closure info checks
/// and recording a typed-feedback observation on every call therefore adds no
/// safety. This smaller guard deliberately keeps the speculation-safe closure
/// header validation used by the universal dispatcher: arbitrary replacement
/// values, small handle-band ids, and bound-function sentinels must miss the
/// direct arm without ever being dereferenced or called as code.
fn closure_ptr_from_value_bits(bits: u64) -> *const crate::closure::ClosureHeader {
    let addr = if (bits & TAG_MASK) == POINTER_TAG {
        (bits & POINTER_MASK) as usize
    } else if bits >> 48 == 0 && crate::value::addr_class::is_above_handle_band(bits as usize) {
        bits as usize
    } else {
        0
    };
    addr as *const crate::closure::ClosureHeader
}

#[no_mangle]
pub extern "C" fn js_closure_exact_func_guard(
    closure_value: f64,
    expected_info: *const crate::closure::JsFunctionInfo,
) -> u64 {
    if expected_info.is_null() {
        return 0;
    }
    let raw_ptr = closure_ptr_from_value_bits(closure_value.to_bits());
    let closure_ptr = crate::closure::clean_closure_ptr(raw_ptr);
    if std::ptr::eq(crate::closure::get_valid_info(closure_ptr), expected_info) {
        closure_ptr as u64
    } else {
        0
    }
}

/// Words in a per-site imported-object own-method cache: the one
/// `(ShapeId << 32 | class_id)` token the emitted guard compares.
pub const METHOD_PIC_WORDS: usize = 1;
/// A per-site own-method cache, as the emitted slot resolves it.
pub type MethodPicCache = [u64; METHOD_PIC_WORDS];
/// The emitted `@perry_ic_N = private global ptr null` for such a site: null
/// until the site's first priming miss (#9708).
pub type MethodPicCacheSlot = *mut MethodPicCache;

/// Revalidate and prime the shape token for an own object-literal method.
///
/// The exported adapter object may append ordinary state fields during
/// `setup()` after its module-initial ShapeId was published. Exact initial-
/// shape guards therefore miss permanently even though the method's key,
/// slot, descriptor, and closure are unchanged. This cold IC-miss helper
/// accepts such append-only successors by re-proving the method key at its
/// original slot and the live closure identity, then publishes the live packed
/// `(class_id, ShapeId)` token for an inline hot-path comparison.
///
/// Deletion/compaction changes the key at `field_index`; replacement changes
/// the closure; descriptor/prototype mutation either sets the descriptor bit
/// or mints a semantic successor. A spill-only metadata record is allowed:
/// appending past the object's inline birth width creates one even though the
/// original method slot and its lookup semantics remain unchanged.
///
/// `cache_slot` is the site's [`MethodPicCacheSlot`] address (#9708). A miss
/// that cannot prime clears an existing cache's token but never allocates
/// one; only the publishing tail below resolves the slot.
#[no_mangle]
pub unsafe extern "C" fn js_object_own_method_cache_miss(
    receiver: f64,
    expected_class_id: u32,
    field_index: u32,
    method_name_ptr: *const i8,
    method_name_len: usize,
    expected_info: *const crate::closure::JsFunctionInfo,
    cache_slot: *mut MethodPicCacheSlot,
) -> u64 {
    {
        let cache = crate::object::pic_slot_peek(cache_slot);
        if !cache.is_null() {
            (*cache)[0] = 0;
        }
    }
    if expected_class_id == 0 || expected_info.is_null() || cache_slot.is_null() {
        return 0;
    }
    let Some(method_bytes) = method_name_bytes(method_name_ptr, method_name_len) else {
        return 0;
    };
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return 0;
    };
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
        || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*gc_header)._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
                | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF)
            != 0
    {
        return 0;
    }
    let object = object_addr as *const ObjectHeader;
    if !crate::object::object_is_regular(object) || (*object).class_id != expected_class_id {
        return 0;
    }
    let meta = (*object).meta;
    if !meta.is_null()
        && ((*meta).prototype != 0
            || (*meta).attr_key_bits != 0
            || (*meta).accessor_key_bits != 0
            || (*meta).flags != 0
            || (*meta).private_evaluation_brand != 0)
    {
        return 0;
    }
    let Some(shape) = crate::object::shapes::object_shape_descriptor(object) else {
        return 0;
    };
    if field_index >= shape.logical_key_count || field_index >= shape.live_inline_slot_count {
        return 0;
    }
    let keys = shape.keys as usize as *const ArrayHeader;
    if keys.is_null()
        || !crate::string::js_string_key_matches_bytes(
            crate::array::js_array_get(keys, field_index),
            method_bytes,
        )
    {
        return 0;
    }

    let fields = (object as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let closure_bits = std::ptr::read(fields.add(field_index as usize));
    let raw_ptr = closure_ptr_from_value_bits(closure_bits);
    let closure = crate::closure::clean_closure_ptr(raw_ptr);
    if !std::ptr::eq(crate::closure::get_valid_info(closure), expected_info) {
        return 0;
    }

    let shape_id = crate::object::shapes::object_shape_id(object);
    if !crate::object::shapes::is_shape_id(shape_id) {
        return 0;
    }
    let cache = crate::object::pic_slot_resolve(cache_slot);
    (*cache)[0] = ((shape_id as u64) << 32) | expected_class_id as u64;
    closure as u64
}

// #1764 (follow-up): the guard helpers in this submodule are codegen-emitted
// `#[no_mangle]` exports with no Rust-side caller, so the auto-optimize
// whole-program thin-LTO + `strip=true` build internalizes + dead-strips them
// — dangling the codegen call at final link (`Undefined symbols:
// _js_typed_feedback_class_field_set_guard` for any class-field program).
// `typed_feedback.rs`'s `#[used]` block covers the helpers defined there;
// these typed fn-pointer statics extend the same `@llvm.used` retention to the
// guard helpers defined here. (A `usize`/`*const()` cast does NOT survive
// thin-LTO — only individual typed fn-pointer statics keep the symbol
// external.) The statics must mirror each guard's exact signature, so keep
// them in sync if a guard's parameter list changes.
#[rustfmt::skip]
#[cfg(feature = "keepalive-anchors")]
mod keep_guard_symbols {
    use super::*;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G0: extern "C" fn(u64, f64, u32, u32, *const crate::StringHeader, u32, i32) -> i32 = js_typed_feedback_class_field_get_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G1: extern "C" fn(u64, f64, u32, u32, *const crate::StringHeader, u32, f64, i32) -> i32 = js_typed_feedback_class_field_set_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G1C: extern "C" fn(u64, u64, u64, f64) = js_class_field_set_fallback;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G1D: extern "C" fn(u64, f64, u32, u32, *const crate::StringHeader, u32, f64, i32) = js_class_field_set_ic;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G1E: extern "C" fn(u64, f64, u32, u32, *const crate::StringHeader, u32, i32, *mut crate::object::PicCacheSlot) -> f64 = js_class_field_get_ic;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G2: unsafe extern "C" fn(u64, f64, u32, u32, *const i8, usize, *const u8) -> i32 = js_typed_feedback_method_direct_call_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G3: extern "C" fn(u64, f64, *const crate::closure::JsFunctionInfo, u32, u32) -> i32 = js_typed_feedback_closure_direct_call_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G3B: extern "C" fn(f64, *const crate::closure::JsFunctionInfo) -> u64 = js_closure_exact_func_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G3C: unsafe extern "C" fn(f64, u32, u32, *const i8, usize, *const crate::closure::JsFunctionInfo, *mut MethodPicCacheSlot) -> u64 = js_object_own_method_cache_miss;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G4: unsafe extern "C" fn(f64, u32, u32, u32) -> i32 = js_method_direct_shape_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G4B: unsafe extern "C" fn(f64, *mut u32, u32) -> u32 = js_method_direct_shape_class;
}
