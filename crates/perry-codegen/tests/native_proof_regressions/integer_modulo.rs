use super::*;

fn modulo(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn factorial_shaped_ir(divisor: Expr) -> String {
    compile_ir(
        "value_first_i32_modulo.ts",
        vec![
            number_let(1, "sum", true, int(0)),
            number_let(3, "iterations", false, int(64)),
            for_loop(
                2,
                local(3),
                vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(add(local(1), modulo(local(2), divisor))),
                ))],
            ),
            Stmt::Return(Some(local(1))),
        ],
    )
}

fn assert_integer_modulo_with_negative_zero_repair(ir: &str) {
    assert!(
        ir.contains("srem i64"),
        "signed i32 counter modulo a positive Expr::Integer should reach the existing integer remainder path:\n{ir}"
    );
    assert!(
        !ir.contains("frem double"),
        "eligible signed i32 counter modulo must not retain floating remainder:\n{ir}"
    );
    assert!(
        ir.contains("icmp eq i64")
            && ir.contains("fcmp olt double")
            && ir.contains("fneg double")
            && ir.contains("select i1"),
        "integer remainder must retain the existing IEEE-754 negative-zero repair:\n{ir}"
    );
}

fn assert_floating_modulo(ir: &str) {
    assert!(
        ir.contains("frem double"),
        "ineligible modulo shape must remain on floating remainder:\n{ir}"
    );
    assert!(
        !ir.contains("srem i64") && !ir.contains("srem i32"),
        "ineligible modulo shape must not emit integer remainder:\n{ir}"
    );
}

#[test]
fn i32_counter_mod_positive_literal_reaches_integer_fast_path() {
    assert_integer_modulo_with_negative_zero_repair(&factorial_shaped_ir(int(1000)));
}

/// #7494 (was `i32_counter_mod_unsafe_or_nonliteral_divisors_keep_frem`):
/// `negative` and `out_of_i32` used to belong to this "must stay on frem"
/// bucket. #7404/#7416 (`expr/mod.rs::lower_numeric_binary_value`) added a
/// SECOND hand-off for a dividend the integer fast path can prove even
/// without an `i32_counter_slots` entry — gated on `right` being ANY
/// non-zero `Expr::Integer` literal, not (like the older, `i32_counter_slots`
/// -only hand-off just above it in that function) one that also fits `i32`
/// and is positive. That widening is sound, not merely convenient: the
/// dividend side is independently capped at `MAX_FPTOSI_I64_BITS` (62 bits,
/// a full bit of headroom below `i64::MIN`), so `fptosi` on the dividend
/// never overflows regardless of the divisor's sign or magnitude, and the
/// srem-then-negative-zero-repair sequence (below) reproduces JS `%`
/// exactly: ECMA-262 says the result's sign follows the DIVIDEND only ("the
/// result is negative zero if and only if the dividend is negative and
/// nonzero"), which is precisely the dividend-only test the repair already
/// performs — the divisor's sign never enters it. `negative` and
/// `out_of_i32` are consequently reclassified as eligible below, exercised
/// the same way as the sibling positive-literal case
/// (`i32_counter_mod_positive_literal_reaches_integer_fast_path`), instead
/// of asserted by a "does the compiler still do X" text scan that a future
/// intentional change could silently re-break.
#[test]
fn i32_counter_mod_any_nonzero_integer_literal_divisor_reaches_integer_fast_path() {
    for (case, divisor) in [
        ("negative", int(-1)),
        ("out_of_i32", int(i64::from(i32::MAX) + 1)),
    ] {
        let ir = factorial_shaped_ir(divisor);
        assert!(
            ir.contains("srem i64"),
            "{case} is a non-zero Expr::Integer divisor against a proven-integer-valued \
             dividend, which #7404/#7416 route to the integer remainder path:\n{ir}"
        );
        assert_integer_modulo_with_negative_zero_repair(&ir);
    }
}

#[test]
fn i32_counter_mod_unsafe_or_nonliteral_divisors_keep_frem() {
    let cases = [
        // Not `Expr::Integer` at all — an integral-VALUED `Expr::Number` is
        // still a float literal syntactically, so neither Mod hand-off in
        // `lower_numeric_binary_value` (both pattern-match `Expr::Integer`
        // specifically) fires for it.
        ("integral_number", number(1000.0)),
        // `srem(x, 0)` is UB in LLVM; JS `x % 0` is NaN. Excluded by both
        // hand-offs' explicit non-zero-divisor guard.
        ("zero", int(0)),
        // Fractional — `Expr::Number`, same as `integral_number` above.
        ("fractional", number(2.5)),
    ];
    for (case, divisor) in cases {
        let ir = factorial_shaped_ir(divisor);
        assert!(
            ir.contains("frem double"),
            "{case} divisor must remain on floating remainder:\n{ir}"
        );
        assert!(
            !ir.contains("srem i64") && !ir.contains("srem i32"),
            "{case} divisor must not emit integer remainder:\n{ir}"
        );
    }

    let dynamic_ir = compile_ir(
        "value_first_i32_modulo_dynamic_divisor.ts",
        vec![
            number_let(1, "sum", true, int(0)),
            number_let(3, "divisor", false, int(7)),
            number_let(4, "iterations", false, int(64)),
            for_loop(
                2,
                local(4),
                vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(add(local(1), modulo(local(2), local(3)))),
                ))],
            ),
            Stmt::Return(Some(local(1))),
        ],
    );
    assert_floating_modulo(&dynamic_ir);
}

#[test]
fn non_i32_left_operands_keep_frem() {
    let f64_ir = compile_ir(
        "value_first_f64_modulo.ts",
        vec![
            number_let(1, "value", false, number(5.5)),
            Stmt::Return(Some(modulo(local(1), int(2)))),
        ],
    );
    assert_floating_modulo(&f64_ir);

    // A `>>> 0` initializer plus only `>>> 0` writes is the harness's existing
    // way to obtain an unsigned i32 shadow slot. The specialization must not
    // treat its native bit pattern as a signed dividend.
    let u32_ir = compile_ir(
        "value_first_u32_modulo.ts",
        vec![
            number_let(1, "sum", true, int(0)),
            number_let(3, "iterations", false, int(64)),
            Stmt::For {
                init: Some(Box::new(number_let(2, "i", true, ushr_zero(int(0))))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(local(2)),
                    right: Box::new(local(3)),
                }),
                update: Some(Expr::LocalSet(
                    2,
                    Box::new(ushr_zero(add(local(2), int(1)))),
                )),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(add(local(1), modulo(local(2), int(7)))),
                ))],
            },
            Stmt::Return(Some(local(1))),
        ],
    );
    assert_floating_modulo(&u32_ir);
}
