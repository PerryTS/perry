use super::*;

#[test]
fn branded_site_revalidates_prototype_brand_and_retirement() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let site = BrandedBirthSite::new();
    let class = crate::weakref::CLASS_ID_WEAKMAP;
    let brand = u64::from(class);
    let first = site.resolve(class, brand);
    assert_eq!(site.resolve(class, brand), first);
    assert!(branded_birth_matches(first, class, brand));
    assert!(!branded_birth_matches(first, class + 1, brand));
    assert!(!branded_birth_matches(first, class, brand + 1));
    // Put a real but incompatible shape in the production site, not a fake id.
    let other = site.resolve(class + 1, brand + 1);
    assert_ne!(first, other);
    assert_eq!(site.resolve(class, brand), first);
    let table = &crate::state::state().shapes;
    super::super::remove_descriptor_and_reverse_indices(&mut table.inner.borrow_mut(), first);
    assert!(!branded_birth_matches(first, class, brand));
    site.0.set(first);
    let reminted = site.resolve(class, brand);
    assert_ne!(reminted, first);
    assert!(branded_birth_matches(reminted, class, brand));
}

#[test]
fn branded_births_share_shape_and_keep_independent_identity() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let site = BrandedBirthSite::new();
    let class = crate::weakref::CLASS_ID_WEAKMAP;
    let a = crate::object::object_alloc_branded(class, u64::from(class), &site);
    let b = crate::object::object_alloc_branded(class, u64::from(class), &site);
    assert_ne!(a, b);
    unsafe {
        let shape = object_shape_stamp(a);
        assert_eq!(shape, object_shape_stamp(b));
        assert!(branded_birth_matches(shape, class, u64::from(class)));
        assert_eq!(
            shape_object_kind_by_id(shape),
            Some(ShapeObjectKind::Ordinary)
        );
    }
}
