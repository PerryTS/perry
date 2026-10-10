use super::*;
use perry_hir::Stmt;

fn native(method: &str) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args: vec![Expr::Undefined],
    }
}

#[test]
fn record_primitives_keep_only_opaque_values_in_pointer_slots() {
    let methods = [
        ("arrayRecordNeedsIterator", Type::Boolean),
        ("iteratorStep", Type::Boolean),
        ("arrayRecordIndex", Type::Number),
        ("arrayRecordLength", Type::Number),
        ("unknownResult", Type::Boolean),
    ];
    let stmts: Vec<_> = methods
        .iter()
        .enumerate()
        .map(|(i, (method, ty))| Stmt::Let {
            id: i as u32 + 1,
            name: format!("value{i}"),
            ty: ty.clone(),
            mutable: false,
            init: Some(native(method)),
        })
        .collect();
    let slots = collect_pointer_typed_locals(&[], &stmts, &HashSet::new());
    for id in 1..=4 {
        assert!(
            !slots.contains_key(&id),
            "constructor {id} has a primitive ABI result"
        );
    }
    assert!(
        slots.contains_key(&5),
        "an annotation cannot discharge an opaque value"
    );
}
