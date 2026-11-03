//! Own symbol slots read accessor attributes from the canonical key list.
use super::*;

/// Ordinary objects consult their canonical key attributes once. Other
/// receivers keep the established descriptor-holder and native-owner routes.
pub(crate) unsafe fn own_symbol_slot(obj_f64: f64, sym_f64: f64) -> Option<OwnSymbolSlot> {
    let bits = obj_f64.to_bits();
    let addr = (bits & POINTER_MASK) as usize;
    {
        if let Some(header) = crate::value::addr_class::pointer_value_header(bits) {
            if crate::gc_type_is_object!(header.obj_type)
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                let obj = addr as *const crate::object::ObjectHeader;
                let symbol = sym_key_from_f64(sym_f64);
                let (keys, live) = crate::object::object_keys_and_live_slot_count(obj);
                let position = crate::object::shaped_symbols::position_in(keys, symbol)?;
                let lane =
                    crate::object::object_key_field_at_with_keys(obj, keys.arr(), position, live)
                        .bits();
                if lane == crate::value::TAG_HOLE {
                    return None;
                }
                return if crate::object::key_attrs::key_is_accessor_at(keys.arr(), position) {
                    let acc = crate::object::accessor_pair::pair_of_value(lane).unwrap_or_default();
                    Some(OwnSymbolSlot::Accessor {
                        get: acc.get,
                        set: acc.set,
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
    let (keys, live) = crate::object::object_keys_and_live_slot_count(holder);
    let position = crate::object::shaped_symbols::position_in(keys, sym_key)?;
    let lane =
        crate::object::object_key_field_at_with_keys(holder, keys.arr(), position, live).bits();
    if lane == crate::value::TAG_HOLE {
        return None;
    }
    if crate::object::key_attrs::keys_entry(keys.arr(), position)
        & crate::object::key_attrs::ENTRY_ACCESSOR
        != 0
    {
        let acc = crate::object::accessor_pair::pair_of_value(lane).unwrap_or_default();
        return Some(OwnSymbolSlot::Accessor {
            get: acc.get,
            set: acc.set,
        });
    }
    Some(OwnSymbolSlot::Data(lane))
}
