//! get_field_by_name_f64, IC-miss slow path, and private-brand guards.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;
use crate::codegen_abi::LayoutWord;

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    if !cannot_be_private_member_name(key) {
        if let Some(value) = private_member_get_by_name(obj, key) {
            return value;
        }
    }
    if (obj as usize) > 0 && (obj as usize) < 0x10000 && !key.is_null() {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            let class_id = obj as usize as u32;
            if name == "name"
                && !super::super::class_registry::class_static_key_deleted(class_id, name)
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
    if value.bits() == crate::value::TAG_UNDEFINED && !key.is_null() {
        if let Some(name) = unsafe { super::super::has_own_helpers::str_from_string_header(key) } {
            if let Some(fwd) = super::super::native_this_alias::alias_forward_property_read(
                f64::from_bits(obj as u64),
                name,
            ) {
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
pub(crate) fn primitive_proto_method_name_static(key: &[u8]) -> Option<&'static [u8]> {
    match key {
        b"toString" => Some(b"toString"),
        b"valueOf" => Some(b"valueOf"),
        b"hasOwnProperty" => Some(b"hasOwnProperty"),
        b"isPrototypeOf" => Some(b"isPrototypeOf"),
        b"propertyIsEnumerable" => Some(b"propertyIsEnumerable"),
        b"toLocaleString" => Some(b"toLocaleString"),
        _ => None,
    }
}

/// Bind a primitive receiver's inherited method without allowing the closure
/// to retain the caller's key storage. Both finite-number guards route through
/// this helper so the pointer-lifetime rule has a single implementation.
pub(crate) unsafe fn bind_primitive_proto_method_static(
    receiver: f64,
    key: &[u8],
) -> Option<JSValue> {
    let method = primitive_proto_method_name_static(key)?;
    let result = super::super::js_class_method_bind(receiver, method.as_ptr(), method.len());
    Some(JSValue::from_bits(result.to_bits()))
}

/// Static-name lowering traffics in immutable AOT descriptors instead of
/// thread-local heap pointers. APIs below this wrapper still consume a
/// `StringHeader*`, so descriptors are lazily interned once per runtime thread.
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
    let key = crate::string::materialize_dispatch_key(key_ref);
    js_object_get_field_by_name_f64(obj, key)
}

/// By-id sibling of `js_object_set_field_by_name`. See
/// `js_object_get_field_by_property_id_f64` for descriptor materialization.
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
    let key = crate::string::materialize_dispatch_key(key_ref);
    js_object_set_field_by_name(obj, key, value);
}

pub(crate) fn is_array_method_value_name(key: &[u8]) -> bool {
    matches!(
        key,
        b"pop" | b"push" | b"shift" | b"unshift" | b"splice" | b"slice"
    )
}

/// A `Timeout` / `Immediate` handle method key, as a `'static` byte string:
/// the literal out of this list, never a borrow of `key`.
///
/// #8133 — this replaced the `is_timer_handle_method_key` PREDICATE it used to
/// be, and the replacement is the fix, not a refactor. Every caller derives
/// `key` as `key_string + size_of::<StringHeader>()` — the interior of a
/// movable GC heap string that is unreachable the moment the read returns —
/// and then handed that same pointer to `js_class_method_bind`, which captures
/// it into the bound closure for `dispatch_bound_method` to re-read at CALL
/// time. #7747 fixed exactly this on the Buffer path and its commit message
/// names the consequence: "whether the stale bytes still spell the method is an
/// allocator property, not a program property", which is why it passed locally
/// and took a SIGSEGV on conformance-smoke.
///
/// Returning the literal rather than answering `bool` is deliberate: with no
/// predicate left, a caller has nothing to pair with its own pointer, so the
/// bug cannot be reintroduced by writing the obvious code. Same shape as
/// `buffer_method_name_static` (#7747).

/// Bit flipped into the ShapeId a compact MRU word publishes when the key is
/// SPILL-located. **Must equal `PACKED_SPILL_FLIP` in perry-codegen.**
///
/// ShapeIds live in [0x8000_0000, 0xC000_0000), so flipping the TOP TWO bits
/// lands a spill entry in [0x4000_0000, 0x8000_0000) — the one u32 band that
/// is neither a ShapeId, nor a synthetic class id (at or above 0x8000_0000
/// today, [0xC000_0000, 0xFFFF_0000) under #10824), nor reachable by an
/// ordinary HIR class id without ~2^30 classes. Flipping only bit 30 would
/// land it in [0xC000_0000, 2^32), which #10824 turns into the synthetic
/// class-id range — a receiver carrying one would then take the inline load
/// with a spill index, silently.
/// The emitted hit path's plain compare therefore declines a spill entry for
/// free, which is what let the overflow-bit test (a 10-byte `movabs`, a `test`
/// and a branch) leave the hit path of every site, including the ones whose
/// field is inline and could never see the bit.
pub(crate) const PACKED_SPILL_FLIP: u32 = 0xC000_0000;

/// Does this heap property key have exactly these bytes?
///
/// Length first, so a mismatched key costs one `u32` load and a compare — the
/// point is to keep the fast-path probe cheaper than the ladder it skips.
///
/// # Safety
/// `key` must be null or a live heap `StringHeader` (the same contract every
/// other key read in this file relies on — property-name literals are interned
/// as heap strings, never SSO immediates).
#[inline]
unsafe fn key_bytes_are(key: *const crate::StringHeader, want: &[u8]) -> bool {
    if key.is_null() || (*key).byte_len as usize != want.len() {
        return false;
    }
    let p = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::slice::from_raw_parts(p, want.len()) == want
}
#[path = "ic_miss/class_field_chain.rs"]
mod class_field_chain;
#[path = "ic_miss/class_field_define_fast.rs"]
mod class_field_define_fast;

/// The complete read of `key` on a receiver the probe declined or never
/// sees: the exotic receivers' spec paths (a Proxy, a native-alias object, an
/// Array subclass's elements and `length`, an Array's `length`, a closure's
/// own properties, a Buffer, a typed array, a small native handle); every
/// other receiver takes the complete by-name operation. Nothing is learned.
pub(crate) fn get_field_miss(obj: *const ObjectHeader, key: *const crate::StringHeader) -> f64 {
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
    // A Proxy is a small fake pointer in its own band; route it before any
    // header read (#2846).
    {
        let addr = obj as u64;
        if crate::value::addr_class::is_proxy_cell(addr as usize) {
            let boxed = f64::from_bits(crate::value::POINTER_TAG | (addr & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy_hooks::proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy_hooks::proxy_get(boxed, key_f64);
            }
        }
    }
    unsafe { crate::object::field_rep_store::migrate_on_miss(obj as usize) };
    let gc_header = unsafe { crate::value::addr_class::try_read_gc_header(obj as usize) };
    let gc_kind = gc_header.map(|h| h.obj_type);
    if gc_kind == Some(crate::gc::GC_TYPE_OBJECT)
        && unsafe { super::super::native_this_alias::object_alias(obj) }.is_some()
    {
        return js_object_get_field_by_name_f64(obj, key);
    }
    if crate::value::addr_class::is_above_handle_band(obj as usize) {
        if gc_kind == Some(crate::gc::GC_TYPE_OBJECT) {
            if let Some(elements_key) =
                unsafe { crate::array::subclass_elements::key_of_header(key) }
            {
                if let Some((_, elements)) =
                    unsafe { crate::array::subclass_elements::backed(obj as usize) }
                {
                    if let Some(value) = unsafe {
                        crate::array::subclass_elements::get_by_key(elements, elements_key)
                    } {
                        return value;
                    }
                }
            }
            if unsafe { key_bytes_are(key, b"length") } {
                let receiver = crate::value::js_nanbox_pointer(obj as i64);
                if let Some(length) = crate::array::array_subclass_fast_length(receiver) {
                    return length;
                }
            }
        } else {
            if gc_kind == Some(crate::gc::GC_TYPE_ARRAY) && unsafe { key_bytes_are(key, b"length") }
            {
                let arr = obj as *const crate::array::ArrayHeader;
                return crate::array::js_array_length(arr) as f64;
            }
            unsafe {
                if let Some(val) = closure_dynamic_prop_by_key(obj as usize, key) {
                    return val;
                }
                if super::has_property::closure_get_is_complete(obj as usize, key) {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                let binary_receiver = crate::typedarray::is_offheap_sidetable_alloc(obj as usize);
                if binary_receiver
                    && (crate::buffer::is_registered_buffer(obj as usize)
                        || crate::typedarray::lookup_typed_array_kind(obj as usize).is_some())
                {
                    let value = js_object_get_field_by_name(obj, key);
                    return f64::from_bits(value.bits());
                }
            }
        }
    }
    if crate::value::addr_class::is_small_handle(obj as usize) {
        {
            let boxed =
                f64::from_bits(crate::value::POINTER_TAG | ((obj as u64) & 0x0000_FFFF_FFFF_FFFF));
            if crate::proxy_hooks::proxy_is_proxy(boxed) != 0 {
                let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
                return crate::proxy_hooks::proxy_get(boxed, key_f64);
            }
        }
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
                return crate::object::null_stub_value();
            }
        }
        if let Some(dispatch) = handle_property_dispatch() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let bits = dispatch(obj as i64, key_ptr, key_len);
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
    let value = super::get_field_by_name::get_field_by_name_after_site_miss(obj, key);
    f64::from_bits(value.bits())
}

// Polymorphic numeric-key get/set (`js_object_get_index_polymorphic` /
// `js_object_set_index_polymorphic`) live in `polymorphic_index.rs`:
// they dispatch by GC type (array vs object vs closure vs buffer) rather
// than touching object field storage directly, so they were split out
// of this module. See `polymorphic_index.rs` for the implementations
// and the #471 fix notes.

#[cfg(test)]
mod ladder_skip_tests {
    //! The receiver-classification ladder (closure / buffer / typed-array
    //! probes) runs only for receivers that are NOT `GC_TYPE_OBJECT`. The
    //! typed-array registry counts its probes per thread, so "the ladder was
    //! skipped" is directly observable — and the Array receiver case pins that
    //! the counter still moves when the ladder does run, so this cannot pass
    //! by the counter having stopped counting.
    use crate::object::ObjectHeader;

    fn key_of(bytes: &[u8]) -> *const crate::StringHeader {
        crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    #[test]
    fn an_object_miss_does_not_consult_the_typed_array_registry() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
        let present = scope.root_string_ptr(key_of(b"ladder_present"));
        let absent = scope.root_string_ptr(key_of(b"ladder_absent"));
        obj.with_mut_ptr(|o| {
            present.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 3.0))
        });

        // The OWN-key miss — the read every megamorphic site takes, and the
        // one the ladder skip was measured on. It resolves in the keys scan
        // and returns before the handler's by-name tail.
        let before = crate::typedarray::test_typed_array_registry_probe_count();
        let hit = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            present.with_const_ptr(|k| super::get_field_miss(o, k))
        });
        assert_eq!(hit, 3.0, "test premise: the own key is answered");
        assert_eq!(
            crate::typedarray::test_typed_array_registry_probe_count(),
            before,
            "a GC_TYPE_OBJECT receiver must skip the closure/buffer/typed-array \
             ladder: its kind byte already rules all three out (#10828, rule 3)"
        );
        // An ABSENT key falls through to `js_object_get_field_by_name`, whose
        // own dispatch still probes the registries for an object receiver (8
        // probes, measured) — a separate ladder this change does not touch,
        // so its count is deliberately not asserted here.
        let missing = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            absent.with_const_ptr(|k| super::get_field_miss(o, k))
        });
        assert_eq!(missing.to_bits(), crate::value::TAG_UNDEFINED);

        // The ladder still runs for a receiver that is not an object: a dense
        // array asked for a non-`length` key reaches the typed-array probe.
        let arr = crate::array::js_array_alloc(2);
        let before = crate::typedarray::test_typed_array_registry_probe_count();
        let v = absent.with_const_ptr(|k| super::get_field_miss(arr as *const ObjectHeader, k));
        assert_eq!(v.to_bits(), crate::value::TAG_UNDEFINED);
        assert!(
            crate::typedarray::test_typed_array_registry_probe_count() > before,
            "test premise: a non-object receiver still walks the ladder, so the \
             counter this test reads is live"
        );
    }
}

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
        {
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

crate::perry_thread_local! {
    /// The ClassDefinitionEvaluation captured by the currently executing
    /// method function. Each vtable call pushes one entry, including an
    /// `undefined` delimiter for ordinary classes, so a nested call never
    /// inherits its caller's private-name environment by accident.
    static PRIVATE_LEXICAL_BRAND_STACK: std::cell::RefCell<crate::exception::CatchStack<u64>> =
        const { std::cell::RefCell::new(crate::exception::CatchStack::new()) };
}

/// Push the lexical brand for the calling frame's dispatch.
#[inline(always)]
pub(crate) fn private_lexical_brand_push(value: f64) {
    private_lexical_brand_push_at(crate::exception::frame_mark(), value);
}

/// Push the lexical brand for the dispatch the frame `mark` names.
#[inline]
pub(crate) fn private_lexical_brand_push_at(mark: usize, value: f64) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow_mut().push(mark, value.to_bits()));
}

pub(crate) fn private_lexical_brand_pop() {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn private_lexical_brand_unwind_to(sp: usize) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        stack.borrow_mut().unwind_to(sp);
    });
}

pub(crate) fn private_lexical_brand_check_marks(sp: usize) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        crate::exception::marks::check_marks(stack.borrow().marks(), sp, "private_lexical_brand")
    });
}

pub(crate) fn private_lexical_brand_depth() -> usize {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow().len())
}

/// Drop the brands at or above `depth` (a scope's own exit; idempotent after
/// a landing pad already popped them).
pub(crate) fn private_lexical_brand_truncate(depth: usize) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

pub(crate) fn scan_private_lexical_brand_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        for bits in stack.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
    scan_private_member_access_hint_roots_mut(visitor);
}

fn current_private_lexical_brand(declaring_class_id: u32) -> Option<u64> {
    PRIVATE_LEXICAL_BRAND_STACK.with(|stack| {
        let bits = *stack.borrow().last()?;
        let value = f64::from_bits(bits);
        // Ordinary inherited dispatch can publish the most-derived evaluation.
        // Resolve the declaring template on that pinned chain; an explicitly
        // bound ancestor method already starts at its exact lexical owner.
        instance_ancestor_evaluation_brand(value, declaring_class_id)
    })
}

/// The class-definition evaluation `value` belongs to for members of
/// `declaring_class_id`: a class object of that template is its own, and an
/// instance's is its recorded brand's ancestor for that template.
pub(crate) fn class_evaluation_of(value: f64, declaring_class_id: u32) -> Option<f64> {
    private_evaluation_brand(value, declaring_class_id).map(f64::from_bits)
}

pub(crate) fn current_private_lexical_brand_value(declaring_class_id: u32) -> Option<f64> {
    current_private_lexical_brand(declaring_class_id).map(f64::from_bits)
}

/// Record the owner of a `ClassExprFresh` instance or evaluation prototype.
/// Prototypes carry lexical ownership only; private access still separately
/// requires an initialized instance element. The brand lives in the
/// shared evaluation holder named by the ordinary instance's birth shape.
/// Replacement receivers and lexical prototypes keep their per-object record.
pub(crate) unsafe fn stamp_private_evaluation_brand(obj: *mut ObjectHeader, class_value: f64) {
    if obj.is_null() || !super::super::class_registry::is_class_object_value(class_value) {
        return;
    }
    let meta = (*obj).meta;
    if (meta.is_null() || (*meta).private_evaluation_brand == 0)
        && crate::object::shapes::object_evaluation_brand(obj) == class_value.to_bits()
    {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let class_handle = scope.root_nanbox_f64(class_value);
    let (meta, _) =
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(obj));
    let brand = class_handle.get_nanbox_f64().to_bits();
    (*meta).private_evaluation_brand = brand;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).private_evaluation_brand as *const u64 as usize,
        brand,
    );
}

/// Record an evaluation on a fresh class receiver's shape. This also serves
/// static subclass births and class-env stamps, whose actual prototype may
/// differ from the evaluation holder. Return-override stamps use the metadata
/// helper above instead.
pub(crate) unsafe fn stamp_private_birth_evaluation_brand(
    obj: *mut ObjectHeader,
    class_value: f64,
) {
    if obj.is_null() || !super::super::class_registry::is_class_object_value(class_value) {
        return;
    }
    crate::object::shapes::stamp_evaluation_holder(obj, class_value.to_bits());
}

/// Return the per-evaluation brand carried by `value`, provided it belongs to
/// `declaring_class_id`'s compile-time template. A fresh class object is its
/// own static brand; instances name it through their shape's evaluation holder.
fn private_evaluation_brand(value: f64, declaring_class_id: u32) -> Option<u64> {
    if declaring_class_id == 0 {
        return None;
    }
    let value_is_class_object = super::super::class_registry::is_class_object_value(value);
    if value_is_class_object {
        let object = JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
        if !object.is_null() && js_object_get_class_id(object) == declaring_class_id {
            return Some(value.to_bits());
        }
    }
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let brand = unsafe {
        if object.is_null() || !crate::object::object_is_shaped(object) {
            return None;
        }
        f64::from_bits(private_receiver_evaluation_bits(object))
    };
    if !super::super::class_registry::is_class_object_value(brand) {
        return None;
    }
    if !value_is_class_object {
        // #11127/#11131: an instance carries its MOST-DERIVED evaluation; an
        // ancestor's brand is that evaluation's pinned heritage chain.
        return instance_ancestor_evaluation_brand(brand, declaring_class_id);
    }
    let object = JSValue::from_bits(brand.to_bits()).as_pointer::<ObjectHeader>();
    (!object.is_null() && js_object_get_class_id(object) == declaring_class_id)
        .then_some(brand.to_bits())
}

/// Per-object brands cover replacement receivers and lexical prototypes;
/// ordinary instances carry their evaluation on the shape.
#[inline]
unsafe fn private_receiver_evaluation_bits(object: *const ObjectHeader) -> u64 {
    let meta = (*object).meta;
    if !meta.is_null() && (*meta).private_evaluation_brand != 0 {
        return (*meta).private_evaluation_brand;
    }
    crate::object::shapes::object_evaluation_brand(object)
}

/// Return the fresh ClassDefinitionEvaluation object carried by a constructor
/// or instance/prototype. Unlike `private_evaluation_brand`, this does not require the
/// caller to know the compile-time template id; method dispatch uses it to
/// establish the callee's lexical private-name environment.
pub(crate) fn private_evaluation_brand_value(value: f64) -> Option<f64> {
    if super::super::class_registry::is_class_object_value(value) {
        return Some(value);
    }
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>();
    let brand = unsafe {
        if object.is_null() || !crate::object::object_is_shaped(object) {
            return None;
        }
        f64::from_bits(private_receiver_evaluation_bits(object))
    };
    super::super::class_registry::is_class_object_value(brand).then_some(brand)
}

include!("ic_miss/private_member_access.rs");
include!("ic_miss/private_guard_fast.rs");

/// The object that holds `value`'s private elements (#11791): `value`
/// itself when it is a shaped object, or its own property bag when it is a
/// function, an exotic cell (Map, Set, Date, Error, Promise, Temporal — the
/// instances `class X extends Map { #k = ... }` constructs) or a native
/// handle (the instance `class X extends http.Agent { #m() {} }` constructs).
/// A Proxy holds none of its target's private elements, and a primitive holds
/// none at all.
///
/// # Safety
/// Reads only headers; `value` may be any NaN-boxed value.
unsafe fn private_element_holder(value: f64) -> Option<*mut ObjectHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_handle() {
        let bag = crate::object::handle_expando::handle_property_bag(value.as_handle() as i64);
        return (!bag.is_null()).then_some(bag);
    }
    if !value.is_pointer() {
        return None;
    }
    let object = value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    let header = crate::value::addr_class::try_read_gc_header(object as usize)?;
    if header.obj_type == crate::gc::GC_TYPE_CLOSURE {
        let bag = crate::closure::props::bag_of(object as usize);
        return (!bag.is_null()).then_some(bag);
    }
    if crate::object::exotic_expando::exotic_kind_of_gc_type(header.obj_type).is_some() {
        let bag = crate::object::exotic_expando::property_bag(object as usize);
        return (!bag.is_null()).then_some(bag);
    }
    (crate::value::addr_class::is_plausible_heap_addr(object as usize)
        && crate::object::object_is_shaped(object))
    .then_some(object)
}

/// [`private_element_holder`] for a definition (a private field, or the
/// class brand of its private methods): a function, exotic cell or native
/// handle gets its property bag allocated on the first private element, as
/// on the first named property. May allocate; callers root `value` and read the holder
/// only after this returns.
///
/// # Safety
/// As [`private_element_holder`].
unsafe fn private_element_holder_for_definition(value: f64) -> Option<*mut ObjectHeader> {
    let boxed = JSValue::from_bits(value.to_bits());
    if boxed.is_handle() {
        let handle = boxed.as_handle() as i64;
        // Only a live native owner record has a bag word; any other handle
        // holds no private elements, and the definition throws below.
        if crate::object::class_handles::native_property_bag_access(handle, 0, 0).is_some() {
            crate::object::handle_expando::handle_property_bag_ensure(handle);
        }
    } else if boxed.is_pointer() {
        let addr = boxed.as_pointer::<ObjectHeader>() as usize;
        if crate::closure::is_closure_ptr(addr) {
            crate::closure::props::bag_ensure(addr);
        } else if crate::object::exotic_expando::exotic_expando_kind(addr).is_some() {
            crate::object::exotic_expando::property_bag_ensure(addr);
        }
    }
    private_element_holder(value)
}

// Instance evaluation brands never grant access to static private elements.
fn private_static_receiver_is_constructor(value: f64) -> bool {
    super::super::class_value::class_value_id(value).is_some()
        || super::super::class_registry::is_class_object_value(value)
}

/// The private brand of class `class_id` in evaluation `evaluation_id`
/// (#11791): the class id for the shared evaluation, the evaluation's scalar
/// id (`private_storage_evaluation_id`) with the top bit set for a fresh one,
/// so the two spaces never meet. Scalars: a brand retains no class object.
#[inline]
fn private_brand_id(class_id: u32, evaluation_id: u64) -> u64 {
    if evaluation_id == PRIVATE_TEMPLATE_EVALUATION_ID {
        u64::from(class_id)
    } else {
        PRIVATE_FRESH_EVALUATION_BRAND | evaluation_id
    }
}

/// Marks a brand that names a fresh evaluation rather than a class id.
pub(crate) const PRIVATE_FRESH_EVALUATION_BRAND: u64 = 1 << 63;

/// Is private element `name` (`kind` 0 = field, else a method or accessor,
/// which the class brand covers) of `declaring_class_id` present on
/// `storage`, in the evaluation the access resolves to?
fn private_instance_element_is_present(
    storage: f64,
    declaring_class_id: u32,
    field_name_ptr: *const u8,
    field_name_len: u32,
    kind: u32,
) -> bool {
    if field_name_ptr.is_null() || field_name_len == 0 {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(field_name_ptr, field_name_len as usize) };
    let Some(name) = intern_private_name(bytes) else {
        return false;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let storage = scope.root_nanbox_f64(storage);
    let evaluation_id =
        private_storage_evaluation_id(declaring_class_id, Some(storage.get_nanbox_f64()), None);
    if kind == 0 {
        return private_storage_key_by_id(declaring_class_id, evaluation_id, name)
            .is_present(storage.get_nanbox_f64());
    }
    let brand = private_brand_id(declaring_class_id, evaluation_id);
    unsafe { private_element_holder(storage.get_nanbox_f64()) }
        .is_some_and(|holder| unsafe { crate::object::shapes::object_has_brand(holder, brand) })
}

/// The name a class takes in its brand-check messages.
fn private_class_display_name(class_id: u32) -> String {
    super::super::class_registry::class_name_for_id(class_id)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "anonymous".to_string())
}

/// Install one class's instance-private brand on `obj` (#11791): move `obj`
/// to the shape that also carries the brand.
///
/// A class contributes one brand regardless of how many private methods or
/// accessors it declares. Re-installing that brand on the same object is the
/// observable error PrivateMethodOrAccessorAdd requires (for example when a
/// base constructor returns an object that was already initialized by the
/// derived class once).
#[no_mangle]
pub extern "C" fn js_private_brand_add(obj: f64, declaring_class_id: u32) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    // Resolving the evaluation can allocate class metadata; the holder is
    // derived from the rooted value afterwards.
    let evaluation_id =
        private_storage_evaluation_id(declaring_class_id, Some(obj.get_nanbox_f64()), None);
    let brand = private_brand_id(declaring_class_id, evaluation_id);
    let Some(holder) = (unsafe { private_element_holder_for_definition(obj.get_nanbox_f64()) })
    else {
        throw_private_type_error(&format!(
            "Cannot initialize private methods of class {} on this object",
            private_class_display_name(declaring_class_id)
        ));
    };
    if !unsafe { crate::object::shapes::transition_object_shape_add_brand(holder, brand) } {
        throw_private_type_error(&format!(
            "Cannot initialize private methods of class {} twice on the same object",
            private_class_display_name(declaring_class_id)
        ));
    }
    obj.get_nanbox_f64()
}

/// Define an instance private field (#11791): claim its qualified name as an
/// own `ENTRY_PRIVATE` entry and store the value. Never Proxy [[Set]] or
/// [[DefineOwnProperty]]: a private field is not a property.
#[no_mangle]
pub extern "C" fn js_private_field_add(
    obj: f64,
    declaring_class_id: u32,
    field_key: f64,
    value: f64,
) -> f64 {
    // A short-string key may be materialized here, so root every GC-managed
    // operand before asking for a StringHeader and use only refreshed handles
    // afterwards.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj);
    let field_key = scope.root_nanbox_f64(field_key);
    let value = scope.root_nanbox_f64(value);
    let field_key_ptr = crate::value::js_get_string_pointer_unified(field_key.get_nanbox_f64())
        as *const crate::StringHeader;
    if field_key_ptr.is_null() {
        throw_private_type_error("Invalid private field name");
    }
    let field_name_len = unsafe { (*field_key_ptr).byte_len };
    let field_name_ptr = crate::string::string_data(field_key_ptr);
    let field_name = intern_private_name(unsafe {
        std::slice::from_raw_parts(field_name_ptr, field_name_len as usize)
    })
    .unwrap_or_else(|| throw_private_type_error("Invalid private field name"));
    let storage =
        private_instance_value_name(declaring_class_id, field_name, obj.get_nanbox_f64(), None);
    if storage.is_present(obj.get_nanbox_f64()) {
        throw_private_type_error(&format!(
            "Cannot initialize {field_name} twice on the same object"
        ));
    }
    let Some(holder) = (unsafe { private_element_holder_for_definition(obj.get_nanbox_f64()) })
    else {
        throw_private_type_error(&format!("Cannot initialize {field_name} on this object"));
    };
    // A class family's layout lists the element (`ENTRY_OPTIONAL`): the
    // definition fills its slot. Allocation-free.
    if storage.fill_optional(obj.get_nanbox_f64(), value.get_nanbox_f64()) {
        return value.get_nanbox_f64();
    }
    // The claim allocates the key (inside its own no-move window); the store
    // re-reads the holder from its handle.
    let pred_rep = crate::object::shapes::shape_rep_of_receiver(unsafe {
        (crate::object::shapes::object_shape_stamp(holder)).0
    });
    let holder = scope.root_raw_mut_ptr(holder);
    holder.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
        crate::object::key_attrs::apply_edits(
            holder,
            &[crate::object::key_attrs::AttrsEdit::Private(
                storage.as_bytes(),
            )],
        )
    });
    if !storage.set_cached(obj.get_nanbox_f64(), value.get_nanbox_f64()) {
        // A dictionary holder: its private list carries the entry, and the
        // generic store overwrites the claimed key in place.
        let key = scope.root_string_ptr(crate::string::intern_ascii_literal(storage.as_bytes()));
        holder.with_mut_ptr::<ObjectHeader, _>(|holder| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(holder, key, value.get_nanbox_f64())
            })
        });
        return value.get_nanbox_f64();
    }
    // The claim is an append: like any key-add's, its successor carries the
    // predecessor's lanes, and the initializer's value types the new slot (a
    // Number in an inline slot is an `F64` lane, which compiled field sites
    // read and write raw).
    let slot = holder.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
        let keys = crate::object::object_keys(holder);
        if keys.is_null() {
            None
        } else {
            crate::object::keys_find_private_slot_by_bytes(
                keys.arr(),
                keys.count(),
                storage.as_bytes(),
            )
        }
    });
    if let Some(slot) = slot {
        let lanes = holder.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
            crate::object::field_rep_store::private_field_lanes_target(holder, slot, pred_rep)
        });
        if let Some(lanes) = lanes {
            holder.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
                crate::object::field_rep_store::install_private_field_lanes(holder, slot, lanes)
            });
        }
    }
    value.get_nanbox_f64()
}

/// Define one public class field with DefineField/CreateDataProperty
/// semantics. In particular, inherited setters are bypassed and Proxy
/// receivers observe `defineProperty`, not `set`.
#[no_mangle]
pub extern "C" fn js_class_field_add(receiver: f64, key: f64, value: f64) -> f64 {
    if unsafe { crate::object::layout_desc::family_define_field(receiver, key, value) } {
        return family_holder_built(receiver, value);
    }
    if unsafe { class_field_chain::try_define(receiver, key, value) } {
        return value;
    }
    class_field_add_slow(receiver, key, value)
}

/// After a family object's field definition: a family constructor's run
/// proves its stores from the prototype chain's holders
/// (`layout_probe::perry_family_run_admits`), and a declared class's holder
/// is built on its first ordinary lookup, which a `[[Set]]` makes and
/// DefineField never does. Build the receiver's class holder when its word
/// still names the unbuilt surface, once per class, so the next run's chain
/// is readable. Returns `value`, which the build may move.
fn family_holder_built(receiver: f64, value: f64) -> f64 {
    let obj = (receiver.to_bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    // SAFETY: `family_define_field` admitted `receiver` as a live family object.
    let class_id = unsafe { (*obj).class_id };
    if crate::object::class_registry::class_holder_word(class_id) != crate::value::TAG_UNDEFINED {
        return value;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    crate::object::class_registry::class_decl_prototype_value(class_id);
    value.get_nanbox_f64()
}

#[inline(never)]
fn class_field_add_slow(receiver: f64, key: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    let predecessor = unsafe { class_field_chain::predecessor(receiver.get_nanbox_f64()) };
    let stored = unsafe {
        crate::object::define_own_data::define_own_data_from_shape(
            receiver.get_nanbox_f64(),
            key.get_nanbox_f64(),
            value.get_nanbox_f64(),
        )
    }
    .is_some();
    if !stored
        && !class_field_define_fast::try_define_new_class_field(
            receiver.get_nanbox_f64(),
            key.get_nanbox_f64(),
            value.get_nanbox_f64(),
        )
        && !crate::proxy::create_data_property(
            receiver.get_nanbox_f64(),
            key.get_nanbox_f64(),
            value.get_nanbox_f64(),
        )
    {
        throw_private_type_error("Cannot define class field on receiver");
    }
    if predecessor != LayoutWord(0) {
        unsafe {
            class_field_chain::learn(predecessor, receiver.get_nanbox_f64(), key.get_nanbox_f64());
        }
    }
    value.get_nanbox_f64()
}

/// Brand + kind/op guard for a private member access `obj.#name`. Returns
/// the object the access reads or writes when it is legal (`obj` itself, or
/// for an instance FIELD of a native handle its property bag, which holds the
/// field: [`private_field_access_object`]); otherwise throws a `TypeError`.
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
    brand_owner: f64,
    declaring_class_id: u32,
    _field_name_ptr: *const u8,
    _field_name_len: u32,
    kind: u32,
    op: u32,
) -> f64 {
    if declaring_class_id == 0 {
        return obj;
    }
    if _field_name_ptr.is_null() || _field_name_len == 0 {
        throw_private_type_error("Invalid private field name");
    }
    let field_name =
        unsafe { std::slice::from_raw_parts(_field_name_ptr, _field_name_len as usize) };
    // The member hint belongs to the compiled frame's access sequence: the
    // mark of this entry point lies just below that frame, so the frame's own
    // landing pad drops a hint whose access a throw skipped.
    private_guard_checked(
        obj,
        brand_owner,
        declaring_class_id,
        field_name,
        kind,
        op,
        true,
        crate::exception::frame_mark(),
    )
}

/// The body of [`js_private_guard`]. `record_hints == false` performs the same
/// checks and throws but leaves the member-hint stack alone, for a caller that
/// consumes the access itself (`js_private_method_guard`).
fn private_guard_checked(
    obj: f64,
    brand_owner: f64,
    declaring_class_id: u32,
    field_name_bytes: &[u8],
    kind: u32,
    op: u32,
    record_hints: bool,
    mark: usize,
) -> f64 {
    let is_static = op >= 2;
    let is_write = op & 1 != 0;
    // #10501: settle the common instance access without spelling a marker.
    if !is_static {
        if let Some(proof) = private_instance_access_is_proven(
            obj,
            brand_owner,
            declaring_class_id,
            field_name_bytes,
            kind,
        ) {
            private_guard_record_access(
                declaring_class_id,
                proof.name,
                kind,
                false,
                is_write,
                None,
                record_hints,
                mark,
            );
            return private_field_access_object(obj, kind, is_static);
        }
    }
    // Interning also makes the name independent of the caller's buffer across
    // any allocation below (a first private-name lookup can allocate class
    // metadata). Only a non-UTF-8 spelling, which codegen never emits, keeps
    // the historical owned copy and the empty hint name.
    let interned = intern_private_name(field_name_bytes);
    let owned_name;
    let (_field_name_ptr, _field_name_len) = match interned {
        Some(name) => (name.as_ptr(), name.len() as u32),
        None => {
            owned_name = field_name_bytes.to_vec();
            (owned_name.as_ptr(), owned_name.len() as u32)
        }
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_root = scope.root_nanbox_f64(obj);
    let brand_owner_root = scope.root_nanbox_f64(brand_owner);
    let field_name = interned.unwrap_or("");
    if is_static
        && (crate::proxy_hooks::proxy_is_proxy(obj) != 0
            || !private_static_receiver_is_constructor(obj))
    {
        throw_private_brand_mismatch(
            obj,
            declaring_class_id,
            field_name,
            kind,
            is_static,
            is_write,
        );
    }
    let _owner = PrivateHintBrandScope::new(private_access_owner(brand_owner, declaring_class_id));
    let has_brand = if is_static {
        // Static elements require the exact constructor evaluation.
        private_evaluation_brand_matches(obj, brand_owner, declaring_class_id)
            .unwrap_or_else(|| super::super::class_ref_id(obj) == Some(declaring_class_id))
    } else {
        // An existing receiver can acquire elements from several evaluations
        // via constructor return overrides. Its installed shape entries decide
        // presence in the lexical namespace; its birth evaluation cannot deny it.
        private_instance_element_is_present(
            obj,
            declaring_class_id,
            _field_name_ptr,
            _field_name_len,
            kind,
        )
    };
    if !has_brand {
        throw_private_brand_mismatch(
            obj,
            declaring_class_id,
            field_name,
            kind,
            is_static,
            is_write,
        );
    }
    if is_static
        && kind == 0
        && !static_private_field_key(declaring_class_id, field_name)
            .is_present(obj_root.get_nanbox_f64())
    {
        throw_private_brand_mismatch(
            obj_root.get_nanbox_f64(),
            declaring_class_id,
            field_name,
            kind,
            true,
            is_write,
        );
    }
    let access_owner = private_access_owner(brand_owner_root.get_nanbox_f64(), declaring_class_id);
    private_guard_record_access(
        declaring_class_id,
        interned.unwrap_or(""),
        kind,
        is_static,
        is_write,
        access_owner,
        record_hints,
        mark,
    );
    private_field_access_object(obj_root.get_nanbox_f64(), kind, is_static)
}

/// The object a legal private access reads or writes by its storage key. An
/// instance field of a native handle (`class X extends http.Agent { #n }`)
/// lives in the handle's property bag ([`private_element_holder`]), where its
/// definition put it; a by-name access on the handle value itself never
/// reaches that bag. Every other access, and every method or accessor (whose
/// `this` is the receiver), keeps `obj`.
fn private_field_access_object(obj: f64, kind: u32, is_static: bool) -> f64 {
    if kind != 0 || is_static || !JSValue::from_bits(obj.to_bits()).is_handle() {
        return obj;
    }
    match unsafe { private_element_holder(obj) } {
        Some(bag) => crate::value::js_nanbox_pointer(bag as i64),
        None => obj,
    }
}

/// Throw node's TypeError for a private access whose receiver lacks the
/// element (#11791): a field names the operation, a method or accessor names
/// the class the receiver must be an instance of. Diverges.
fn throw_private_brand_mismatch(
    receiver: f64,
    class_id: u32,
    name: &str,
    kind: u32,
    is_static: bool,
    is_write: bool,
) -> ! {
    let nullish = match receiver.to_bits() {
        crate::value::TAG_NULL => Some("null"),
        crate::value::TAG_UNDEFINED => Some("undefined"),
        _ => None,
    };
    let message = match (nullish, kind, is_write) {
        // A field (or a static field) names itself; an instance method or
        // accessor names its class, as V8 does.
        (Some(what), 0, false) => format!("Cannot read properties of {what} (reading '{name}')"),
        (Some(what), 0, true) => format!("Cannot set properties of {what} (setting '{name}')"),
        (Some(what), _, _) if !is_static => format!(
            "Cannot read properties of {what} (reading '{}')",
            private_class_display_name(class_id)
        ),
        _ => throw_private_receiver_mismatch(class_id, name, kind, is_static, is_write),
    };
    throw_private_type_error(&message)
}

fn throw_private_receiver_mismatch(
    class_id: u32,
    name: &str,
    kind: u32,
    is_static: bool,
    is_write: bool,
) -> ! {
    let message = match (kind, is_write) {
        (0, false) => {
            format!(
                "Cannot read private member {name} from an object whose class did not declare it"
            )
        }
        (0, true) => {
            format!(
                "Cannot write private member {name} to an object whose class did not declare it"
            )
        }
        _ if is_static => format!(
            "Receiver must be class {}",
            private_class_display_name(class_id)
        ),
        _ => format!(
            "Receiver must be an instance of class {}",
            private_class_display_name(class_id)
        ),
    };
    throw_private_type_error(&message)
}

#[cfg(test)]
mod private_evaluation_brand_tests {
    use super::*;

    #[test]
    fn stamping_a_fresh_brand_preserves_instance_shape_and_slots() {
        unsafe {
            const CID: u32 = 62_441;
            let class = crate::object::js_object_alloc(CID, 0);
            crate::object::class_registry::js_object_mark_class(class as i64);
            let class_value = crate::value::js_nanbox_pointer(class as i64);
            assert!(crate::object::class_registry::is_class_object_value(
                class_value
            ));

            let instance = crate::object::js_object_alloc(CID, 2);
            let shape_before = crate::object::shapes::object_layout_stamp(instance);
            let keys_before_view = crate::object::object_keys(instance);
            let keys_before = keys_before_view.arr();
            let slots_before = crate::object::object_live_slot_count(instance);

            stamp_private_evaluation_brand(instance, class_value);

            assert_eq!(
                crate::object::shapes::object_layout_stamp(instance),
                shape_before
            );
            assert_eq!(crate::object::object_keys(instance).arr(), keys_before);
            assert_eq!(
                crate::object::object_live_slot_count(instance),
                slots_before
            );
            assert_eq!(
                private_evaluation_brand(crate::value::js_nanbox_pointer(instance as i64), CID),
                Some(class_value.to_bits())
            );
        }
    }
}
