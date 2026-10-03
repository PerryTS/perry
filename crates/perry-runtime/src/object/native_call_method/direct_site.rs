//! Learned receiver words for compiled class-method sites.
//!
//! A compiled `recv.m(args)` whose receiver class is known statically compares
//! the receiver's `(class_id | ShapeId << 32)` word against the class's birth
//! shape and calls the method body directly on a match. A receiver that left
//! the birth shape — a field added in only one branch of the constructor, a
//! field added after construction, a private brand, the method surface a native
//! base such as `EventEmitter` installs — missed that compare on every call and
//! paid the whole dispatch tower, which then re-proved the same facts by name.
//!
//! Each such site owns one `i64`, all-ones while empty (no header word is
//! all-ones: a ShapeId is below `0xC000_0000`). A stored word always has a
//! non-zero class id and a non-zero ShapeId. The site's miss edge calls one of the two
//! entries below, which store the receiver's word when the receiver's SHAPE
//! proves what the site needs, and then perform the unchanged dispatch. A later
//! receiver with the same word takes the direct call after one more compare.
//!
//! # What a learned word claims, and why it cannot go stale
//!
//! The direct call at a class site needs: the receiver's class is the declared
//! class, nothing on the receiver shadows the method, and the class's
//! prototype chain still resolves the name to the compiled body. The emitted
//! code re-checks the last part on every call (the prototype guard bytes) and
//! the GC header (object kind, not forwarded, no descriptor entries). The
//! first two are facts of the word:
//!
//! * the class id is part of the word;
//! * a ShapeId names one key list, one descriptor state and one `[[Prototype]]`
//!   (#11342). The prime proves the key list does not contain the method name,
//!   the object is not a dictionary, and its metadata record (if any) holds
//!   only overflow storage: no per-instance prototype, no flags, no
//!   descriptor summary bits, no fresh-evaluation brand. Any later own key,
//!   descriptor or prototype change re-stamps the receiver, so the word stops
//!   matching.
//!
//! The word holds no address, so the collector never needs to see it, and a
//! racing store from another agent can only publish another correct fact:
//! ShapeIds are process-global and never reused.
//!
//! The prime never allocates, so the receiver read at entry is the receiver
//! the dispatch then sees.

use super::class_vtable_receiver_guard;

/// Store `object`'s receiver word in `site` when its shape proves that an
/// own-property lookup of `method` finds nothing, and (when
/// `expected_class_id` is non-zero) that its class is exactly that class.
///
/// Every refusal leaves the site as it was; the caller dispatches as before.
#[inline]
unsafe fn learn_absent_method_word(
    object: f64,
    method: &[u8],
    expected_class_id: u32,
    site: *mut u64,
) {
    if site.is_null() || method.is_empty() {
        return;
    }
    // Names whose dispatch depends on per-object state the key list does not
    // pin (symbol-keyed disposal hooks, iterator helpers).
    let Ok(name) = std::str::from_utf8(method) else {
        return;
    };
    if super::method_name_is_fast_dispatch_ineligible(name) {
        return;
    }
    // A word is only consulted after the prototype guard bytes pass, so a
    // site whose name is retired would learn for nothing on every miss.
    if crate::object::class_registry::class_prototype_fast_guard_invalidated_for_method(
        crate::object::class_registry::class_prototype_method_guard_slot(name),
    ) {
        return;
    }
    // The receiver predicate the tower's class fast path uses — an ordinary
    // heap instance of a user class, not a dictionary, no own key equal to
    // `method`, no recorded prototype — with a metadata record accepted only
    // for overflow storage.
    let Some((addr, class_id)) = class_vtable_receiver_guard::<true>(object, method) else {
        return;
    };
    if expected_class_id != 0 && class_id != expected_class_id {
        return;
    }
    let obj = addr as *const crate::object::ObjectHeader;
    let shape_id = crate::object::shapes::object_shape_stamp(obj);
    if shape_id == 0 {
        return;
    }
    let word = (u64::from(shape_id) << 32) | u64::from(class_id);
    (*(site as *const std::sync::atomic::AtomicU64))
        .store(word, std::sync::atomic::Ordering::Relaxed);
}

/// The miss edge of a compiled class-method site: learn the receiver's word
/// for `expected_class_id`, then dispatch exactly as
/// [`super::js_native_call_method_by_id`] does.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_method_by_id_learn(
    object: f64,
    method_id: i64,
    args_ptr: *const f64,
    args_len: usize,
    site: *mut u64,
    expected_class_id: u32,
) -> f64 {
    if method_id == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // A null `site` is the compiled miss edge saying no word could be
    // consulted (its prototype guard bytes are set): dispatch only.
    if !site.is_null() && expected_class_id != 0 {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(name_ref) =
            crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
        {
            let method = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
            learn_absent_method_word(object, method, expected_class_id, site);
        }
    }
    super::js_native_call_method_by_id(object, method_id, args_ptr, args_len)
}

/// The miss edge of a compiled own-method-override probe (#620): answer
/// exactly as [`crate::object::js_object_get_own_field_or_undef`], and when
/// the answer is "no own method", learn the receiver's word so the site can
/// skip the probe for every receiver of that shape.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_object_get_own_field_or_undef_learn(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    site: *mut u64,
) -> f64 {
    if !name_ptr.is_null() && name_len != 0 {
        let method = std::slice::from_raw_parts(name_ptr, name_len);
        learn_absent_method_word(obj_value, method, 0, site);
    }
    crate::object::js_object_get_own_field_or_undef(obj_value, name_ptr, name_len)
}
