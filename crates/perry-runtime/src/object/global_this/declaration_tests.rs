use super::*;

#[test]
fn builtin_identity_and_lanes_never_branch_on_names() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/check_builtin_declarations.py"))
        .arg("--self-test")
        .output()
        .expect("run the whole-module declaration invariant and its negative controls");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prototype_class_publication_uses_the_record_even_after_relabeling() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = super::super::proto_room::alloc_builtin_prototype();
    assert!(!proto.is_null());
    let mut declared = 0;
    for declaration in GLOBAL_THIS_BUILTIN_CONSTRUCTORS {
        let Some(class) = declaration.prototype_class else {
            continue;
        };
        declared += 1;
        let identity = crate::object::shapes::PROTO_ID_CLASS | u64::from(class);
        let original = crate::object::shapes::identity_prototype_word(identity);
        for name in ["RenamedBuiltin", "RegExp", "WeakMap"] {
            let relabeled = BuiltinConstructorDeclaration {
                name,
                ..*declaration
            };
            finish_builtin_prototype(&relabeled, proto);
            assert_eq!(
                crate::object::shapes::identity_prototype_word(identity),
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
                "the declaration's class word must survive the label {name}"
            );
        }
        crate::object::shapes::write_identity_word(identity, original);
    }
    assert_eq!(
        declared, 4,
        "all four weak builtin declarations were exercised"
    );
}

#[test]
fn objops_instance_identity_is_declared_and_body_checked() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    for (index, declaration) in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().enumerate() {
        let constructor = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
            declaration.name.as_ptr(),
            declaration.name.len(),
        ));
        let closure = crate::JSValue::from_bits(constructor.get_nanbox_u64())
            .as_pointer::<crate::closure::ClosureHeader>();
        let Some(info) = crate::closure::closure_info(closure) else {
            continue;
        };
        // Buffer's native-module constructor has its own declared export body.
        if info.code != declaration.info.code {
            continue;
        }
        let decoded = unsafe {
            super::super::super::global_this_tables::declaration_of_constructor(closure, info)
        }
        .expect("installed constructor must carry its declaration");
        assert_eq!(decoded.instance_identity, declaration.instance_identity);
        let relabeled = BuiltinConstructorDeclaration {
            name: "OtherLabel",
            ..*declaration
        };
        assert_eq!(relabeled.instance_identity, decoded.instance_identity);
        // A scalar capture alone cannot prove identity on an ordinary body.
        let ordinary = crate::closure::js_closure_alloc(
            crate::fn_info!(crate::object::global_this::global_this_parse_int_thunk, 2),
            1 | crate::closure::NO_THIS_REBIND_FLAG,
        );
        crate::closure::js_closure_set_capture_bits(
            ordinary,
            0,
            crate::JSValue::int32(index as i32 + 1).bits(),
        );
        let ordinary_info = crate::closure::closure_info(ordinary).unwrap();
        assert!(unsafe {
            super::super::super::global_this_tables::declaration_of_constructor(
                ordinary,
                ordinary_info,
            )
        }
        .is_none());
    }
}
