//! #11021: every `Expr::ArrayPush` slow arm has an own-`push` exit, and the
//! inline store needs none because its admission mask already excludes every
//! array that owns a named property.
//!
//! Both halves are asserted on emitted IR, because each can fail silently:
//!
//! * a slow arm that still calls the bare `js_array_push_f64_spec` compiles,
//!   runs, and prints the builtin's length for an array whose own `push`
//!   should have won — the bug, back, in one tier;
//! * an admission that drops the named-layout test (`ArrayHeader.named`, where
//!   every own named property lives) lets an array that owns `push` take the
//!   inline STORE, which no exit can see.
//!
//! And the no-cost claim is asserted too: the inline store block carries no
//! call of the new entry.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

const OR_OWN_CALL: &str = "call i64 @js_array_push_f64_spec_or_own(";
const BARE_SPEC_CALL: &str = "call i64 @js_array_push_f64_spec(";

/// `let a: <ty> = []; a.push(<value>); return a;` as one function.
fn push_ir(ty: Type, value: Expr) -> String {
    let mut hir = HirModule::new("apush_own_test");
    hir.functions.push(Function {
        id: 0,
        name: "pushes".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 0,
                name: "a".to_string(),
                ty,
                mutable: true,
                init: Some(Expr::Array(Vec::new(), None)),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 0,
                value: Box::new(value),
                field_writeback: None,
            }),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// The body of the first block whose label starts with `prefix`.
fn block<'a>(ir: &'a str, prefix: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{prefix}"))
        .unwrap_or_else(|| panic!("no {prefix} block in:\n{ir}"))
        + 1;
    let rest = &ir[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn both_slow_arms_of_the_inline_tier_take_the_own_exit() {
    let ir = push_ir(
        Type::Any,
        Expr::Object(vec![("v".to_string(), Expr::Number(1.0))]),
    );
    assert!(
        ir.contains("apush.nofwd"),
        "the push must take its inline tier, or this test proves nothing:\n{ir}"
    );
    // The forwarded arm and the realloc arm.
    assert_eq!(
        ir.matches(OR_OWN_CALL).count(),
        2,
        "the forwarded and realloc arms must each call the own-aware push:\n{ir}"
    );
    assert!(
        !ir.contains(BARE_SPEC_CALL),
        "no slow arm may call the bare builtin push: its value is recomputed from the \
         length, so it cannot carry an own `push` method's return:\n{ir}"
    );
    // The exit is live: its value reaches the expression through the join.
    let join = block(&ir, "apush.own.join");
    assert!(
        join.contains("phi double"),
        "the own exits must meet the ordinary result in a phi:\n{join}"
    );
}

#[test]
fn the_inline_store_pays_nothing_for_the_own_exit() {
    let ir = push_ir(
        Type::Any,
        Expr::Object(vec![("v".to_string(), Expr::Number(1.0))]),
    );
    let inbounds = block(&ir, "apush.inbounds");
    assert!(
        !inbounds.contains("js_array_push_f64_spec_or_own") && !inbounds.contains("apush.own"),
        "the inline store must not test for an own `push` — its admission mask already \
         did:\n{inbounds}"
    );
}

/// The absence proof the whole design rests on: every inline admission mask
/// tests the array's named layout, so an array that owns any named property
/// never reaches the inline store. Checked over the three admission shapes
/// (generic, #7839 numeric, #7469 all-pointer share the `nofwd` block).
#[test]
fn every_inline_admission_mask_excludes_an_array_with_named_properties() {
    for (label, ty, value) in [
        (
            "pointer",
            Type::Any,
            Expr::Object(vec![("v".to_string(), Expr::Number(1.0))]),
        ),
        (
            "number",
            Type::Array(Box::new(Type::Number)),
            Expr::Number(1.5),
        ),
        ("string", Type::Any, Expr::String("s".to_string())),
    ] {
        let ir = push_ir(ty, value);
        let nofwd = block(&ir, "apush.nofwd");
        let masks = nofwd.lines().filter(|l| l.contains("= and i16 ")).count();
        assert!(
            masks != 0,
            "{label}: no `_reserved` admission mask in the nofwd block:\n{nofwd}"
        );
        // `ArrayHeader.named` (+8): loaded and tested against null.
        let named_addr = nofwd
            .lines()
            .any(|l| l.contains("= add i64 ") && l.trim_end().ends_with(", 8"));
        let named_null = nofwd
            .lines()
            .any(|l| l.contains("= icmp ne i64 ") && l.trim_end().ends_with(", 0"));
        assert!(
            named_addr && named_null,
            "{label}: the admission must test the array's named layout, or an \
             array that owns `push` takes the inline store:\n{nofwd}"
        );
    }
}

/// The spec-ordered arm (#7634) is a slow arm too.
#[test]
fn the_spec_ordered_arm_takes_the_own_exit() {
    let ir = push_ir(
        Type::Any,
        Expr::Sequence(vec![
            Expr::LocalSet(0, Box::new(Expr::Array(vec![Expr::Number(9.0)], None))),
            Expr::Number(2.0),
        ]),
    );
    assert!(
        ir.contains("apush.spec.writeback"),
        "the rebinding argument must take the spec-ordered arm:\n{ir}"
    );
    assert!(
        ir.contains(OR_OWN_CALL) && !ir.contains(BARE_SPEC_CALL),
        "the spec-ordered arm must call the own-aware push:\n{ir}"
    );
}
