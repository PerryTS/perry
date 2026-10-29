//! Owner-local Reflect record storage. Composite string keys encode a scoped
//! property byte length, keeping metadata and property namespaces distinct.
use super::*;
use crate::object::ObjectHeader;

fn prefix(property: Option<&str>) -> String {
    match property {
        None => "u:".to_string(),
        Some(p) => format!("p{}:{}", p.len(), p),
    }
}
pub(super) fn storage_key(key: &str, property: Option<&str>) -> String {
    prefix(property) + key
}

pub(super) unsafe fn metadata_record(target: f64, create: bool) -> *mut ObjectHeader {
    // Class-prototype aliases retain the established decorator scoping rule,
    // but the constructor's actual function bag now owns the metadata.
    let _no_move = crate::gc::GcSuppressScope::new();
    let bits = normalize_target_bits(target);
    let target = if bits & !POINTER_MASK == crate::value::INT32_TAG {
        crate::object::class_constructor_ref_value(bits as u32)
    } else {
        f64::from_bits(bits)
    };
    if target.to_bits() & !POINTER_MASK != POINTER_TAG {
        return std::ptr::null_mut();
    }
    let addr = (target.to_bits() & POINTER_MASK) as usize;
    let _holder_edit = if create {
        crate::object::descriptor_state::HolderEdit::new(addr)
    } else {
        None
    };
    let owner = crate::object::descriptor_state::descriptor_holder(addr);
    if owner.is_null() {
        return std::ptr::null_mut();
    }
    let mut meta = (*owner).meta;
    if meta.is_null() {
        if !create {
            return std::ptr::null_mut();
        }
        // A1_UNPROVEN: descriptor_holder(addr) may return the user object itself (kind-A/B question for S0b-S0e).
        meta = crate::object::object_meta_ensure(
            crate::meta_obj_runtime_born!(owner),
            crate::object::MetaReason::ReflectMetadata,
        );
    }
    if (*meta).reflect_metadata == 0 && create {
        let record = crate::object::js_object_alloc_null_proto(0, 0);
        let bits = crate::value::js_nanbox_pointer(record as i64).to_bits();
        // GC_STORE_AUDIT(BARRIERED): private record edge and exact-slot barrier.
        (*meta).reflect_metadata = bits;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).reflect_metadata as *const _ as usize,
            bits,
        );
    }
    ((*meta).reflect_metadata & POINTER_MASK) as *mut ObjectHeader
}

pub(super) fn lookup(target: f64, key: &str, property: Option<&str>) -> Option<f64> {
    unsafe {
        let record = metadata_record(target, false);
        if record.is_null() {
            return None;
        }
        let name = storage_key(key, property);
        let keys = crate::object::object_keys(record);
        let index =
            crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), name.as_bytes())?;
        Some(f64::from_bits(
            crate::object::field_get_set::js_object_get_key_field(record, index).bits(),
        ))
    }
}
pub(super) fn own_keys(target: f64, property: Option<&str>) -> Vec<String> {
    unsafe {
        let record = metadata_record(target, false);
        if record.is_null() {
            return Vec::new();
        }
        let prefix = prefix(property);
        crate::object::descriptor_state::holder_key_names(record, false)
            .into_iter()
            .filter_map(|name| name.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }
}
