use super::*;

fn object_has_own_key_bytes(obj: *const ObjectHeader, key_bytes: &[u8]) -> bool {
    if obj.is_null() || key_bytes.is_empty() || key_bytes.len() > 4096 {
        return false;
    }
    let object_addr = normalize_raw_object_addr(obj as u64);
    let (shape_id, _, heap_type) = object_shape(object_addr);
    if heap_type != crate::gc::GC_TYPE_OBJECT as u16 || shape_id == 0 {
        return false;
    }
    unsafe {
        let obj = object_addr as *const ObjectHeader;
        // An object with an overflow lists every own key there; its layout
        // does not (object model step 4).
        if let Some(present) = crate::object::overflow::own_has_bytes(obj, key_bytes) {
            return present;
        }
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

/// Does `method_name` on an instance of `class_id` still resolve to the
/// compiled body `expected_func_ptr`?
///
/// A built holder chain resolves the name through its shapes. The data slot
/// carries the declaration's function object, including its retained raw
/// body ABI. A lazy holder declines until a collecting path materializes it.
fn declared_method_resolves_to(class_id: u32, method_name: &str, expected_func_ptr: usize) -> bool {
    if class_id == 0 || expected_func_ptr == 0 {
        return false;
    }
    let holder = crate::object::class_holder_prototype(class_id);
    if holder.is_null() {
        return false;
    }
    let key =
        crate::object::native_call_method::class_holder::MethodKey::bytes(method_name.as_bytes());
    let value = match unsafe {
        crate::object::native_call_method::class_holder::chain_method(holder, &key)
    } {
        crate::object::native_call_method::class_holder::ChainMethod::Data { value, .. } => value,
        _ => return false,
    };
    unsafe { crate::object::class_method_value_target(value) }
        .is_some_and(|target| target.0 == expected_func_ptr)
}

fn method_direct_call_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    method_name_ptr: *const i8,
    method_name_len: usize,
    expected_func_ptr: *const u8,
) -> (u32, u32, u16, u64, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    let (shape_id, class_id, gc_type) = object_shape(object_addr);
    let Some(method_bytes) = method_name_bytes(method_name_ptr, method_name_len) else {
        return (shape_id, class_id, gc_type, 0, false);
    };
    let Some(method_name) = method_name_str(method_name_ptr, method_name_len) else {
        return (shape_id, class_id, gc_type, hash_bytes(method_bytes), false);
    };
    let name_hash = hash_bytes(method_bytes);
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_layout_word(expected_shape_id)
        || expected_func_ptr.is_null()
    {
        return (shape_id, class_id, gc_type, name_hash, false);
    }
    let Some(gc_header) = gc_header_for_user_addr(object_addr) else {
        return (shape_id, class_id, gc_type, name_hash, false);
    };
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (*gc_header)._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES != 0
        {
            return (shape_id, class_id, gc_type, name_hash, false);
        }
        let obj = object_addr as *const ObjectHeader;
        if !crate::object::object_is_regular(obj) {
            return (shape_id, class_id, gc_type, name_hash, false);
        }
        if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID
            || (*obj).class_id != expected_class_id
            || crate::object::shapes::object_layout_stamp(obj) != expected_shape_id
            || shape_id != expected_shape_id
        {
            return (shape_id, class_id, gc_type, name_hash, false);
        }
        if object_has_own_key_bytes(obj, method_bytes) {
            return (shape_id, class_id, gc_type, name_hash, false);
        }
    }

    let expected_func = expected_func_ptr as usize;
    let valid = declared_method_resolves_to(class_id, method_name, expected_func);
    (shape_id, class_id, gc_type, name_hash, valid)
}

/// Borrow the key text for the guard's side-table lookups. Every consumer
/// (`descriptor_blocks_class_field_*`, `get_accessor_descriptor`,
/// `get_property_attrs`) reads Rust-side tables
/// and allocates nothing on the GC heap, so the payload cannot move while the
/// borrow is live; the `String` this used to return was one `malloc` + UTF-8
/// scan per guarded class-field access.
fn key_as_str<'a>(key: *const crate::StringHeader) -> Option<&'a str> {
    if !valid_string_key(key) {
        return None;
    }
    unsafe { crate::string::header_str_checked(key) }
}

fn descriptor_blocks_class_field_get(obj_addr: usize, _class_id: u32, key_name: &str) -> bool {
    crate::object::get_accessor_descriptor(obj_addr, key_name).is_some()
}

/// Decide representation after the expected ShapeId and slot bounds match.
/// F64 lanes are a fact of that shape; stores generalize it before writing
/// values that would violate its representation.
fn class_field_raw_f64_layout_contract(
    expected_shape_id: u32,
    field_index: u32,
    require_raw_f64: bool,
) -> bool {
    !require_raw_f64
        || crate::object::field_rep_store::shape_slot_is_f64(expected_shape_id, field_index)
}

fn class_field_get_contract(
    receiver: f64,
    expected_class_id: u32,
    expected_shape_id: u32,
    key: *const crate::StringHeader,
    expected_field_index: u32,
    require_raw_f64: bool,
) -> (u32, u32, u16, bool) {
    let object_addr = normalize_raw_object_addr(receiver.to_bits());
    if object_addr == 0
        || expected_class_id == 0
        || !crate::object::shapes::is_layout_word(expected_shape_id)
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
        let shape_id = crate::object::shapes::object_layout_stamp(obj);
        let Some(descriptor) = crate::object::shapes::shape_descriptor_for_word(shape_id) else {
            return (shape_id, class_id, gc_type, false);
        };
        let key_name = match key_as_str(key) {
            Some(name) => name,
            None => return (shape_id, class_id, gc_type, false),
        };
        let keys = descriptor.keys as usize as *const ArrayHeader;
        let valid = crate::object::object_is_regular(obj)
            && class_id == expected_class_id
            && crate::object::field_rep_store::completed_shape_matches_layout(
                shape_id,
                expected_shape_id,
            )
            && expected_field_index < descriptor.live_inline_slot_count
            && expected_field_index < descriptor.logical_key_count
            && plain_array_index_guard(keys, expected_field_index, true)
            && object_key_matches_field(obj, key, expected_field_index)
            && class_field_raw_f64_layout_contract(
                expected_shape_id,
                expected_field_index,
                require_raw_f64,
            )
            && !descriptor_blocks_class_field_get(object_addr, class_id, key_name);
        (shape_id, class_id, gc_type, valid)
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
        || !crate::object::shapes::is_layout_word(expected_shape_id)
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
        // The completed layout or a compatible ConstFn shape proves the offsets and
        // numeric lanes. The setter separately requires the live boxed lane
        // to be Any before skipping the checked store funnel.
        let shape_ok = (*obj).class_id == expected_class_id
            && crate::object::field_rep_store::completed_shape_matches_layout(
                shape_id,
                expected_shape_id,
            )
            && descriptor.is_some_and(|facts| {
                facts.object_kind.is_ordinary_layout()
                    && expected_field_index < facts.live_inline_slot_count
            });
        shape_ok
            && class_field_raw_f64_layout_contract(
                expected_shape_id,
                expected_field_index,
                require_raw_f64,
            )
    }
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
    if !typed_feedback_enabled() {
        return class_field_fast_contract(
            receiver,
            expected_class_id,
            expected_shape_id,
            expected_field_index,
            require_raw_f64 != 0,
        ) as i32;
    }
    let (shape_id, class_id, gc_type, contract_valid) = class_field_get_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        key,
        expected_field_index,
        require_raw_f64 != 0,
    );
    let observation = Observation {
        source: ObservationSource::Property,
        shape_id,
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

#[inline]
fn receiver_has_numeric_proof_shape(obj: *const ObjectHeader) -> bool {
    crate::object::shapes::shape_object_kind_of_receiver(unsafe {
        crate::object::shapes::object_shape_stamp(obj)
    }) == Some(crate::object::shapes::ShapeObjectKind::OrdinaryNumericProof)
}

/// The class-field store's cold arm: record the miss for typed feedback, then
/// route the write by name (frozen / accessor / non-writable /
/// setter-in-chain). Its one caller is a statically shape-proven raw-f64 store
/// whose value is not a plain finite number (`property_set.rs`); every other
/// class-field store is the static-key store site (#12483). `obj_bits` keeps
/// the full NaN-box tag (the by-name setter inspects it for proxy/exotic
/// dispatch before masking to the heap address); `key_raw` is the
/// POINTER_MASK-stripped key handle.
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
    let (shape_id, class_id, gc_type) = object_shape(object_addr);
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
        shape_id,
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
    // #11915: the ONE generic path of a failed `Pristine` proof. A receiver
    // whose builtin family's prototype was modified reads the method for
    // real; only the builtin itself continues to the by-name natives below.
    if !crate::object::builtin_proto::receiver_pristine(object)
        && valid_method_name(method_name_ptr, method_name_len)
    {
        let name = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        if let Some(value) =
            crate::object::builtin_proto::redirect_call(object, name, args_ptr, args_len)
        {
            return value;
        }
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
    let (shape_id, class_id, gc_type) = object_shape(object_addr);
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
        shape_id,
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
    if !crate::object::builtin_proto::receiver_pristine(object)
        && valid_method_name(method_name_ptr, method_name_len)
    {
        let name = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        if let Some(value) =
            crate::object::builtin_proto::redirect_call_array(object, name, args_array)
        {
            return value;
        }
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
    let (shape_id, class_id, gc_type, name_hash, contract_valid) = method_direct_call_contract(
        receiver,
        expected_class_id,
        expected_shape_id,
        method_name_ptr,
        method_name_len,
        expected_func_ptr,
    );
    let observation = Observation {
        source: ObservationSource::Method,
        shape_id,
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
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
            != 0
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
    if shape_id == 0 || receiver_has_numeric_proof_shape(obj) {
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
) -> i32 {
    if expected_class_id == 0 || !crate::object::shapes::is_layout_word(expected_shape_id) {
        return 0;
    }
    let mut shape_id = 0;
    let class_id = js_method_direct_shape_class(receiver, &mut shape_id);
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

        shape_id: 0,
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

// #1764 (follow-up): the guard helpers in this submodule are codegen-emitted
// `#[no_mangle]` exports with no Rust-side caller, so the auto-optimize
// whole-program thin-LTO + `strip=true` build internalizes + dead-strips them
// — dangling the codegen call at final link (`Undefined symbols:
// _js_typed_feedback_class_field_get_guard` for a typed method clone).
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
    #[used(compiler)] static G1C: extern "C" fn(u64, u64, u64, f64) = js_class_field_set_fallback;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G2: unsafe extern "C" fn(u64, f64, u32, u32, *const i8, usize, *const u8) -> i32 = js_typed_feedback_method_direct_call_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G3: extern "C" fn(u64, f64, *const crate::closure::JsFunctionInfo, u32, u32) -> i32 = js_typed_feedback_closure_direct_call_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G3B: extern "C" fn(f64, *const crate::closure::JsFunctionInfo) -> u64 = js_closure_exact_func_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G4: unsafe extern "C" fn(f64, u32, u32) -> i32 = js_method_direct_shape_guard;
    #[cfg(feature = "keepalive-anchors")]
    #[used(compiler)] static G4B: unsafe extern "C" fn(f64, *mut u32) -> u32 = js_method_direct_shape_class;
}
