//! Shared ValidateAndApplyPropertyDescriptor for symbol-keyed own properties.
use super::*;

#[inline(never)]
pub(super) unsafe fn define_symbol_property(
    scope: &crate::gc::RuntimeHandleScope,
    obj_value: f64,
    receiver_value: f64,
    key_value: f64,
    descriptor_value: f64,
    desc_view: Option<&super::descriptor_helpers::DescView<'_>>,
) -> f64 {
    let obj_value_handle = scope.root_heap_word_u64(obj_value.to_bits());
    let receiver_handle = scope.root_heap_word_u64(receiver_value.to_bits());
    let key_handle = scope.root_nanbox_f64(key_value);
    let desc_handle = scope.root_nanbox_f64(descriptor_value);
    let current_obj = || f64::from_bits(obj_value_handle.get_heap_word_u64());
    let current_key = || key_handle.get_nanbox_f64();
    let current_desc = || desc_handle.get_nanbox_f64();
    let _holder_edit = super::super::descriptor_state::HolderEdit::new(
        crate::symbol::obj_key_from_f64(current_obj()),
    );
    let current_owner = || crate::symbol::obj_key_from_f64(current_obj());
    let current_sym = || crate::symbol::sym_key_from_f64(current_key());
    let existing_accessor_bits =
        crate::symbol::symbol_accessor_descriptor_bits(current_owner(), current_sym());
    let existing_data_bits = existing_accessor_bits
        .is_none()
        .then(|| crate::symbol::symbol_property_root_bits(current_owner(), current_sym()))
        .flatten();
    let existing_get =
        scope.root_nanbox_u64(existing_accessor_bits.map(|(get, _)| get).unwrap_or(0));
    let existing_set =
        scope.root_nanbox_u64(existing_accessor_bits.map(|(_, set)| set).unwrap_or(0));
    let existing_data =
        scope.root_nanbox_u64(existing_data_bits.unwrap_or(crate::value::TAG_UNDEFINED));
    let existed = existing_accessor_bits.is_some() || existing_data_bits.is_some();
    if !existed && crate::value::js_is_truthy(js_object_is_extensible(current_obj())) == 0 {
        throw_object_type_error(b"Cannot define property on non-extensible object");
    }
    // A symbol installed by ordinary assignment has no explicit
    // attrs side-table entry and therefore has the ordinary
    // writable/enumerable/configurable defaults.
    let existing_attrs = existed.then(|| {
        crate::symbol::get_symbol_property_attrs(current_owner(), current_sym())
            .unwrap_or(PropertyAttrs::new(true, true, true))
    });
    if let Some(attrs) = existing_attrs {
        if !attrs.configurable() {
            validate_nonconfigurable_redefine(
                "symbol",
                attrs,
                existing_accessor_bits.map(|_| super::super::AccessorDescriptor {
                    get: existing_get.get_nanbox_u64(),
                    set: existing_set.get_nanbox_u64(),
                }),
                existing_data.get_nanbox_f64(),
                current_desc(),
                desc_view,
            );
        }
    }

    let has_get = desc_has_field(current_desc(), b"get");
    let has_set = desc_has_field(current_desc(), b"set");
    let has_value = desc_has_field(current_desc(), b"value");
    let has_writable = desc_has_field(current_desc(), b"writable");
    if has_get || has_set {
        let get = scope.root_nanbox_u64(if has_get {
            let field = desc_read_field(current_desc(), b"get");
            (!field.is_undefined())
                .then(|| {
                    crate::closure::clone_closure_rebind_this(
                        field.bits(),
                        f64::from_bits(receiver_handle.get_heap_word_u64()),
                    )
                })
                .unwrap_or(0)
        } else {
            existing_get.get_nanbox_u64()
        });
        let set = if has_set {
            let field = desc_read_field(current_desc(), b"set");
            (!field.is_undefined())
                .then(|| {
                    crate::closure::clone_closure_rebind_this(
                        field.bits(),
                        f64::from_bits(receiver_handle.get_heap_word_u64()),
                    )
                })
                .unwrap_or(0)
        } else {
            existing_set.get_nanbox_u64()
        };
        crate::symbol::set_symbol_accessor_property(
            current_obj(),
            current_key(),
            get.get_nanbox_u64(),
            set,
        );
    } else if has_value || has_writable || !existed {
        let value = if has_value {
            f64::from_bits(desc_read_field(current_desc(), b"value").bits())
        } else if existing_accessor_bits.is_some() || existing_data_bits.is_none() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            existing_data.get_nanbox_f64()
        };
        crate::symbol::define_symbol_data_property(current_obj(), current_key(), value);
    }
    let read_flag = |name: &[u8]| -> Option<bool> {
        desc_has_field(current_desc(), name).then(|| {
            crate::value::js_is_truthy(f64::from_bits(desc_read_field(current_desc(), name).bits()))
                != 0
        })
    };
    crate::symbol::set_symbol_property_attrs(
        current_owner(),
        current_sym(),
        PropertyAttrs::new(
            if has_get || has_set {
                false
            } else {
                read_flag(b"writable").unwrap_or_else(|| {
                    existing_attrs
                        .map(|attrs| attrs.writable())
                        .unwrap_or(false)
                })
            },
            read_flag(b"enumerable").unwrap_or_else(|| {
                existing_attrs
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(false)
            }),
            read_flag(b"configurable").unwrap_or_else(|| {
                existing_attrs
                    .map(|attrs| attrs.configurable())
                    .unwrap_or(false)
            }),
        ),
    );
    current_obj()
}
