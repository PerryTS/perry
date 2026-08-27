//! IR-census + encoding tests for the inline strict-equality lowering.
//!
//! The lowering's whole value is that a `===` against a string literal stops
//! being a call, so the assertion that matters is a census: the `streqlit.*`
//! blocks are present AND `js_eq` is gone. A fast path that is implemented but
//! never reached still compiles, still prints the right answer, and still
//! produces a different object file (CLAUDE.md, "a gate must assert its subject
//! was live") — only the emitted labels tell the two apart.
//!
//! Every positive is paired with a negative that must keep the call, so an arm
//! that quietly widened — to loose `==`, which coerces, or to a comparison with
//! no literal operand at all, where none of the compile-time facts hold — fails
//! here rather than in the gap suite.

use super::compare::{i8_literal, sso_immediate};
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Stmt};

/// Module-init statements compile into `main` for an entry module, and
/// `main_ir_for` returns exactly that function's slice — so every `contains` /
/// `!contains` below is scoped to the code under test instead of the whole
/// module. Shared with the #6951 temp-root family rather than re-spelling its
/// ~90-line `CompileOptions` / `Module` harness.
use crate::temp_root_coverage::main_ir_for as ir_for;

use super::slice8_rooting_tests::{call_operand_of, producer_line};

const X: u32 = 1;
const Y: u32 = 2;
const R: u32 = 3;

/// `let x: any = undefined; let y: any = undefined; let r: any = <lhs> op <rhs>;`
fn cmp_ir(name: &str, op: CompareOp, lhs: Expr, rhs: Expr) -> String {
    ir_for(
        name,
        vec![
            Stmt::Let {
                id: X,
                name: "x".to_string(),
                ty: Type::Any,
                mutable: true,
                // `undefined`, not a string literal: initializing an `any`
                // local with a string refines its static type to `string`,
                // which routes the comparison to the both-strings arm and
                // makes the two negatives below vacuous.
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: Y,
                name: "y".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Compare {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                }),
            },
        ],
    )
}

/// A CALL, not the unconditional `declare` line.
const JS_EQ_CALL: &str = "call i64 @js_eq(";
const JS_LOOSE_EQ_CALL: &str = "call i64 @js_loose_eq(";

#[test]
fn strict_eq_against_a_proven_symbol_is_raw_identity() {
    let ir = ir_for(
        "streq_proven_symbol",
        vec![
            Stmt::Let {
                id: X,
                name: "value".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: Y,
                name: "sentinel".to_string(),
                // The proof must come from the initializer, not this erased
                // declaration, so keep the source type deliberately broad.
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::SymbolNew(None)),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );
    assert!(
        !ir.contains(JS_EQ_CALL),
        "proven Symbol identity fell through to js_eq:\n{ir}"
    );
    assert!(
        ir.contains("icmp eq i64"),
        "proven Symbol identity did not become a raw-bit compare:\n{ir}"
    );
}

#[test]
fn loose_eq_against_a_proven_symbol_keeps_the_coercing_helper() {
    let ir = ir_for(
        "looseeq_proven_symbol",
        vec![
            Stmt::Let {
                id: X,
                name: "value".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: Y,
                name: "sentinel".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::SymbolNew(None)),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::LooseEq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );
    assert!(ir.contains(JS_LOOSE_EQ_CALL), "{ir}");
}

#[test]
fn a_symbol_annotation_without_symbol_provenance_keeps_js_eq() {
    let ir = ir_for(
        "streq_lying_symbol_annotation",
        vec![
            Stmt::Let {
                id: X,
                name: "value".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: Y,
                name: "not_really_a_symbol".to_string(),
                ty: Type::Symbol,
                mutable: false,
                init: Some(Expr::Object(vec![("x".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );
    assert!(
        ir.contains(JS_EQ_CALL),
        "an erased Symbol annotation was mistaken for runtime proof:\n{ir}"
    );
}

#[test]
fn a_reassigned_symbol_constructor_local_keeps_js_eq() {
    let ir = ir_for(
        "streq_reassigned_symbol_local",
        vec![
            Stmt::Let {
                id: X,
                name: "value".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: Y,
                name: "sentinel".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::SymbolNew(None)),
            },
            // Whole-region reassignment analysis must revoke the constructor
            // proof even though this write appears before the comparison. The
            // replacement may be a moving/forwarded object whose identity
            // requires `js_eq`'s forwarding resolution.
            Stmt::Expr(Expr::LocalSet(
                Y,
                Box::new(Expr::Object(vec![("x".to_string(), Expr::Number(1.0))])),
            )),
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );
    assert!(
        ir.contains(JS_EQ_CALL),
        "a reassigned Symbol constructor local retained stale provenance:\n{ir}"
    );
}

/// `makeLeft() === makeRight()` has to keep the first call result alive while
/// the second call runs. Object literals give the IR test the same two
/// allocating, pointer-valued temporaries without depending on call lowering:
/// before #7979, `js_eq`'s left operand traces back to the FIRST allocation;
/// after the fix it traces back to a root re-read below the second one.
///
/// Follow the full pure-op chain instead of checking the `bitcast` handed to
/// `js_eq`: that bitcast is emitted below both allocations even when its input
/// is the stale pre-collection register, which made a one-level ordering check
/// green against the bug.
#[test]
fn strict_eq_rereads_its_left_operand_below_an_allocating_right_operand() {
    let ir = cmp_ir(
        "streq_rooted_operands",
        CompareOp::Eq,
        Expr::Object(vec![("left".to_string(), Expr::Number(1.0))]),
        Expr::Object(vec![("right".to_string(), Expr::Number(2.0))]),
    );
    let left = call_operand_of(&ir, "js_eq", 0);
    let right = call_operand_of(&ir, "js_eq", 1);
    let left_producer = producer_line(&ir, &left);
    let right_producer = producer_line(&ir, &right);
    assert!(
        left_producer > right_producer,
        "js_eq's left operand ({left}) is produced at line {left_producer}, above the right \
         operand ({right}) at line {right_producer}. The right allocation can collect, so the \
         left value must be rooted before it and re-read below it.\n{ir}"
    );
}

/// #7990 surfaced the same stale comparison-operand class through a different
/// consumer: the copier reached bytes reporting the impossible combination
/// `GC_TYPE_MAP | GC_FLAG_INTERNED`. Keep the reported typed-Map population in
/// the regression. The generic object case above would stay green if a future
/// type-analysis shortcut accidentally classified Maps as non-pointers.
#[test]
fn strict_eq_rereads_a_map_operand_below_an_allocating_right_operand() {
    let ir = cmp_ir(
        "streq_rooted_map_operand_7990",
        CompareOp::Eq,
        Expr::MapNew,
        Expr::Object(vec![("right".to_string(), Expr::Number(2.0))]),
    );
    let left = call_operand_of(&ir, "js_eq", 0);
    let right = call_operand_of(&ir, "js_eq", 1);
    let left_producer = producer_line(&ir, &left);
    let right_producer = producer_line(&ir, &right);
    assert!(
        left_producer > right_producer,
        "js_eq's Map operand ({left}) is produced at line {left_producer}, below the right \
         operand ({right}) at line {right_producer}. An intervening collection would leave \
         the comparison holding a retired Map address (#7990).\n{ir}"
    );
}

/// The complementary cost assertion: even with an allocating right operand,
/// a proven-number left operand cannot be invalidated by relocation and must
/// stay in its original register. A blanket "root every comparison" fix would
/// move its producer below the right allocation and fail this test.
#[test]
fn strict_eq_reuses_a_non_pointer_left_operand_across_an_allocating_right_operand() {
    let ir = ir_for(
        "streq_reused_primitive",
        vec![
            Stmt::Let {
                id: X,
                name: "x".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Let {
                id: R,
                name: "r".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::Object(vec![("right".to_string(), Expr::Number(2.0))])),
                }),
            },
        ],
    );
    // A proven-Number left operand lowers the whole comparison inline: every
    // non-Number NaN-box reads as a NaN double, so `fcmp oeq` answers `false`
    // for the object exactly as `js_eq` would, and no helper call remains.
    assert!(
        !ir.contains("@js_eq(") && !ir.contains("@js_strict_eq("),
        "a proven-Number left operand must not pay a runtime equality call:\n{ir}"
    );
    let fcmp = ir
        .lines()
        .map(str::trim)
        .find(|line| line.contains("fcmp oeq double"))
        .unwrap_or_else(|| panic!("no inline numeric strict-equality compare in:\n{ir}"));
    let left = super::class_field_barrier_tests::operand(fcmp, 1).expect("fcmp left operand");
    let right = super::class_field_barrier_tests::operand(fcmp, 2).expect("fcmp right operand");
    let left_producer = producer_line(&ir, &left);
    let right_producer = producer_line(&ir, &right);
    assert!(
        left_producer < right_producer,
        "a non-pointer numeric operand cannot become stale, so it should stay in the \
         register produced at line {left_producer}, above the right allocation at line \
         {right_producer}. Rooting/re-reading it adds traffic without protecting anything.\n{ir}"
    );
}

#[test]
fn strict_eq_against_a_string_literal_emits_the_inline_dispatch_and_no_js_eq_call() {
    let ir = cmp_ir(
        "streq_lit",
        CompareOp::Eq,
        Expr::LocalGet(X),
        Expr::String("num".to_string()),
    );
    assert!(
        ir.contains("streqlit.tag"),
        "inline literal dispatch not reached:\n{ir}"
    );
    assert!(
        !ir.contains(JS_EQ_CALL),
        "js_eq call survived the inline literal dispatch:\n{ir}"
    );
    assert!(
        ir.contains("streqlit.bm"),
        "three-byte literal did not compare its remaining middle byte inline:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_string_equals("),
        "three-byte literal retained the full string-equality helper:\n{ir}"
    );
}

#[test]
fn longer_string_literal_keeps_the_full_content_fallback() {
    let ir = cmp_ir(
        "streq_long_lit",
        CompareOp::Eq,
        Expr::LocalGet(X),
        Expr::String("destroy".to_string()),
    );
    assert!(ir.contains("streqlit.slow"), "{ir}");
    assert!(ir.contains("call i32 @js_string_equals("), "{ir}");
}

#[test]
fn the_literal_may_sit_on_either_side() {
    let ir = cmp_ir(
        "streq_lit_left",
        CompareOp::Eq,
        Expr::String("num".to_string()),
        Expr::LocalGet(X),
    );
    assert!(ir.contains("streqlit.tag"), "{ir}");
    assert!(!ir.contains(JS_EQ_CALL), "{ir}");
}

#[test]
fn strict_ne_against_a_string_literal_uses_the_same_dispatch() {
    let ir = cmp_ir(
        "strne_lit",
        CompareOp::Ne,
        Expr::LocalGet(X),
        Expr::String("num".to_string()),
    );
    assert!(ir.contains("streqlit.tag"), "{ir}");
    assert!(!ir.contains(JS_EQ_CALL), "{ir}");
}

/// Negative pair #1. Loose `==` coerces (`"5" == 5`), which the inline
/// dispatch does not implement, so it must stay on the runtime helper.
#[test]
fn loose_eq_against_a_string_literal_keeps_the_coercing_runtime_call() {
    let ir = cmp_ir(
        "looseeq_lit",
        CompareOp::LooseEq,
        Expr::LocalGet(X),
        Expr::String("num".to_string()),
    );
    assert!(
        !ir.contains("streqlit.tag"),
        "loose == was captured by the strict-only literal dispatch:\n{ir}"
    );
    assert!(
        ir.contains(JS_LOOSE_EQ_CALL),
        "loose == lost its coercing helper:\n{ir}"
    );
}

/// Negative pair #2. With no literal operand there is no compile-time pooled
/// pointer, no compile-time SSO immediate and no compile-time length, so the
/// literal dispatch must not appear.
#[test]
fn strict_eq_between_two_any_locals_does_not_use_the_literal_dispatch() {
    let ir = cmp_ir(
        "streq_nolit",
        CompareOp::Eq,
        Expr::LocalGet(X),
        Expr::LocalGet(Y),
    );
    assert!(
        !ir.contains("streqlit.tag"),
        "literal dispatch fired without a literal operand:\n{ir}"
    );
    assert!(
        ir.contains(JS_EQ_CALL),
        "the no-literal strict-equality fallback disappeared:\n{ir}"
    );
}

#[test]
fn string_vs_generic_key_uses_the_identity_first_dispatch() {
    let ir = ir_for(
        "streq_generic_key",
        vec![
            Stmt::Let {
                id: X,
                name: "stored".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String("stored".to_string())),
            },
            Stmt::Let {
                id: Y,
                name: "key".to_string(),
                ty: Type::TypeVar("K".to_string()),
                mutable: false,
                init: Some(Expr::Undefined),
            },
            Stmt::Let {
                id: R,
                name: "same".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );

    assert!(
        ir.contains("streq.tag"),
        "string-vs-generic comparison did not enter the identity-first dispatch:\n{ir}"
    );
    assert!(
        !ir.contains("streq.heap.len"),
        "generic-key equality paid the short-string checks without two proven strings:\n{ir}"
    );
}

#[test]
fn string_equality_inlines_short_heap_content_checks_before_the_helper() {
    let ir = ir_for(
        "streq_short_heap",
        vec![
            Stmt::Let {
                id: X,
                name: "left".to_string(),
                ty: Type::String,
                mutable: true,
                init: Some(Expr::String("left".to_string())),
            },
            Stmt::Let {
                id: Y,
                name: "right".to_string(),
                ty: Type::String,
                mutable: true,
                init: Some(Expr::String("right".to_string())),
            },
            Stmt::Let {
                id: R,
                name: "same".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(X)),
                    right: Box::new(Expr::LocalGet(Y)),
                }),
            },
        ],
    );

    for label in [
        "streq.heap.len",
        "streq.heap.first",
        "streq.heap.last",
        "streq.heap.middle",
    ] {
        assert!(
            ir.contains(label),
            "missing {label} short-string arm:\n{ir}"
        );
    }
    assert!(
        ir.contains("call i32 @js_string_equals("),
        "long heap strings lost their full-content fallback:\n{ir}"
    );
}

/// The SSO immediate is hand-built here but consumed by `perry-runtime`'s
/// canonical encoding (`JSValue::try_short_string`): tag `0x7FF9` in bits
/// 48..=63, byte length in bits 40..=47, bytes little-endian in bits 0..=39,
/// every other bit zero. `perry-codegen` cannot depend on `perry-runtime`, so
/// these pin the layout numerically. If they drift, `"+" === "+"` between a
/// `charAt` result and a literal silently becomes false.
#[test]
fn sso_immediate_matches_the_runtime_encoding() {
    assert_eq!(sso_immediate(b""), Some(0x7FF9_0000_0000_0000));
    assert_eq!(sso_immediate(b"+"), Some(0x7FF9_0100_0000_002B));
    assert_eq!(sso_immediate(b"if"), Some(0x7FF9_0200_0000_6669));
    // 'n' = 0x6E (byte 0, bits 0..8), 'u' = 0x75, 'm' = 0x6D (byte 2).
    assert_eq!(sso_immediate(b"num"), Some(0x7FF9_0300_006D_756E));
    assert_eq!(sso_immediate(b"abcde"), Some(0x7FF9_0565_6463_6261));
}

#[test]
fn sso_immediate_declines_anything_longer_than_the_inline_payload() {
    assert_eq!(sso_immediate(b"abcdef"), None);
    assert_eq!(sso_immediate(b"parse error"), None);
}

/// LLVM integer literals are signed, so a high byte must be written in two's
/// complement or the `icmp eq i8` never matches. Multi-byte UTF-8 literals
/// ("é" = 0xC3 0xA9) are exactly the case that needs it.
#[test]
fn i8_literal_writes_high_bytes_in_twos_complement() {
    assert_eq!(i8_literal(0x00), "0");
    assert_eq!(i8_literal(0x7F), "127");
    assert_eq!(i8_literal(0x80), "-128");
    assert_eq!(i8_literal(0xC3), "-61");
    assert_eq!(i8_literal(0xFF), "-1");
}

#[test]
fn local_typeof_strict_ne_literal_uses_the_integer_classifier() {
    let ir = cmp_ir(
        "typeof_local_ne_number",
        CompareOp::Ne,
        Expr::TypeOf(Box::new(Expr::LocalGet(X))),
        Expr::String("number".to_string()),
    );
    assert!(
        ir.contains("call i32 @js_value_typeof_tag("),
        "literal typeof comparison did not use the integer classifier:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_value_typeof("),
        "literal typeof comparison still materialized a typeof string:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_string_equals("),
        "literal typeof comparison still entered string equality:\n{ir}"
    );
}

#[test]
fn reversed_local_typeof_strict_eq_uses_the_same_integer_classifier() {
    let ir = cmp_ir(
        "typeof_local_eq_reversed",
        CompareOp::Eq,
        Expr::String("string".to_string()),
        Expr::TypeOf(Box::new(Expr::LocalGet(X))),
    );
    assert!(ir.contains("call i32 @js_value_typeof_tag("), "{ir}");
    assert!(!ir.contains("call i64 @js_value_typeof("), "{ir}");
}

#[test]
fn nonliteral_typeof_comparison_keeps_runtime_string_semantics() {
    let ir = cmp_ir(
        "typeof_nonliteral_compare",
        CompareOp::Eq,
        Expr::TypeOf(Box::new(Expr::LocalGet(X))),
        Expr::LocalGet(Y),
    );
    assert!(
        ir.contains("call i64 @js_value_typeof("),
        "a nonliteral comparison incorrectly took the integer-tag ABI:\n{ir}"
    );
    assert!(!ir.contains("call i32 @js_value_typeof_tag("), "{ir}");
}

#[test]
fn dynamic_strict_eq_against_number_normalizes_int32_without_js_eq() {
    let ir = cmp_ir(
        "dynamic_strict_eq_number",
        CompareOp::Eq,
        Expr::LocalGet(X),
        Expr::Number(7.0),
    );
    assert!(
        ir.contains(crate::nanbox::INT32_TAG_TOP16_I64),
        "the compact-INT32 normalization guard is absent:\n{ir}"
    );
    assert!(
        ir.contains("fcmp oeq double"),
        "dynamic-vs-number equality did not become numeric fcmp:\n{ir}"
    );
    assert!(
        !ir.contains(JS_EQ_CALL),
        "dynamic-vs-number strict equality retained js_eq:\n{ir}"
    );
}

#[test]
fn reversed_dynamic_strict_ne_against_number_uses_unordered_numeric_compare() {
    let ir = cmp_ir(
        "dynamic_strict_ne_number_reversed",
        CompareOp::Ne,
        Expr::Number(7.0),
        Expr::LocalGet(X),
    );
    assert!(
        ir.contains("fcmp une double"),
        "strict !== must treat NaN and every non-number tag as unequal:\n{ir}"
    );
    assert!(!ir.contains(JS_EQ_CALL), "{ir}");
}

#[test]
fn dynamic_relational_against_number_inlines_primitive_arm_and_keeps_coercing_fallback() {
    let ir = cmp_ir(
        "dynamic_lt_number",
        CompareOp::Lt,
        Expr::LocalGet(X),
        Expr::Number(7.0),
    );
    assert!(
        ir.contains("relnum.fast") && ir.contains("relnum.slow"),
        "dynamic-vs-number relational compare lacks guarded primitive dispatch:\n{ir}"
    );
    assert!(
        ir.contains("fcmp olt double"),
        "the admitted primitive arm did not lower to fcmp:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_rel_lt("),
        "objects, strings, BigInts, and Symbols lost their coercing fallback:\n{ir}"
    );
}

#[test]
fn loose_equality_against_number_keeps_coercion() {
    let ir = cmp_ir(
        "dynamic_loose_eq_number",
        CompareOp::LooseEq,
        Expr::LocalGet(X),
        Expr::Number(7.0),
    );
    assert!(
        ir.contains(JS_LOOSE_EQ_CALL),
        "dynamic == number incorrectly bypassed coercion:\n{ir}"
    );
}

/// `typeof x === "number"` decides the definitely-a-Number cases inline and
/// keeps the integer classifier only on the slow arm: a value whose top 16
/// bits are outside the NaN-box tag range, that is not an untagged raw
/// typed-array pointer, and that lies outside the Web Streams id band is a
/// Number by construction. Everything else still asks `js_value_typeof_tag`,
/// so the two routes can never disagree.
#[test]
fn local_typeof_number_literal_is_decided_inline_for_plain_doubles() {
    let ir = cmp_ir(
        "typeof_local_eq_number_inline",
        CompareOp::Eq,
        Expr::TypeOf(Box::new(Expr::LocalGet(X))),
        Expr::String("number".to_string()),
    );
    assert!(
        ir.contains("typeof.num.fast") && ir.contains("typeof.num.slow"),
        "the inline Number test must fork a fast and a slow arm:\n{ir}"
    );
    let slow = super::class_field_barrier_tests::block_body(&ir, "typeof.num.slow.")
        .expect("slow arm exists");
    assert!(
        slow.contains("call i32 @js_value_typeof_tag("),
        "the classifier call must live on the slow arm:\n{slow}"
    );
    let calls = ir.matches("call i32 @js_value_typeof_tag(").count();
    assert_eq!(
        calls, 1,
        "exactly one classifier call, on the slow arm:\n{ir}"
    );
    assert!(
        ir.contains("icmp ugt i64") && ir.contains(", 6\n") || ir.contains(", 6 "),
        "the tag-range test must be the single unsigned range compare:\n{ir}"
    );
    assert!(
        (ir.contains("fcmp olt double") && ir.contains("1048576.0"))
            || ir.contains("0x4130000000000000"),
        "the stream id band must be excluded inline:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_value_typeof(") && !ir.contains("call i32 @js_string_equals("),
        "no typeof string or string equality may be materialized:\n{ir}"
    );
}
