//! Buffer static value reads must retain their actual receiver (#11257).
use super::*;

fn lowered_value(source: &str) -> Expr {
    let parsed = perry_parser::parse_typescript(source, "buffer-values.ts").unwrap();
    let hir = super::lower_module(&parsed, "buffer-values", "buffer-values.ts").unwrap();
    hir.init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(value),
                ..
            } if name == "value" => Some(value.clone()),
            _ => None,
        })
        .expect("value declaration must be present")
}

fn assert_builtin_receiver(value: &Expr, builtin: &str, member: &str) {
    let Expr::PropertyGet {
        object, property, ..
    } = value
    else {
        panic!("expected property read: {value:?}")
    };
    assert_eq!(property, member);
    let Expr::PropertyGet {
        object: global,
        property: receiver,
        ..
    } = object.as_ref()
    else {
        panic!("{builtin}.{member} lost its receiver: {value:?}")
    };
    assert!(matches!(global.as_ref(), Expr::GlobalGet(0)));
    assert_eq!(receiver, builtin);
}

#[test]
fn buffer_static_values_keep_the_buffer_receiver() {
    for member in [
        "from",
        "alloc",
        "allocUnsafe",
        "allocUnsafeSlow",
        "concat",
        "isBuffer",
        "byteLength",
        "compare",
        "isEncoding",
    ] {
        let value = lowered_value(&format!("const value = Buffer.{member};"));
        assert_builtin_receiver(&value, "Buffer", member);
    }
}

#[test]
fn buffer_custom_value_reads_keep_the_buffer_receiver() {
    let value = lowered_value("const value = Buffer.custom;");
    assert_builtin_receiver(&value, "Buffer", "custom");
}

#[test]
fn other_receivers_are_preserved() {
    assert_builtin_receiver(&lowered_value("const value = Array.from;"), "Array", "from");
    let value = lowered_value("const Buffer = { from: 42 }; const value = Buffer.from;");
    let Expr::PropertyGet {
        object, property, ..
    } = &value
    else {
        panic!("expected local property: {value:?}")
    };
    assert_eq!(property, "from");
    assert!(
        matches!(object.as_ref(), Expr::LocalGet(_)),
        "shadowed Buffer: {value:?}"
    );
}
