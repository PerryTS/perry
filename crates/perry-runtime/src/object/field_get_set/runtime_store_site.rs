//! A builtin's fixed-key throwing Set: the complete strict `[[Set]]`
//! (`js_put_value_set`), whose own-key store reads the receiver's layout.
//! Nothing is remembered between stores.

pub(crate) struct RuntimeStoreSite;

impl RuntimeStoreSite {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn store(&self, target: f64, key: &'static [u8], value: f64) {
        self.store_slow(target, key, value);
    }

    /// A Rust-owned operation needs an abrupt completion, not a longjmp
    /// across its native owners.
    pub(crate) fn store_caught(
        &self,
        target: f64,
        key: &'static [u8],
        value: f64,
    ) -> Result<(), f64> {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(target);
        let value = scope.root_nanbox_f64(value);
        crate::exception::catch_js_throw(|| {
            self.store_slow(target.get_nanbox_f64(), key, value.get_nanbox_f64());
        })
    }

    fn store_slow(&self, target: f64, key: &'static [u8], value: f64) {
        let scope = crate::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(target);
        let value = scope.root_nanbox_f64(value);
        let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
        let atom = match crate::string::atom_lookup(key, hash) {
            Some(atom) => atom,
            None => crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0)
                as *const crate::StringHeader,
        };
        let key = scope.root_string_ptr(atom);
        let target = target.get_nanbox_f64();
        crate::proxy::js_put_value_set(
            target,
            crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64),
            value.get_nanbox_f64(),
            target,
            1,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "regex-engine")]
    #[test]
    fn first_store_never_overwrites_a_private_entry_of_the_same_spelling() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::object_alloc_plain(2));
        let receiver = || {
            object.with_const_ptr::<crate::object::ObjectHeader, _>(|p| {
                crate::value::js_nanbox_pointer(p as i64)
            })
        };
        crate::object::intrinsic_private_add(receiver(), "coldStore", 11.0);
        let private = crate::object::IntrinsicPrivateReadSite::new(
            &crate::object::runtime_layouts::intrinsic_private_atoms::COLD_STORE,
        );
        assert_eq!(private.read(receiver()), Some(11.0));
        static SITE: RuntimeStoreSite = RuntimeStoreSite::new();
        const KEY: &[u8] = b"#<perry:private-value:0:@0:coldStore>";
        SITE.store(receiver(), KEY, 22.0);
        assert_eq!(private.read(receiver()), Some(11.0));
        let key = scope.root_string_ptr(crate::string::intern_ascii_literal(KEY));
        object.with_mut_ptr::<crate::object::ObjectHeader, _>(|o| {
            key.with_const_ptr(|key| {
                assert_eq!(
                    crate::object::js_object_get_field_by_name(o, key).as_number(),
                    22.0
                );
            })
        });
        SITE.store(receiver(), KEY, 33.0);
        assert_eq!(private.read(receiver()), Some(11.0));
    }

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
                keys,
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
