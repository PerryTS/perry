#[test]
fn refusal_names_cover_every_counter() {
    assert_eq!(super::refusal_name(0), "not_object_pointer");
    assert_eq!(super::refusal_name(11), "inh_workers");
    assert_eq!(super::refusal_name(18), "site_megamorphic");
    assert_eq!(super::refusal_name(19), "function_implicit_own_key");
    for i in 0..super::SITE_REFUSED.len() {
        assert_ne!(super::refusal_name(i), "?", "unnamed counter {i}");
    }
    assert_eq!(super::refusal_name(super::SITE_REFUSED.len()), "?");
}
