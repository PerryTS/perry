use super::*;

/// Walk the keys array of `obj` and apply the given attribute mask AND filter to every existing key.
/// Used by `Object.freeze` (drops `writable` + `configurable`) and `Object.seal` (drops `configurable`).
pub(crate) unsafe fn mark_all_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    _drop_enumerable: bool,
    drop_configurable: bool,
) {
    // Charter step 3: an ordinary object's attributes live with its keys, and
    // freeze/seal rebuilds them ONCE, from the first key.
    if super::key_attrs::attrs_live_in_keys_for_install(obj as usize) {
        super::prop_plan::prop_plan_epoch_bump_for_owner(obj as usize);
        note_descriptor_target_edits(
            obj as usize,
            &[AttrsEdit::Integrity {
                freeze: drop_writable,
            }],
        );
        let _ = drop_configurable;
        return;
    }
    let keys_view = crate::object::object_keys(obj);
    let keys = keys_view.arr();
    if keys.is_null() {
        return;
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return;
    }
    let key_count = keys_view.count() as usize;
    if key_count == 0 || key_count > 65536 {
        return;
    }
    let obj_addr = obj as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if !key_val.is_any_string() {
            continue;
        }
        let Some(Some(key_str)) =
            crate::string::with_string_value_bytes(f64::from_bits(key_val.bits()), |bytes| {
                std::str::from_utf8(bytes).ok().map(str::to_owned)
            })
        else {
            continue;
        };
        // Start from existing attrs (or default `{w:true, e:true, c:true}`) and clear bits.
        let mut attrs =
            get_property_attrs(obj_addr, &key_str).unwrap_or(PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
        set_property_attrs(obj_addr, key_str, attrs);
    }
}
