//! The computed `super[key] = rhs` reference keeps its raw key until PutValue.
//! Key-expression evaluation still precedes RHS lowering, while ToPropertyKey
//! happens in `js_put_value_set` after the live home parent lookup.

use crate::temp_root_coverage::main_ir_for;
use perry_hir::{Expr, Stmt};

fn computed_super_set_ir() -> String {
    main_ir_for(
        "computed_super_set_order",
        vec![Stmt::Expr(Expr::ObjectSuperPropertySet {
            home: Box::new(Expr::Object(Vec::new())),
            key: Box::new(Expr::String("x".to_string())),
            value: Box::new(Expr::SymbolFor(Box::new(Expr::String(
                "rhs-value".to_string(),
            )))),
            receiver: Box::new(Expr::Object(Vec::new())),
        })],
    )
}

#[test]
fn computed_super_assignment_defers_key_coercion_until_put_value() {
    let ir = computed_super_set_ir();
    let rhs = ir
        .find("call double @js_symbol_for(")
        .expect("RHS marker must lower to an observable runtime call");
    let base = ir
        .find("call double @js_object_get_prototype_of(")
        .expect("the home parent must be captured after the RHS");
    let put = ir
        .find("call double @js_put_value_set(")
        .expect("super assignment must use the existing PutValue path");

    assert!(
        rhs < base && base < put,
        "RHS, home-parent capture, and PutValue must stay ordered:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_to_property_key("),
        "codegen must pass the rooted raw key to PutValue instead of coercing it early:\n{ir}"
    );
}
