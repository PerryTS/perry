//! P3 B2 (DESIGN-p3 rev 3.1 §3, REVIEW-p3 D3): the `prototype` read of a
//! registered class id is the declaration path's answer, decided at the read.
//!
//! The one id with no prototype object is an anonymous-shape id: registered
//! (for `typeof`, literal methods) with an ANONYMOUS declaration, so it has no
//! declaration holder and publishes no CLASS word
//! (`!class_identity_has_surface`). Its `prototype` reads `undefined`. Before
//! B2 this fallback returned a JS-visible `0x7FFE | CLASS_PROTOTYPE_REF_FLAG`
//! word from the INT32 tag space.

fn register(cid: u32) {
    let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
    guard
        .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
        .insert(cid);
}

fn read_prototype(cid: u32) -> crate::value::JSValue {
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let bits = crate::object::class_constructor_key_bits(cid);
    super::get_field_by_name::class_value_get_field(std::ptr::null(), key, bits, cid)
}

#[test]
fn an_anonymous_shape_id_reads_prototype_as_undefined() {
    let cid = 0x6b21;
    register(cid);
    unsafe { crate::object::declarations::test_declare_anonymous(cid) };
    assert_eq!(
        crate::object::class_registry::class_decl_prototype_value(cid).to_bits(),
        crate::value::TAG_UNDEFINED,
        "an anonymous-shape id has no prototype object"
    );
    let value = read_prototype(cid);
    assert_eq!(
        value.bits(),
        crate::value::TAG_UNDEFINED,
        "prototype of an anonymous-shape id"
    );
}

#[test]
fn a_registered_class_reads_prototype_as_its_object_never_the_ref_word() {
    let cid = 0x6b22;
    register(cid);
    let value = read_prototype(cid);
    assert!(
        value.is_pointer(),
        "a registered class's prototype is an object: {:#x}",
        value.bits()
    );
    assert!(
        !crate::value::is_int32_bits(value.bits()),
        "never an INT32-space word"
    );
    assert_eq!(
        value.bits(),
        crate::object::class_registry::class_decl_prototype_value(cid).to_bits(),
        "the read is the declaration path's prototype"
    );
}
