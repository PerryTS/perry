//! Rooted ObjectHeader tail of `js_object_set_field_by_name`: everything from
//! the `RuntimeHandleScope` onwards — the store-plan gate, class setter walk,
//! URL setters, frozen/sealed/descriptor guards, the shape-transition fast
//! path, the keys-array creation/append arms, and the overflow-map arms.
//! Split out of `object/field_set_by_name.rs` (issue #7402) — pure
//! relocation, no logic changes.
//!
//! The `refresh_roots_after_alloc!()` macro (#7341) and all of its call sites
//! live together in this file by construction: the split point is the scope
//! creation, so no rooted step crosses the module boundary.

use super::write_helpers::{
    closure_set_field_by_name, key_to_str_for_diag, mirror_class_object_static_write,
};
use super::*;

/// Bits to publish into an overflow slot for a NEWLY appended key.
///
/// Must be called AFTER the last `refresh_roots_after_alloc!()` on the path,
/// so it reads the post-collection `value` (#7538). Also scrubs the
/// `0x7FFD_0000_0000_0000` null-POINTER_TAG shape that must never reach
/// overflow storage.
#[inline]
fn overflow_store_bits(value: f64, obj: *mut ObjectHeader, new_index: usize) -> u64 {
    let vbits = value.to_bits();
    if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
        eprintln!("[WARN_NULL_PTR] overflow new store: null POINTER_TAG at obj={obj:p} new_index={new_index} — replacing with undefined");
        crate::value::TAG_UNDEFINED
    } else {
        vbits
    }
}

/// Tail of `js_object_set_field_by_name`, entered with `obj` already stripped
/// of its NaN-box tag and vetted against the handle band, typed arrays, and
/// the `arr.length` special case. Body moved verbatim.
#[allow(unused_assignments)]
pub(crate) fn set_field_by_name_object_tail(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    // An elements-backed Array-subclass instance stores its indices and
    // `length` in its store: no index key ever becomes a shape property.
    if let Some((backed_obj, elements)) =
        unsafe { crate::array::subclass_elements::backed(obj as usize) }
    {
        if let Some(elements_key) = unsafe { crate::array::subclass_elements::key_of_header(key) } {
            unsafe {
                crate::array::subclass_elements::set_by_key(
                    backed_obj,
                    elements,
                    elements_key,
                    value,
                )
            };
            return;
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
        // Validate this is an ObjectHeader, not some other heap type. Every
        // shaped object has a tracked GcHeader; the payload's first word is only
        // a compatibility mirror and is never a kind fallback.
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
            if (*gc_header)._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
                && crate::state::state().descriptors.accessors_in_use.get()
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
            // had MapHeader.size aliasing the first ObjectHeader word,
            // so `m.customProp = 5` walked the Map's bytes as object fields —
            // deterministic heap corruption (2026-07-02 audit P1). The
            return;
        }

        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            closure_set_field_by_name(obj, key, value);
            return;
        }

        // Check if this is a ClosureHeader — closures support dynamic props via separate storage.
        // ClosureHeader has CLOSURE_MAGIC (0x434C4F53) at offset 12.
        // Without this check, crate::object::object_keys_array(obj) reads capture[0] → corruption/crash.
        let type_tag_at_12 =
            *((obj as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            closure_set_field_by_name(obj, key, value);
            return;
        }

        if super::arguments_object_set_field(obj, key, value) {
            return;
        }

        // The disposable-stack `disposed` property is an inherited builtin
        // getter with no setter. Its prototype descriptor is installed
        // gate-neutrally, and these reserved native class ids are not present
        // in the JS class-prototype registry, so reject the write here instead
        // of creating an own field. A user `defineProperty` own property still
        // shadows the inherited accessor.
        if !key.is_null()
            && ((*obj).class_id == crate::disposable::CLASS_ID_DISPOSABLE_STACK
                || (*obj).class_id == crate::disposable::CLASS_ID_ASYNC_DISPOSABLE_STACK)
        {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"disposed"
                && !super::object_ops::own_key_present(obj as *mut ObjectHeader, key)
            {
                crate::error::throw_immutable_write(0, "disposed");
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            // Namespace-object stores route through the armed ops table (see
            // `nm_namespace_hooks`) so binaries without module imports don't
            // statically link the namespace override machinery. Unarmed +
            // matching class_id is unreachable (only the arming bootstrap
            // assigns NATIVE_MODULE_CLASS_ID).
            if let Some(ops) = super::nm_namespace_ops() {
                if (ops.field_set_override)(obj, key, value) {
                    return;
                }
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
        const PLAN_BLOCKING_FLAGS: u16 =
            crate::gc::OBJ_FLAG_NULL_PROTO | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
        let obj_class_id = (*obj).class_id;
        // #6595: class objects are excluded by their authoritative ShapeId
        // kind — their
        // writes must always reach the `mirror_class_object_static_write`
        // completions, and their cid is shared with their instances so a
        // plan keyed on it conflates two different prototype chains.
        let plan_eligible = !key.is_null()
            && obj_class_id != 0
            && obj_class_id != NATIVE_MODULE_CLASS_ID
            && crate::object::object_is_regular(obj)
            && (*gc_header)._reserved & PLAN_BLOCKING_FLAGS == 0
            && !super::prototype_chain::object_has_prototype_override(obj as usize);
        let plan_fast = plan_eligible
            && super::prop_plan::store_plan_check(obj_class_id, interned_key as usize);

        // A per-evaluation class object carries dynamically installed static
        // accessors in the descriptor side table, not in its ordinary field
        // slots. Invoke that setter before the generic instance-vtable and
        // own-data paths below; otherwise `C.x = value` silently appends or
        // overwrites a data field after `defineProperty(C, "x", { set })`.
        if !plan_fast
            && !key.is_null()
            && crate::object::class_registry::is_class_object_ptr(obj.cast())
        {
            let name_ptr = crate::string::string_data(key);
            let name_len = (*key).byte_len as usize;
            if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
                let receiver =
                    f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                if super::class_registry::class_static_accessor_setter_apply(
                    obj_class_id,
                    name,
                    receiver,
                    value,
                ) {
                    return;
                }
            }
        }

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
        // OrdinarySet step 1 is `O.[[GetOwnProperty]](P)`: an own data property
        // on the RECEIVER shadows an inherited accessor, so the vtable walk only
        // applies when the receiver has no own property of this name (Node
        // 26.5.1: `class A { x = 1; set x(v){} }` + `Object.assign(a, {x: 7})`
        // stores 7 and never calls the setter). `own_key_present` is a
        // keys-array scan, so it is evaluated LAST and only on the slow path --
        // a `plan_fast` hit must not pay for it.
        if !plan_fast
            && !key.is_null()
            && (key as usize) > 0x10000
            && !super::object_ops::own_key_present(obj as *mut ObjectHeader, key)
        {
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

        // Binary size: this whole arm statically references eight
        // `js_url_set_*` entry points, which is what kept the URL parser in
        // every binary. Gated with the other URL shape probes; see
        // `value/to_string.rs`.
        #[cfg(feature = "url-engine")]
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
        let record_array_tail = crate::array::is_array_subclass_class_id((*obj).class_id);

        let keys = crate::object::object_keys_array(obj);

        // Validate keys_array is a real heap pointer or null.
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                return;
            }
        }

        let mut prev_keys_usize = keys as usize;
        let prev_shape_id = super::shapes::object_shape_stamp(obj);

        // #7341: call after ANY allocating step, before the next use of
        // obj/key/value. Rationale in changelog.d/7381-*, 7383-*.
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
            let record_plan_eligible = obj_class_id != 0
                && obj_class_id != NATIVE_MODULE_CLASS_ID
                && crate::object::object_is_regular(obj)
                && obj_flags & PLAN_BLOCKING_FLAGS == 0
                && !super::prototype_chain::object_has_prototype_override(obj as usize);
            if !plan_fast && record_plan_eligible {
                super::prop_plan::store_plan_record(obj_class_id, interned_key as usize);
            }
            if let Some((next_keys, slot_idx, target_shape_id)) =
                transition_cache_lookup(prev_shape_id, interned_key)
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
                if !super::shapes::install_cached_object_shape_transition(
                    obj,
                    prev_shape_id,
                    target_shape_id,
                    next_keys as *mut ArrayHeader,
                ) {
                    set_object_keys_array(obj, next_keys as *mut ArrayHeader);
                }
                // #8113: one bound probe, reused.
                let live_slots = crate::object::object_live_slot_count(obj);
                let alloc_limit =
                    std::cmp::max(live_slots, crate::object::INLINE_SLOT_FLOOR as u32) as usize;
                if (slot_idx as usize) < alloc_limit {
                    // Inline the field write — `obj` has already been
                    // validated (GC header read, type check, closure
                    // check) by the prelude above, and `vbits` has had
                    // the null-POINTER-TAG replacement applied. No
                    // point re-doing it in `js_object_set_field`.
                    let fields_ptr =
                        (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                    let slot = fields_ptr.add(slot_idx as usize);
                    // Publish the expanded traced range and its exact
                    // descriptor before the pointer-bearing slot value.
                    if slot_idx >= live_slots {
                        set_object_live_slot_count(obj, slot_idx + 1);
                    }
                    crate::gc::runtime_store_jsvalue_slot(
                        obj as usize,
                        slot as usize,
                        slot_idx as usize,
                        vbits,
                    );
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
                refresh_roots_after_alloc!();
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
            // #7154 publication order: `gc_field_slot_range` bounds the
            // collector's view of the payload by `field_count`, so a slot at an
            // index the count does not yet cover is invisible to BOTH tracing
            // and evacuation rewriting. Widen the count FIRST — every physical
            // slot is undefined-initialized at allocation, so the widened range
            // can only expose non-pointer sentinels — then publish the value.
            // Bump field_count so Object.keys()/values()/entries() see the new property.
            if crate::object::object_live_slot_count(obj) == 0 {
                set_object_live_slot_count(obj, 1);
            }
            js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
            refresh_roots_after_alloc!();
            mirror_class_object_static_write(obj, key, value);
            // Record the null→single-key transition so the next object
            // that starts with `{}` and sets the same first key hits the
            // fast path above instead of allocating a fresh 4-elem
            // keys_array here.
            transition_cache_insert(
                if record_array_tail {
                    obj as *const ObjectHeader
                } else {
                    std::ptr::null()
                },
                prev_shape_id,
                interned_key,
                new_keys as usize,
                0,
                super::shapes::object_shape_stamp(obj),
            );
            // #6804: birth-stamp the new dynamic shape (once per shape
            // birth — the transition edge above serves the siblings).
            // #6759 C3 rung 1: no `class_id == 0` gate — a keyless class
            // instance gaining its first by-name property is stamped like
            // any other receiver.
            super::shapes::stamp_object_shape(obj, new_keys, 1, 1);
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
        let needs_descriptor_key = !plan_fast && has_own_descriptors;
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
        if !plan_fast && has_own_descriptors {
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
        let alloc_limit = std::cmp::max(
            crate::object::object_live_slot_count(obj),
            crate::object::INLINE_SLOT_FLOOR as u32,
        ) as usize;

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
                refresh_roots_after_alloc!();
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
                let keys = crate::object::object_keys_array(obj);
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
                // #8067: migrate only the owned array's validated slot index;
                // the immutable descriptor is versioned to the new facts. A
                // shared fork must NOT migrate: the old address still serves
                // the siblings' live shape.
                if !keys_shared {
                    super::shapes::shape_keys_grown(prev_keys_usize, new_keys);
                }
                // #7538: derive the stored bits from the REFRESHED `value` —
                // see the twin below the linear scan.
                let vbits = overflow_store_bits(value, obj, new_index);
                overflow_set(obj as usize, new_index, vbits);
                refresh_roots_after_alloc!();
                mirror_class_object_static_write(obj, key, value);
                transition_cache_insert(
                    if record_array_tail {
                        obj as *const ObjectHeader
                    } else {
                        std::ptr::null()
                    },
                    prev_shape_id,
                    interned_key,
                    new_keys as usize,
                    new_index as u32,
                    super::shapes::object_shape_stamp(obj),
                );
                keys_index_insert(
                    crate::object::object_keys_array(obj),
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
            // #8067: owned grow keeps the slot index, while the immutable
            // descriptor is versioned (see the overflow branch above).
            if !keys_shared {
                super::shapes::shape_keys_grown(prev_keys_usize, new_keys);
            }
            // #7154 publication order: `gc_field_slot_range` bounds the
            // collector's view of the payload by `field_count`, so a slot at an
            // index the count does not yet cover is invisible to BOTH tracing
            // and evacuation rewriting. Widen the count FIRST — every physical
            // slot is undefined-initialized at allocation, so the widened range
            // can only expose non-pointer sentinels — then publish the value.
            if new_index as u32 >= crate::object::object_live_slot_count(obj) {
                set_object_live_slot_count(obj, new_index as u32 + 1);
            }
            js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
            refresh_roots_after_alloc!();
            mirror_class_object_static_write(obj, key, value);
            transition_cache_insert(
                if record_array_tail {
                    obj as *const ObjectHeader
                } else {
                    std::ptr::null()
                },
                prev_shape_id,
                interned_key,
                new_keys as usize,
                new_index as u32,
                super::shapes::object_shape_stamp(obj),
            );
            // #6759 C1 note: `keys_index_insert` delegates to the keys-keyed
            // shape records and takes the POST-append keys_array — with the
            // C3a migration above, an owned grow lands the append on the
            // migrated record rather than forcing a rebuild.
            keys_index_insert(
                crate::object::object_keys_array(obj),
                (new_index + 1) as u32,
                key_hash,
                new_index as u32,
            );
            return;
        }

        // #8936/#8950's resolver narrows the candidate to one slot via the
        // shape hash index instead of walking every key (and probing each for
        // per-index accessors via `js_array_get`); the original match below
        // still gates the hit, so this cannot widen what is accepted.
        if let Some(i) = crate::object::keys_find_slot_by_key_ptr(keys, key_count as u32, key)
            .map(|v| v as usize)
        {
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
                if has_own_descriptors {
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
                refresh_roots_after_alloc!();
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
            let keys = crate::object::object_keys_array(obj);
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
            // #8067: migrate the owned slot index, not the descriptor id.
            if !keys_shared {
                super::shapes::shape_keys_grown(prev_keys_usize, new_keys);
            }
            // #7538: the bits stored into overflow must come from the
            // REFRESHED `value`. This was snapshotted ABOVE the
            // `js_array_push` that grows the keys array — an allocation, so a
            // collection point. `refresh_roots_after_alloc!` re-reads `value`
            // from its handle, but a `let vbits = value.to_bits()` taken
            // before the push is a dead copy the macro cannot reach, and this
            // is the branch that stores it. An evacuating minor inside the
            // push therefore published the from-space address of the value
            // into the owner's overflow slot: the collector had already
            // rewritten every live reference, so nothing ever corrected it
            // and no verifier could see it (the target is live, just moved).
            //
            // Reachable for any object grown past its inline allocation by
            // NAME — which is exactly what `json_tape`'s element-wise
            // materializer does (`js_object_alloc(0, 0)` + one
            // `js_object_set_field_by_name` per key), so a parsed record with
            // more than `INLINE_SLOT_FLOOR` keys parks its last field here.
            // The direct parser pre-sizes from a known field count and never
            // overflows, which is why `PERRY_JSON_TAPE=0` did not reproduce.
            let vbits = overflow_store_bits(value, obj, new_index);
            overflow_set(obj as usize, new_index, vbits);
            refresh_roots_after_alloc!();
            mirror_class_object_static_write(obj, key, value);
            // Record the shape transition so the next object with this exact
            // predecessor ShapeId that adds the same key hits the fast path.
            // The cached target is stamped `GC_FLAG_SHAPE_SHARED` by
            // `transition_cache_insert`, which triggers clone-on-extend
            // on either object if someone later appends past this key.
            transition_cache_insert(
                if record_array_tail {
                    obj as *const ObjectHeader
                } else {
                    std::ptr::null()
                },
                prev_shape_id,
                interned_key,
                new_keys as usize,
                new_index as u32,
                super::shapes::object_shape_stamp(obj),
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
        // #8067: migrate the owned slot index, not the descriptor id.
        if !keys_shared {
            super::shapes::shape_keys_grown(prev_keys_usize, new_keys);
        }

        // Set the field at the new index and update logical field_count
        // #7154 publication order: `gc_field_slot_range` bounds the
        // collector's view of the payload by `field_count`, so a slot at an
        // index the count does not yet cover is invisible to BOTH tracing
        // and evacuation rewriting. Widen the count FIRST — every physical
        // slot is undefined-initialized at allocation, so the widened range
        // can only expose non-pointer sentinels — then publish the value.
        // Bump field_count to reflect the newly added property
        if new_index as u32 >= crate::object::object_live_slot_count(obj) {
            set_object_live_slot_count(obj, new_index as u32 + 1);
        }
        js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
        refresh_roots_after_alloc!();
        mirror_class_object_static_write(obj, key, value);
        // Record the shape transition — see above for semantics.
        transition_cache_insert(
            if record_array_tail {
                obj as *const ObjectHeader
            } else {
                std::ptr::null()
            },
            prev_shape_id,
            interned_key,
            new_keys as usize,
            new_index as u32,
            super::shapes::object_shape_stamp(obj),
        );
    }
}
