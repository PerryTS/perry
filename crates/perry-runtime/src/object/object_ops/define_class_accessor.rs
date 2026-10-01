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
    descriptor_value: f64,
    desc_view: Option<&super::descriptor_helpers::DescView<'_>>,
) -> bool {
    if !is_static {
        let scope = crate::gc::RuntimeHandleScope::new();
        let desc = scope.root_nanbox_f64(descriptor_value);
        let Some(proto) = super::super::class_registry::decl_prototype_own_accessor(class_id, name)
        else {
            return false;
        };
        let proto = scope.root_nanbox_f64(proto);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
        super::js_object_define_property(proto.get_nanbox_f64(), key, desc.get_nanbox_f64());
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc = scope.root_nanbox_f64(descriptor_value);
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
            desc.get_nanbox_f64(),
            desc_view,
        );
    }
    // `ToPropertyDescriptor` field presence is HasProperty (own or inherited).
    let has = |index: usize, field: &[u8]| -> bool {
        match desc_view {
            Some(view) => view.has(index),
            None => desc_has_field(desc.get_nanbox_f64(), field),
        }
    };
    if has(DESC_GET, b"get")
        || has(DESC_SET, b"set")
        || has(DESC_VALUE, b"value")
        || has(DESC_WRITABLE, b"writable")
    {
        return false;
    }
    // A present field is `ToBoolean(value)` — `{ enumerable: undefined }` is
    // an explicit `false`, not an omission.
    let flag = |index: usize, field: &[u8]| -> Option<bool> {
        has(index, field).then(|| {
            let value = match desc_view {
                Some(view) => view.read(index),
                None => desc_read_field(desc.get_nanbox_f64(), field),
            };
            crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0
        })
    };
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
