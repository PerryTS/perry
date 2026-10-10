//! Own symbol slots read accessor attributes from the canonical key list.
use super::*;

/// Ordinary objects consult their canonical key attributes once. Other
/// receivers keep the established descriptor-holder and native-owner routes.
pub(crate) unsafe fn own_symbol_slot(obj_f64: f64, sym_f64: f64) -> Option<OwnSymbolSlot> {
    let bits = obj_f64.to_bits();
    let addr = (bits & POINTER_MASK) as usize;
    if bits >> 48 == 0x7FFD {
        if let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(addr) {
            let header = header.as_ref();
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

    if let Some(acc) = accessors::symbol_accessor_property(obj_f64, sym_f64) {
        return Some(OwnSymbolSlot::Accessor {
            get: acc.get,
            set: acc.set,
        });
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    if crate::object::shaped_symbols::owner(obj_key).is_some() {
        return crate::object::shaped_symbols::get(obj_key, sym_key).map(OwnSymbolSlot::Data);
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_ref() {
        if let Some(entries) = map.get(&obj_key) {
            for &(sk, vb) in entries.iter() {
                if sk == sym_key {
                    return Some(OwnSymbolSlot::Data(vb));
                }
            }
        }
    }
    None
}
