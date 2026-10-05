//! A builtin's fixed-key throwing Set through the generated store site's
//! cache and miss entries. The site has process lifetime, as emitted sites do;
//! its words contain ShapeIds and its cache lives in the existing PIC arena.
use crate::proxy::{PackedSetSite, PackedSetWays, PACKED_SET_EMPTY};
use std::sync::atomic::{AtomicPtr, AtomicU64};

pub(crate) struct RuntimeStoreSite {
    site: PackedSetSite,
    slot: AtomicPtr<PackedSetWays>,
}

impl RuntimeStoreSite {
    pub(crate) const fn new() -> Self {
        Self {
            site: PackedSetSite {
                set: AtomicU64::new(PACKED_SET_EMPTY),
                add_shapes: AtomicU64::new(0),
                add_guard: AtomicU64::new(0),
                add_ways: AtomicU64::new(0),
                constfn_info: AtomicU64::new(0),
            },
            slot: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    pub(crate) fn store(&self, target: f64, key: &'static [u8], value: f64) {
        let slot = self.slot.as_ptr();
        let stored = crate::proxy::js_put_value_set_packed_fast(target, value, slot);
        if stored.to_bits() != crate::value::TAG_HOLE {
            return;
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(target);
        let value = scope.root_nanbox_f64(value);
        let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
        let atom = match crate::string::atom_lookup(key, hash) {
            Some(atom) => atom,
            None => crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0)
                as *const crate::StringHeader,
        };
        crate::proxy::js_put_value_set_packed_miss(
            target.get_nanbox_f64(),
            atom,
            value.get_nanbox_f64(),
            1,
            slot,
            &self.site.set,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warmed_store_site_observes_a_nonwritable_descriptor_transition() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::object_alloc_plain(2));
        static SITE: RuntimeStoreSite = RuntimeStoreSite::new();
        let receiver = || {
            object.with_const_ptr::<crate::object::ObjectHeader, _>(|o| {
                crate::value::js_nanbox_pointer(o as i64)
            })
        };
        SITE.store(receiver(), b"lastIndex", 1.0);
        SITE.store(receiver(), b"lastIndex", 2.0);
        object.with_mut_ptr::<crate::object::ObjectHeader, _>(|o| unsafe {
            crate::object::key_attrs::apply_edits(
                o,
                &[crate::object::key_attrs::AttrsEdit::Data(b"lastIndex", 0)],
            );
        });
        object.with_const_ptr::<crate::object::ObjectHeader, _>(|o| unsafe {
            let keys = crate::object::object_keys(o);
            let index = crate::object::keys_find_property_slot_by_bytes(
                keys.arr(),
                keys.count(),
                b"lastIndex",
            )
            .unwrap();
            assert_ne!(
                crate::object::key_attrs::keys_entry(keys.arr(), index)
                    & crate::object::key_attrs::ENTRY_NON_WRITABLE,
                0
            );
        });
        assert!(
            crate::exception::catch_js_throw(|| SITE.store(receiver(), b"lastIndex", 3.0)).is_err()
        );
        let key = scope.root_string_ptr(crate::string::intern_ascii_literal(b"lastIndex"));
        object.with_mut_ptr::<crate::object::ObjectHeader, _>(|o| {
            key.with_const_ptr(|key| {
                assert_eq!(
                    crate::object::js_object_get_field_by_name(o, key).as_number(),
                    2.0
                );
            })
        });
    }
}
