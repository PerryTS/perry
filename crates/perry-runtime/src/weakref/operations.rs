//! One receiver proof, one key classification and one identity probe.
use super::*;
use storage::{Entry, WeakStorage};

/// Callback-free, non-collecting view. Never survives the rooted grow path.
struct View {
    storage: *mut WeakStorage,
}

impl View {
    #[inline]
    fn of(receiver: f64, expected: u32) -> Option<Self> {
        let addr = js_nanbox_get_pointer(receiver) as usize;
        unsafe {
            let header = crate::value::addr_class::try_read_gc_header(addr)?;
            if header.obj_type != crate::gc::GC_TYPE_OBJECT {
                return None;
            }
            let object = addr as *mut ObjectHeader;
            let brand = storage::collection_brand(object)?;
            if expected != 0 && brand != expected {
                return None;
            }
            Some(Self {
                storage: storage::owned_storage(object),
            })
        }
    }

    #[inline]
    unsafe fn find(&self, key: u64) -> Option<(u32, *mut Entry)> {
        if self.storage.is_null() {
            None
        } else {
            (*self.storage).find(key)
        }
    }
}

pub(super) fn set(map: f64, key: f64, value: f64, weakset: bool) -> f64 {
    let Some(view) = View::of(
        map,
        if weakset {
            CLASS_ID_WEAKSET
        } else {
            CLASS_ID_WEAKMAP
        },
    ) else {
        return if weakset {
            crate::object::dispatch_foreign_weak_receiver(map, "add", &[key])
        } else {
            crate::object::dispatch_foreign_weak_receiver(map, "set", &[key, value])
        };
    };
    if !is_valid_weak_target(key) {
        if weakset {
            throw_invalid_weakset_value();
        } else {
            throw_invalid_weakmap_key();
        }
    }
    // These are actual strong call arguments. Internal table growth below
    // copies conditional pairs through a separate, non-shading store barrier.
    read_barrier::weak_read_barrier(key.to_bits());
    read_barrier::weak_read_barrier(value.to_bits());
    unsafe {
        let bucket = if view.storage.is_null() {
            std::ptr::null_mut()
        } else {
            match (*view.storage).insertion_probe(key.to_bits()) {
                storage::Probe::Hit(_, entry) => {
                    (*entry).value = value.to_bits();
                    (*view.storage).remember(entry);
                    return map;
                }
                storage::Probe::Vacant(bucket) => bucket,
            }
        };
        if !view.storage.is_null() && !(*view.storage).full() {
            (*view.storage).insert_at(key.to_bits(), value.to_bits(), bucket);
            return map;
        }
        // The view is discarded before the first possible collection.
        let scope = crate::gc::RuntimeHandleScope::new();
        let map = scope.root_nanbox_f64(map);
        let key = scope.root_nanbox_f64(key);
        let value = scope.root_nanbox_f64(value);
        let grown = storage::grow(map);
        (*grown).insert(
            key.get_nanbox_f64().to_bits(),
            value.get_nanbox_f64().to_bits(),
        );
        map.get_nanbox_f64()
    }
}

#[no_mangle]
pub extern "C" fn js_weakmap_set(map: f64, key: f64, value: f64) -> f64 {
    set(map, key, value, false)
}

#[no_mangle]
pub extern "C" fn js_weakmap_get(map: f64, key: f64) -> f64 {
    let Some(view) = View::of(map, CLASS_ID_WEAKMAP) else {
        return crate::object::dispatch_foreign_weak_receiver(map, "get", &[key]);
    };
    if is_valid_weak_target(key) {
        unsafe {
            if let Some((_, entry)) = view.find(key.to_bits()) {
                read_barrier::weak_read_barrier((*entry).key);
                return read_barrier::weak_read_barrier_f64((*entry).value);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_weakmap_has(map: f64, key: f64) -> f64 {
    let Some(view) = View::of(map, 0) else {
        return crate::object::dispatch_foreign_weak_receiver(map, "has", &[key]);
    };
    if is_valid_weak_target(key) {
        unsafe {
            if let Some((_, entry)) = view.find(key.to_bits()) {
                read_barrier::weak_read_barrier((*entry).key);
                return f64::from_bits(TAG_TRUE);
            }
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_weakmap_delete(map: f64, key: f64) -> f64 {
    let Some(view) = View::of(map, 0) else {
        return crate::object::dispatch_foreign_weak_receiver(map, "delete", &[key]);
    };
    if is_valid_weak_target(key) {
        unsafe {
            if let Some((slot, _)) = view.find(key.to_bits()) {
                (*view.storage).remove(slot);
                return f64::from_bits(TAG_TRUE);
            }
        }
    }
    f64::from_bits(TAG_FALSE)
}
