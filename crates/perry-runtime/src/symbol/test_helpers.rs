//! Owned symbol/class fixture helpers; no registry or scanner.
#[cfg(test)]
pub(crate) fn test_seed_symbol_property_root(owner: usize, sym_key: usize, value_bits: u64) {
    super::store_object_symbol_property_root(owner, sym_key, value_bits);
}

#[cfg(test)]
pub(crate) fn test_symbol_property_root_bits(owner: usize, sym_key: usize) -> Option<u64> {
    super::symbol_property_root_bits(owner, sym_key)
}

#[cfg(test)]
pub(crate) fn test_seed_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    super::store_class_static_symbol_root(class_id, sym_key, value_bits);
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_root_bits(class_id: u32, sym_key: usize) -> Option<u64> {
    let owner = crate::object::class_value::class_value_ptr(class_id) as usize;
    super::symbol_property_root_bits(owner, sym_key)
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_roots_for_class(class_id: u32) -> Vec<(usize, u64)> {
    let owner = crate::object::class_value::class_value_ptr(class_id) as usize;
    super::clone_symbol_entries_for_obj_ptr(owner)
}

#[test]
fn symbol_identity_keeps_creation_order_and_distinctness() {
    // Enumeration's defensive fallback still needs birth order, even though
    // property buckets use the mixed identity and descriptions can be equal.
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let mut previous = super::creation_order(super::next_id());
    for _ in 0..64 {
        let order = super::creation_order(super::next_id());
        assert!(previous < order, "enumeration must retain birth order");
        previous = order;
    }
    let a = unsafe { super::alloc_symbol(std::ptr::null_mut(), false) };
    let b = unsafe { super::alloc_symbol(std::ptr::null_mut(), false) };
    unsafe {
        assert_ne!((*a).id, (*b).id);
        assert!(super::creation_order((*a).id) < super::creation_order((*b).id));
    }
    unsafe {
        let a = crate::value::js_nanbox_pointer(a as i64);
        let b = crate::value::js_nanbox_pointer(b as i64);
        assert_eq!(super::js_symbol_equals(a, b), 0);
        assert_eq!(super::js_symbol_equals(a, a), 1);
    }
}
