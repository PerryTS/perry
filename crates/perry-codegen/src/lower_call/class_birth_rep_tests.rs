//! Charter step 5, T1: the class birth rep is decided once, in codegen
//! (`typed_shape::class_birth_rep_in`), and every consumer takes it from
//! there: the module-init mint, the inline allocation's birth fill and the
//! class-field store precheck.

use super::typed_shape_bake_tests::{emit, loop_new_module};
use perry_hir::types::Type;
use perry_hir::Expr;

/// The last argument of the first call to `callee` in `ir`.
fn mint_rep(ir: &str, callee: &str) -> String {
    let at = ir
        .find(&format!("call i32 @{callee}("))
        .unwrap_or_else(|| panic!("no {callee} call:\n{ir}"));
    let line = ir[at..].lines().next().unwrap();
    let args = line.rsplit_once(')').unwrap().0;
    args.rsplit_once(", ").unwrap().1.to_string()
}

/// (a): a class declarable at allocation mints `F64` for exactly its
/// `number` fields (pointer-free and pointer-bearing mints alike); one whose
/// `number` field could be read before its constructor store (a field
/// initializer runs first) mints all-`Any`.
#[test]
fn the_birth_rep_is_f64_for_exactly_the_raw_f64_fields_declared_at_allocation() {
    // `class Pair { a: number; b: number }`, both prologue-assigned.
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 5");
    // `class Link { a: number; b: string }`: only slot 0 is raw-f64.
    let ir = emit(&loop_new_module(
        "Link",
        Type::String,
        Expr::String("s".into()),
    ));
    assert_eq!(mint_rep(&ir, "js_gc_typed_shape_id_for_keys"), "i64 1");
    // `class Late { a: number; b: number = 3 }`: an initializer runs before
    // the constructor body, so nothing is declared at allocation.
    let mut m = loop_new_module("Late", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::Number(3.0));
    let ir = emit(&m);
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 0");
}

/// (c) + (d): the inline allocation birth-fills the `F64` lanes the mint
/// declared with +0.0 (`store i64 0`), and the undeclared twin fills them with
/// `undefined`: one decision drives the mint and the fill.
#[test]
fn the_inline_allocation_fills_exactly_the_minted_f64_lanes() {
    let undefined = format!("store i64 {}, ptr", crate::nanbox::TAG_UNDEFINED_I64);
    let declared = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    let mut m = loop_new_module("Late", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::Number(3.0));
    let late = emit(&m);
    let fill = |ir: &str| {
        (
            ir.matches("store i64 0, ptr").count(),
            ir.matches(undefined.as_str()).count(),
        )
    };
    let (zeros, undef) = fill(&declared);
    let (late_zeros, late_undef) = fill(&late);
    assert!(
        zeros >= late_zeros + 2 && late_undef >= undef + 2,
        "declared: {zeros} zero / {undef} undefined fills; undeclared: {late_zeros} / {late_undef}\n{declared}"
    );
}
