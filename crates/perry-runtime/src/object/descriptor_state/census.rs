use super::*;

#[test]
fn builtin_function_attribute_census() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    super::super::global_this::ensure_object_intrinsics();
    let descriptors = state().descriptors.property_descriptors.borrow();
    let attrs = descriptors
        .keys()
        .filter(|(owner, _)| crate::closure::is_closure_ptr(*owner))
        .count();
    let owners = state().descriptors.attr_keys_by_owner.borrow();
    let indexed_owners = owners
        .keys()
        .filter(|owner| crate::closure::is_closure_ptr(**owner))
        .count();
    let indexed_keys: usize = owners
        .iter()
        .filter(|(owner, _)| crate::closure::is_closure_ptr(**owner))
        .map(|(_, keys)| keys.len())
        .sum();
    let accessors = state()
        .descriptors
        .accessor_descriptors
        .borrow()
        .keys()
        .filter(|(owner, _)| crate::closure::is_closure_ptr(*owner))
        .count();
    assert_eq!(
        (attrs, indexed_owners, indexed_keys, accessors),
        (0, 0, 0, 0)
    );
    println!("function-census property_descriptors={} attr_index_owners={} attr_index_keys={} accessor_descriptors={}", attrs, indexed_owners, indexed_keys, accessors);
}
