//! Own symbol slots read accessor attributes from the canonical key list.
use super::*;

/// Ordinary objects consult their canonical key attributes once. Other
/// receivers keep the established descriptor-holder and native-owner routes.
pub(crate) unsafe fn own_symbol_slot(obj_f64: f64, sym_f64: f64) -> Option<OwnSymbolSlot> {
    let bits = obj_f64.to_bits();
    let addr = (bits & POINTER_MASK) as usize;
    {
        if let Some(header) = crate::value::addr_class::pointer_value_header(bits) {
            if header.obj_type == crate::gc::GC_TYPE_OBJECT
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                let obj = addr as *const crate::object::ObjectHeader;
                let symbol = sym_key_from_f64(sym_f64);
                let position = crate::object::shaped_symbols::position(obj, symbol)?;
                let keys = crate::object::object_keys(obj);
                let lane = crate::object::js_object_get_field(obj, position).bits();
                return if crate::object::key_attrs::key_is_accessor_at(keys.arr(), position) {
                    let pair = (lane & POINTER_MASK) as *const crate::array::ArrayHeader;
                    let get = crate::array::js_array_get(pair, 0).bits();
                    let set = crate::array::js_array_get(pair, 1).bits();
                    Some(OwnSymbolSlot::Accessor {
                        get: if get == TAG_UNDEFINED { 0 } else { get },
                        set: if set == TAG_UNDEFINED { 0 } else { set },
                    })
                } else {
                    Some(OwnSymbolSlot::Data(lane))
                };
            }
        }
    }

    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    // One holder resolution and one key scan answer both the accessor and
    // the data question.
    let holder = crate::object::shaped_symbols::owner(obj_key)?;
    let position = crate::object::shaped_symbols::position(holder, sym_key)?;
    let keys = crate::object::object_keys(holder);
    let lane = crate::object::js_object_get_field(holder, position).bits();
    if crate::object::key_attrs::keys_entry(keys.arr(), position)
        & crate::object::key_attrs::ENTRY_ACCESSOR
        != 0
    {
        let pair = (lane & POINTER_MASK) as *const crate::array::ArrayHeader;
        let bits = |i| {
            let bits = crate::array::js_array_get(pair, i).bits();
            if bits == TAG_UNDEFINED {
                0
            } else {
                bits
            }
        };
        return Some(OwnSymbolSlot::Accessor {
            get: bits(0),
            set: bits(1),
        });
    }
    Some(OwnSymbolSlot::Data(lane))
}
