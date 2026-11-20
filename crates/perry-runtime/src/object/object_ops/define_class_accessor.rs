//! `Object.defineProperty` onto a DECLARED static class accessor (#10480)
//! reached through the class constructor `C`.
//!
//! A static accessor is an accessor property of the class function object's
//! own-property object (`object::class_value`), attributes with its key. (An
//! instance accessor is a real accessor property of the class's decl
//! prototype object, so a define on `C.prototype` is the ordinary define on
//! that object and never reaches here.)
use super::*;

/// ValidateAndApplyPropertyDescriptor for the declared static accessor `name`
/// of `class_id`.
///
/// `None` means this is not a declared accessor definition and the caller's
/// path decides; `Some(verdict)` is the definition's verdict.
///
/// * Static, not a live declared accessor → `None`.
/// * Static, current accessor non-configurable → the spec's rejections are
///   `Some(false)`, exactly as for any other property.
/// * Static, generic descriptor (none of `get`/`set`/`value`/`writable`) →
///   only the attributes change: an omitted field keeps its current value and
///   the getter/setter stay in place. Returns `Some(true)`.
/// * Static, anything else → `None`: replacing a static accessor half or
///   converting it to a data property is not modelled here, so the caller's
///   existing path is unchanged.
pub(super) unsafe fn define_declared_class_accessor(
    class_id: u32,
    name: &str,
    descriptor: &DescView<'_>,
) -> Option<bool> {
    let Some((acc, enumerable, configurable)) =
        crate::object::class_value::class_static_own_accessor(class_id, name)
    else {
        return None;
    };
    // The per-field reads below allocate a field-name string (and may run a
    // user getter on a non-plain descriptor), so the descriptor is re-read from
    // its root at every use.
    // The validator compares accessor halves by closure identity: the
    // property's own closures.
    if !configurable
        && !nonconfigurable_redefine_allowed(
            PropertyAttrs::new(false, enumerable, false),
            Some(AccessorDescriptor {
                get: acc.get,
                set: acc.set,
            }),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            descriptor,
        )
    {
        return Some(false);
    }
    if descriptor.has(DESC_GET)
        || descriptor.has(DESC_SET)
        || descriptor.has(DESC_VALUE)
        || descriptor.has(DESC_WRITABLE)
    {
        return None;
    }
    let flag = |_index: usize, name: &[u8]| descriptor.flag(name);
    let enumerable = flag(DESC_ENUMERABLE, b"enumerable").unwrap_or(enumerable);
    let configurable = flag(DESC_CONFIGURABLE, b"configurable").unwrap_or(configurable);
    crate::object::class_value::class_static_set_accessor_attrs(
        class_id,
        name,
        enumerable,
        configurable,
    );
    Some(true)
}
