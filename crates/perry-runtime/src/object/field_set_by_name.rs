//! Dynamic `obj[key] = value` write path (`js_object_set_field_by_name`).
//!
//! Split out of `object/field_get_set.rs` (issue #1103), then split again
//! into topical sub-modules (issue #7402) when the single file reached the
//! 2000-line cap. Pure relocation — no logic changes.
//!
//! This file keeps the entry point and its pre-rooting head: the
//! `process.env` / Proxy routing, the plan-certified fast lane, the
//! exotic-receiver gauntlet, the NaN-box strip, and the handle / typed-array
//! / `arr.length` guards. Everything from the `RuntimeHandleScope` onwards
//! lives in [`tail`].

use super::*;

// ── Topical sub-modules (issue #7402: keep every file < 2000 lines) ──
mod attr_variants;
mod fast_paths;
mod tail;
mod write_helpers;

// Explicit named re-exports so existing `crate::object::…` /
// `object::field_set_by_name::…` paths keep resolving through
// `object/mod.rs`'s `pub use field_set_by_name::*`, and so the sub-modules
// can reach the shared helpers via their own `use super::*;`.
pub use attr_variants::{
    js_object_set_field_by_name_nonconfigurable, js_object_set_field_by_name_nonenum,
};
pub use fast_paths::js_object_set_field_by_name_transition_fast;
pub(crate) use fast_paths::{
    object_set_field_by_name_transition_only_fast, try_existing_own_data_overwrite,
};
#[cfg(test)]
pub(crate) use fast_paths::{test_reset_transition_fast_hits, test_transition_fast_hits};
pub(crate) use tail::set_field_by_name_object_tail;
pub(crate) use write_helpers::nm_field_set_override;
use write_helpers::string_key_eq;

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if super::private_member_set_by_name(obj, key, value) {
        return;
    }
    // A heap class value is an exotic constructor object. Its own
    // `prototype` property is non-writable, so both ordinary assignment and
    // a computed static field whose PropertyKey resolves to "prototype" must
    // fail instead of appending an ordinary shape slot.
    let obj_bits = obj as u64;
    let normalized_obj = if (obj_bits >> 48) == 0x7FFD {
        (obj_bits & crate::value::POINTER_MASK) as *mut ObjectHeader
    } else {
        obj
    };
    if !key.is_null()
        && crate::value::addr_class::is_plausible_heap_addr(normalized_obj as usize)
        && crate::object::class_registry::is_class_object_ptr(normalized_obj.cast())
    {
        unsafe {
            if string_key_eq(key, b"prototype") {
                crate::error::throw_immutable_write((*obj).class_id, "prototype");
            }
        }
    }
    // Aliased `process.env` writes must update the OS environment, not only the
    // materialized object's field bag. The helper declines internal cache
    // mirror writes so those can proceed through the ordinary setter below.
    if crate::process::process_env_set_field(obj, key, value) {
        return;
    }
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
                let o = raw as *mut ObjectHeader;
                if try_existing_own_data_overwrite(o, key, value) {
                    return;
                }
                const LANE_BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
                    | crate::gc::OBJ_FLAG_SEALED
                    | crate::gc::OBJ_FLAG_NO_EXTEND
                    | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                    | crate::gc::OBJ_FLAG_NULL_PROTO
                    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
                if (*gc_hdr).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*gc_hdr)._reserved & LANE_BLOCKING == 0
                {
                    let class_id = (*o).class_id;
                    // #6595: a per-evaluation CLASS OBJECT must never take
                    // this lane — it skips the #6530
                    // `mirror_class_object_static_write` hook every in-body
                    // completion runs, and (template cid, key) plans recorded
                    // by instances sharing the cid would falsely certify it.
                    // See the matching gates at the plan record sites.
                    if crate::object::object_is_regular(o)
                        && class_id != 0
                        && class_id != NATIVE_MODULE_CLASS_ID
                        && !super::prototype_chain::object_has_prototype_override(raw)
                        && super::prop_plan::store_plan_check(class_id, key as usize)
                    {
                        let prev_shape_id = super::shapes::object_shape_stamp(o);
                        if prev_shape_id != 0 {
                            if let Some((next_keys, slot_idx, target_shape_id)) =
                                transition_cache_lookup(prev_shape_id, key)
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
                                if !super::shapes::install_cached_object_shape_transition(
                                    o,
                                    prev_shape_id,
                                    target_shape_id,
                                    next_keys as *mut ArrayHeader,
                                ) {
                                    set_object_keys_array(o, next_keys as *mut ArrayHeader);
                                }
                                // #8113: one bound probe, reused.
                                let live_slots = crate::object::object_live_slot_count(o);
                                let alloc_limit = std::cmp::max(
                                    live_slots,
                                    crate::object::INLINE_SLOT_FLOOR as u32,
                                ) as usize;
                                if (slot_idx as usize) < alloc_limit {
                                    let fields_ptr = (o as *mut u8)
                                        .add(std::mem::size_of::<ObjectHeader>())
                                        as *mut JSValue;
                                    let slot = fields_ptr.add(slot_idx as usize);
                                    if slot_idx >= live_slots {
                                        set_object_live_slot_count(o, slot_idx + 1);
                                    }
                                    crate::gc::runtime_store_jsvalue_slot(
                                        o as usize,
                                        slot as usize,
                                        slot_idx as usize,
                                        vbits,
                                    );
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
    // `Object.prototype["2"] = v` (stringified-index write) makes the index
    // visible through array hole/OOB reads. Cheap gate: one relaxed flag
    // load, then an address compare against the cached canonical
    // Object.prototype; the digit scan only runs on a match (test262
    // concat/S15.4.4.4_A3_T3). Hoisted above the exotic gauntlet (#6809):
    // the canonical prototype IS a genuine ObjectHeader, so it must run
    // even when the gauntlet below is skipped.
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
    // #6809: header-first receiver classification. A receiver whose GC
    // header identifies a genuine `ObjectHeader` (GC_TYPE_OBJECT, not a
    // RegExpHeader) can never be a Buffer, Web-Stream handle, Temporal
    // cell, typed array, INT32 class ref, primitive, or Date/RegExp — those
    // are different header types or non-heap encodings — so the whole
    // exotic-receiver gauntlet below is skipped in one header read. The
    // write profile (#6759 acceptance) showed the gauntlet's address-keyed
    // registry probes (each with its own TLS fetch, several behind locks)
    // dominating hot stores. Skipping by HEADER also closes the stale-
    // registry misroute where a dead exotic's recycled address, re-tenanted
    // by a plain object, could hijack the write. RegExp receivers fail
    // `meta_capable_object` (header magic) and keep taking the gauntlet.
    let receiver_is_object_header = {
        let bits = obj as u64;
        let cleaned = if bits >> 48 == 0x7FFD {
            (bits & crate::value::POINTER_MASK) as usize
        } else if bits >> 48 == 0 {
            bits as usize
        } else {
            0
        };
        cleaned != 0 && unsafe { super::prototype_chain::meta_capable_object(cleaned).is_some() }
    };
    'exotic_gauntlet: {
        if receiver_is_object_header {
            break 'exotic_gauntlet;
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
                if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len))
                {
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
                    let name_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key).byte_len as usize;
                    let name =
                        String::from_utf8_lossy(std::slice::from_raw_parts(name_ptr, name_len))
                            .into_owned();
                    let receiver = f64::from_bits(obj as u64);
                    let _ = super::exotic_expando::exotic_set_property(
                        addr, kind, &name, value, receiver,
                    );
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
                    let name_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let name_len = (*key).byte_len as usize;
                    let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                        .unwrap_or("")
                        .to_string();
                    let recv = f64::from_bits(bits);
                    let is_prototype_ref = super::class_prototype_ref_id(recv).is_some();
                    if !is_prototype_ref
                        && name == "name"
                        && !super::class_registry::class_is_key_deleted(class_id, &name)
                        && super::class_registry::lookup_static_method_in_chain(class_id, &name)
                            .is_none()
                    {
                        return;
                    }
                    let has_own_data = if is_prototype_ref {
                        super::class_registry::lookup_own_prototype_method(class_id, &name)
                            .is_some()
                            || super::native_module::class_has_own_method(class_id, &name)
                    } else {
                        CLASS_DYNAMIC_PROPS.with(|m| {
                            m.borrow()
                                .get(&class_id)
                                .is_some_and(|props| props.contains_key(&name))
                        })
                    };
                    // `C.prototype[key] = v` where `key` is an instance
                    // accessor invokes the setter with `this = C.prototype`.
                    // A getter-only accessor absorbs a non-strict assignment.
                    if is_prototype_ref && !has_own_data {
                        if super::class_registry::class_instance_setter_apply(
                            class_id, &name, recv, value,
                        ) {
                            return;
                        }
                        if super::class_registry::class_has_instance_getter(class_id, &name) {
                            return;
                        }
                    } else if !is_prototype_ref
                        && !has_own_data
                        && super::class_registry::class_static_accessor_setter_apply(
                            class_id, &name, recv, value,
                        )
                    {
                        return;
                    }
                    // Writing `.caller` / `.arguments` on a class constructor
                    // hits the poison-pill %ThrowTypeError% accessor inherited
                    // from Function.prototype. Prototype refs are plain objects.
                    if !is_prototype_ref
                        && !has_own_data
                        && matches!(name.as_str(), "caller" | "arguments")
                    {
                        crate::fs::validate::throw_type_error_with_code(
                            "Restricted function property access",
                            "ERR_INVALID_ARG_TYPE",
                        );
                    }
                    if is_prototype_ref {
                        // Imported `C.prototype.m = value` reaches this generic
                        // class-ref path. Publish it as an enumerable prototype
                        // data property so instance dispatch sees the replacement.
                        super::class_registry::class_prototype_method_set_enumerable(
                            class_id, &name, true,
                        );
                        super::class_registry::class_prototype_method_root_store(
                            class_id,
                            name,
                            value.to_bits(),
                        );
                        crate::typed_feedback::invalidate_method_change(class_id);
                    } else {
                        class_dynamic_prop_root_store(class_id, &name, value);
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
        if string_key_eq(key, b"length") {
            let receiver = crate::value::js_nanbox_pointer(obj as i64);
            if crate::array::is_array_subclass_value(receiver) {
                crate::array::array_object_set_length(receiver, value);
                return;
            }
        }
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
    tail::set_field_by_name_object_tail(obj, key, value);
}
