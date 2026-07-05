//! `class X extends Intl.<Ctor>` construction + `instanceof Intl.<Ctor>`
//! support. Split out of `intl.rs` to keep that file under the workspace's
//! 2,000-line ceiling. The Intl-constructor recognition helper
//! (`super::is_intl_constructor_value`) stays in `intl.rs` next to the
//! constructor thunks it matches against.

use super::{
    canonicalize_language_tag, get_string_field, object_ptr_from_value, string_from_string_value,
    throw_invalid_language_tag, throw_type_error, value_to_string, KEY_KIND,
};
use crate::closure::ClosureHeader;
use crate::value::JSValue;

/// CanonicalizeLocaleList element handler: a present element must be a String or
/// an Object (an `Intl.Locale` or anything ToString-able), else `TypeError`; the
/// resulting tag is canonicalized (`RangeError` if structurally invalid) and
/// pushed if not already present.
pub(super) fn push_locale_element(out: &mut Vec<String>, value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    let tag = if jv.is_any_string() {
        string_from_string_value(value).unwrap_or_default()
    } else if let Some(locale_tag) = locale_instance_tag(value) {
        locale_tag
    } else if object_ptr_from_value(value).is_some() {
        value_to_string(value)
    } else {
        // undefined / null / boolean / number / Symbol element → TypeError.
        throw_type_error("locale must be a String or Object");
    };
    let Some(canonical) = canonicalize_language_tag(&tag) else {
        throw_invalid_language_tag(&tag);
    };
    if !out.iter().any(|existing| existing == &canonical) {
        out.push(canonical);
    }
}

/// If `value` is an `Intl.Locale` instance (its `[[InitializedLocale]]` slot,
/// modeled by the `__intlKind == "Locale"` internal field) return its
/// `[[Locale]]` tag string — the canonical `__localeFull` field. Per
/// CanonicalizeLocaleList, a Locale element contributes `.toString()`'s value
/// *without invoking the (user-overridable) `toString` method*: the abstract op
/// reads the internal slot directly. Also matches `class X extends Intl.Locale`
/// subclass instances, which carry the copied brand fields (see
/// `intl_subclass_super`).
pub(super) fn locale_instance_tag(value: f64) -> Option<String> {
    let obj = object_ptr_from_value(value)?;
    if get_string_field(obj, KEY_KIND).as_deref() != Some("Locale") {
        return None;
    }
    // `__localeFull` — the constructor-canonicalized full tag.
    get_string_field(obj, "__localeFull")
}

/// The compiled function pointers of every `Intl.*` service constructor thunk.
/// Used by [`is_intl_constructor_value`] to recognize a `class X extends
/// Intl.<Ctor>` parent value from its closure so `super(...)` can construct it
/// correctly (with `new.target` set) rather than tripping the
/// `require_new_target` guard.
fn intl_constructor_func_ptrs() -> [*const u8; 10] {
    [
        super::number_format_constructor_thunk as *const u8,
        super::date_time_format_constructor_thunk as *const u8,
        super::collator_constructor_thunk as *const u8,
        super::segmenter_constructor_thunk as *const u8,
        super::list_format_constructor_thunk as *const u8,
        super::relative_time_format_constructor_thunk as *const u8,
        super::plural_rules_constructor_thunk as *const u8,
        super::duration_format::constructor_thunk as *const u8,
        super::display_names::constructor_thunk as *const u8,
        super::locale::locale_constructor_thunk as *const u8,
    ]
}

/// `true` when `parent_val` is (the closure for) an `Intl.*` service
/// constructor. `class X extends Intl.ListFormat` routes its `super()` through
/// the generic runtime-value dispatcher, which would invoke the constructor
/// without a `new.target` and throw "Constructor Intl.X requires 'new'"; this
/// lets the super-call path recognize the parent and construct it properly.
pub(crate) fn is_intl_constructor_value(parent_val: f64) -> bool {
    let jsval = JSValue::from_bits(parent_val.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let closure = jsval.as_pointer() as *const ClosureHeader;
    if closure.is_null() {
        return false;
    }
    let fp = unsafe { (*closure).func_ptr };
    intl_constructor_func_ptrs().iter().any(|p| *p == fp)
}

/// `class X extends Intl.<Ctor>` super-call handling. An `Intl.*` service
/// constructor allocates and returns a fresh branded object (internal
/// `__intl*` fields plus own `format`/`resolvedOptions`/… methods) and does not
/// mutate the implicit `this`; it also throws "requires 'new'" when
/// `new.target` is undefined. So when `parent_val` is an Intl constructor: set
/// `new.target` to the parent for the duration of the construct (so the guard
/// passes), run it, then copy every own field of the returned instance onto the
/// subclass `this` — giving `this` the Intl brand and its bound methods.
/// Returns `true` when handled (mirrors `temporal_subclass_super`).
///
/// # Safety
/// `args_ptr` must point at `args_len` readable f64 slots (or be null when
/// `args_len` is 0).
pub(crate) unsafe fn intl_subclass_super(
    parent_val: f64,
    this_box: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> bool {
    if !is_intl_constructor_value(parent_val) {
        return false;
    }
    let prev_this = crate::object::js_implicit_this_set(this_box);
    let prev_nt = crate::object::js_new_target_set(parent_val);
    let instance = crate::closure::js_native_call_value(parent_val, args_ptr, args_len);
    crate::object::js_new_target_set(prev_nt);
    crate::object::js_implicit_this_set(prev_this);
    // Re-home the freshly-built instance's brand + bound methods onto `this`.
    let this_bits = this_box.to_bits();
    if (this_bits >> 48) == 0x7FFD {
        let dst = (this_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if dst >= 0x10000 {
            crate::object::js_object_copy_own_fields(dst, instance);
        }
    }
    true
}

/// `value instanceof Intl.<Ctor>` (OrdinaryHasInstance) when the right operand
/// is an Intl service constructor. Intl instances are plain heap objects whose
/// `[[Prototype]]` is set to `Intl.<Ctor>.prototype` (via
/// `object_set_static_prototype`), but the generic dynamic-`instanceof` path has
/// no class-id for them and no generic prototype walk, so it returned `false`
/// even though `Object.getPrototypeOf(inst) === Intl.<Ctor>.prototype`. Walk the
/// value's static-prototype chain and compare each link against the
/// constructor's `.prototype`. Returns `None` when `type_ref` is not an Intl
/// constructor (caller keeps its existing resolution); `Some(bool)` otherwise.
pub(crate) fn intl_instanceof(value: f64, type_ref: f64) -> Option<bool> {
    if !is_intl_constructor_value(type_ref) {
        return None;
    }
    let jsval = JSValue::from_bits(type_ref.to_bits());
    let closure = jsval.as_pointer::<u8>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(closure, "prototype");
    let proto_js = JSValue::from_bits(proto.to_bits());
    if !proto_js.is_pointer() {
        return Some(false);
    }
    let target_bits = proto.to_bits();
    // Walk `value`'s [[Prototype]] chain (bounded against cycles).
    let mut cur = value.to_bits();
    for _ in 0..64 {
        let top16 = cur >> 48;
        let raw = if top16 == 0x7FFD {
            (cur & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top16 == 0 {
            cur as usize
        } else {
            return Some(false);
        };
        if raw < 0x10000 {
            return Some(false);
        }
        match crate::object::prototype_chain::object_static_prototype(raw) {
            Some(p) => {
                if p == target_bits {
                    return Some(true);
                }
                cur = p;
            }
            None => return Some(false),
        }
    }
    Some(false)
}
