//! Dynamic `obj[key] = value` write path
//! (`js_object_set_field_by_name`) plus its diagnostic helper.
//!
//! Split out of `object/field_get_set.rs` (issue #1103). Pure relocation
//! — no logic changes.

use super::*;

/// Fast transition-cache-backed dynamic property write.
///
/// This is intentionally narrower than `js_object_set_field_by_name`: it only
/// handles plain object-shape transitions that have already been learned by
/// the runtime transition cache. Accessors/descriptors, frozen/sealed objects,
/// class/prototype receivers, closures, native handles, arrays, strings, and
/// cache misses return 0 so callers preserve the full setter semantics by
/// falling back to `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_transition_fast(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> i32 {
    if key.is_null() || (key as usize) < 0x10000 {
        return 0;
    }

    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 != 0x7FFD {
                // Not a POINTER-tagged heap receiver (SSO string payload,
                // UNDEFINED/NULL remnant, INT32, BIGINT…). The old catch-all
                // masked these to 48 bits — a 2–5-char SSO payload lands in
                // the 2–5.5TB range, passes the macOS heap floor, and the
                // GcHeader read below deref'd unmapped memory (write-side
                // #5429 twin, 2026-07-02 audit). Return 0 = defer to the
                // full dynamic path, which triages by tag.
                return 0;
            }
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || crate::value::addr_class::is_small_handle(raw as usize) {
                return 0;
            }
            raw
        } else {
            obj
        }
    };

    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);

    unsafe {
        let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let key = key_handle.get_raw_const_ptr::<crate::StringHeader>();

        // Validated header probe (rejects the handle band, implausible
        // addresses, and slab allocations without touching memory) instead
        // of the bare floor + raw deref.
        let gc_header = match crate::value::addr_class::try_read_gc_header(obj as usize) {
            Some(h) => h as *const crate::gc::GcHeader,
            None => return 0,
        };
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return 0;
        }
        let object_flags = (*gc_header)._reserved;
        if object_flags
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                // #6084 item 6: an own descriptor on THIS object (accessor or
                // non-writable) must route through the full setter semantics.
                | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS)
            != 0
        {
            return 0;
        }
        if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR || (*obj).class_id != 0 {
            return 0;
        }

        // #6084 item 6: this used to be a `GLOBAL_DESCRIPTORS_IN_USE` check at
        // the top of the function — one `Object.freeze` anywhere in the process
        // (even on an unrelated object) permanently disabled this fast path for
        // every object. Vet the receiver's own flag (above) and its prototype
        // chain (here) instead. `class_id` is 0 at this point, so the only
        // inherited interceptor is `Object.prototype` (or a recorded
        // `setPrototypeOf` target).
        let key_f64 = f64::from_bits(JSValue::string_ptr(key as *mut _).bits());
        if super::plain_data_write_may_intercept(obj as usize, 0, key_f64) {
            return 0;
        }

        let key_gc =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*key_gc).obj_type != crate::gc::GC_TYPE_STRING {
            return 0;
        }
        let interned_key = if (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
            key
        } else {
            let hash = key_content_hash(key);
            crate::string::js_string_intern(key, hash)
        };
        if interned_key.is_null() {
            return 0;
        }

        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let value = value_handle.get_nanbox_f64();

        let keys = (*obj).keys_array;
        let prev_keys = keys as usize;
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                return 0;
            }
        }

        let Some((next_keys, slot_idx)) = transition_cache_lookup(prev_keys, interned_key) else {
            return 0;
        };
        if next_keys == 0 {
            return 0;
        }

        set_object_keys_array(obj, next_keys as *mut ArrayHeader);
        super::mark_object_dynamic_shape_unknown(obj);

        let alloc_limit =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
        let slot_usize = slot_idx as usize;
        let vbits = value.to_bits();
        let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            crate::value::TAG_UNDEFINED
        } else {
            vbits
        };

        if slot_usize < alloc_limit {
            store_object_field_slot(obj, slot_usize, vbits);
            if slot_idx >= (*obj).field_count {
                (*obj).field_count = slot_idx + 1;
            }
        } else {
            overflow_set(obj as usize, slot_usize, vbits);
        }
    }

    1
}

/// Issue #615 helper — read a `*const StringHeader` as a Rust `String`
/// for inclusion in TypeError diagnostic messages. Returns `"<unknown>"`
/// for null / non-UTF-8 / corrupt headers so the throw still fires
/// rather than panicking on the slow-path edge case.
unsafe fn key_to_str_for_diag(key: *const crate::StringHeader) -> String {
    if key.is_null() {
        return "<unknown>".to_string();
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if name_len == 0 {
        return String::new();
    }
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string())
}

unsafe fn string_key_eq(key: *const crate::StringHeader, expected: &[u8]) -> bool {
    if key.is_null() || (key as usize) < 0x10000 {
        return false;
    }
    let len = (*key).byte_len as usize;
    if len != expected.len() {
        return false;
    }
    let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::slice::from_raw_parts(data, len) == expected
}

/// Shared closure-receiver named-property WRITE path — used by both ways a
/// closure is recognized in `js_object_set_field_by_name` (a `GC_TYPE_CLOSURE`
/// GcHeader-typed pointer, and the raw `CLOSURE_MAGIC`-tagged fallback for a
/// pointer reached without a full GC header). #3143: honors a non-writable
/// registered descriptor (a built-in method's `.name`/`.length` are spec'd
/// `writable: false`); `Object.defineProperty(Function.prototype, k, {...})`
/// round-trips via `closure_set_via_function_prototype_descriptor` before
/// falling back to a plain own-property write.
/// #6530: mirror a SUCCESSFUL own-data write on a per-evaluation CLASS OBJECT
/// (`object_type == OBJECT_TYPE_CLASS` — what a capture-carrying class
/// statement materializes as) into the class_id-keyed `CLASS_DYNAMIC_PROPS`
/// side table. Compiled method bodies reference sibling classes as INT32
/// ClassRefs (bundled zod's `ZodOptional.create(this, this._def)` inside
/// `ZodType.optional()`), and `js_class_static_method_call` resolves statics
/// through that table only — without the mirror the dispatch missed and
/// handed back the class ref itself, so `.optional()` returned the
/// ZodOptional CLASS instead of an instance.
///
/// Called ONLY at the own-data write completions in
/// `js_object_set_field_by_name` (after the accessor walk, frozen/sealed
/// gates, and writable checks have all passed), so a setter-intercepted or
/// rejected assignment never desyncs the ClassRef read path from the class
/// object's real state. Internal `__perry_*` markers (the pinned-parent
/// edge) stay object-local. Last-wins across evaluations of the same class
/// statement, matching the established template-cid compromise.
unsafe fn mirror_class_object_static_write(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if (*obj).object_type != crate::error::OBJECT_TYPE_CLASS
        || (*obj).class_id == 0
        || key.is_null()
    {
        return;
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        if !name.is_empty() && !name.starts_with("__perry_") {
            class_dynamic_prop_root_store((*obj).class_id, name.to_string(), value);
        }
    }
}

unsafe fn closure_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if key.is_null() {
        return;
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let Ok(name_str) = std::str::from_utf8(name_bytes) else {
        return;
    };
    // ECMAScript "poison pill" — assigning `caller`/`arguments` on any
    // strict-mode function (Perry compiles everything strict: declarations,
    // expressions, bound and built-in closures, arrows) throws via the
    // %ThrowTypeError% accessor's missing setter. A genuine own data prop of
    // that name (defineProperty round-trip) still wins.
    // Refs test262 13.2-*-s / StrictFunction_restricted-*.
    if matches!(name_str, "caller" | "arguments")
        && !crate::closure::closure_has_own_dynamic_prop(obj as usize, name_str)
    {
        crate::fs::validate::throw_type_error_with_code(
            "Restricted function property assignment",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if let Some(attrs) = super::get_property_attrs(obj as usize, name_str) {
        if !attrs.writable() {
            return;
        }
    } else if matches!(name_str, "name" | "length") {
        return;
    } else if !crate::closure::closure_has_own_dynamic_prop(obj as usize, name_str)
        && crate::closure::closure_set_via_function_prototype_descriptor(
            obj as usize,
            name_str,
            value,
            crate::value::js_nanbox_pointer(obj as i64),
        )
    {
        // Handled by an inherited %Function.prototype% descriptor
        // (`Object.defineProperty(Function.prototype, k, {...})`) — an
        // accessor's setter ran (or threw for a getter-only accessor), or a
        // non-writable data property blocked the write; no own property is
        // created.
        return;
    }
    crate::closure::closure_set_dynamic_prop(obj as usize, name_str, value);
}

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[allow(unused_assignments)]
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    // #5135: the receiver may be a Proxy id arriving with its NaN-box tag
    // already masked off (the `obj.prop++` / `PropertyUpdate` codegen path
    // hands us the bare pointer band, not the full POINTER_TAG value). A Proxy
    // is encoded as a small registered id; deref-ing one as an `ObjectHeader`
    // reads unmapped memory and SIGSEGVs. Mirror the read-side dispatch in
    // `js_object_get_field_by_name` so a `proxy.foo = v` write goes through the
    // `set` trap instead of corrupting the cell. `js_proxy_is_proxy` validates
    // the value is a *registered* proxy so a real heap object whose masked
    // address happens to be small isn't misrouted.
    {
        // #6699 (mirror of the read side): the class-field IC-miss fallback
        // (`js_class_field_set_fallback`, reached when a typed-`this` field set
        // rejects an off-shape receiver) forwards the *full NaN-box* value with
        // the `0x7FFD` heap-pointer tag still set, whereas the `obj.prop++`
        // path (#5135) hands us the bare masked band. A tagged proxy value is
        // not itself in the proxy id band, so the un-normalized test missed it
        // and a `this.field = v` write whose `this` is a Proxy skipped the set
        // trap. Strip the tag first (the FAST LANE below already does) so both
        // encodings route to `js_proxy_set` identically.
        let addr = obj as u64;
        let raw_addr = if (addr >> 48) == 0x7FFD {
            addr & 0x0000_FFFF_FFFF_FFFF
        } else {
            addr
        };
        if crate::value::addr_class::is_proxy_id_band(raw_addr as usize) && !key.is_null() {
            const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
            let boxed = f64::from_bits(POINTER_TAG | (raw_addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                crate::proxy::js_proxy_set(boxed, key_f64, value);
                return;
            }
        }
    }
    // FAST LANE (mirror of the read lane in `js_object_get_field_by_name`,
    // same gate rationale — see that comment): a provably-plain arena class
    // instance whose store plan says "no interceptor for this (class, key)"
    // takes the shape-transition cache directly, with no rooting scope and no
    // exotic-registry probes. Additional store-only gates: the frozen family
    // and the chain-divergence flags must be clear (same set the in-body fast
    // path vets), and the plan hit itself certifies no vtable setter /
    // prototype interceptor / URL / native-module route. Nothing on this path
    // allocates from the arena, so raw pointers stay valid without handles.
    unsafe {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let raw = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000
            && !crate::value::addr_class::is_small_handle(raw)
            && !crate::value::addr_class::is_stream_id_band(raw)
            && crate::value::addr_class::is_above_handle_band(key as usize)
        {
            let key_gc =
                (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*key_gc).gc_flags & crate::gc::GC_FLAG_INTERNED != 0
                && crate::arena::classify_heap_generation(raw)
                    != crate::arena::HeapGeneration::Unknown
            {
                let gc_hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                const LANE_BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
                    | crate::gc::OBJ_FLAG_SEALED
                    | crate::gc::OBJ_FLAG_NO_EXTEND
                    | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                    | crate::gc::OBJ_FLAG_PROTO_OVERRIDE
                    | crate::gc::OBJ_FLAG_NULL_PROTO
                    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                if (*gc_hdr).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*gc_hdr)._reserved & LANE_BLOCKING == 0
                {
                    let o = raw as *mut ObjectHeader;
                    let class_id = (*o).class_id;
                    // #6595: a per-evaluation CLASS OBJECT must never take
                    // this lane — it skips the #6530
                    // `mirror_class_object_static_write` hook every in-body
                    // completion runs, and (template cid, key) plans recorded
                    // by instances sharing the cid would falsely certify it.
                    // See the matching gates at the plan record sites.
                    if (*o).object_type == crate::error::OBJECT_TYPE_REGULAR
                        && class_id != 0
                        && class_id != NATIVE_MODULE_CLASS_ID
                        && super::prop_plan::store_plan_check(class_id, key as usize)
                    {
                        let keys = (*o).keys_array;
                        let keys_ok = keys.is_null()
                            || (((keys as u64) >> 48) == 0
                                && crate::value::addr_class::is_above_handle_band(keys as usize));
                        if keys_ok {
                            // Overwrite of an EXISTING own key: the keys array
                            // doesn't change, so the shape-transition cache
                            // (which stores append EDGES) can never serve it —
                            // the (keys, key) → index read-plan cache can.
                            // Miss → one bounded scan populates it; absent own
                            // key falls to the append-edge lookup below.
                            if !keys.is_null() {
                                let mut own_idx =
                                    super::prop_plan::read_plan_lookup(keys as usize, key as usize);
                                if own_idx.is_none() {
                                    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                        as *const crate::gc::GcHeader;
                                    if (*keys_gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                                        let key_count =
                                            crate::array::keys_array_len_capped_to_capacity(keys);
                                        if key_count <= 4096 {
                                            for i in 0..key_count {
                                                let kv = crate::array::js_array_get(keys, i as u32);
                                                if crate::string::js_string_key_matches(kv, key) {
                                                    super::prop_plan::read_plan_record(
                                                        keys as usize,
                                                        key as usize,
                                                        i as u32,
                                                    );
                                                    own_idx = Some(i as u32);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(idx) = own_idx {
                                    let vbits = value.to_bits();
                                    let vbits = if (vbits >> 48) == 0x7FFD
                                        && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0
                                    {
                                        crate::value::TAG_UNDEFINED
                                    } else {
                                        vbits
                                    };
                                    // Layout safety (#6495 family): the slot's
                                    // pointer-ness may change — degrade the
                                    // layout to full-visit before the store.
                                    super::mark_object_dynamic_shape_unknown(o);
                                    let alloc_limit = std::cmp::max(
                                        (*o).field_count,
                                        crate::object::INLINE_SLOT_FLOOR as u32,
                                    )
                                        as usize;
                                    if (idx as usize) < alloc_limit {
                                        let fields_ptr = (o as *mut u8)
                                            .add(std::mem::size_of::<ObjectHeader>())
                                            as *mut JSValue;
                                        let slot = fields_ptr.add(idx as usize);
                                        crate::gc::runtime_store_jsvalue_slot(
                                            o as usize,
                                            slot as usize,
                                            idx as usize,
                                            vbits,
                                        );
                                        if idx >= (*o).field_count {
                                            (*o).field_count = idx + 1;
                                        }
                                    } else {
                                        overflow_set(o as usize, idx as usize, vbits);
                                    }
                                    return;
                                }
                            }
                            if let Some((next_keys, slot_idx)) =
                                transition_cache_lookup(keys as usize, key)
                            {
                                // Same store semantics as the in-body fast
                                // path: strip a raw-null POINTER_TAG value,
                                // transition the keys array, note the dynamic
                                // shape, then write inline or overflow.
                                let vbits = value.to_bits();
                                let vbits = if (vbits >> 48) == 0x7FFD
                                    && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0
                                {
                                    crate::value::TAG_UNDEFINED
                                } else {
                                    vbits
                                };
                                set_object_keys_array(o, next_keys as *mut ArrayHeader);
                                super::mark_object_dynamic_shape_unknown(o);
                                let alloc_limit = std::cmp::max(
                                    (*o).field_count,
                                    crate::object::INLINE_SLOT_FLOOR as u32,
                                ) as usize;
                                if (slot_idx as usize) < alloc_limit {
                                    let fields_ptr = (o as *mut u8)
                                        .add(std::mem::size_of::<ObjectHeader>())
                                        as *mut JSValue;
                                    let slot = fields_ptr.add(slot_idx as usize);
                                    crate::gc::runtime_store_jsvalue_slot(
                                        o as usize,
                                        slot as usize,
                                        slot_idx as usize,
                                        vbits,
                                    );
                                    if slot_idx >= (*o).field_count {
                                        (*o).field_count = slot_idx + 1;
                                    }
                                } else {
                                    overflow_set(o as usize, slot_idx as usize, vbits);
                                }
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
    // A Buffer is an ordinary object in Node (a Uint8Array), so `buf.foo = v`
    // stores an own property — and an own key SHADOWS the same-named prototype
    // method. Perry keeps buffers outside the object model (raw BufferHeader,
    // no GcHeader), so this write used to be dropped entirely. mysql2's
    // `MockBuffer` packet sizer depends on it: it replaces the write methods of
    // a zero-length Buffer with a no-op, serializes once to measure, then
    // allocates for real. Store into the GC-traced buffer own-prop table (the
    // read side and the method-call dispatch both consult it).
    if !key.is_null() && crate::buffer::is_registered_buffer(obj as usize) {
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)) {
                // Numeric keys are element writes (`buf[0] = 1`) — leave those
                // to the index path; only NAMED props become expandos.
                if name.parse::<u32>().is_err() {
                    crate::buffer::buffer_set_own_prop(obj as usize, name, value);
                    return;
                }
            }
        }
    }
    // #5437: a live Web Stream handle arrives here as its raw id in the
    // stream band (the `stream.prop = v` codegen path). React's
    // `renderToReadableStream` attaches its shell-ready promise as an
    // expando (`stream.allReady = ...`); without a store the write was
    // dropped, which stalled the Next.js dynamic-SSR render. Route to the
    // stdlib per-stream expando table (GC-traced there).
    {
        let addr = obj as usize;
        if crate::value::addr_class::is_stream_id_band(addr) {
            if !key.is_null() {
                if let (Some(probe), Some(setter)) = (
                    crate::object::stream_handle_probe(),
                    crate::object::stream_expando_set(),
                ) {
                    if unsafe { probe(addr) } {
                        if let Some(name) =
                            unsafe { super::has_own_helpers::str_from_string_header(key) }
                        {
                            unsafe { setter(addr, name.as_ptr(), name.len(), value) };
                        }
                    }
                }
            }
            // A stream-band address is a reserved handle id, never a real
            // `ObjectHeader`. Stop unconditionally — even when the expando
            // write was a no-op (dead/unregistered handle, hooks absent, or a
            // non-UTF-8 key). Falling through would reach the ObjectHeader
            // path below and deref `addr - GC_HEADER_SIZE` (unmapped) → crash.
            // Mirrors the reserved small-handle early-return further down.
            return;
        }
    }
    // `Object.prototype["2"] = v` (stringified-index write) makes the index
    // visible through array hole/OOB reads. Cheap gate: one relaxed flag
    // load, then an address compare against the cached canonical
    // Object.prototype; the digit scan only runs on a match (test262
    // concat/S15.4.4.4_A3_T3).
    {
        let raw = (obj as u64 & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::array::object_prototype_addr_matches(raw) && !key.is_null() {
            if let Some(name) = unsafe { super::has_own_helpers::str_from_string_header(key) } {
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
                    crate::array::note_object_prototype_index_write(raw);
                }
            }
        }
    }
    // A `Temporal.*` value is an opaque, immutable NaN-boxed cell that is NOT
    // an `ObjectHeader` — writing an arbitrary property (e.g. test262's
    // `instance.constructor = …` subclassing probes) must NOT interpret the
    // cell as an `ObjectHeader` and corrupt its boxed payload (which segfaults
    // on the next deref). The cell's `temporal_rs` slots are immutable, but a
    // user-defined *expando* property is legal and lives in the exotic side
    // table (like Date/RegExp). `obj` still carries its NaN-box tag here
    // (`0x7FFD…` for a real cell), so route through `exotic_expando_kind_of_value`,
    // which checks the tag before masking to the cleaned heap address.
    if let Some((addr, kind @ super::exotic_expando::ExoticKind::Temporal)) =
        super::exotic_expando::exotic_expando_kind_of_value(f64::from_bits(obj as u64))
    {
        if !key.is_null() {
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = String::from_utf8_lossy(std::slice::from_raw_parts(name_ptr, name_len))
                    .into_owned();
                let receiver = f64::from_bits(obj as u64);
                let _ =
                    super::exotic_expando::exotic_set_property(addr, kind, &name, value, receiver);
            }
        }
        return;
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        unsafe {
            crate::typedarray_props::typed_array_set_own_property(
                addr as *mut crate::typedarray::TypedArrayHeader,
                key,
                value,
            );
        }
        return;
    }

    // Issue #618-followup: detect INT32-tagged class ref (top16 == 0x7FFE).
    // Drizzle's `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)` pattern sets
    // a static property on an imported class — Perry stores classes as
    // INT32-tagged class ids, so the receiver here is e.g. 0x7FFE_0000_0000_002A
    // not a real ObjectHeader. Route to the CLASS_DYNAMIC_PROPS side-table
    // so a later `SQL.Aliased` read can find it.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("")
                    .to_string();
                // Empty-string is a legal accessor key (`set ''(v)`); the
                // `!name.is_empty()` guard below skips it, so dispatch a
                // prototype-ref instance setter / constructor-ref static setter
                // named "" here (Test262 accessor-name-* literal-string-empty).
                if name.is_empty() {
                    let recv = f64::from_bits(bits);
                    if super::class_prototype_ref_id(recv).is_some()
                        && super::class_registry::class_instance_setter_apply(
                            class_id, &name, recv, value,
                        )
                    {
                        return;
                    }
                    if super::class_registry::class_static_accessor_setter_apply(
                        class_id, &name, recv, value,
                    ) {
                        return;
                    }
                }
                if !name.is_empty() {
                    if name == "name"
                        && !super::class_registry::class_is_key_deleted(class_id, &name)
                        && super::class_registry::lookup_static_method_in_chain(class_id, &name)
                            .is_none()
                    {
                        return;
                    }
                    let has_own_data = CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow()
                            .get(&class_id)
                            .is_some_and(|props| props.contains_key(&name))
                    });
                    // `C.prototype[key] = v` where `key` is an instance
                    // `set key(v)` accessor defined on the prototype: invoke the
                    // setter with `this` = the prototype ref. The prototype ref
                    // and the constructor ref are both INT32-tagged class refs;
                    // distinguish via `class_prototype_ref_id`. Instance setters
                    // live in the vtable; static accessors (below) live in the
                    // constructor ref's table (Test262 accessor-name-inst).
                    if !has_own_data
                        && super::class_prototype_ref_id(f64::from_bits(bits)).is_some()
                        && super::class_registry::class_instance_setter_apply(
                            class_id,
                            &name,
                            f64::from_bits(bits),
                            value,
                        )
                    {
                        return;
                    }
                    if !has_own_data
                        && super::class_registry::class_static_accessor_setter_apply(
                            class_id,
                            &name,
                            f64::from_bits(bits),
                            value,
                        )
                    {
                        return;
                    }
                    // Writing `.caller` / `.arguments` on a class constructor
                    // hits the poison-pill %ThrowTypeError% accessor (which has
                    // no [[Set]]) on `Function.prototype`, so a strict-mode
                    // assignment throws. Mirrors the read side in
                    // get_field_by_name and the ordinary-closure setter path.
                    // A `defineProperty`-installed own data prop was handled by
                    // `has_own_data` above; prototype-refs (`C.prototype`) are
                    // plain objects with no such restriction.
                    if !has_own_data
                        && matches!(name.as_str(), "caller" | "arguments")
                        && super::class_prototype_ref_id(f64::from_bits(bits)).is_none()
                    {
                        crate::fs::validate::throw_type_error_with_code(
                            "Restricted function property access",
                            "ERR_INVALID_ARG_TYPE",
                        );
                    }
                    class_dynamic_prop_root_store(class_id, name, value);
                }
            }
            return;
        }
    }
    // Property writes to primitive values operate on temporary wrapper objects
    // and do not persist. More importantly for Perry's raw-f64 numbers, they
    // must never fall through to the ObjectHeader dereference path below.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let jv = JSValue::from_bits(bits);
        if (jv.is_number() && top16 != 0)
            || jv.is_bool()
            || jv.is_any_string()
            || jv.is_undefined()
            || jv.is_null()
            || jv.is_bigint()
        {
            return;
        }
    }
    // #2089: a `Date` is a NaN-boxed pointer to an 8-byte `DateCell`, and a
    // RegExp is a `RegExpHeader` — neither is an `ObjectHeader`, so a write
    // must NOT fall through to the object deref below (memory corruption).
    // Expando properties on these exotic instances live in the side table
    // (`object::exotic_expando`), honoring accessor descriptors and
    // attribute writability installed by `Object.defineProperty`.
    {
        let bits = obj as u64;
        let top16 = bits >> 48;
        let addr = if top16 == 0x7FFD {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            bits as usize
        } else {
            0
        };
        if addr != 0 {
            if let Some(kind) = super::exotic_expando::exotic_expando_kind(addr) {
                if !key.is_null() {
                    unsafe {
                        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                        if let Some(name_bytes) = crate::string::js_string_key_bytes(
                            crate::value::JSValue::string_ptr(key as *mut _),
                            &mut sso,
                        ) {
                            if let Ok(name) = std::str::from_utf8(name_bytes) {
                                let receiver = f64::from_bits(
                                    crate::value::JSValue::pointer(addr as *const u8).bits(),
                                );
                                let _ = super::exotic_expando::exotic_set_property(
                                    addr, kind, name, value, receiver,
                                );
                            }
                        }
                    }
                }
                return;
            }
        }
    }
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                return;
            }
            if crate::value::addr_class::is_small_handle(raw as usize) {
                // Handle-band id (2026-07-02 audit P1: the old `< 0x10000`
                // guard was one zero short of HANDLE_BAND_MAX, so a
                // POINTER-tagged fetch (0x40000+) or zlib id skipped handle
                // dispatch and reached the raw GcHeader deref below —
                // `response.myProp = v` deref'd the id as memory). Dispatch
                // to the registered handle property setter.
                if let Some(dispatch) = handle_property_set_dispatch() {
                    if !key.is_null() {
                        unsafe {
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            dispatch(raw as i64, name_ptr, name_len, value);
                        }
                    }
                }
                return;
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || crate::value::addr_class::is_small_handle(obj as usize) {
        // Handle-band value (full band, not the old `< 0x10000` — see above)
        // or a stripped handle after ensure_i64 removed the NaN-box tag.
        if !obj.is_null() && (obj as usize) > 0 {
            if let Some(dispatch) = handle_property_set_dispatch() {
                if !key.is_null() {
                    unsafe {
                        let name_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).byte_len as usize;
                        dispatch(obj as i64, name_ptr, name_len, value);
                    }
                }
            }
        }
        return;
    }
    unsafe {
        if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
            crate::typedarray_props::typed_array_set_own_property(
                obj as *mut crate::typedarray::TypedArrayHeader,
                key,
                value,
            );
            return;
        }
    }
    unsafe {
        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 && string_key_eq(key, b"length") {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                // Assignment (`arr.length = v`) is a strict `Set` with
                // `Throw = true`: throw on a frozen array's non-writable length.
                crate::array::js_array_set_length_strict(
                    obj as *mut crate::array::ArrayHeader,
                    value,
                );
                return;
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    let mut value = value_handle.get_nanbox_f64();
    // Safety: obj is a valid heap pointer (> 0x10000) at this point
    unsafe {
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ARRAY {
            if key.is_null() {
                return;
            }
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            let name = match std::str::from_utf8(key_bytes) {
                Ok(s) => s,
                Err(_) => return,
            };
            let arr = obj as *mut crate::array::ArrayHeader;
            if name == "length" {
                // Strict `Set(arr, "length", v, true)` — throw on frozen.
                crate::array::js_array_set_length_strict(arr, value);
                return;
            }
            if let Some(index) = super::canonical_array_index(name) {
                // Strict `Set(arr, i, v, true)` — throw on frozen/non-extensible.
                crate::array::js_array_set_f64_extend_strict(arr, index, value);
                return;
            }
            // Own-accessor short-circuit — an Array can carry a named accessor
            // property installed via `Object.defineProperty(arr, k, {get,set})`.
            // A `[[Set]]` on such a property must invoke the setter (a
            // getter-only accessor is read-only), exactly as the generic-object
            // path below does. The array branch otherwise dropped the write at
            // the writable gate (an accessor has no `[[Writable]]`) and never
            // ran the setter (test262 Object/defineProperty + defineProperties
            // accessor-on-Array cases, e.g. 15.2.3.6-4-278).
            //
            // Gated on the per-array `OBJ_FLAG_ARRAY_DESCRIPTORS` flag (the
            // ArrayHeader analogue of `object_has_descriptors` — the ObjectHeader
            // `OBJ_FLAG_HAS_DESCRIPTORS` is never set for an ArrayHeader). The
            // flag is set unconditionally by `define_array_property` whenever any
            // descriptor is installed on the array and travels with it across
            // evacuation; `ACCESSOR_DESCRIPTORS` is keyed by raw address, so a
            // fresh array reusing a freed address (its `_reserved` zeroed at
            // allocation) skips this lookup and can't fire a previous tenant's
            // stale accessor.
            if ACCESSORS_IN_USE.with(|c| c.get())
                && (*gc_header)._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
            {
                if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                    if acc.set != 0 {
                        let closure = (acc.set & crate::value::POINTER_MASK)
                            as *const crate::closure::ClosureHeader;
                        if !closure.is_null() {
                            let receiver = crate::value::js_nanbox_pointer(obj as i64);
                            let previous_this = super::js_implicit_this_set(receiver);
                            crate::closure::js_closure_call1(closure, value);
                            super::js_implicit_this_set(previous_this);
                        }
                    } else {
                        crate::error::throw_immutable_write(0, name);
                    }
                    return;
                }
            }
            if let Some(attrs) = super::get_property_attrs(obj as usize, name) {
                if !attrs.writable() {
                    return;
                }
            }
            if crate::array::array_is_frozen(arr) {
                return;
            }
            let existing = crate::array::array_named_property_get(arr, key).is_some();
            if !existing && crate::array::array_is_sealed_or_no_extend(arr) {
                return;
            }
            crate::array::array_named_property_set(arr, key, value);
            return;
        }
        // Error objects have a fixed `#[repr(C)]` layout with no field-storage
        // region (`message`/`name`/`stack`/`cause`/`errors` are dedicated
        // slots), so a user assignment like `err.code = "X"` or
        // `err.errno = -2` has nowhere to land in the header. Route it to the
        // per-error user-property side table so the matching getter — and
        // `assert.throws(fn, { code })` (#2014) — can read it back. The getter
        // checks this table first, so this also lets a user override the
        // built-in message/name accessors, matching Node.
        if gc_type == crate::gc::GC_TYPE_ERROR {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                if let Ok(name_str) =
                    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                {
                    crate::node_submodules::set_error_user_prop(obj as usize, name_str, value);
                }
            }
            return;
        }
        if gc_type != crate::gc::GC_TYPE_OBJECT && gc_type != crate::gc::GC_TYPE_CLOSURE {
            // A RECOGNIZED non-object heap type (Map/Set/Buffer/TypedArray/…)
            // must never fall through to the plain-object write below: their
            // layouts alias ObjectHeader fields. A Map with EXACTLY one entry
            // had MapHeader.size aliasing object_type == OBJECT_TYPE_REGULAR,
            // so `m.customProp = 5` walked the Map's bytes as object fields —
            // deterministic heap corruption (2026-07-02 audit P1). The
            // object_type fallback exists ONLY for static/const objects whose
            // preceding bytes decode to no known GC type.
            if crate::gc::gc_type_info(gc_type).is_some() {
                return;
            }
            if !is_valid_obj_ptr(obj as *const u8) {
                return;
            }
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return;
            }
        }

        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            closure_set_field_by_name(obj, key, value);
            return;
        }

        // Check if this is a ClosureHeader — closures support dynamic props via separate storage.
        // ClosureHeader has CLOSURE_MAGIC (0x434C4F53) at offset 12.
        // Without this check, (*obj).keys_array reads capture[0] → corruption/crash.
        let type_tag_at_12 =
            *((obj as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            closure_set_field_by_name(obj, key, value);
            return;
        }

        if super::arguments_object_set_field(obj, key, value) {
            return;
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let property_name =
                std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
            let module_name =
                get_module_name_from_namespace(crate::value::js_nanbox_pointer(obj as i64));
            if module_name == "buffer.Buffer" && property_name == "poolSize" {
                super::set_buffer_pool_size(value);
                return;
            }
            // CommonJS module exports are MUTABLE in Node: monkey-patching
            // like Next.js's `require('node:timers').setImmediate = patched`
            // must store the override (read back via `vt_get_own_field`)
            // instead of falling through to the frozen-object throw.
            if !module_name.is_empty() && property_name != "__module__" {
                super::native_module::native_namespace_prop_override_store(
                    module_name,
                    property_name,
                    value,
                );
                return;
            }
        }

        // Resolve the interned key EARLY (hoisted from below the interception
        // vet): the store-plan cache and the shape-transition cache both key
        // on interned pointer identity. If the key is already interned
        // (GC_FLAG_INTERNED set — e.g. from js_string_concat intern hit), skip
        // the FNV-1a hash entirely. No allocation happens here, so the raw
        // `obj`/`key` pointers stay valid.
        let mut interned_key = if !key.is_null() && (key as usize) > 0x10000 {
            let gc_hdr =
                (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_hdr).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
                key // already interned
            } else {
                let kh = key_content_hash(key);
                crate::string::js_string_intern(key, kh)
            }
        } else {
            key
        };
        let interned_key_handle = scope.root_string_ptr(interned_key);
        interned_key = interned_key_handle.get_raw_const_ptr::<crate::StringHeader>();

        // Store-plan fast gate (`object::prop_plan`): a recorded verdict means
        // the full interception vet below (class vtable setter walk, URL-shape
        // probe, `plain_data_write_may_intercept`) proved a store of this key
        // to this class cannot be intercepted, and no invalidation (vtable /
        // descriptor / prototype mutation, GC) happened since. Per-OBJECT
        // conditions stay outside the verdict: frozen/sealed/own-descriptor
        // flags are checked below as always, and an instance whose chain
        // diverges from its class chain (per-instance `setPrototypeOf`
        // override, null-proto) never records or honors a plan.
        // Flags that make an object ineligible for class-keyed plans: a
        // diverging chain (per-instance proto override / null proto) or own
        // descriptors (an own accessor must dispatch through the short-circuit
        // below, which a plan hit skips).
        const PLAN_BLOCKING_FLAGS: u16 = crate::gc::OBJ_FLAG_PROTO_OVERRIDE
            | crate::gc::OBJ_FLAG_NULL_PROTO
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
        let obj_class_id = (*obj).class_id;
        // #6595: class objects (`OBJECT_TYPE_CLASS`) are excluded — their
        // writes must always reach the `mirror_class_object_static_write`
        // completions, and their cid is shared with their instances so a
        // plan keyed on it conflates two different prototype chains.
        let plan_eligible = !key.is_null()
            && obj_class_id != 0
            && obj_class_id != NATIVE_MODULE_CLASS_ID
            && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR
            && (*gc_header)._reserved & PLAN_BLOCKING_FLAGS == 0;
        let plan_fast = plan_eligible
            && super::prop_plan::store_plan_check(obj_class_id, interned_key as usize);

        // Refs #486 (hono): class setter dispatch. JS spec: a `set X(...)`
        // accessor on the prototype intercepts `obj.X = value` writes
        // before they hit the instance's data slots. Hono's `set res(_res)
        // { …; this.#res = _res; this.finalized = true; }` is the canonical
        // example — without setter dispatch, `c.res = response` from inside
        // compose stored the response into a regular field slot but never
        // ran the body, so `this.finalized = true` never executed and
        // hono-base's `if (!context.finalized) throw` fired on every
        // request. Walk the class -> parent chain mirroring the getter
        // dispatch in `js_object_get_field_by_name`.
        if !plan_fast && !key.is_null() && (key as usize) > 0x10000 {
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        let key_bytes = {
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            std::slice::from_raw_parts(name_ptr, name_len)
                        };
                        let mut cid = class_id;
                        let mut depth = 0usize;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cid) {
                                if let Ok(name) = std::str::from_utf8(key_bytes) {
                                    if let Some(&setter_ptr) = vtable.setters.get(name) {
                                        // Setters take `(this_f64, value_f64)`
                                        // matching the codegen calling
                                        // convention for class methods (this
                                        // = NaN-boxed POINTER_TAG of the
                                        // receiver).
                                        let this_f64: f64 = f64::from_bits(
                                            crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                        );
                                        let f: extern "C" fn(f64, f64) -> f64 =
                                            std::mem::transmute(setter_ptr);
                                        let _ = f(this_f64, value);
                                        return;
                                    }
                                }
                            }
                            match get_parent_class_id(cid) {
                                Some(p) if p != 0 && p != cid => {
                                    cid = p;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }

        if !plan_fast
            && !key.is_null()
            && (key as usize) > 0x10000
            && crate::url::is_url_object_shape(obj)
        {
            let key_str = key_to_str_for_diag(key);
            let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            let value = value_handle.get_nanbox_f64();
            match key_str.as_str() {
                "pathname" => {
                    crate::url::js_url_set_pathname(obj, value);
                    return;
                }
                "search" => {
                    crate::url::js_url_set_search(obj, value);
                    return;
                }
                "hash" => {
                    crate::url::js_url_set_hash(obj, value);
                    return;
                }
                "protocol" => {
                    crate::url::js_url_set_protocol(obj, value);
                    return;
                }
                "hostname" => {
                    crate::url::js_url_set_hostname(obj, value);
                    return;
                }
                "port" => {
                    crate::url::js_url_set_port(obj, value);
                    return;
                }
                "username" => {
                    crate::url::js_url_set_username(obj, value);
                    return;
                }
                "password" => {
                    crate::url::js_url_set_password(obj, value);
                    return;
                }
                "href" => {
                    crate::url::js_url_set_href(obj, value);
                    return;
                }
                _ => {}
            }
        }

        // Check Object.freeze/seal/preventExtensions flags
        let obj_flags = (*gc_header)._reserved;
        let is_frozen = obj_flags & crate::gc::OBJ_FLAG_FROZEN != 0;
        let is_sealed_or_no_extend =
            obj_flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;

        let keys = (*obj).keys_array;

        // Validate keys_array is a real heap pointer or null.
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                return;
            }
        }

        let mut prev_keys_usize = keys as usize;

        macro_rules! refresh_roots_after_alloc {
            () => {{
                obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
                key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
                value = value_handle.get_nanbox_f64();
                interned_key = interned_key_handle.get_raw_const_ptr::<crate::StringHeader>();
            }};
        }

        // FAST PATH: shape-transition cache with interned string pointer identity.
        //
        // #6084 item 6: the descriptor gate here used to be the process-global
        // `GLOBAL_DESCRIPTORS_IN_USE` latch, so ONE `Object.freeze` anywhere
        // (even on an object never written to again) permanently forced every
        // dynamic write in the process down the O(own-key-count) slow walk
        // below. It is now vetted per receiver: an own descriptor is visible in
        // this object's `OBJ_FLAG_HAS_DESCRIPTORS`, and only prototype-level
        // interceptors need a chain walk.
        let has_own_descriptors = obj_flags & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0;
        if !key.is_null()
            && !is_frozen
            && !is_sealed_or_no_extend
            && !has_own_descriptors
            && (plan_fast
                || !super::plain_data_write_may_intercept(
                    obj as usize,
                    (*obj).class_id,
                    f64::from_bits(JSValue::string_ptr(key as *mut _).bits()),
                ))
        {
            // The full interception vet just returned negative for this
            // (class, key) — the vtable setter walk above found nothing, the
            // URL-shape probe fell through, and `plain_data_write_may_intercept`
            // cleared the chain. Record the verdict so the next store skips
            // the vet (`plan_fast` above). Eligibility is re-derived from the
            // freshly read `obj_flags`, not the pre-vet read.
            if !plan_fast
                && obj_class_id != 0
                && obj_class_id != NATIVE_MODULE_CLASS_ID
                && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR
                && obj_flags & PLAN_BLOCKING_FLAGS == 0
            {
                super::prop_plan::store_plan_record(obj_class_id, interned_key as usize);
            }
            if let Some((next_keys, slot_idx)) =
                transition_cache_lookup(prev_keys_usize, interned_key)
            {
                // Defensive: strip a raw-null POINTER_TAG value the same
                // way the slow overflow path below does, so a bogus
                // 0x7FFD_0000_0000_0000 store doesn't leak into an
                // overflow map.
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                set_object_keys_array(obj, next_keys as *mut ArrayHeader);
                super::mark_object_dynamic_shape_unknown(obj);
                let alloc_limit =
                    std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32)
                        as usize;
                if (slot_idx as usize) < alloc_limit {
                    // Inline the field write — `obj` has already been
                    // validated (GC header read, type check, closure
                    // check) by the prelude above, and `vbits` has had
                    // the null-POINTER-TAG replacement applied. No
                    // point re-doing it in `js_object_set_field`.
                    let fields_ptr =
                        (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                    let slot = fields_ptr.add(slot_idx as usize);
                    crate::gc::runtime_store_jsvalue_slot(
                        obj as usize,
                        slot as usize,
                        slot_idx as usize,
                        vbits,
                    );
                    // Bump field_count only for inline slots — leaving
                    // it at the physical capacity is what steers
                    // `js_object_get_field_by_name`'s reads to the
                    // overflow map for slots ≥ alloc_limit. Bumping it
                    // past capacity would make reads dereference past
                    // the object's inline field array into adjacent
                    // arena data.
                    if slot_idx >= (*obj).field_count {
                        (*obj).field_count = slot_idx + 1;
                    }
                } else {
                    // Cached slot is past the object's inline capacity —
                    // store in the overflow map (same as the slow path's
                    // `new_index >= alloc_limit` branch).
                    overflow_set(obj as usize, slot_idx as usize, vbits);
                    // Deliberately do NOT bump field_count here — see
                    // above.
                }
                // #6530: this shape-cache hit is a successful own-data
                // write; class objects repeat identical key sequences
                // (bundled zod assigns `create` onto ~40 sibling class
                // objects), so from the SECOND class on the write lands
                // here — the mirror must fire on this path too.
                mirror_class_object_static_write(obj, key, value);
                return;
            }
        }

        // If no keys array exists, create one (adding new key)
        if keys.is_null() {
            // Frozen or sealed/non-extensible objects reject new keys.
            // Issue #615 — strict-mode throw instead of silent return.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Create a new keys array with the key
            let new_keys = crate::array::js_array_alloc(4);
            refresh_roots_after_alloc!();
            let new_keys =
                crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);

            // Reallocate fields to hold at least one value
            // Note: We assume the object has enough field slots pre-allocated
            js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
            mirror_class_object_static_write(obj, key, value);
            // Bump field_count so Object.keys()/values()/entries() see the new property.
            if (*obj).field_count == 0 {
                (*obj).field_count = 1;
            }
            // Record the null→single-key transition so the next object
            // that starts with `{}` and sets the same first key hits the
            // fast path above instead of allocating a fresh 4-elem
            // keys_array here.
            transition_cache_insert(0, interned_key, new_keys as usize, 0);
            return;
        }

        // Defer the Rust-String allocation for the incoming key: we only
        // need it if an accessor descriptor or per-property writable
        // attribute has been installed on this object. Both paths are
        // guarded by process-wide flags (`ACCESSORS_IN_USE` and
        // `PROPERTY_ATTRS_IN_USE`) so the common case — plain data
        // properties on a normal object — avoids the `.to_string()`
        // entirely. A 20-property row object written at 10k rows saw
        // 200k of those allocations per query; with this guard the
        // count drops to zero unless userland actually defined a
        // descriptor.
        // On a store-plan hit the object provably has no own descriptors
        // (OBJ_FLAG_HAS_DESCRIPTORS is clear — vetted below before the plan is
        // honored), so the descriptor key string can never be consulted: skip
        // the per-store String allocation entirely.
        let needs_descriptor_key = !plan_fast
            && (ACCESSORS_IN_USE.with(|c| c.get()) || PROPERTY_ATTRS_IN_USE.with(|c| c.get()));
        let incoming_key_str: Option<String> = if needs_descriptor_key && !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        } else {
            None
        };

        // Accessor short-circuit — must precede the frozen/sealed and
        // writable checks below: a property defined with a setter is invoked
        // via [[Set]] regardless of the object's frozen/sealed state (freezing
        // an accessor only clears [[Configurable]]; the setter still runs). A
        // getter-only accessor is read-only. Hoisted above the sidecar + the
        // linear-scan blocks so BOTH key-lookup paths honor it — previously the
        // frozen check at the top of each block threw before the accessor was
        // consulted (test262
        // assign/target-is-frozen-accessor-property-set-succeeds).
        //
        // Gate on the per-object `OBJ_FLAG_HAS_DESCRIPTORS` flag, not just the
        // thread-global `ACCESSORS_IN_USE`: `ACCESSOR_DESCRIPTORS` is keyed by
        // raw address, so a fresh object reusing a freed address would otherwise
        // read back the previous tenant's stale getter-only accessor and falsely
        // throw "Cannot assign to read only property" on a plain `{}` (Next.js
        // app-page-turbo runtime's `exports.Fragment = …`). A fresh allocation
        // has the flag clear, so it skips the stale lookup entirely.
        if !plan_fast
            && ACCESSORS_IN_USE.with(|c| c.get())
            && super::object_has_descriptors(obj as usize)
        {
            if let Some(ref k) = incoming_key_str {
                if let Some(acc) = get_accessor_descriptor(obj as usize, k) {
                    if acc.set != 0 {
                        let closure = (acc.set & crate::value::POINTER_MASK)
                            as *const crate::closure::ClosureHeader;
                        if !closure.is_null() {
                            let receiver = crate::value::js_nanbox_pointer(obj as i64);
                            let previous_this = super::js_implicit_this_set(receiver);
                            crate::closure::js_closure_call1(closure, value);
                            super::js_implicit_this_set(previous_this);
                        }
                    } else {
                        crate::error::throw_immutable_write(0, k);
                    }
                    return;
                }
            }
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let alloc_limit =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32) as usize;

        // Sidecar O(1) lookup when keys_array has grown past the
        // linear-scan break-even. Without this, the build-then-fill
        // pattern (`for i in 0..N { obj["k_"+i] = i; }`) is O(N²)
        // because every insert does a linear scan that grows by one
        // each iteration. With the sidecar, the per-insert cost is
        // O(1) amortized (rebuild after a `js_array_push` realloc is
        // bounded by the doubling growth pattern).
        if !key.is_null() && (key as usize) > 0x10000 && key_count >= KEYS_INDEX_THRESHOLD as usize
        {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            let key_hash = key_bytes_hash(name_ptr, name_len);
            if let Some(i) = keys_index_lookup(obj, keys, name_bytes, key_hash) {
                let i = i as usize;
                if is_frozen {
                    let key_str = key_to_str_for_diag(key);
                    crate::error::throw_immutable_write(0, &key_str);
                }
                if i < alloc_limit {
                    js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                } else {
                    let vbits = value.to_bits();
                    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                        crate::value::TAG_UNDEFINED
                    } else {
                        vbits
                    };
                    overflow_set(obj as usize, i, vbits);
                }
                mirror_class_object_static_write(obj, key, value);
                return;
            }
            // Miss path: the linear scan below will confirm and then
            // append. We skip the scan entirely and just append the
            // key (the sidecar would have found it if it existed).
            // Same effect as scanning all N entries with no match.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Skip the linear-scan loop by jumping past it via a
            // labeled-block break. The append code that follows the
            // scan is shared.
            // We achieve this by setting a marker, then the linear
            // scan checks it and skips.
            let keys_gc_header =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
                && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
            {
                (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
            } else {
                true
            };
            let owned_keys = if keys_shared {
                let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
                refresh_roots_after_alloc!();
                let keys = (*obj).keys_array;
                prev_keys_usize = keys as usize;
                let src_data = (keys as *const u8).add(8) as *const f64;
                let dst_data = (cloned as *mut u8).add(8) as *mut f64;
                for i in 0..key_count {
                    // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
                    *dst_data.add(i) = *src_data.add(i);
                }
                (*cloned).length = key_count as u32;
                super::rebuild_array_layout_from_slots(cloned);
                set_object_keys_array(obj, cloned);
                cloned
            } else {
                keys
            };
            let new_index = key_count;
            if new_index >= alloc_limit {
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
                let new_keys =
                    crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
                prev_keys_usize = if keys_shared {
                    prev_keys_usize
                } else {
                    owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
                };
                refresh_roots_after_alloc!();
                set_object_keys_array(obj, new_keys);
                super::mark_object_dynamic_shape_unknown(obj);
                overflow_set(obj as usize, new_index, vbits);
                mirror_class_object_static_write(obj, key, value);
                transition_cache_insert(
                    prev_keys_usize,
                    interned_key,
                    new_keys as usize,
                    new_index as u32,
                );
                keys_index_insert(
                    obj as usize,
                    (new_index + 1) as u32,
                    key_hash,
                    new_index as u32,
                );
                return;
            }
            let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            prev_keys_usize = if keys_shared {
                prev_keys_usize
            } else {
                owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
            };
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);
            js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
            mirror_class_object_static_write(obj, key, value);
            if new_index as u32 >= (*obj).field_count {
                (*obj).field_count = new_index as u32 + 1;
            }
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
            // The sidecar is keyed on the OBJECT pointer (see
            // `keys_index_lookup`, which probes `obj as usize`), NOT the
            // keys-array pointer — shape-sharing clones the keys array on
            // every insert, so a keys-keyed entry would be orphaned each
            // iteration. Previously this inline-slot append registered
            // under `new_keys as usize`, so the obj-keyed lookup never
            // found it and rebuilt the full O(key_count) index on every
            // write — turning a wide build that stays on the inline-slot
            // path (e.g. a class instance whose pre-sized inline capacity
            // keeps appends below the overflow threshold) into O(n²). Use
            // the object address to match the lookup + the overflow path.
            keys_index_insert(
                obj as usize,
                (new_index + 1) as u32,
                key_hash,
                new_index as u32,
            );
            return;
        }

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // #1781: SSO-aware match — keys are stored as either a
            // STRING_TAG pointer OR a SHORT_STRING_TAG inline value for
            // ≤5-byte names. Pre-fix the assignment `obj.id = v` would
            // append a duplicate `id` key instead of updating the slot
            // when the original `id` was stored inline as SSO.
            if crate::string::js_string_key_matches(key_val, key) {
                // Found it - update the field. Frozen objects must
                // throw a TypeError on writes to existing keys
                // (issue #615 — strict-mode behavior, default for TS).
                // Accessors were already handled by the hoisted short-circuit
                // above; a key found here is a data property, so a frozen object
                // throws on the write (issue #615 — strict-mode default for TS).
                if is_frozen {
                    let key_str = key_to_str_for_diag(key);
                    crate::error::throw_immutable_write(0, &key_str);
                }
                // Per-property writable check (set by Object.defineProperty / freeze).
                // Issue #615 — strict-mode throw on read-only assign.
                //
                // Gate on the per-object `OBJ_FLAG_HAS_DESCRIPTORS` flag, not just
                // the thread-global `PROPERTY_ATTRS_IN_USE`: `PROPERTY_DESCRIPTORS`
                // is keyed by raw address, so a fresh object reusing a freed
                // address would otherwise read back the previous tenant's stale
                // `(addr, key)` descriptor and falsely throw "Cannot assign to
                // read only property" on a plain `{}` (Next.js app-page-turbo
                // runtime's `exports.Fragment = …`). A fresh allocation has the
                // flag clear, so it skips the lookup entirely.
                if PROPERTY_ATTRS_IN_USE.with(|c| c.get())
                    && super::object_has_descriptors(obj as usize)
                {
                    if let Some(ref k) = incoming_key_str {
                        if let Some(attrs) = get_property_attrs(obj as usize, k) {
                            if !attrs.writable() {
                                crate::error::throw_immutable_write(0, k);
                            }
                        }
                    }
                }
                if i < alloc_limit {
                    js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                } else {
                    // This key was previously stored in the overflow map — update it there
                    let vbits = value.to_bits();
                    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                        crate::value::TAG_UNDEFINED
                    } else {
                        vbits
                    };
                    overflow_set(obj as usize, i, vbits);
                }
                mirror_class_object_static_write(obj, key, value);
                return;
            }
        }

        // Key not found - add it to the object.
        // Frozen/sealed/non-extensible objects reject new keys.
        // Issue #615 — strict-mode throw.
        if is_frozen || is_sealed_or_no_extend {
            let key_str = key_to_str_for_diag(key);
            crate::error::throw_immutable_write(1, &key_str);
        }
        // CRITICAL: The keys_array may be SHARED via SHAPE_CACHE (multiple objects with
        // the same shape hash share the same keys array). We must clone it before mutating
        // to avoid corrupting other objects' keys.
        //
        // We detect sharing via the `GC_FLAG_SHAPE_SHARED` bit that
        // `shape_cache_insert` stamps onto the array's GC header —
        // arrays allocated in the `keys.is_null()` branch above are
        // exclusively owned and don't have the flag, so we skip the
        // clone entirely. This saves ~19 clones of growing size per
        // 20-property plain-object literal.
        //
        // Validate the GC header before reading it. `keys_array` has
        // already been range-checked for user address space but may
        // still point at something other than a GC-allocated array
        // in rare cases (static data, buffers re-interpreted as keys
        // arrays). If the header doesn't identify as GC_TYPE_ARRAY,
        // assume shared and clone (the previous, always-safe behaviour).
        let keys_gc_header =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
            && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        {
            (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
        } else {
            // Unknown provenance — take the safe side.
            true
        };
        let owned_keys = if keys_shared {
            let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
            refresh_roots_after_alloc!();
            let keys = (*obj).keys_array;
            prev_keys_usize = keys as usize;
            let src_data = (keys as *const u8).add(8) as *const f64;
            let dst_data = (cloned as *mut u8).add(8) as *mut f64;
            for i in 0..key_count {
                // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
                *dst_data.add(i) = *src_data.add(i);
            }
            (*cloned).length = key_count as u32;
            super::rebuild_array_layout_from_slots(cloned);
            set_object_keys_array(obj, cloned);
            cloned
        } else {
            keys
        };

        // Check if we have a spare physical slot (js_object_alloc_with_shape allocates max(N,8) slots).
        // Class objects (js_object_alloc_class_with_keys) have only exactly field_count slots;
        // attempting to write to new_index = key_count would overflow into the next heap allocation.
        let new_index = key_count;
        if new_index >= alloc_limit {
            // No inline room — store in the overflow HashMap so the value is not lost.
            // Also add the key to keys_array so Object.keys() sees it.
            let vbits = value.to_bits();
            let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                eprintln!("[WARN_NULL_PTR] overflow new store: null POINTER_TAG at obj={:p} new_index={} — replacing with undefined", obj, new_index);
                crate::value::TAG_UNDEFINED
            } else {
                vbits
            };
            let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            prev_keys_usize = if keys_shared {
                prev_keys_usize
            } else {
                owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
            };
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);
            overflow_set(obj as usize, new_index, vbits);
            mirror_class_object_static_write(obj, key, value);
            // Record the shape transition so the next object sharing
            // `prev_keys` that adds the same key hits the fast path.
            // The cached target is stamped `GC_FLAG_SHAPE_SHARED` by
            // `transition_cache_insert`, which triggers clone-on-extend
            // on either object if someone later appends past this key.
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
            return;
        }
        // First, add the key to the keys array (may reallocate)
        let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
        let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
        prev_keys_usize = if keys_shared {
            prev_keys_usize
        } else {
            owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
        };
        refresh_roots_after_alloc!();
        // Update the object's keys_array pointer in case js_array_push reallocated
        set_object_keys_array(obj, new_keys);
        super::mark_object_dynamic_shape_unknown(obj);

        // Set the field at the new index and update logical field_count
        js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
        mirror_class_object_static_write(obj, key, value);
        // Bump field_count to reflect the newly added property
        if new_index as u32 >= (*obj).field_count {
            (*obj).field_count = new_index as u32 + 1;
        }
        // Record the shape transition — see above for semantics.
        transition_cache_insert(
            prev_keys_usize,
            interned_key,
            new_keys as usize,
            new_index as u32,
        );
    }
}

/// Set `obj[key] = value` as a non-enumerable (but writable + configurable)
/// own data property. Used by derived-class `super(message)` into a built-in
/// `Error`/`NativeError`: the spec sets `message` via DefinePropertyOrThrow
/// with `{ writable: true, enumerable: false, configurable: true }`, whereas an
/// ordinary assignment would create an enumerable property. (Test262
/// subclass/.../NativeError/*-message.)
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_nonenum(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    js_object_set_field_by_name(obj, key, value);
    // Only ordinary heap objects carry the attrs side-table. Class refs,
    // TypedArrays, Temporal cells, etc. are handled by `set_field_by_name`'s own
    // routing and never reach the ordinary enumerable default, so skip them.
    let bits = obj as u64;
    if (bits >> 48) == 0x7FFE
        || crate::value::addr_class::is_handle_band(obj as usize)
        || key.is_null()
    {
        return;
    }
    unsafe {
        if !crate::object::is_valid_obj_ptr(obj as *const u8) {
            return;
        }
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            crate::object::set_property_attrs(
                obj as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
}

/// Set `obj[key] = value` as a non-configurable (but writable + enumerable)
/// own data property. Used for `globalThis`'s reflection of a Script's
/// top-level `function`/`var` declarations: GlobalDeclarationInstantiation's
/// `CreateGlobalFunctionBinding`/`CreateGlobalVarBinding` call with `D = false`
/// (unlike sloppy-eval's Annex B.3.3.3 path, which uses `D = true` and already
/// has its own `Object.defineProperty` HIR synthesis in
/// `global_eval_hoist.rs`) — an ordinary assignment would instead create a
/// configurable property. Called once at program start before any user
/// statement runs, so there's no pre-existing descriptor to preserve or
/// `CanDeclareGlobalFunction`/`CanDeclareGlobalVar` extensibility check to
/// replicate here (test262 `language/global-code/decl-func.js`).
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_nonconfigurable(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    js_object_set_field_by_name(obj, key, value);
    let bits = obj as u64;
    if (bits >> 48) == 0x7FFE
        || crate::value::addr_class::is_handle_band(obj as usize)
        || key.is_null()
    {
        return;
    }
    unsafe {
        if !crate::object::is_valid_obj_ptr(obj as *const u8) {
            return;
        }
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            crate::object::set_property_attrs(
                obj as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, true, false),
            );
        }
    }
}
