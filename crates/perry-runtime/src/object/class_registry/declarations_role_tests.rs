//! Roles belong to the compiler/native descriptor; numeric role reuse is an error.
use super::*;

#[test]
fn anonymous_only_descriptor_precedes_its_first_shape() {
    const CID: u32 = 0x32a0_0001;
    static DECL: ClassDeclaration = ClassDeclaration::anonymous();
    unsafe { js_register_class_declaration(CID, &DECL) };
    let current = class_declaration(CID).expect("anonymous-only descriptor exists");
    assert!(
        std::ptr::eq(current, &DECL),
        "registration retains compiler constant"
    );
    assert!(current.is_anonymous());
    assert!(!current.is_declared());
    assert!(current.members().is_empty());
    assert_eq!(
        crate::object::shapes::class_proto_id(CID),
        crate::object::shapes::PROTO_ID_DEFAULT
    );
    let obj = crate::object::js_object_alloc(CID, 0);
    assert!(unsafe { crate::object::shapes::object_is_plain_record(obj) });
}

#[test]
fn a_late_declared_collision_is_rejected_without_changing_a_live_literal() {
    const CID: u32 = 0x32a0_0002;
    static ANON: ClassDeclaration = ClassDeclaration::anonymous();
    static DECLARED: ClassDeclaration = ClassDeclaration::native(&[]);
    register_class_declaration(CID, &ANON).unwrap();
    let old = class_declaration(CID).unwrap();
    let obj = crate::object::js_object_alloc(CID, 0);
    assert!(
        unsafe { crate::object::shapes::object_is_plain_record(obj) },
        "live receiver before conflicting registration"
    );
    let shape = unsafe { crate::object::shapes::object_shape_stamp(obj) };
    let error = register_class_declaration(CID, &DECLARED).unwrap_err();
    assert!(error
        .to_string()
        .contains("reused for anonymous and declared roles"));
    assert_eq!(error.class_id, CID);
    assert_eq!(
        register_declared_identity(CID),
        Err(ClassRoleConflict { class_id: CID }),
        "name registration must reject the same collision"
    );
    assert!(
        std::ptr::eq(old, class_declaration(CID).unwrap()),
        "failed registration cannot replace the owner"
    );
    assert!(old.is_anonymous() && !old.is_declared());
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_stamp(obj) },
        shape
    );
    assert!(unsafe { crate::object::shapes::object_is_plain_record(obj) });
}

#[test]
fn a_reversed_collision_retains_declared_payload_and_member_retirement_keeps_identity() {
    let _no_move = crate::gc::GcSuppressScope::new();
    extern "C" fn body(_: f64) -> f64 {
        17.0
    }
    static MEMBERS: [ClassMemberDecl; 1] =
        [ClassMemberDecl::native_method("m", body as *const u8, 0)];
    static DECLARED: ClassDeclaration = ClassDeclaration::native(&MEMBERS);
    static ANON: ClassDeclaration = ClassDeclaration::anonymous();
    const CID: u32 = 0x32a0_0003;
    register_class_declaration(CID, &DECLARED).unwrap();
    let obj = crate::object::js_object_alloc(CID, 0);
    assert!(!unsafe { crate::object::shapes::object_is_plain_record(obj) });
    assert_eq!(
        register_class_declaration(CID, &ANON),
        Err(ClassRoleConflict { class_id: CID })
    );
    let d = class_declaration(CID).unwrap();
    assert!(std::ptr::eq(d, &DECLARED));
    assert!(!d.is_anonymous() && d.is_declared());
    assert_eq!(d.members().len(), 1);
    assert_eq!(d.members()[0].code, MEMBERS[0].code);
    assert!(
        crate::object::class_has_own_method(CID, "m"),
        "the member must be live before retirement"
    );
    let holder = crate::value::js_nanbox_get_pointer(crate::object::class_decl_prototype_value(CID))
        as *mut crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes(b"m".as_ptr(), 1);
    assert_eq!(crate::object::js_object_delete_field(holder, key), 1);
    assert!(
        !crate::object::class_has_own_method(CID, "m"),
        "the live property must actually retire"
    );
    assert!(
        class_declaration(CID).unwrap().is_declared(),
        "retirement cannot erase identity"
    );
    assert!(!unsafe { crate::object::shapes::object_is_plain_record(obj) });
}

#[test]
fn literal_method_dispatch_declines_but_a_declared_method_still_runs() {
    let _no_move = crate::gc::GcSuppressScope::new();
    const LITERAL_CID: u32 = 0x32a0_0004;
    const DECLARED_CID: u32 = 0x32a0_0005;
    static LITERAL: ClassDeclaration = ClassDeclaration::anonymous();
    extern "C" fn method(_: f64) -> f64 {
        29.0
    }
    static MEMBERS: [ClassMemberDecl; 1] =
        [ClassMemberDecl::native_method("m", method as *const u8, 0)];
    static DECLARED: ClassDeclaration = ClassDeclaration::native(&MEMBERS);
    register_class_declaration(LITERAL_CID, &LITERAL).unwrap();
    register_class_declaration(DECLARED_CID, &DECLARED).unwrap();
    for (cid, expected) in [
        (LITERAL_CID, None),
        (crate::object::native_module::NATIVE_MODULE_CLASS_ID, None),
        (DECLARED_CID, Some(29.0)),
    ] {
        let receiver = crate::object::js_object_alloc(cid, 0);
        assert_eq!(
            unsafe { crate::object::shapes::receiver_has_class_identity(receiver) },
            expected.is_some(),
            "the shape/header must distinguish native module and literal holders from classes"
        );
        let value = crate::value::js_nanbox_pointer(receiver as i64);
        let result = unsafe {
            crate::object::native_call_method::try_dispatch_instance_method_value(
                value,
                b"m".as_ptr().cast(),
                1,
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(result, expected);
    }
    assert_eq!(
        crate::object::shapes::identity_prototype_word(
            crate::object::shapes::PROTO_ID_CLASS | u64::from(LITERAL_CID)
        ),
        0,
        "a declined literal cannot acquire a class prototype"
    );
}

#[test]
fn a_descriptor_without_an_anonymous_flag_cannot_bypass_role_conflicts() {
    static DECLARED: ClassDeclaration = ClassDeclaration {
        flags: 0,
        ..ClassDeclaration::native(&[])
    };
    static ANONYMOUS: ClassDeclaration = ClassDeclaration::anonymous();
    assert!(DECLARED.is_declared());
    const DECLARED_FIRST: u32 = 0x32a0_0006;
    const ANONYMOUS_FIRST: u32 = 0x32a0_0007;
    register_class_declaration(DECLARED_FIRST, &DECLARED).unwrap();
    assert_eq!(
        register_class_declaration(DECLARED_FIRST, &ANONYMOUS),
        Err(ClassRoleConflict {
            class_id: DECLARED_FIRST
        })
    );
    assert!(std::ptr::eq(
        class_declaration(DECLARED_FIRST).unwrap(),
        &DECLARED
    ));
    register_class_declaration(ANONYMOUS_FIRST, &ANONYMOUS).unwrap();
    assert_eq!(
        register_class_declaration(ANONYMOUS_FIRST, &DECLARED),
        Err(ClassRoleConflict {
            class_id: ANONYMOUS_FIRST
        })
    );
    assert!(std::ptr::eq(
        class_declaration(ANONYMOUS_FIRST).unwrap(),
        &ANONYMOUS
    ));
}
