//! #10498: the class-accessor cache's obligations to the collector.
//!
//! An entry is found by comparing the KEY's address. If a moving collection
//! relocated a key without this table learning of it, the entry would keep
//! the from-space address, and a string later allocated there would match it:
//! a different property name served by this key's accessor.

use super::super::*;

const ROOTS_CLASS_ID: u32 = 0x0011_0499;
const ACCESSOR: &[u8] = b"cacRootLevel";

extern "C" fn roots_getter_10498(_this: f64) -> f64 {
    1.0
}

/// A collection that moves a recorded key must rewrite the entry's copy of
/// its address, so the old address stops matching.
#[test]
fn a_relocated_key_is_rewritten_by_the_root_scan() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    crate::object::class_accessor_cache::test_reset();

    unsafe {
        crate::object::js_register_class_id(ROOTS_CLASS_ID);
        crate::object::js_register_class_getter(
            ROOTS_CLASS_ID as i64,
            ACCESSOR.as_ptr(),
            ACCESSOR.len() as i64,
            roots_getter_10498 as *const () as usize as i64,
        );
        let obj = crate::object::js_object_alloc(ROOTS_CLASS_ID, 4);
        let own = crate::string::js_string_from_bytes(b"cacRootOwn".as_ptr(), 10);
        crate::object::js_object_set_field_by_name(obj, own, 1.0);
        let key = crate::string::js_string_from_bytes(ACCESSOR.as_ptr(), ACCESSOR.len() as u32);
        crate::object::js_object_get_field_by_name(obj, key);
        assert!(
            crate::object::class_accessor_cache::test_lookup(obj, key, false).is_some(),
            "fixture: the read must have recorded the getter before anything moves"
        );

        // Model the evacuation: a to-space copy of the key, with the original
        // forwarded to it.
        let relocated =
            crate::string::js_string_from_bytes(ACCESSOR.as_ptr(), ACCESSOR.len() as u32);
        let valid_ptrs = crate::gc::trace::build_valid_pointer_set();
        set_forwarding_address(
            header_from_user_ptr(key as *const u8) as *mut GcHeader,
            relocated as *mut u8,
        );
        crate::object::class_accessor_cache::scan_class_accessor_cache_roots_mut(
            &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
        );

        let keys = crate::object::class_accessor_cache::test_recorded_keys();
        assert!(
            keys.contains(&(relocated as usize)),
            "the root scan did not follow the key's forwarding pointer"
        );
        assert!(
            !keys.contains(&(key as usize)),
            "the entry still names the from-space key, which a later string \
             could be allocated at"
        );
        assert!(crate::object::class_accessor_cache::test_lookup(obj, key, false).is_none());
    }
    crate::object::class_accessor_cache::test_reset();
}

/// A scanner that is written but never registered is documentation.
#[test]
fn the_class_accessor_cache_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::object::class_accessor_cache::scan_class_accessor_cache_roots_mut
                    as MutableRootScanner as usize
        })
    });
    assert!(
        registered,
        "the class-accessor cache matches entries by key ADDRESS; a moving \
         collector that cannot see this table leaves it naming from-space"
    );
}
