//! P3 B2 (DESIGN-p3 rev 3.1 §3, REVIEW-p3 D3): a registered class id with no
//! declaration (`!class_identity_has_surface`) reads `prototype` as
//! `undefined`, the declaration path's own answer, decided at the read. Before
//! B2 this fallback returned a JS-visible `0x7FFE | CLASS_PROTOTYPE_REF_FLAG`
//! word from the INT32 tag space.

#[test]
fn an_undeclared_class_reads_prototype_as_undefined() {
    let cid = 0x6b21;
    {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }
    assert_eq!(
        crate::object::class_registry::class_decl_prototype_value(cid).to_bits(),
        crate::value::TAG_UNDEFINED,
        "an id with no declaration has no prototype object"
    );
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let bits = crate::object::class_constructor_key_bits(cid);
    let value = super::get_field_by_name::class_value_get_field(std::ptr::null(), key, bits, cid);
    assert_eq!(value.bits(), crate::value::TAG_UNDEFINED, "C.prototype of an undeclared id");
}
