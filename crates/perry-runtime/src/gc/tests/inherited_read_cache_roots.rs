//! The inherited-read cache's two obligations to the collector.
//!
//! The cache records a holder ADDRESS and, on a hit, LOADS through it. So a
//! relocation that this table does not learn about is not a stale answer, it
//! is a read of whatever now lives at that address — a wrong value, returned
//! with no error. These two tests are what stop that being a code comment.

use super::super::*;

/// A collection that moves a cached holder must rewrite this table's copy of
/// its address.
///
/// The assertion is made through the cache's own hit path rather than by
/// reading the entry: an entry whose address was rewritten but whose recorded
/// shape no longer matches would be a rewrite that achieved nothing.
#[test]
fn a_relocated_holder_is_rewritten_by_the_root_scan() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    crate::object::inherited_read_cache::test_clear_cache();

    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        let proto_key = crate::string::js_string_from_bytes(b"ircroot_a".as_ptr(), 9);
        crate::object::js_object_set_field_by_name(proto, proto_key, 7.0);
        let obj = crate::object::js_object_alloc(0, 4);
        let own_key = crate::string::js_string_from_bytes(b"ircroot_own".as_ptr(), 11);
        crate::object::js_object_set_field_by_name(obj, own_key, 1.0);
        crate::object::js_object_set_prototype_of(
            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits()),
            f64::from_bits(crate::value::js_nanbox_pointer(proto as i64).to_bits()),
        );

        let lookup = crate::string::js_string_from_bytes(b"ircroot_a".as_ptr(), 9);
        assert!(
            crate::object::inherited_read_cache::inherited_read_cache_prime(obj, lookup).is_some(),
            "fixture: the chain walk must resolve the key before there is \
             anything for a collection to relocate"
        );
        assert!(
            crate::object::inherited_read_cache::inherited_read_cache_hit(obj, lookup).is_some(),
            "fixture: the entry must be serving the read before the move"
        );

        // Model the evacuation: a to-space twin carrying the same shape stamp
        // and the same slot, with the original forwarded to it.
        let relocated = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(relocated, proto_key, 7.0);
        std::ptr::copy_nonoverlapping(
            proto as *const u8,
            relocated as *mut u8,
            std::mem::size_of::<crate::object::ObjectHeader>() + 4 * 8,
        );
        let valid_ptrs = crate::gc::trace::build_valid_pointer_set();
        set_forwarding_address(
            header_from_user_ptr(proto as *const u8) as *mut GcHeader,
            relocated as *mut u8,
        );
        crate::object::inherited_read_cache::scan_inherited_read_cache_roots_mut(
            &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
        );

        let value = crate::object::inherited_read_cache::inherited_read_cache_hit(obj, lookup);
        assert!(
            value.is_some(),
            "the root scan did not follow the holder's forwarding pointer, so \
             the entry still names from-space"
        );
        assert_eq!(f64::from_bits(value.unwrap().bits()), 7.0);
    }
}

/// A scanner that is written but never registered is documentation. The same
/// gap this test closes is the one #6981 left open for the memoized
/// `Array.prototype` address.
#[test]
fn the_inherited_read_cache_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::object::inherited_read_cache::scan_inherited_read_cache_roots_mut
                    as MutableRootScanner as usize
        })
    });
    assert!(
        registered,
        "the inherited-read cache records holder addresses and dereferences \
         them; a moving collector that cannot see this table hands it \
         from-space"
    );
}
