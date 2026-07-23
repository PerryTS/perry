//! get_field_by_name_f64, IC-miss slow path, and private-brand guards.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    if (obj as usize) > 0 && (obj as usize) < 0x10000 && !key.is_null() {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            let class_id = obj as usize as u32;
            if name == "name" && !super::super::class_registry::class_is_key_deleted(class_id, name)
            {
                if let Some(cname) = super::super::class_registry::class_name_for_id(class_id) {
                    let s = crate::string::js_string_from_bytes(cname.as_ptr(), cname.len() as u32);
                    return crate::js_nanbox_string(s as i64);
                }
            }
        }
    }
    // date-fns `constructFrom`: `new date.constructor(value)`. A Date is a
    // NaN-boxed `DateCell` pointer (#2089); `js_object_get_field_by_name`
    // routes `.constructor` to the global Date constructor closure and every
    // other key to `undefined` without derefing the small cell as an object.
    let value = js_object_get_field_by_name(obj, key);
    // #4973: inherits-pattern instances (`http.Server.call(this, …)`) —
    // a read that missed every layer forwards to the aliased native handle
    // so `server.listen` / `server.address` resolve to bound callables on
    // the codegen static-typed read-then-call path.
    if value.bits() == crate::value::TAG_UNDEFINED
        && super::super::native_this_alias::alias_active()
        && !key.is_null()
    {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            if let Some(fwd) =
                super::super::native_this_alias::alias_forward_property_read(obj as usize, name)
            {
                return fwd;
            }
        }
    }
    f64::from_bits(value.bits())
}

/// Read a field by name from a *boxed* receiver, returning `undefined` when the
/// receiver is not an object.
///
/// `js_object_get_field_by_name_f64` takes an already-unboxed `*const
/// ObjectHeader` and dereferences it on faith. That is fine when codegen has
/// proven the receiver is an object, but `Response.json(data, init)` reads its
/// fields off a *runtime* `init` value that can be anything — a number, a
/// string, a symbol. A non-integer double like `3.14` unboxes to a bit pattern
/// squarely inside the heap-pointer magnitude window, so the raw read SIGSEGVs
/// (observed on `Response.json(x, 3.14)`).
///
/// This wrapper applies the same handle-band / `is_valid_obj_ptr` guard the
/// runtime fetch-option reader uses, so a non-object `init` yields `undefined`
/// fields instead of dereferencing a forged pointer. Codegen calls this with
/// the boxed value rather than re-implementing the pointer checks in IR.
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_boxed(
    receiver: f64,
    key: *const crate::StringHeader,
) -> f64 {
    let value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !value.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let raw = crate::value::js_nanbox_get_pointer(receiver);
    if raw == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // A handle-band id (a `Response`/`Request` forwarded as init) is not a heap
    // ObjectHeader; `js_object_get_field_by_name_f64` routes it through the
    // handle property dispatch, so hand it over directly.
    if crate::value::addr_class::is_handle_band(raw as usize) {
        return js_object_get_field_by_name_f64(raw as *const ObjectHeader, key);
    }
    if raw < 0x10000 || !crate::value::addr_class::is_valid_obj_ptr(raw as *const u8) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    js_object_get_field_by_name_f64(raw as *const ObjectHeader, key)
}

/// #2058: the universal `Object.prototype` methods inherited by every value,
/// including primitive numbers. Read as a property *value* (e.g.
/// `const f = n.toString`, `typeof n.isPrototypeOf`), these resolve to real
/// callable functions in Node — Perry binds them lazily via
/// `js_class_method_bind` so the value is both `typeof "function"` and
/// dispatchable through `js_native_call_method` (every name here has a
/// corresponding dispatch arm). `constructor` is excluded: it is a property
/// holding the `Number` function, not a bound method.
pub(crate) fn is_primitive_proto_method(key: &[u8]) -> bool {
    matches!(
        key,
        b"toString"
            | b"valueOf"
            | b"hasOwnProperty"
            | b"isPrototypeOf"
            | b"propertyIsEnumerable"
            | b"toLocaleString"
    )
}

/// Static-name lowering should traffic in interned property ids instead of
/// raw name bytes. The first representation is the interned heap string
/// pointer already emitted by the StringPool; the wrapper preserves the
/// existing by-name semantics while giving codegen a by-id ABI to target.
#[no_mangle]
pub extern "C" fn js_object_get_field_by_property_id_f64(
    obj: *const ObjectHeader,
    property_id: i64,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(key_ref) = crate::string::perry_string_ref_from_dispatch_id(property_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let key = if key_ref.heap.is_null() {
        crate::string::js_string_from_bytes(key_ref.ptr, key_ref.len as u32)
            as *const crate::StringHeader
    } else {
        key_ref.heap
    };
    js_object_get_field_by_name_f64(obj, key)
}

/// By-id sibling of `js_object_set_field_by_name`. See
/// `js_object_get_field_by_property_id_f64` for why the initial id
/// representation is the interned StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_property_id(
    obj: *mut ObjectHeader,
    property_id: i64,
    value: f64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(key_ref) = crate::string::perry_string_ref_from_dispatch_id(property_id, &mut scratch)
    else {
        return;
    };
    let key = if key_ref.heap.is_null() {
        crate::string::js_string_from_bytes(key_ref.ptr, key_ref.len as u32)
            as *const crate::StringHeader
    } else {
        key_ref.heap
    };
    js_object_set_field_by_name(obj, key, value);
}

pub(crate) fn is_array_method_value_name(key: &[u8]) -> bool {
    matches!(
        key,
        b"pop" | b"push" | b"shift" | b"unshift" | b"splice" | b"slice"
    )
}

pub(crate) fn set_method_value_name(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"add" => Some(b"add"),
        b"clear" => Some(b"clear"),
        b"delete" => Some(b"delete"),
        b"entries" => Some(b"entries"),
        b"forEach" => Some(b"forEach"),
        b"has" => Some(b"has"),
        b"keys" => Some(b"keys"),
        b"values" => Some(b"values"),
        b"union" => Some(b"union"),
        b"intersection" => Some(b"intersection"),
        b"difference" => Some(b"difference"),
        b"symmetricDifference" => Some(b"symmetricDifference"),
        b"isSubsetOf" => Some(b"isSubsetOf"),
        b"isSupersetOf" => Some(b"isSupersetOf"),
        b"isDisjointFrom" => Some(b"isDisjointFrom"),
        b"@@iterator" => Some(b"@@iterator"),
        _ => None,
    }
}

pub(crate) fn is_timer_handle_method_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"ref"
            | b"unref"
            | b"hasRef"
            | b"refresh"
            | b"close"
            | b"__perry_dispose__"
            // `using t = setTimeout(...)` / `t[Symbol.dispose]` — the
            // well-known dispose symbol lowers to this key. (#1213)
            | b"@@__perry_wk_dispose"
            | b"@@__perry_wk_toPrimitive"
    )
}

/// #6759 C3c: is `keys` safe to prime into a per-site PIC cache whose hit
/// path does an UNVALIDATED compare-and-load? True only for
/// `GC_FLAG_SHAPE_SHARED` arrays — those are shape-cache-resident
/// (process-rooted, so the address can never be freed and recycled under a
/// different shape). Conservative `false` for anything else.
pub(crate) unsafe fn keys_cacheable_for_pic(keys: *const crate::array::ArrayHeader) -> bool {
    if (keys as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
        && (*gc).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
}

/// Monomorphic inline cache miss handler (issue #51).
///
/// Called when the codegen-emitted shape check (`obj->keys_array == cache[0]`)
/// fails. Performs the full field lookup via `js_object_get_field_by_name`,
/// then populates the per-site cache so subsequent calls with the same shape
/// hit the inline fast path (no function call, direct field load).
///
/// `cache` layout: `[keys_array_ptr: i64, field_slot_index: i64]`
///
/// Only caches when:
/// - obj is a valid ObjectHeader (not null, not handle, not string/array/etc.)
/// - field exists and its slot index < 8 (inline allocation limit)
///
/// Overflow fields (slot >= alloc_limit) are NOT cached and fall through to
/// the slow path — the fast path loads from `obj_ptr + 24 + slot*8` which
/// would read past the inline allocation.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache: *mut [i64; 2],
) -> f64 {
    // SSO receiver — never cacheable. Route through the SSO-aware
    // `js_object_get_field_by_name` which handles `.length` inline
    // and returns undefined for other keys.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            let v = js_object_get_field_by_name(obj, key);
            return f64::from_bits(v.bits());
        }
    }
    if obj.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // A Proxy value may reach the inline-cache miss handler when a fused
    // property read `proxy.col` misses its monomorphic shape check (a Proxy
    // has no stable `keys_array`, so every read is a miss). Proxies are encoded
    // as small fake pointers in the band [0xF0000, 0x100000); deref-ing one as
    // an ObjectHeader — or passing it to `closure_dynamic_prop_by_key`, which
    // reads `CLOSURE_MAGIC` at offset 12 via `is_closure_ptr` — reads unmapped
    // memory and SIGSEGVs (drizzle's aliased-column Proxy in `findMany`). Route
    // to the proxy get dispatch first, exactly like `js_object_get_field_by_name`
    // (#2846). `js_proxy_is_proxy` validates the value is a *registered* proxy so
    // a real heap object whose address happens to be small isn't misrouted.
    {
        let addr = obj as u64;
        if crate::value::addr_class::is_proxy_id_band(addr as usize) {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy::js_proxy_get(boxed, key_f64);
            }
        }
    }
    // Only run the closure / buffer / typedarray probes on real heap
    // receivers (>= 0x100000). A Web-Fetch handle (Headers/Request/Response/
    // Blob, id in [0x40000, 0x100000)) or any other small native handle is NOT
    // a heap pointer; `closure_dynamic_prop_by_key` reaches `is_closure_ptr`,
    // which dereferences `[obj + 12]` for CLOSURE_MAGIC and SIGSEGVs on the
    // handle's unmapped low address (hit by hono's logger reading a property
    // off a Response/Headers handle). Small handles fall through to the
    // `< 0x100000` proxy / HANDLE_PROPERTY_DISPATCH routing below — matching
    // the ordering in `js_object_get_field_by_name`. The macOS heap floor
    // (0x200_0000_0000 in is_valid_obj_ptr) masked this; Linux's is 0x1000.
    if crate::value::addr_class::is_above_handle_band(obj as usize) {
        unsafe {
            if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
                return val;
            }
            // Buffers have no GcHeader. The generic IC-miss object path below may
            // inspect GC/object metadata, so mirror js_object_get_field_by_name's
            // buffer-first dispatch here.
            if crate::buffer::is_registered_buffer(obj as usize) {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
            if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
        }
    }
    // Issue #340: small-handle receivers (axios, fastify, ioredis,
    // ...) are passed here from the codegen IC miss path with the
    // lower-48 of the NaN-box stripped — `obj as usize` is the
    // raw handle id (1, 2, 3, ...). Route to HANDLE_PROPERTY_DISPATCH
    // (registered by stdlib via js_register_handle_property_dispatch)
    // so `r.status` / `r.data` and similar handle-property accesses
    // dispatch to the per-module accessor instead of silently
    // returning undefined.
    if crate::value::addr_class::is_small_handle(obj as usize) {
        // #2846: a revocable Proxy is encoded as a small fake pointer in the
        // proxy-id range (also `< 0x100000`). A generic `proxy.key` read funnels
        // here via the IC-miss path; route it to the proxy get dispatch (which
        // forwards to the target, or throws on a revoked proxy) before the
        // handle-dispatch fallback. `js_proxy_is_proxy` validates the value is a
        // registered proxy so real small handles aren't misrouted.
        {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | ((obj as u64) & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy::js_proxy_get(boxed, key_f64);
            }
        }
        // #1213: Timeout/Immediate handle methods (ref/unref/hasRef/refresh/
        // close) read as bound-method function values so `typeof t.ref ===
        // "function"` holds (the call form already works via
        // js_native_call_method). The IC fast path funnels small handles here,
        // bypassing the identical block in `js_object_get_field_by_name`, so it
        // must be mirrored.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if is_timer_handle_method_key(key_bytes) && crate::timer::is_known_timer_id(obj as i64)
            {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                return super::super::js_class_method_bind(this_f64, key_ptr, key_len);
            }
            // TextDecoder/TextEncoder registry handles — IC-miss mirror of
            // the arms in `js_object_get_field_by_name` /
            // `get_field_by_name_object_tail`; static-name reads (`td.decode`,
            // `td.encoding`) funnel here. See `text_handle_property`.
            if let Some(v) =
                crate::text::text_handle_property(obj as usize, key_bytes, key_ptr, key_len)
            {
                return f64::from_bits(v.bits());
            }
        }
        // Drizzle-sqlite blocker: synth `data.constructor` for small-handle
        // receivers — IC-miss path mirror of the constructor intercept in
        // `js_object_get_field_by_name`. Refs #645 deeper followup.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                if let Some(dispatch) = handle_property_dispatch() {
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    if bits.to_bits() != crate::value::TAG_UNDEFINED {
                        return bits;
                    }
                }
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }
        if let Some(dispatch) = handle_property_dispatch() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let bits = dispatch(obj as i64, key_ptr, key_len);
                // Wall 10 — fall back to a `setPrototypeOf(handle, proto)` member
                // (Express's augmented `res`/`req`) when the native dispatch
                // doesn't know the key. Mirrors `js_object_get_field_by_name`.
                if bits.to_bits() == crate::value::TAG_UNDEFINED {
                    if let Some(v) = crate::object::prototype_chain::object_static_prototype(
                        obj as usize,
                    )
                    .and(
                        crate::object::prototype_chain::resolve_inherited_field(obj as usize, key),
                    ) {
                        if v.bits() != crate::value::TAG_UNDEFINED {
                            return f64::from_bits(v.bits());
                        }
                    }
                }
                return bits;
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if (obj as usize) < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // When accessors are active anywhere in the program, skip the cache
    // entirely: the PIC fast path does a direct field load that bypasses
    // getter dispatch, so any object that uses defineProperty / get / set
    // would silently return the raw slot value instead of calling the
    // getter. The slow path through js_object_get_field_by_name handles
    // accessors correctly.
    let can_cache = !crate::state::state().descriptors.accessors_in_use.get();
    unsafe {
        // Issue #72: validate this really is a GC_TYPE_OBJECT before reading
        // (*obj).keys_array — otherwise an Array/String/Buffer/etc. receiver
        // (whose `object_type` byte at offset 0 happens to be 1, matching
        // OBJECT_TYPE_REGULAR for a length-1 array) would be treated as
        // cacheable and seed the per-site PIC with garbage from element[1].
        // The codegen guard funnels non-OBJECT receivers here too, so this
        // belt-and-braces check keeps the cache from being primed with
        // values that would survive into the inline hot path.
        let is_object = (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000
            && is_valid_obj_ptr(obj as *const u8)
            && {
                let gc_header =
                    (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
            };
        let is_regular = is_object && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
        if can_cache && is_regular {
            let keys = (*obj).keys_array;
            if keys.is_null() || (keys as usize) <= 0x10000 {
                let value = js_object_get_field_by_name(obj, key);
                return f64::from_bits(value.bits());
            }
            let key_count = *(keys as *const u32) as usize;
            let keys_data = (keys as *const u8).add(8) as *const f64;
            let alloc_limit =
                std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
            // #6804: stamp the receiver's stable ShapeId at PIC-miss
            // resolution, so the id-keyed FIELD_CACHE (and the future
            // id-comparing PIC) see a stamped object from its first read.
            if (*obj).class_id == 0
                && !crate::object::shapes::is_shape_id((*obj).parent_class_id)
                && !crate::regex::regex_header_has_magic(obj as *const crate::regex::RegExpHeader)
            {
                let id = crate::object::shapes::shape_id_for_keys_ensure(keys, key_count as u32);
                if id != 0 {
                    (*(obj as *mut ObjectHeader)).parent_class_id = id;
                }
            }
            for i in 0..key_count {
                let k_bits = (*keys_data.add(i)).to_bits();
                let k_ptr = (k_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
                if !k_ptr.is_null() && crate::string::js_string_equals(k_ptr, key) != 0 {
                    if i >= alloc_limit {
                        // Field is in the overflow map — fall through to the
                        // slow path which handles overflow correctly.
                        break;
                    }
                    // The codegen IC fast path computes `obj + object_header_size + slot*8`
                    // and does a direct load. Any inline slot (`i <
                    // alloc_limit`) is reachable via that path, so cache
                    // every inline slot — including the ones at index >= 8
                    // for classes whose `field_count` exceeds the
                    // MIN_FIELD_SLOTS=8 baseline (e.g. World.commandBuffer
                    // sits at slot 12). Pre-fix this branch capped the cache
                    // at `i < 8` which left every >8-slot field permanently
                    // missing the cache: every access fell through to a
                    // fresh keys_array walk + js_string_equals chain. On
                    // perf-comprehensive's hot loops that path was hit
                    // ~900k times per run (40% inclusive samples per
                    // perfcomp.profile).
                    //
                    // #6804: a stamped plain receiver primes an ID token
                    // (`stamp | PIC_ID_TOKEN_BIT`, matching the emitted
                    // PIC's discriminated compare). Ids are never reused,
                    // so id tokens are immune to the address-recycling ABA
                    // that keys-pointer tokens have — which also makes
                    // OWNED keys arrays safely cacheable again for plain
                    // objects. #6759 C3c: keys-POINTER tokens stay
                    // restricted to SHAPE-SHARED arrays (literal shapes,
                    // class-keys arrays — shape-cache-resident,
                    // process-rooted, address-stable), because that compare
                    // is unvalidated and a recycled owned-array address
                    // would read the wrong slot.
                    let stamp = (*obj).parent_class_id;
                    if (*obj).class_id == 0 && crate::object::shapes::is_shape_id(stamp) {
                        (*cache)[0] =
                            (stamp as u64 | crate::object::shapes::PIC_ID_TOKEN_BIT) as i64;
                        (*cache)[1] = i as i64;
                    } else if keys_cacheable_for_pic(keys) {
                        (*cache)[0] = keys as i64;
                        (*cache)[1] = i as i64;
                    }
                    let field_ptr = (obj as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + i * 8)
                        as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// #5391 path 3: full-outlined generic property GET.
///
/// In oversized (full-outline) modules the inline generic-get diamond expands to
/// ~60 IR instructions and ~13 basic blocks per property-get site: receiver-tag
/// routing (SSO / INT32 class-ref / valid-pointer / nullish), a monomorphic
/// inline cache (shape check + hit/miss), typed-feedback recording, and the
/// nullish-throw. On a large minified bundle that is the single biggest
/// contributor to generated `__text`. This helper collapses the whole site to one
/// call by reproducing that branch ladder here, dispatching to the *exact same*
/// runtime entries the inline code calls — so behavior is unchanged. The only
/// thing dropped is the inline monomorphic fast-load: every read goes through the
/// cache-priming slow path (`js_object_get_field_ic_miss`), trading a little speed
/// for a large code-size win, the same trade the class-field GET/SET full-outline
/// paths (`js_class_field_get_ic` / `js_class_field_set_ic`) already make.
///
/// Argument shapes mirror the inline site operands exactly:
/// - `obj_bits`: the receiver's full (unmasked) NaN-box bits
/// - `key`: the property-name `StringHeader`, already masked to a raw pointer
/// - `site_id`: the typed-feedback site id
/// - `cache`: the per-site monomorphic IC cache global (primed by `..._ic_miss`)
#[no_mangle]
pub extern "C" fn js_object_get_field_ic(
    obj_bits: i64,
    key: *const crate::StringHeader,
    site_id: u64,
    cache: *mut [i64; 2],
) -> f64 {
    // POINTER_MASK: lower 48 bits — strips the NaN-box tag to a raw heap pointer.
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = obj_bits as u64;
    let tag = bits >> 48;
    // `obj_bits` reinterpreted as a pointer keeps the tag bits (the SSO / class-ref
    // / by-name helpers need the unmasked value); `obj_handle` is the masked heap
    // pointer the inline-cache miss handler + feedback observe expect.
    let obj_unmasked = bits as usize as *const ObjectHeader;
    let obj_handle = (bits & POINTER_MASK) as usize as *const ObjectHeader;

    // SSO receiver (SHORT_STRING_TAG = 0x7FF9): the SSO-aware by-name helper reads
    // `.length` from the NaN-box payload and returns undefined for other keys.
    if tag == 0x7FF9 {
        return js_object_get_field_by_name_f64(obj_unmasked, key);
    }
    // INT32-tagged class ref (0x7FFE): static-field / dynamic-prop / synthetic
    // `constructor` lookup via the feedback-wrapped by-name helper. Passes the
    // unmasked bits so the runtime can detect the INT32 tag.
    if tag == 0x7FFE {
        return crate::typed_feedback::js_typed_feedback_object_get_field_by_name_f64(
            site_id,
            obj_unmasked,
            key,
        );
    }
    // Valid heap pointer or string (masked tag 0x7FFD): record feedback, then route
    // through the cache-priming inline-cache-miss handler — the same entry the
    // inline diamond's miss arm calls (objects, closures, buffers, typed arrays,
    // proxies, small handles all dispatch correctly there, and the per-site cache
    // is primed for any future inline sites sharing this global).
    if (tag & 0xFFFD) == 0x7FFD {
        crate::typed_feedback::js_typed_feedback_observe_property_get(site_id, obj_handle, key);
        return js_object_get_field_ic_miss(obj_handle, key, cache);
    }
    // Invalid (non-pointer) receiver. `undefined`/`null` throw a TypeError (#462 —
    // matches the inline nullish path, which aborts with a node-shaped message);
    // other primitives route through the by-name helper, which can still resolve
    // typed-shape reads (e.g. Date `.constructor`).
    if bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL {
        let is_null = u32::from(bits == crate::value::TAG_NULL);
        let (ptr, len) = unsafe {
            match super::super::has_own_helpers::str_from_string_header(key) {
                Some(s) => (s.as_ptr(), s.len()),
                None => (std::ptr::null(), 0),
            }
        };
        crate::error::js_throw_type_error_property_access(is_null, ptr, len);
    }
    js_object_get_field_by_name_f64(obj_unmasked, key)
}

// Polymorphic numeric-key get/set (`js_object_get_index_polymorphic` /
// `js_object_set_index_polymorphic`) live in `polymorphic_index.rs`:
// they dispatch by GC type (array vs object vs closure vs buffer) rather
// than touching object field storage directly, so they were split out
// of this module. See `polymorphic_index.rs` for the implementations
// and the #471 fix notes.

#[cfg(test)]
mod sso_tests_1781 {
    use super::super::*;

    #[test]
    fn object_keys_values_entries_on_string_do_not_crash() {
        // Regression: Object.keys/values/entries on a string segfaulted
        // (the value was deref'd as an ObjectHeader; SSO strings aren't even
        // pointers). Now they yield index keys / chars / [index,char].
        let heap = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
        let v = crate::value::js_nanbox_string(heap as i64);
        assert_eq!(crate::array::js_array_length(js_object_keys_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_values_value(v)), 3);
        assert_eq!(crate::array::js_array_length(js_object_entries_value(v)), 3);
        // SSO string (<= 5 bytes) — the non-pointer case that crashed hardest.
        let sso = crate::value::JSValue::try_short_string(b"hi").unwrap();
        assert_eq!(
            crate::array::js_array_length(js_object_keys_value(f64::from_bits(sso.bits()))),
            2
        );
        // Number / boolean primitives → empty array (no own enumerable keys).
        assert_eq!(crate::array::js_array_length(js_object_keys_value(42.0)), 0);
    }

    /// #1781: `"id" in obj` for a key <= 5 bytes — the lookup key arrives as
    /// an inline SSO value (tag 0x7FF9). `is_string()` (STRING_TAG-only)
    /// rejected it, so `js_object_has_property` returned false even though the
    /// object had the key (stored keys are always heap, so materializing the
    /// SSO lookup key lets js_string_equals match).
    #[test]
    fn in_operator_finds_object_key_via_sso_lookup() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            crate::object::js_object_set_field_by_name(obj, key, 42.0);

            let obj_box = crate::value::js_nanbox_pointer(obj as i64);
            let sso = crate::value::JSValue::try_short_string(b"id").unwrap();
            assert!(sso.is_short_string());
            let present = js_object_has_property(obj_box, f64::from_bits(sso.bits()));
            assert_ne!(
                crate::value::js_is_truthy(present),
                0,
                "SSO key 'id' should be found via `in`"
            );

            let missing = crate::value::JSValue::try_short_string(b"zz").unwrap();
            let absent = js_object_has_property(obj_box, f64::from_bits(missing.bits()));
            assert_eq!(
                crate::value::js_is_truthy(absent),
                0,
                "absent SSO key 'zz' should not be found"
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn js_private_brand_check(
    obj: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
) -> f64 {
    let false_value = f64::from_bits(crate::value::TAG_FALSE);
    let true_value = f64::from_bits(crate::value::TAG_TRUE);
    if declaring_class_id == 0 || field_name_ptr.is_null() || field_name_len == 0 {
        return false_value;
    }

    let value = JSValue::from_bits(obj.to_bits());
    if !value.is_pointer() {
        return false_value;
    }
    let obj_ptr = value.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_value;
    }

    let obj_class_id = js_object_get_class_id(obj_ptr);
    if obj_class_id == 0 {
        return false_value;
    }

    let mut cur = obj_class_id;
    let mut has_declaring_brand = false;
    for _ in 0..32 {
        if cur == declaring_class_id {
            has_declaring_brand = true;
            break;
        }
        match super::super::class_registry::get_parent_class_id(cur) {
            Some(parent) if parent != 0 && parent != cur => cur = parent,
            _ => break,
        }
    }
    if !has_declaring_brand {
        return false_value;
    }

    true_value
}

/// Throw a `TypeError` with `msg` through Perry's exception machinery so a
/// surrounding `try { ... } catch (e) { ... }` catches it. Diverges.
fn throw_private_type_error(msg: &str) -> ! {
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    let v = crate::value::JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(v))
}

/// Brand check core shared with `js_private_brand_check`: does `obj` carry the
/// brand of `declaring_class_id` (it is an instance of that class or a
/// subclass)? Walks the class-id parent chain.
unsafe fn private_object_has_brand(obj: f64, declaring_class_id: u32) -> bool {
    if declaring_class_id == 0 {
        return false;
    }
    let value = JSValue::from_bits(obj.to_bits());
    if !value.is_pointer() {
        return false;
    }
    let obj_ptr = value.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false;
    }
    let obj_class_id = js_object_get_class_id(obj_ptr);
    if obj_class_id == 0 {
        return false;
    }
    let mut cur = obj_class_id;
    for _ in 0..32 {
        if cur == declaring_class_id {
            return true;
        }
        match super::super::class_registry::get_parent_class_id(cur) {
            Some(parent) if parent != 0 && parent != cur => cur = parent,
            _ => break,
        }
    }
    false
}

/// Brand + kind/op guard for a private member access `obj.#name`. Returns
/// `obj` unchanged when the access is legal; otherwise throws a `TypeError`.
///
/// The enclosing `PropertyGet` / `PropertySet` / method-call lowering operates
/// on the returned receiver, so this helper only enforces the two access
/// preconditions the spec attaches to a PrivateReference:
///   1. The receiver must carry the private brand (be an instance of the
///      declaring class). A plain object, or an instance of an unrelated /
///      enclosing class, throws.
///   2. The operation must match the member kind — reading a setter-only
///      accessor, or writing a getter-only accessor or a private method,
///      throws.
///
/// `kind`: 0=field, 1=method, 2=getter-only, 3=setter-only, 4=getter+setter.
/// `op`:   0=read, 1=write (instance); 2=read, 3=write (static).
///
/// For a STATIC private member the brand is identity-based: the receiver must
/// BE the declaring class constructor itself (static private elements are not
/// inherited, so a subclass constructor does not carry them). For an INSTANCE
/// member the receiver must be an instance of the declaring class (or a
/// subclass).
///
/// `declaring_class_id == 0` means codegen could not resolve the declaring
/// class (e.g. an unusual class-expression shape); the guard then degrades to
/// a no-op so it can never reject a legal access.
#[no_mangle]
pub extern "C" fn js_private_guard(
    obj: f64,
    declaring_class_id: u32,
    _field_name_ptr: *const u8,
    _field_name_len: u32,
    kind: u32,
    op: u32,
) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    let is_static = op >= 2;
    let read_write = op & 1; // 0=read, 1=write
    let has_brand = if is_static {
        // Static private brand: the receiver must be exactly the declaring
        // class constructor (identity), not an instance or a subclass.
        super::super::class_ref_id(obj) == Some(declaring_class_id)
    } else {
        unsafe { private_object_has_brand(obj, declaring_class_id) }
    };
    if !has_brand {
        throw_private_type_error(
            "Cannot access private member from an object whose class did not declare it",
        );
    }
    let op = read_write;
    // Kind/op legality, after the brand check (spec order).
    let illegal = matches!(
        (op, kind),
        (0, 3) /* read setter-only: [[Get]] of accessor without getter */
            | (1, 2) /* write getter-only: [[Set]] of accessor without setter */
            | (1, 1) /* write private method */
    );
    if illegal {
        throw_private_type_error("Invalid private member operation for its kind");
    }
    obj
}

#[cfg(test)]
mod c3c_pic_tests {
    /// #6759 C3c: the PIC only caches SHAPE-SHARED (process-rooted,
    /// address-stable) keys arrays; an owned array's address can be
    /// recycled under a different shape, which the unvalidated PIC hit
    /// path cannot detect.
    #[test]
    fn pic_caches_only_shape_shared_keys() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let keys = crate::array::js_array_alloc(4);
            assert!(
                !super::keys_cacheable_for_pic(keys),
                "a fresh owned keys array must not be PIC-cacheable"
            );
            let gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*gc).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
            assert!(
                super::keys_cacheable_for_pic(keys),
                "a shape-shared keys array must stay PIC-cacheable"
            );
        }
    }
}
