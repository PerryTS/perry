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

/// The compiled function pointers of every `Intl.*` service constructor thunk,
/// paired with the `__intlKind` brand string each stamps on its instances.
/// Used by [`is_intl_constructor_value`] to recognize a `class X extends
/// Intl.<Ctor>` parent value from its closure so `super(...)` can construct it
/// correctly (with `new.target` set) rather than tripping the
/// `require_new_target` guard, and by [`intl_instanceof`] to brand-match
/// subclass instances (#6960).
fn intl_constructor_entries() -> [(*const u8, &'static str); 10] {
    [
        (
            super::number_format_constructor_thunk as *const u8,
            "NumberFormat",
        ),
        (
            super::date_time_format_constructor_thunk as *const u8,
            "DateTimeFormat",
        ),
        (super::collator_constructor_thunk as *const u8, "Collator"),
        (super::segmenter_constructor_thunk as *const u8, "Segmenter"),
        (
            super::list_format_constructor_thunk as *const u8,
            "ListFormat",
        ),
        (
            super::relative_time_format_constructor_thunk as *const u8,
            "RelativeTimeFormat",
        ),
        (
            super::plural_rules_constructor_thunk as *const u8,
            "PluralRules",
        ),
        (
            super::duration_format::constructor_thunk as *const u8,
            "DurationFormat",
        ),
        (
            super::display_names::constructor_thunk as *const u8,
            "DisplayNames",
        ),
        (
            super::locale::locale_constructor_thunk as *const u8,
            "Locale",
        ),
    ]
}

/// If `parent_val` is an `Intl.*` service constructor closure, return the
/// `__intlKind` brand string it stamps on instances (`"NumberFormat"`, …).
fn intl_constructor_kind(parent_val: f64) -> Option<&'static str> {
    let jsval = JSValue::from_bits(parent_val.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let closure = jsval.as_pointer() as *const ClosureHeader;
    if closure.is_null() {
        return None;
    }
    let fp = unsafe { (*closure).func_ptr };
    intl_constructor_entries()
        .iter()
        .find(|(p, _)| *p == fp)
        .map(|(_, kind)| *kind)
}

/// `true` when `parent_val` is (the closure for) an `Intl.*` service
/// constructor. `class X extends Intl.ListFormat` routes its `super()` through
/// the generic runtime-value dispatcher, which would invoke the constructor
/// without a `new.target` and throw "Constructor Intl.X requires 'new'"; this
/// lets the super-call path recognize the parent and construct it properly.
pub(crate) fn is_intl_constructor_value(parent_val: f64) -> bool {
    intl_constructor_kind(parent_val).is_some()
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
/// no class-id for them, so without this hook a direct instance returned
/// `false` even though `Object.getPrototypeOf(inst) === Intl.<Ctor>.prototype`.
///
/// Two recognition arms (#6960):
///
/// 1. **Brand** — `intl_subclass_super` copies the constructor's `__intlKind`
///    field onto the subclass `this`, so a `class X extends Intl.NumberFormat`
///    instance carries the same brand as a direct instance even when the
///    prototype chain is not yet wired through `Intl.NumberFormat.prototype`
///    (Perry's class-registry parent edge only tracks class-id parents, and
///    Intl constructors are closures). Mirrors the Temporal brand-cell arm.
/// 2. **Prototype walk** — OrdinaryHasInstance via
///    `js_object_get_prototype_of`, covering direct instances and any subclass
///    whose `X.prototype` *is* linked to `Intl.<Ctor>.prototype`.
///
/// Returns `None` when `type_ref` is not an Intl constructor (caller keeps its
/// existing resolution); `Some(bool)` otherwise.
pub(crate) fn intl_instanceof(value: f64, type_ref: f64) -> Option<bool> {
    let Some(expected_kind) = intl_constructor_kind(type_ref) else {
        return None;
    };
    // OrdinaryHasInstance step 3: a non-Object left operand is never an
    // instance. Guard before any walk — primitives would either throw
    // (null/undefined) or climb a wrapper chain (string/symbol) and spuriously
    // match.
    {
        let jv = JSValue::from_bits(value.to_bits());
        if jv.is_null()
            || jv.is_undefined()
            || jv.is_bool()
            || jv.is_int32()
            || jv.is_any_string()
            || jv.is_bigint()
            || unsafe { crate::symbol::js_is_symbol(value) != 0 }
        {
            return Some(false);
        }
    }
    // Brand arm: subclass instances re-homed by `intl_subclass_super`.
    if let Some(obj) = object_ptr_from_value(value) {
        if get_string_field(obj, KEY_KIND).as_deref() == Some(expected_kind) {
            return Some(true);
        }
    }
    let jsval = JSValue::from_bits(type_ref.to_bits());
    let closure = jsval.as_pointer::<u8>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(closure, "prototype");
    let target = proto_identity_addr(proto);
    if target == 0 {
        return Some(false);
    }
    // Prototype-walk arm: direct instances (and any fully-linked subclass).
    let mut cur = crate::object::js_object_get_prototype_of(value);
    for _ in 0..64 {
        if JSValue::from_bits(cur.to_bits()).is_null() {
            return Some(false);
        }
        let cur_addr = proto_identity_addr(cur);
        if cur_addr == 0 {
            return Some(false);
        }
        if cur_addr == target {
            return Some(true);
        }
        cur = crate::object::js_object_get_prototype_of(cur);
    }
    Some(false)
}

/// Normalize a value to its heap-pointer address for prototype identity
/// comparison — a NaN-boxed `POINTER_TAG` value or a raw heap pointer both
/// resolve to their address; any non-pointer / non-heap yields 0. Mirrors
/// `object/instanceof.rs::proto_identity_addr` but routes the floor check
/// through the canonical `is_plausible_heap_addr` predicate so handle-band
/// rejection stays single-sourced.
fn proto_identity_addr(v: f64) -> usize {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return 0;
    };
    if crate::value::addr_class::is_plausible_heap_addr(addr) {
        addr
    } else {
        0
    }
}
