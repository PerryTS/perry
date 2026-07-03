//! has_property + wide-key index + native-module own-field probe.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

/// Check if a property exists in an object by its string key name
/// Returns NaN-boxed true if the property exists, NaN-boxed false otherwise
/// This implements the JavaScript 'in' operator: "key" in obj
#[no_mangle]
pub extern "C" fn js_object_has_property(obj: f64, key: f64) -> f64 {
    let nanbox_false = f64::from_bits(0x7FFC_0000_0000_0003u64); // TAG_FALSE
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64); // TAG_TRUE

    // The [[Prototype]] walk at the tail recurses through this entry point (a
    // loop bounded at 1024 in earlier PRs became a recursion). `setPrototypeOf`
    // rejects cycles, but a pathologically deep chain — or a cycle that slips
    // past cycle-detection (see #b201538f3) — would otherwise overflow the
    // stack. Bound total recursion depth to the same 1024 the manual walk below
    // already caps at, so this introduces no new false-negative under that limit.
    thread_local! {
        static HP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    struct DepthGuard;
    impl Drop for DepthGuard {
        fn drop(&mut self) {
            HP_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }
    if HP_DEPTH.with(|d| d.get()) > 1024 {
        return nanbox_false;
    }
    HP_DEPTH.with(|d| d.set(d.get() + 1));
    let _depth_guard = DepthGuard;

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    // A Proxy is a small registered id (POINTER_TAG with a tiny pointer), not a
    // heap object. Falling through to the symbol/class/pointer paths below would
    // deref the fake pointer (or call symbol helpers that do) and segfault. Route
    // `key in proxy` through the proxy `has` trap and ToBoolean-coerce, matching
    // `Reflect.has`.
    if crate::proxy::js_proxy_is_proxy(obj) != 0 {
        let r = crate::proxy::js_proxy_has(obj, key);
        return if crate::value::js_is_truthy(r) != 0 {
            nanbox_true
        } else {
            nanbox_false
        };
    }

    // A Web Fetch / zlib handle-band value (Headers/Request/Response, zlib
    // streams) at or above the fetch band is a registry id, not a heap object —
    // the pointer paths below would dereference the id and segfault. `key in
    // <handle>` has no own-property meaning for these, so report `false`.
    // Common/small handles (below the fetch band) are intentionally NOT caught
    // here: they fall through to the registered small-handle property path later
    // in this function. Same family as the string_from_header / inline-`.length`
    // guards.
    if obj_val.is_pointer() {
        let addr = (obj_val.bits() & crate::value::POINTER_MASK) as usize;
        if addr >= crate::value::addr_class::COMMON_HANDLE_BAND_END
            && crate::value::addr_class::is_handle_band(addr)
        {
            if key_val.is_any_string() {
                unsafe {
                    if let Some(dispatch) = super::super::class_registry::handle_property_dispatch()
                    {
                        let key_ptr = crate::value::js_get_string_pointer_unified(key)
                            as *const crate::StringHeader;
                        let name_ptr =
                            (key_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key_ptr).byte_len as usize;
                        let result = dispatch(addr as i64, name_ptr, name_len);
                        if result.to_bits() != crate::value::TAG_UNDEFINED {
                            return nanbox_true;
                        }
                    }
                }
            }
            return nanbox_false;
        }
    }

    // #1758: a SYMBOL key. The class-ref path below + the keys_array scan
    // (string keys only) can't see a class-object's static `[Sym]` props nor
    // ones inherited from a class-expression parent. Delegate to the symbol
    // resolver (handles INT32 class refs, POINTER class-objects, own +
    // prototype-chain), mirroring the string-key "present-and-not-undefined"
    // semantics. Fixes effect's `Predicate.hasProperty(classObj, TypeId)`
    // (`isSchema` → `dual` → `transformOrFail`) and `Sym in obj` generally.
    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        let v = unsafe { crate::symbol::js_object_get_symbol_property(obj, key) };
        return if v.to_bits() != crate::value::TAG_UNDEFINED {
            nanbox_true
        } else {
            nanbox_false
        };
    }

    // Refs #420 / #618: `Symbol in ClassRef` — drizzle's `entityKind in cls`.
    // Class refs are INT32-tagged. Check CLASS_STATIC_SYMBOLS for symbol
    // keys and CLASS_DYNAMIC_PROPS for string keys.
    {
        let bits = obj.to_bits();
        if (bits >> 48) == 0x7FFE {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            // Symbol key path.
            if crate::symbol::class_static_symbol_lookup(class_id, key).is_some() {
                return nanbox_true;
            }
            // String key path: check CLASS_DYNAMIC_PROPS via the get-by-name fn.
            if !key_val.is_pointer() && key_val.is_string() {
                // is_string covers heap StringHeader. Route through the
                // CLASS_DYNAMIC_PROPS-aware get fn.
            }
            // Fallback: emit false for class refs that aren't in either table.
            return nanbox_false;
        }
    }

    if !obj_val.is_pointer() {
        // Web Streams handles are raw finite f64 ids, not NaN-boxed pointers.
        // Property reads already route these through the stdlib handle
        // dispatcher; mirror that for the `in` operator so `"closed" in reader`
        // observes getter-backed handle properties without dereferencing the id.
        let f = f64::from_bits(obj.to_bits());
        if key_val.is_any_string() && f.is_finite() && f > 0.0 && f.fract() == 0.0 {
            let id = f as usize;
            if crate::value::addr_class::is_stream_id_band(id) {
                if let Some(probe) = crate::object::stream_handle_probe() {
                    unsafe {
                        if probe(id) {
                            if let Some(dispatch) =
                                super::super::class_registry::handle_property_dispatch()
                            {
                                let key_ptr = crate::value::js_get_string_pointer_unified(key)
                                    as *const crate::StringHeader;
                                let name_ptr = (key_ptr as *const u8)
                                    .add(std::mem::size_of::<crate::StringHeader>());
                                let name_len = (*key_ptr).byte_len as usize;
                                let result = dispatch(id as i64, name_ptr, name_len);
                                if result.to_bits() != crate::value::TAG_UNDEFINED {
                                    return nanbox_true;
                                }
                            }
                        }
                    }
                }
            }
        }
        return nanbox_false;
    }

    let obj_addr = obj_val.bits() & 0x0000_FFFF_FFFF_FFFF;
    // Date / RegExp / Error exotic instances: own expando props + builtin
    // slots + prototype methods. The generic pointer path below would
    // bit-cast the cell as an `ObjectHeader`.
    if let Some(kind) = super::super::exotic_expando::exotic_expando_kind(obj_addr as usize) {
        use super::super::exotic_expando::ExoticKind;
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let Some(kb) = (unsafe { crate::string::js_string_key_bytes(key_val, &mut sso) }) else {
            return nanbox_false;
        };
        let Ok(name) = std::str::from_utf8(kb) else {
            return nanbox_false;
        };
        if super::super::exotic_expando::exotic_has_own_property(kind, obj_addr as usize, name) {
            return nanbox_true;
        }
        let builtin_own = match kind {
            ExoticKind::RegExp => name == "lastIndex",
            ExoticKind::Error => matches!(name, "message" | "stack"),
            // Temporal built-in fields (year/month/calendar/…) are prototype
            // getters, not own data properties (like Date). Promise's
            // then/catch/finally are prototype methods, not own props.
            ExoticKind::Date | ExoticKind::Temporal | ExoticKind::Promise => false,
        };
        if builtin_own {
            return nanbox_true;
        }
        // Inherited prototype members (`"getTime" in date`, `"exec" in re`,
        // `"name" in err`, `"toString" in any`): the per-kind get arms in
        // `js_object_get_field_by_name` already resolve prototype methods,
        // so reuse them via a value-level read.
        let key_hdr =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        if !key_hdr.is_null() {
            let v = js_object_get_field_by_name(obj_addr as *const ObjectHeader, key_hdr);
            if !v.is_undefined() {
                return nanbox_true;
            }
        }
        return nanbox_false;
    }
    if obj_addr >= 0x10000 {
        if crate::typedarray::lookup_typed_array_kind(obj_addr as usize).is_some() {
            let ta = obj_addr as *const crate::typedarray::TypedArrayHeader;
            if key_val.is_any_string() {
                let key_str =
                    crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
                // `in` is [[HasProperty]], not [[HasOwnProperty]] — ordinary
                // keys consult the prototype chain (`"subarray" in ta`,
                // inherited `Object.prototype` expandos), while canonical
                // numeric indices stay bounds-only.
                let present =
                    unsafe { crate::typedarray_props::typed_array_has_property(ta, key_str) };
                return if present { nanbox_true } else { nanbox_false };
            }
            if key_val.is_int32() {
                let index = key_val.as_int32();
                let present = unsafe { index >= 0 && (index as u32) < (*ta).length };
                return if present { nanbox_true } else { nanbox_false };
            }
            if key_val.is_number() {
                let f = f64::from_bits(key_val.bits());
                let present = unsafe {
                    f.is_finite()
                        && f >= 0.0
                        && f.fract() == 0.0
                        && f <= i32::MAX as f64
                        && (f as u32) < (*ta).length
                };
                return if present { nanbox_true } else { nanbox_false };
            }
            return nanbox_false;
        }
        let obj_ptr = obj_addr as *mut ObjectHeader;
        unsafe {
            if !obj_ptr.is_null() && (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID {
                let key_ptr =
                    crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
                let present = super::super::native_module::read_native_module_name(obj_ptr)
                    .as_deref()
                    .zip(super::super::has_own_helpers::str_from_string_header(
                        key_ptr,
                    ))
                    .map(|(module, key)| {
                        super::super::native_module::native_module_vtable()
                            .is_some_and(|vt| (vt.has_enumerable_key)(module, key))
                    })
                    .unwrap_or(false);
                return if present { nanbox_true } else { nanbox_false };
            }
        }
    }
    // Small handle receiver (`"prop" in crypto.createDiffieHellman(...)`,
    // Fastify handles, etc.). The generic object path below would treat the
    // handle id as an ObjectHeader pointer and can crash while reading
    // `keys_array`. Mirror the property-get IC miss path: ask the registered
    // handle property dispatcher whether the property resolves to a real
    // value.
    if crate::value::addr_class::is_small_handle(obj_addr as usize) {
        // #1781: accept inline SSO short keys (`"id" in handle`) — is_string()
        // is STRING_TAG-only, so a <=5-char key skipped the handle dispatcher
        // and `in` wrongly returned false. Materialize SSO bytes to a heap
        // header before reading name_ptr/name_len.
        if key_val.is_any_string() {
            unsafe {
                if let Some(dispatch) = super::super::class_registry::handle_property_dispatch() {
                    let key_ptr = crate::value::js_get_string_pointer_unified(key)
                        as *const crate::StringHeader;
                    let name_ptr =
                        (key_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key_ptr).byte_len as usize;
                    let result = dispatch(obj_addr as i64, name_ptr, name_len);
                    if result.to_bits() != crate::value::TAG_UNDEFINED {
                        return nanbox_true;
                    }
                }
            }
        }
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    // Private names are never reflectable via `Reflect.has` / `in`: a
    // `#name`-prefixed string key on a class instance is a private element
    // stored in an internal slot, invisible to ordinary [[HasProperty]]. The
    // genuine private brand check (`#name in obj`) routes through
    // `js_private_brand_check`, not here. Mirrors `js_object_has_own`'s
    // `#`-hiding (gated on `class_id != 0`).
    if unsafe { (*obj_ptr).class_id != 0 } && key_val.is_any_string() {
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        if let Some(k) = unsafe { super::super::has_own_helpers::str_from_string_header(key_ptr) } {
            if k.starts_with('#') {
                return nanbox_false;
            }
        }
    }

    if unsafe { (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID } {
        if !key_val.is_any_string() {
            return nanbox_false;
        }
        let key_str =
            crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
        if key_str.is_null() {
            return nanbox_false;
        }
        let key_name =
            match unsafe { super::super::has_own_helpers::str_from_string_header(key_str) } {
                Some(name) => name,
                None => return nanbox_false,
            };
        let present = unsafe { read_native_module_name(obj_ptr) }
            .as_deref()
            .is_some_and(|module_name| {
                super::super::native_module::native_module_vtable()
                    .is_some_and(|vt| (vt.has_enumerable_key)(module_name, key_name))
            });
        return if present { nanbox_true } else { nanbox_false };
    }

    // Issue #323: array fast path. `n in arr` with a numeric key was always
    // returning false because the receiver was treated as ObjectHeader and
    // the key-is-string guard below rejected the numeric key. Detect an
    // ArrayHeader by GC type byte; for numeric keys check `index < length`
    // and slot != TAG_HOLE (distinguishes a hole from an explicit
    // `arr[i] = undefined` write, the latter overwrites HOLE with UNDEFINED).
    if (obj_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                // Issue #233: resolve a grow forwarding pointer so `index in arr`
                // / `arr.hasOwnProperty(i)` stay correct after `arr.length = N`.
                let arr = crate::array::clean_arr_ptr(obj_ptr as *const crate::array::ArrayHeader);
                let length = (*arr).length;
                // A Proxy installed as the array's `[[Prototype]]`
                // (`Object.setPrototypeOf(arr, proxy)`) — `array_spec_has_index`
                // only recognizes a *real array* custom prototype, so a Proxy
                // hop is silently treated as absent. Recover it here so the
                // idx/string-key misses below can fall back to the proxy's
                // `[[HasProperty]]` instead of a bare `false` (ECMA-262 10.1.7.1
                // step 5).
                let proxy_proto =
                    super::super::prototype_chain::object_static_prototype(obj_ptr as usize)
                        .filter(|&b| (b >> 48) == 0x7FFD)
                        .map(f64::from_bits)
                        .filter(|&v| crate::proxy::js_proxy_is_proxy(v) != 0);
                // Numeric key: extract the index. Accept both NaN-boxed i32
                // and plain f64 (e.g. literal `1`) provided it's a
                // non-negative integer in range.
                let idx: Option<u32> = if key_val.is_int32() {
                    let i = key_val.as_int32();
                    if i >= 0 {
                        Some(i as u32)
                    } else {
                        None
                    }
                } else if key_val.is_number() {
                    let f = f64::from_bits(key_val.bits());
                    if f >= 0.0 && f.fract() == 0.0 && f < u32::MAX as f64 {
                        Some(f as u32)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(idx) = idx {
                    let _ = length;
                    // Spec HasProperty: own (dense slot / sparse named prop /
                    // accessor descriptor) OR inherited — a custom array
                    // [[Prototype]], `Array.prototype[i]`, or an
                    // `Object.prototype` index (data or accessor; test262
                    // sort/precise-comparefn-throws checks `'2' in array`
                    // against an Object.prototype accessor).
                    if crate::array::array_spec_has_index(arr, idx) {
                        return nanbox_true;
                    }
                    if crate::array::object_prototype_has_index_prop(idx) {
                        return nanbox_true;
                    }
                    if let Some(proxy) = proxy_proto {
                        let idx_str = idx.to_string();
                        let key_ptr = crate::string::js_string_from_bytes(
                            idx_str.as_ptr(),
                            idx_str.len() as u32,
                        );
                        let key_val = f64::from_bits(
                            crate::value::js_nanbox_string(key_ptr as i64).to_bits(),
                        );
                        return if crate::value::js_is_truthy(crate::proxy::js_proxy_has(
                            proxy, key_val,
                        )) != 0
                        {
                            nanbox_true
                        } else {
                            nanbox_false
                        };
                    }
                    return nanbox_false;
                }
                if key_val.is_any_string() {
                    let key_str = crate::value::js_get_string_pointer_unified(key)
                        as *const crate::StringHeader;
                    if !key_str.is_null() {
                        if let Some(key_name) =
                            super::super::has_own_helpers::str_from_string_header(key_str)
                        {
                            if super::super::has_own_helpers::array_own_key_present(arr, key_str) {
                                return nanbox_true;
                            }
                            if let Some(idx) = super::super::canonical_array_index(key_name) {
                                // Same spec HasProperty protocol as the
                                // numeric-key arm above: own + inherited
                                // (custom array proto / Array.prototype /
                                // Object.prototype data-or-accessor index;
                                // test262 sort/precise-comparefn-throws does
                                // `'2' in array`).
                                if crate::array::array_spec_has_index(arr, idx)
                                    || crate::array::object_prototype_has_index_prop(idx)
                                {
                                    return nanbox_true;
                                }
                            } else if array_prototype_property_value(key_name, obj_ptr as usize)
                                .is_some()
                            {
                                return nanbox_true;
                            }
                            if let Some(proxy) = proxy_proto {
                                return if crate::value::js_is_truthy(crate::proxy::js_proxy_has(
                                    proxy, key,
                                )) != 0
                                {
                                    nanbox_true
                                } else {
                                    nanbox_false
                                };
                            }
                        }
                    }
                }
                return nanbox_false;
            }
            // #1758: a CLOSURE receiver (functions ARE objects in JS, so
            // `key in fn` is valid). Pre-fix this fell through to the
            // keys_array scan below, which read `(*obj_ptr).keys_array` at
            // the closure's capture-slot offset — a NaN-boxed value, not a
            // real *ArrayHeader — and SIGSEGV'd in `js_array_length`. effect's
            // `dual`-wrapped helpers reach here (`<key> in someClosure` deep in
            // the fiber runtime). Mirror the closure read path
            // (`js_object_get_field_by_name`: `length` → arity, others →
            // CLOSURE_DYNAMIC_PROPS): present-and-not-undefined ⇒ true.
            if (*gc_header).obj_type == crate::gc::GC_TYPE_CLOSURE {
                if !key_val.is_any_string() {
                    return nanbox_false;
                }
                let key_str =
                    crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;
                if key_str.is_null() {
                    return nanbox_false;
                }
                // `'caller' in fn` / `'arguments' in fn` — HasProperty must
                // NOT run the poisoned getter (which throws). The accessor
                // exists on Function.prototype, so the answer is true.
                // Refs test262 S13.2_A8_T1/T2.
                if let Some(key_name) =
                    super::super::has_own_helpers::str_from_string_header(key_str)
                {
                    if matches!(key_name, "caller" | "arguments") {
                        return nanbox_true;
                    }
                }
                let v = js_object_get_field_by_name(obj_ptr, key_str);
                return if v.is_undefined() {
                    nanbox_false
                } else {
                    nanbox_true
                };
            }
        }
    }

    // #1781: accept inline SSO short keys here too — `"abc" in obj` for a
    // <=5-char key arrives as a SHORT_STRING_TAG value that is_string()
    // rejects, so `in` wrongly returned false. Materialize to a heap header
    // (stored keys in keys_array are always heap, so js_string_equals works).
    if !key_val.is_any_string() {
        return nanbox_false;
    }

    let key_str = crate::value::js_get_string_pointer_unified(key) as *const crate::StringHeader;

    unsafe {
        if ordinary_has_property(obj_ptr, key_str) {
            nanbox_true
        } else {
            nanbox_false
        }
    }
}

/// `OrdinaryHasProperty(O, P)` (ECMA-262 10.1.7.1) for ordinary heap objects:
/// true when `P` is an own property of `O` OR of any object in `O`'s
/// `[[Prototype]]` chain.
///
/// Pre-fix the `in`-operator tail only scanned the receiver's own `keys_array`
/// and, fatally, treated a present key whose stored value is `undefined` as
/// absent. That conflated three distinct cases: a deleted property (`delete`
/// actually removes the key from `keys_array`, so it never reaches here), an
/// explicit `obj.x = undefined` (own, present), and an own *accessor* whose
/// backing slot reads `undefined`. It also never walked the prototype chain, so
/// inherited data/accessor properties — and `ToPropertyDescriptor`'s
/// `HasProperty(desc, "value"/"get"/...)` reads on a descriptor whose fields are
/// inherited or accessor-backed — wrongly reported absent.
///
/// This implements the spec walk: at each level check own-key presence (a key in
/// `keys_array`, regardless of stored value) and the own-accessor side table,
/// then advance to the recorded `[[Prototype]]`. When the chain ends without an
/// explicit prototype, an inherited `Object.prototype` method still counts.
unsafe fn ordinary_has_property(
    obj_ptr: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> bool {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let key_name = super::super::has_own_helpers::str_from_string_header(key);
    // Wall 10 follow-up: if `Object.setPrototypeOf(instance, proto)` recorded an
    // explicit replacement `[[Prototype]]` for THIS instance, the class-vtable
    // fallback below must be skipped — the recorded chain (walked above) is now
    // authoritative, so a key that was deleted/replaced off the prototype must
    // not be resurrected from the original class vtable.
    let has_recorded_prototype =
        super::super::prototype_chain::object_static_prototype(obj_ptr as usize).is_some();
    let mut cur = obj_ptr;
    let mut last_valid = obj_ptr;
    let mut guard = 0u32;
    loop {
        guard += 1;
        if guard > 1024 || cur.is_null() || !super::super::is_valid_obj_ptr(cur as *const u8) {
            break;
        }
        last_valid = cur;
        // A prototype hop can land on a real Array (`Foo.prototype = [1,2,3]`,
        // test262 reduce/reduceRight `subclassed array` cases): its layout is
        // `ArrayHeader { length, capacity }` + inline elements, NOT the
        // `ObjectHeader.keys_array` shape `own_key_present` expects, so reading
        // `(*cur).keys_array` off an array node finds garbage (or nothing) and
        // every indexed/`"length"` lookup wrongly reports absent. Detect the
        // GC type and route to the array-aware own-key check instead.
        let cur_is_array = crate::value::addr_class::try_read_gc_header(cur as usize)
            .is_some_and(|hdr| hdr.obj_type == crate::gc::GC_TYPE_ARRAY);
        if cur_is_array {
            if super::super::has_own_helpers::array_own_key_present(
                cur as *const crate::array::ArrayHeader,
                key,
            ) {
                return true;
            }
        } else if super::super::own_key_present(cur as *mut ObjectHeader, key) {
            // Own data / overflow key present (value-agnostic: `delete`
            // removes the key, so a present key — even one holding
            // `undefined` — is an own property).
            return true;
        }
        // Own accessor property (also mirrored into `keys_array`, but check the
        // side table directly so a get-only accessor is never missed).
        if let Some(name) = key_name {
            if get_accessor_descriptor(cur as usize, name).is_some() {
                return true;
            }
        }
        // Advance to the recorded `[[Prototype]]`.
        let cur_addr = cur as usize;
        match super::super::prototype_chain::object_static_prototype(cur_addr) {
            Some(b) if b == TAG_NULL => return false,
            Some(b) => {
                let top16 = b >> 48;
                // A Proxy prototype hop (ECMA-262 10.1.7.1 step 5: `Return ?
                // parent.[[HasProperty]](P)`) — the small registered proxy id is
                // NOT a real heap pointer, so continuing the raw-pointer walk
                // below would misread garbage (or crash). Dispatch through the
                // proxy's own `[[HasProperty]]` (trap, or its trap-less forward
                // through further proxy targets / the eventual real target) and
                // use its boolean result directly — that call already resolves
                // the rest of the chain.
                if top16 == 0x7FFD {
                    let proto_val = f64::from_bits(b);
                    if crate::proxy::js_proxy_is_proxy(proto_val) != 0 {
                        let key_val =
                            f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                        let result = crate::proxy::js_proxy_has(proto_val, key_val);
                        return crate::value::js_is_truthy(result) != 0;
                    }
                }
                let p = if top16 == 0x7FFD {
                    (b & crate::value::POINTER_MASK) as usize
                } else if top16 == 0 && b > 0x10000 {
                    b as usize
                } else {
                    break;
                };
                if p == 0 || p == cur_addr {
                    break;
                }
                cur = p as *const ObjectHeader;
            }
            // No explicit prototype recorded — the default `Object.prototype`
            // applies (handled below), so stop the explicit walk here.
            None => break,
        }
    }
    // Wall 10 — a class instance's prototype METHODS / GETTERS / SETTERS live in
    // `CLASS_VTABLE_REGISTRY`, not as a recorded `[[Prototype]]` object with a
    // `keys_array`, so the own-key + recorded-prototype walk above misses them.
    // Check the class chain so `'method' in instance` is `true` (e.g. NestJS's
    // app Proxy gating on `'listen' in receiver`).
    if !has_recorded_prototype {
        if let Some(name) = key_name {
            let class_id = unsafe { (*obj_ptr).class_id };
            if class_id != 0
                && super::super::native_module::class_instance_has_member(class_id, name)
            {
                return true;
            }
        }
    }
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits());
    let proto = super::super::js_object_get_prototype_of(receiver);
    let proto_bits = proto.to_bits();
    if proto_bits != crate::value::TAG_NULL
        && proto_bits != crate::value::TAG_UNDEFINED
        && proto_bits != receiver.to_bits()
    {
        let key_value = crate::value::js_nanbox_string(key as i64);
        if crate::value::js_is_truthy(super::super::js_object_has_property(proto, key_value)) != 0 {
            return true;
        }
    }

    // Inherited `Object.prototype` properties (`toString`, `hasOwnProperty`, …,
    // plus any user-assigned `Object.prototype` members).
    ordinary_object_prototype_property_value(last_valid, key).is_some()
}

/// Get a field by its string key name
/// Returns the field value or undefined if the key is not found
pub(crate) unsafe fn closure_dynamic_prop_by_key(
    obj: usize,
    key: *const crate::StringHeader,
) -> Option<f64> {
    if key.is_null() {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let name = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()?;
    let val = crate::closure::closure_get_dynamic_prop(obj, name);
    if val.to_bits() != crate::value::TAG_UNDEFINED {
        return Some(val);
    }
    // #4533/#3716: reading an inherited Function/Object prototype method as a
    // value off a closure (`Error.isPrototypeOf`, `f.bind`) must yield a real
    // callable, not `undefined`, so `typeof Error.isPrototypeOf === "function"`.
    if crate::closure::is_closure_ptr(obj) {
        if let Some(method) = reified_function_method_name(name) {
            let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
            return Some(crate::closure::reify_function_method_value(
                receiver, method,
            ));
        }
    }
    None
}

/// Inherited Function/Object prototype methods that reify into a BOUND_METHOD
/// closure bound to the receiver function when read as a value.
pub(crate) fn reified_function_method_name(name: &str) -> Option<&'static [u8]> {
    match name {
        "bind" => Some(b"bind"),
        "call" => Some(b"call"),
        "apply" => Some(b"apply"),
        "isPrototypeOf" => Some(b"isPrototypeOf"),
        // `fn.toString` read as a VALUE (`original.toString.bind(original)` —
        // Next.js's unhandled-rejection extension preserves patched-function
        // toString this way). Previously read back `undefined`, so the
        // subsequent `.bind` threw "Bind must be called on a function".
        "toString" => Some(b"toString"),
        _ => None,
    }
}

pub(crate) unsafe fn native_module_own_field_by_key(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let target = std::slice::from_raw_parts(key_ptr, key_len);
    if target == b"__module__" {
        return None;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return None;
    }
    let key_count = crate::array::js_array_length(keys);
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i);
        let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if crate::string::js_string_key_bytes(stored, &mut sso_buf) == Some(target) {
            return Some(js_object_get_field(obj, i));
        }
    }
    None
}

// ─── #5054: wide-object key index ─────────────────────────────────────────────
// A `{}`-born object grown to thousands of dynamic properties pays a linear
// keys_array scan per `obj[key]` read once the 1024-entry FIELD_CACHE can't
// hold its key set — O(N) per read, quadratic for read-everything loops. For
// keys arrays past this threshold, build a key→index map once and validate
// every hit against the actual slot (same trust model as FIELD_CACHE: a
// reused keys-array address or a mutated slot fails validation and drops the
// index). Misses still fall through to the linear scan — the index is an
// accelerator, never authoritative — and a scan hit back-fills the map so
// interleaved appends stay amortized O(1).
pub(crate) const WIDE_KEY_INDEX_MIN_KEYS: usize = 257;
const WIDE_KEY_INDEX_CAPACITY: usize = 4;

struct WideKeyIndexEntry {
    keys_id: usize,
    indexed_len: u32,
    map: std::collections::HashMap<Vec<u8>, u32>,
}

thread_local! {
    static WIDE_KEY_INDEX: std::cell::RefCell<Vec<WideKeyIndexEntry>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Probe the wide-object index for `key_bytes` in the keys array identified by
/// `keys_id`. Returns a slot index whose stored key has been re-validated
/// against `key` — `None` means "not found via the index" (caller falls back
/// to the linear scan).
pub(crate) unsafe fn wide_key_index_lookup(
    keys_id: usize,
    key_bytes: &[u8],
    key: *const crate::StringHeader,
    keys: *const crate::array::ArrayHeader,
    key_count: usize,
) -> Option<u32> {
    WIDE_KEY_INDEX.with(|cell| {
        let mut table = cell.borrow_mut();
        let pos = table.iter().position(|e| e.keys_id == keys_id);
        let pos = match pos {
            Some(p) => p,
            None => {
                // Build the full map once (first occurrence wins, matching
                // linear-scan order).
                let mut map = std::collections::HashMap::with_capacity(key_count);
                let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                for i in 0..key_count {
                    let stored = crate::array::js_array_get(keys, i as u32);
                    if let Some(b) = crate::string::js_string_key_bytes(stored, &mut sso) {
                        map.entry(b.to_vec()).or_insert(i as u32);
                    }
                }
                if table.len() >= WIDE_KEY_INDEX_CAPACITY {
                    table.pop();
                }
                table.insert(
                    0,
                    WideKeyIndexEntry {
                        keys_id,
                        indexed_len: key_count as u32,
                        map,
                    },
                );
                0
            }
        };
        let entry = &mut table[pos];
        if (key_count as u32) < entry.indexed_len {
            // The keys array shrank (a delete compacted it) — slot indices
            // are no longer trustworthy. Drop and let the next read rebuild.
            table.remove(pos);
            return None;
        }
        if (key_count as u32) > entry.indexed_len {
            // Catch up on appended keys.
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            for i in entry.indexed_len as usize..key_count {
                let stored = crate::array::js_array_get(keys, i as u32);
                if let Some(b) = crate::string::js_string_key_bytes(stored, &mut sso) {
                    entry.map.entry(b.to_vec()).or_insert(i as u32);
                }
            }
            entry.indexed_len = key_count as u32;
        }
        let idx = entry.map.get(key_bytes).copied();
        match idx {
            Some(i) if (i as usize) < key_count => {
                let stored = crate::array::js_array_get(keys, i);
                if crate::string::js_string_key_matches(stored, key) {
                    if pos != 0 {
                        let e = table.remove(pos);
                        table.insert(0, e);
                    }
                    Some(i)
                } else {
                    // Stale (address reuse or in-place mutation): drop the
                    // whole entry rather than chase it.
                    table.remove(pos);
                    None
                }
            }
            _ => None,
        }
    })
}

/// Back-fill a linear-scan hit into the wide-object index (no-op when the
/// keys array has no entry — the next lookup builds it wholesale).
pub(crate) fn wide_key_index_note_hit(keys_id: usize, key_bytes: &[u8], index: u32) {
    WIDE_KEY_INDEX.with(|cell| {
        let mut table = cell.borrow_mut();
        if let Some(e) = table.iter_mut().find(|e| e.keys_id == keys_id) {
            e.map.entry(key_bytes.to_vec()).or_insert(index);
        }
    });
}
