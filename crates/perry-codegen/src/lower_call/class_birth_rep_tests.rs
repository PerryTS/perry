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

/// `class Early { a: number = this.b; b: number }`: the initializer of `a`
/// observes the instance before any field is written, so nothing is `F64`.
fn early_module() -> perry_hir::Module {
    let mut m = loop_new_module("Early", Type::Number, Expr::Integer(2));
    m.classes[0].fields[0].init = Some(Expr::PropertyGet {
        object: Box::new(Expr::This),
        property: "b".to_string(),
        byte_offset: 0,
    });
    m
}

/// (a): the mint carries `F64` for exactly the `number` fields written before
/// any code can observe the instance (option (a), `birth_lanes`): constructor
/// prologue stores and literal initializers alike.
#[test]
fn the_birth_rep_is_f64_for_exactly_the_fields_written_before_any_observation() {
    // `class Pair { a: number; b: number }`, both prologue-assigned.
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 5");
    // `class Link { a: number; b: string }`: only slot 0 is `number`.
    let ir = emit(&loop_new_module(
        "Link",
        Type::String,
        Expr::String("s".into()),
    ));
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 1");
    // `class Late { a: number; b: number = 3 }`: the literal initializer is
    // `b`'s first write, before anything can read it.
    let mut m = loop_new_module("Late", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::Number(3.0));
    let ir = emit(&m);
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 5");
    // `class Str { a: number; b: number = "s" }`: a non-Number first write.
    let mut m = loop_new_module("Str", Type::Number, Expr::Integer(2));
    m.classes[0].fields[1].init = Some(Expr::String("s".into()));
    let ir = emit(&m);
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 1");
    let ir = emit(&early_module());
    assert_eq!(mint_rep(&ir, "js_object_shape_id_for_class_keys"), "i64 0");
}

/// (c) + (d): the inline allocation birth-fills the `F64` lanes the mint
/// declared with +0.0 (`store i64 0`), and the all-`Any` twin fills them with
/// `undefined`: one decision drives the mint and the fill.
#[test]
fn the_inline_allocation_fills_exactly_the_minted_f64_lanes() {
    let undefined = format!("store i64 {}, ptr", crate::nanbox::TAG_UNDEFINED_I64);
    let declared = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    let late = emit(&early_module());
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
