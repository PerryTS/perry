//! #6511 — a multiply whose operands are `Math.*` results must stay on the
//! inline `fmul` fast path. The #5970 BigInt-correctness routing sends any
//! operand that is not statically numeric through `js_dynamic_mul` (two
//! non-leaf ToNumeric calls per operation); `Math.*` builtins always return a
//! real numeric double, so `is_numeric_expr` must recognize them and keep the
//! `Math.sqrt(i) * Math.sin(i * 0.001)` accumulator loop call-free.

use super::*;

fn mul(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn sqrt_times_sin_loop_ir() -> String {
    // The issue's accumulator-loop shape, with a call-free MathSin operand
    // (`Math.sin(i)`, not the repro's `i * 0.001`) so the only `fmul` in the
    // function is the Math-result multiply under test:
    // `for (i = 0; i < 64; i++) acc += Math.sqrt(i) * Math.sin(i);`
    compile_ir(
        "math_result_multiply.ts",
        vec![
            number_let(1, "acc", true, int(0)),
            number_let(3, "iterations", false, int(64)),
            for_loop(
                2,
                local(3),
                vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(add(
                        local(1),
                        mul(
                            Expr::MathSqrt(Box::new(local(2))),
                            Expr::MathSin(Box::new(local(2))),
                        ),
                    )),
                ))],
            ),
            Stmt::Return(Some(local(1))),
        ],
    )
}

#[test]
fn math_result_multiply_stays_inline_fmul() {
    let ir = sqrt_times_sin_loop_ir();
    assert!(
        ir.contains("call double @llvm.sqrt.f64") && ir.contains("call double @llvm.sin.f64"),
        "Math.sqrt / Math.sin should lower to their intrinsics:\n{ir}"
    );
    assert!(
        ir.contains("fmul double"),
        "a multiply of Math.* results must emit an inline fmul:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_mul"),
        "a multiply of Math.* results must not route through the boxed \
         BigInt-aware multiply helper:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_number_coerce"),
        "Math.* results are already raw doubles — the fast path must not \
         re-coerce them:\n{ir}"
    );
}

#[test]
fn math_result_divide_and_subtract_stay_inline() {
    let ir = compile_ir(
        "math_result_div_sub.ts",
        vec![
            number_let(1, "a", false, int(9)),
            number_let(2, "b", false, int(3)),
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Div,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::MathCbrt(Box::new(local(1)))),
                    right: Box::new(Expr::MathLog(Box::new(local(2)))),
                }),
                right: Box::new(Expr::MathExp(Box::new(local(2)))),
            })),
        ],
    );
    assert!(
        ir.contains("fdiv double") && ir.contains("fsub double"),
        "divide/subtract of Math.* results must stay inline:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_div") && !ir.contains("call double @js_dynamic_sub"),
        "divide/subtract of Math.* results must not route through the \
         dynamic helpers:\n{ir}"
    );
}

#[test]
fn dynamic_operand_multiply_keeps_bigint_aware_helper() {
    // #5970's correctness routing must survive: an operand that may be an
    // object (possible boxed BigInt / BigInt-returning valueOf) still goes
    // through the ToNumeric-running dynamic helper.
    let module = module_with_classes_and_params(
        "math_dynamic_operand_multiply.ts",
        Vec::new(),
        vec![param(2, "x", Type::Any)],
        Type::Number,
        vec![Stmt::Return(Some(mul(
            Expr::MathSqrt(Box::new(int(4))),
            local(2),
        )))],
    );
    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_dynamic_mul"),
        "a possibly-object operand must keep the BigInt-aware dynamic \
         multiply routing:\n{ir}"
    );
}
