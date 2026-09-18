//! `Object.defineProperty` onto a DECLARED class accessor (#10480).
//!
//! A ClassBody accessor is an own property of `C.prototype` (or of `C` for
//! `static`) whose get/set live in the class vtable, so the ordinary define
//! path — which only consults the address-keyed descriptor tables — took it for
//! a brand-new key. See `class_registry/accessor_attrs.rs` for what that broke.
use super::*;

/// ValidateAndApplyPropertyDescriptor for the declared accessor `name` of
/// `class_id` (`is_static` selects `C` over `C.prototype`).
///
/// * Not a live declared accessor → `false`, the caller's path decides.
/// * Current accessor non-configurable → the spec's rejections throw
///   `Cannot redefine property: <name>` exactly as for any other property.
/// * Generic descriptor (none of `get`/`set`/`value`/`writable`) → only the
///   attributes change: an omitted field keeps its current value and the
///   getter/setter stay in place. Returns `true`.
/// * Anything else → `false`: replacing a vtable accessor half or converting it
///   to a data property is not modelled here (compiled receivers call the
///   declared get/set directly), so the caller's existing path is unchanged.
pub(super) unsafe fn define_declared_class_accessor(
    class_id: u32,
    is_static: bool,
    name: &str,
    descriptor_value: f64,
    desc_view: Option<&super::descriptor_helpers::DescView<'_>>,
) -> bool {
    let Some((getter, setter)) =
        super::super::class_registry::class_declared_accessor_ptrs(class_id, is_static, name)
    else {
        return false;
    };
    let (enumerable, configurable) =
        super::super::class_registry::class_accessor_attrs(class_id, is_static, name);
    // The per-field reads below allocate a field-name string (and may run a
    // user getter on a non-plain descriptor), so the descriptor is re-read from
    // its root at every use.
    let scope = crate::gc::RuntimeHandleScope::new();
    let desc = scope.root_nanbox_f64(descriptor_value);
    if !configurable {
        // The validator compares accessor halves by closure `func_ptr`, which a
        // reflected class accessor value carries. Root the getter value across
        // the setter value's allocation; the validator roots both on entry.
        let get = scope.root_nanbox_f64(
            super::super::class_registry::class_accessor_function_value(getter, false, name),
        );
        let set = super::super::class_registry::class_accessor_function_value(setter, true, name);
        validate_nonconfigurable_redefine(
            name,
            PropertyAttrs::new(false, enumerable, false),
            Some(AccessorDescriptor {
                get: get.get_nanbox_u64(),
                set: set.to_bits(),
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
    super::super::class_registry::class_set_accessor_attrs(
        class_id,
        is_static,
        name,
        enumerable,
        configurable,
    );
    true
}
