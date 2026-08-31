//! #9131: a per-instance `[[Prototype]]` override wins over the class vtable.
//!
//! Split out of `get_field_by_name_tail.rs`, which is at the 2000-line cap.
//! Both of that file's own-key misses — the keyless arm and the shaped-receiver
//! arm — ask the same question, so it lives here once instead of twice.

use crate::object::ObjectHeader;
use crate::value::JSValue;

/// An explicit per-instance `[[Prototype]]` REPLACES the class's declaration
/// prototype; it is not an extra link in front of the original vtable. So when
/// the own-key scan misses, walk that authoritative chain before exposing class
/// getters or methods, and do not resurrect the old class surface when the
/// custom chain also misses — hence `Some(undefined)` rather than `None` once
/// an override is present.
///
/// `None` means no override was installed and the caller keeps its existing
/// class-vtable fallback.
pub(super) fn inherited_field_if_overridden(
    obj: *const ObjectHeader,
    key: *const crate::string::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    if !crate::object::prototype_chain::object_has_prototype_override(obj as usize) {
        return None;
    }
    Some(
        crate::object::prototype_chain::resolve_inherited_field(obj as usize, key)
            .unwrap_or_else(JSValue::undefined),
    )
}
