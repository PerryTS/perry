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
        let keys = (*obj).keys_array;
        // #6804: `shape_addr` is an opaque token (see `object_shape`), not
        // necessarily the keys address — the key scan below is the check.
        if keys.is_null() {
            return false;
        }
        let key_count = crate::array::js_array_length(keys) as usize;
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
    expected_keys: *const ArrayHeader,
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
    if object_addr == 0
        || expected_class_id == 0
        || expected_keys.is_null()
        || expected_func_ptr.is_null()
    {
        return (shape_addr, class_id, gc_type, name_hash, false);
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return (shape_addr, class_id, gc_type, name_hash, false);
    };
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
        let obj = object_addr as *const ObjectHeader;
        if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR {
            return (shape_addr, class_id, gc_type, name_hash, false);
        }
        if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
            || (*obj).class_id != expected_class_id
            || !std::ptr::eq((*obj).keys_array, expected_keys)
            || shape_addr != expected_keys as usize
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

fn key_as_str(key: *const crate::StringHeader) -> Option<String> {
    if !valid_string_key(key) {
        return None;
    }
    unsafe {
        let len = (*key).byte_len as usize;
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .ok()
            .map(|s| s.to_string())
    }
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
            .map(|vtable| vtable.setters.contains_key(key_name))
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
            .map(|vtable| vtable.getters.contains_key(key_name))
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

fn class_field_get_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_keys: *const ArrayHeader,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
) -> (usize, u32, u16, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0 || expected_class_id == 0 || expected_keys.is_null() {
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

        let obj = object_addr as *mut ObjectHeader;
        let class_id = (*obj).class_id;
        let shape_addr = (*obj).keys_array as usize;
        let key_name = match key_as_str(key) {
            Some(name) => name,
            None => return (shape_addr, class_id, gc_type, false),
        };
        let expected_shape_addr = expected_keys as usize;
        let valid = (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR
            && class_id == expected_class_id
            && shape_addr == expected_shape_addr
            && expected_field_index < (*obj).field_count
            && plain_array_index_guard(expected_keys, expected_field_index, true)
            && object_key_matches_field(obj, key, expected_field_index)
            && (!require_raw_f64
                || crate::gc::layout_typed_raw_f64_slot_for_user(
                    object_addr,
                    expected_field_index as usize,
                ))
            && !class_getter_in_chain(class_id, &key_name)
            && !descriptor_blocks_class_field_get(object_addr, class_id, &key_name);
        (shape_addr, class_id, gc_type, valid)
    }
}

fn class_field_fast_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_keys: *const ArrayHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
) -> bool {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0 || expected_class_id == 0 || expected_keys.is_null() {
        return false;
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return false;
    };
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        let obj = object_addr as *const ObjectHeader;
        let shape_ok = (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR
            && (*obj).class_id == expected_class_id
            && std::ptr::eq((*obj).keys_array as *const ArrayHeader, expected_keys)
            && expected_field_index < (*obj).field_count;
        // #5093 self-check: the codegen-inlined fast path concludes "slot K is
        // raw-f64" purely from the per-object intact bit (plus a class_id/keys
        // match). Under PERRY_VERIFY_TYPED_INTACT=1, assert that whenever this
        // contract sees a shape match for a raw-f64 candidate field with the
        // intact bit set, the side table actually agrees the slot is raw-f64 —
        // i.e. the inline path could never read a NaN-boxed value as a raw
        // double. Any drift aborts loudly during the test sweep.
        if require_raw_f64 && shape_ok && verify_typed_intact_enabled() {
            let intact = crate::gc::layout_typed_intact_for_user(object_addr);
            let raw = crate::gc::layout_typed_raw_f64_slot_for_user(
                object_addr,
                expected_field_index as usize,
            );
            if intact && !raw {
                eprintln!(
                    "PERRY_VERIFY_TYPED_INTACT: intact bit set on class {} but slot {} is not raw-f64 in the side table (inline fast path would corrupt)",
                    expected_class_id, expected_field_index
                );
                std::process::abort();
            }
        }
        shape_ok
            && (!require_raw_f64
                || crate::gc::layout_typed_raw_f64_slot_for_user(
                    object_addr,
                    expected_field_index as usize,
                ))
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
    expected_keys: *const ArrayHeader,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
) -> i32 {
    if !typed_feedback_enabled() && !crate::object::descriptors_in_use() {
        return class_field_fast_contract(
            receiver,
            expected_class_id,
            expected_keys,
            expected_field_index,
            require_raw_f64 != 0,
        ) as i32;
    }
    let (shape_addr, class_id, gc_type, contract_valid) = class_field_get_contract(
        receiver,
        expected_class_id,
        expected_keys,
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
    expected_keys: *const ArrayHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
    value_bits: u64,
) -> bool {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if !class_field_fast_contract(
        receiver,
        expected_class_id,
        expected_keys,
        expected_field_index,
        require_raw_f64,
    ) {
        return false;
    }
    unsafe {
        let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
            return false;
        };
        if (*gc_header)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
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
    expected_keys: *const ArrayHeader,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
    value_bits: u64,
) -> (usize, u32, u16, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0 || expected_class_id == 0 || expected_keys.is_null() {
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
        if (*gc_header)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            let obj = object_addr as *mut ObjectHeader;
            return ((*obj).keys_array as usize, (*obj).class_id, gc_type, false);
        }

        let obj = object_addr as *mut ObjectHeader;
        let class_id = (*obj).class_id;
        let shape_addr = (*obj).keys_array as usize;
        let key_name = match key_as_str(key) {
            Some(name) => name,
            None => return (shape_addr, class_id, gc_type, false),
        };
        let expected_shape_addr = expected_keys as usize;
        let valid = class_id == expected_class_id
            && shape_addr == expected_shape_addr
            && expected_field_index < (*obj).field_count
            && plain_array_index_guard(expected_keys, expected_field_index, true)
            && object_key_matches_field(obj, key, expected_field_index)
            && (!require_raw_f64
                || (is_plain_number_bits(value_bits)
                    && crate::gc::layout_typed_raw_f64_slot_for_user(
                        object_addr,
                        expected_field_index as usize,
                    )))
            && !class_setter_in_chain(class_id, &key_name)
            && !descriptor_blocks_class_field_set(object_addr, class_id, &key_name);
        (shape_addr, class_id, gc_type, valid)
    }
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_class_field_set_guard(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_keys: *const ArrayHeader,
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
            expected_keys,
            expected_field_index,
            require_raw_f64 != 0,
            value_bits,
        ) as i32;
    }
    let (shape_addr, class_id, gc_type, contract_valid) = class_field_set_contract(
        receiver,
        expected_class_id,
        expected_keys,
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
    expected_keys: *const ArrayHeader,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    value: f64,
    require_raw_f64: i32,
) {
    let guard_ok = js_typed_feedback_class_field_set_guard(
        site_id,
        receiver,
        expected_class_id,
        expected_keys,
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
#[no_mangle]
pub extern "C" fn js_class_field_get_ic(
    site_id: u64,
    receiver: f64,
    expected_class_id: u32,
    expected_keys: *const ArrayHeader,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: i32,
) -> f64 {
    let guard_ok = js_typed_feedback_class_field_get_guard(
        site_id,
        receiver,
        expected_class_id,
        expected_keys,
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

    crate::typed_feedback::js_typed_feedback_record_fallback_call(site_id);
    let obj_bits = receiver.to_bits();
    let key_raw = key as u64 & crate::value::POINTER_MASK;
    crate::object::js_object_get_field_by_name_f64(
        obj_bits as *const ObjectHeader,
        key_raw as *const crate::StringHeader,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_typed_feedback_native_call_method(
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
pub unsafe extern "C" fn js_typed_feedback_native_call_method_by_id(
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
pub unsafe extern "C" fn js_typed_feedback_native_call_method_apply(
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
pub unsafe extern "C" fn js_typed_feedback_native_call_method_apply_by_id(
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
    expected_keys: *const ArrayHeader,
    method_name_ptr: *const i8,
    method_name_len: usize,
    expected_func_ptr: *const u8,
) -> i32 {
    let bits = receiver.to_bits();
    let (shape_addr, class_id, gc_type, name_hash, contract_valid) = method_direct_call_contract(
        receiver,
        expected_class_id,
        expected_keys,
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

#[no_mangle]
pub unsafe extern "C" fn js_method_direct_shape_guard(
    receiver: f64,
    expected_class_id: u32,
    expected_keys: *const ArrayHeader,
) -> i32 {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0 || expected_class_id == 0 || expected_keys.is_null() {
        return 0;
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return 0;
    };
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
        || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || crate::object::descriptors_in_use()
        || crate::object::class_prototype_fast_guards_invalidated()
    {
        return 0;
    }
    let obj = object_addr as *const ObjectHeader;
    if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR {
        return 0;
    }
    ((*obj).class_id == expected_class_id && std::ptr::eq((*obj).keys_array, expected_keys)) as i32
}

#[no_mangle]
pub extern "C" fn js_typed_feedback_closure_direct_call_guard(
    site_id: u64,
    closure_value: f64,
    expected_func_ptr: *const u8,
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
    let func_ptr = crate::closure::get_valid_func_ptr(closure_ptr);
    let has_rest = !func_ptr.is_null() && crate::closure::lookup_closure_rest(func_ptr).is_some();
    let declared = if func_ptr.is_null() {
        None
    } else {
        crate::closure::lookup_closure_arity(func_ptr)
    };
    let contract_valid = !expected_func_ptr.is_null()
        && !func_ptr.is_null()
        && func_ptr == expected_func_ptr
        && func_ptr != crate::closure::BOUND_METHOD_FUNC_PTR
        && !has_rest
        && declared.unwrap_or(expected_arity) == expected_arity
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
mod keep_guard_symbols {
    use super::*;
    #[used] static G0: extern "C" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, i32) -> i32 = js_typed_feedback_class_field_get_guard;
    #[used] static G1: extern "C" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, f64, i32) -> i32 = js_typed_feedback_class_field_set_guard;
    #[used] static G1C: extern "C" fn(u64, u64, u64, f64) = js_class_field_set_fallback;
    #[used] static G1D: extern "C" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, f64, i32) = js_class_field_set_ic;
    #[used] static G1E: extern "C" fn(u64, f64, u32, *const ArrayHeader, *const crate::StringHeader, u32, i32) -> f64 = js_class_field_get_ic;
    #[used] static G2: unsafe extern "C" fn(u64, f64, u32, *const ArrayHeader, *const i8, usize, *const u8) -> i32 = js_typed_feedback_method_direct_call_guard;
    #[used] static G3: extern "C" fn(u64, f64, *const u8, u32, u32) -> i32 = js_typed_feedback_closure_direct_call_guard;
    #[used] static G4: unsafe extern "C" fn(f64, u32, *const ArrayHeader) -> i32 = js_method_direct_shape_guard;
}
