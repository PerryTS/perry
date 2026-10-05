use super::*;

#[test]
fn a_non_string_key_has_no_interned_pointer() {
    unsafe {
        assert!(interned_key_for_store(1.5).is_none());
        assert!(interned_key_for_store(f64::from_bits(crate::value::TAG_UNDEFINED)).is_none());
    }
}

#[test]
fn a_non_object_target_has_no_pre_store_shape() {
    unsafe {
        assert_eq!(pre_store_shape(2.0), 0);
        assert_eq!(pre_store_shape(f64::from_bits(crate::value::TAG_NULL)), 0);
    }
}

#[test]
fn a_late_class_setter_retires_the_marked_holder_verdict() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_89A7;
    extern "C" fn setter(_this: f64, value: f64) -> f64 {
        value
    }
    unsafe {
        crate::object::js_register_class_name(CID, b"LateSetter".as_ptr(), 10);
        let receiver = crate::object::js_object_alloc(CID, 0);
        let target = crate::value::js_nanbox_pointer(receiver as i64);
        let key = crate::string::js_string_from_bytes(b"late_accessor".as_ptr(), 13);
        let hash = crate::object::key_bytes_hash(b"late_accessor".as_ptr(), 13);
        let key = crate::string::js_string_intern(key, hash);
        let slot = Box::leak(Box::new(std::ptr::null_mut()));
        let site = ChainSite::Pic(slot);
        chain_store_prime(site, target, key);
        assert!(
            chain_store_proven(site, receiver, key),
            "negative verdict must prime before registration"
        );
        let holder = crate::object::class_decl_prototype_object(CID);
        assert!(!holder.is_null(), "the verdict must materialize its holder");
        assert!(crate::object::proto_validity::object_is_marked_prototype(
            holder as usize
        ));
        let before = crate::object::shapes::object_shape_stamp(holder);
        crate::object::js_register_class_setter(
            CID as i64,
            b"late_accessor".as_ptr(),
            13,
            setter as *const () as usize as i64,
            1,
        );
        assert_ne!(crate::object::shapes::object_shape_stamp(holder), before);
        assert!(
            !chain_store_proven(site, receiver, key),
            "the holder's accessor install must revoke the verdict without a class registry latch"
        );
    }
}
