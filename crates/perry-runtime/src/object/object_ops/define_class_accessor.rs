//! `Object.defineProperty` onto a DECLARED class accessor (#10480) reached
//! through a class REF value: `C` (static accessors) or a `C.prototype` ref.
//!
//! An instance accessor is a real accessor property of the class's decl
//! prototype, so a define through the prototype ref is the ordinary define on
//! that object. A static accessor is an accessor property of the class
//! function object's own-property object (`object::class_value`), attributes
//! with its key.
use super::*;

/// ValidateAndApplyPropertyDescriptor for the declared accessor `name` of
/// `class_id` (`is_static` selects `C` over `C.prototype`).
///
/// * Instance: when the prototype ref reflects `name` through the decl
///   prototype (`decl_prototype_own_accessor`), the define applies to that
///   object — attributes, replacement and conversion alike. Returns `true`.
/// * Static, not a live declared accessor → `false`, the caller's path decides.
/// * Static, current accessor non-configurable → the spec's rejections throw
///   `Cannot redefine property: <name>` exactly as for any other property.
/// * Static, generic descriptor (none of `get`/`set`/`value`/`writable`) →
///   only the attributes change: an omitted field keeps its current value and
///   the getter/setter stay in place. Returns `true`.
/// * Static, anything else → `false`: replacing a static accessor half or
///   converting it to a data property is not modelled here, so the caller's
///   existing path is unchanged.
pub(super) unsafe fn define_declared_class_accessor(
    class_id: u32,
    is_static: bool,
    name: &str,
    descriptor: &DescView<'_>,
) -> bool {
    if !is_static {
        let scope = crate::gc::RuntimeHandleScope::new();
        let Some(proto) = super::super::class_registry::decl_prototype_own_accessor(class_id, name)
        else {
            return false;
        };
        let proto = scope.root_heap_word_u64(proto.to_bits());
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
        let key = scope.root_nanbox_f64(key);
        super::define_own_property_decoded(&scope, &proto, &key, descriptor);
        return true;
    }
    let Some((acc, enumerable, configurable)) =
        crate::object::class_value::class_static_own_accessor(class_id, name)
    else {
        return false;
    };
    // The per-field reads below allocate a field-name string (and may run a
    // user getter on a non-plain descriptor), so the descriptor is re-read from
    // its root at every use.
    if !configurable {
        // The validator compares accessor halves by closure identity: the
        // property's own closures.
        validate_nonconfigurable_redefine(
            name,
            PropertyAttrs::new(false, enumerable, false),
            Some(AccessorDescriptor {
                get: acc.get,
                set: acc.set,
            }),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            descriptor,
        );
    }
    if descriptor.has(DESC_GET)
        || descriptor.has(DESC_SET)
        || descriptor.has(DESC_VALUE)
        || descriptor.has(DESC_WRITABLE)
    {
        return false;
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
    true
}
