//! #8167 — a specialized entry must be able to re-enter ITSELF.
//!
//! Before this, the only raw-`i32` argument shapes a call site could prove
//! were an `i32` literal and a bare `LocalGet` of an integer local. A
//! recursive call almost never has that shape — its argument is DERIVED
//! (`fib(n - 1)`) — so every recursive edge inside a `$spec_i32` clone
//! targeted the generic public symbol. The clone therefore ran once per
//! top-level call and the whole recursion paid dynamic dispatch.
//!
//! These fixtures pin both directions: the derived argument reaches the
//! clone, and an argument whose value the slot contract does NOT admit still
//! reaches the boxed entry.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt};

fn function_ir<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let line_end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..line_end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function containing {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

/// `function f(n: number): number { return n < 2 ? n : f(<lhs>) + f(<rhs>); }`
/// plus a module-init `f(40)` — the literal site is what makes the plan a
/// raw-`i32` tuple in the first place.
fn recursive_module(lhs: Expr, rhs: Expr) -> Module {
    let f = Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 10,
            name: "n".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(2)),
            }),
            then_expr: Box::new(Expr::LocalGet(10)),
            else_expr: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![lhs],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
                right: Box::new(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![rhs],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            }),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut module = Module::new("spec_self_recursion.ts");
    module.functions.push(f);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Integer(40)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module
}

/// The same recursive body reached through an ordinary `number` parameter
/// guard. The `Number(...)` construction gives the outer call a runtime
/// Number proof without creating a viable raw-i32 tuple, so the emitted clone
/// is the boxed `$spec_b` shape whose recursive routing #8169 exercises.
fn guarded_recursive_module(lhs: Expr, rhs: Expr) -> Module {
    let mut module = recursive_module(lhs, rhs);
    module.init.clear();
    module.init.push(Stmt::Let {
        id: 20,
        name: "k".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::NumberCoerce(Box::new(Expr::Undefined))),
    });
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::LocalGet(20)],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module
}

/// A guarded Number parameter plus an unconstrained value whose arithmetic
/// may produce a BigInt. The recursive first argument must keep the public
/// guard: when `x` is a BigInt, `x * x` is a BigInt too.
fn bigint_capable_guarded_recursive_module() -> Module {
    let f = Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: 10,
                name: "n".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 11,
                name: "x".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(1)),
            }),
            then_expr: Box::new(Expr::LocalGet(10)),
            else_expr: Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![
                    Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(Expr::LocalGet(11)),
                        right: Box::new(Expr::LocalGet(11)),
                    },
                    Expr::LocalGet(11),
                ],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut module = Module::new("spec_self_recursion_bigint.ts");
    module.functions.push(f);
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Undefined, Expr::Undefined],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module
}

fn compile_ir(module: &Module) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

fn arith(op: BinaryOp, right: i64) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(Expr::LocalGet(10)),
        right: Box::new(Expr::Integer(right)),
    }
}

#[test]
fn derived_recursive_i32_argument_re_enters_the_clone_behind_a_range_test() {
    let ir = compile_ir(&recursive_module(
        arith(BinaryOp::Sub, 1),
        arith(BinaryOp::Sub, 2),
    ));
    let clone = function_ir(&ir, "$spec_i32(");

    // The subject has to exist before any of this means anything: the literal
    // module-init site must have produced a raw-i32 clone.
    assert!(
        clone.starts_with(
            "define internal preserve_nonecc double \
                 @perry_fn_spec_self_recursion_ts__f$spec_i32(i32"
        ),
        "expected a raw-i32 clone to specialize:\n{clone}"
    );

    // BOTH recursive edges re-enter the clone.
    assert_eq!(
        clone
            .matches("call preserve_nonecc double @perry_fn_spec_self_recursion_ts__f$spec_i32(i32")
            .count(),
        2,
        "both recursive calls must target the clone:\n{clone}"
    );

    // `n - 1` for an i32 `n` is [-2^31 - 1, 2^31 - 2] — one value wider than
    // the slot — so each edge is guarded by a 32-bit range test, and the
    // always-correct boxed entry is still the cold arm.
    assert_eq!(
        clone.matches("fcmp oge double").count(),
        2,
        "each derived argument needs its own low-bound test:\n{clone}"
    );
    assert!(clone.contains("-2147483648.0"));
    assert!(clone.contains("2147483647.0"));
    assert_eq!(
        clone
            .matches("call double @perry_fn_spec_self_recursion_ts__f(double")
            .count(),
        2,
        "the out-of-range arm must still reach the permanent boxed ABI:\n{clone}"
    );
}

#[test]
fn a_multiplied_recursive_argument_keeps_the_boxed_call() {
    // `n * 0` with `n < 0` is `-0`, which `js_typed_i32_arg_guard` rejects on
    // purpose (the raw slot has no `-0` to round-trip through), and a product
    // of two i32s leaves the exact-integer window. Multiplication is therefore
    // outside the derivation, and this call must NOT be routed.
    let ir = compile_ir(&recursive_module(
        arith(BinaryOp::Mul, 2),
        arith(BinaryOp::Sub, 1),
    ));
    let clone = function_ir(&ir, "$spec_i32(");

    assert!(
        clone.starts_with(
            "define internal preserve_nonecc double \
                 @perry_fn_spec_self_recursion_ts__f$spec_i32(i32"
        ),
        "the clone must still exist, or this asserts nothing:\n{clone}"
    );
    // The `n - 1` edge proves — so the fixture is live — and the `n * 2` edge
    // does not.
    assert_eq!(
        clone
            .matches("call preserve_nonecc double @perry_fn_spec_self_recursion_ts__f$spec_i32(i32")
            .count(),
        1,
        "only the subtracting edge may reach the clone:\n{clone}"
    );
}

#[test]
fn an_unproven_local_recursive_argument_keeps_the_boxed_call() {
    // A parameter the specialized entry did NOT bind as a raw i32 carries no
    // leaf fact, so nothing derived from it can be proven either.
    let ir = compile_ir(&recursive_module(
        Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![Expr::Integer(3)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            right: Box::new(Expr::Integer(1)),
        },
        arith(BinaryOp::Sub, 2),
    ));
    let clone = function_ir(&ir, "$spec_i32(");

    assert!(
        clone.starts_with(
            "define internal preserve_nonecc double \
                 @perry_fn_spec_self_recursion_ts__f$spec_i32(i32"
        ),
        "the clone must still exist, or this asserts nothing:\n{clone}"
    );
    // `f(3)` is a literal site and reaches the clone directly; `f(3) - 1` is
    // not a derivable leaf, so its enclosing call stays boxed. That is one
    // clone call for the literal, one for the `n - 2` edge, and none for the
    // call-result argument.
    assert_eq!(
        clone
            .matches("call preserve_nonecc double @perry_fn_spec_self_recursion_ts__f$spec_i32(i32")
            .count(),
        2,
        "a call-result argument must not be treated as an i32 leaf:\n{clone}"
    );
    assert_eq!(
        clone
            .matches("call double @perry_fn_spec_self_recursion_ts__f(double")
            .count(),
        2,
        "the unprovable edge plus the in-range arm's fallback:\n{clone}"
    );
}

#[test]
fn derived_recursive_number_argument_re_enters_the_guarded_clone() {
    let ir = compile_ir(&guarded_recursive_module(
        arith(BinaryOp::Sub, 1),
        arith(BinaryOp::Sub, 2),
    ));
    let public = function_ir(&ir, "@perry_fn_spec_self_recursion_ts__f(");
    let clone = function_ir(&ir, "$spec_b(");

    // Keep both halves of the subject live: this must be the ordinary boxed
    // clone selected by the public Number guard, not the raw-i32 Tier-A path.
    assert!(
        public.contains(", 32761") && !public.contains("call i32 @js_typed_f64_arg_guard("),
        "the public Number guard is the inline band test:\n{public}"
    );
    assert!(
        clone.starts_with("define internal")
            && clone.contains("double @perry_fn_spec_self_recursion_ts__f$spec_b(double"),
        "expected a guarded boxed clone to specialize:\n{clone}"
    );

    // #8203 gives recursion-participating clones `preserve_nonecc`, which lands
    // between `call` and the return type, so match the call LINE rather than a
    // fixed prefix.
    assert_eq!(
        clone
            .lines()
            .filter(|l| l.contains("call")
                && l.contains("@perry_fn_spec_self_recursion_ts__f$spec_b(double"))
            .count(),
        2,
        "both derived Number arguments must re-enter the guarded clone directly:\n{clone}"
    );
    assert_eq!(
        clone
            .lines()
            .filter(
                |l| l.contains("call") && l.contains("@perry_fn_spec_self_recursion_ts__f(double")
            )
            .count(),
        0,
        "a constructively numeric recursive argument must not re-run the public guard:\n{clone}"
    );
}

#[test]
fn bigint_capable_recursive_argument_keeps_the_public_guard() {
    let ir = compile_ir(&bigint_capable_guarded_recursive_module());
    let public = function_ir(&ir, "@perry_fn_spec_self_recursion_bigint_ts__f(");
    let clone = function_ir(&ir, "$spec_b_b(");

    assert!(
        public.contains(", 32761") && !public.contains("call i32 @js_typed_f64_arg_guard("),
        "the public Number guard is the inline band test:\n{public}"
    );
    assert!(
        clone.starts_with("define internal")
            && clone.contains("double @perry_fn_spec_self_recursion_bigint_ts__f$spec_b_b(double"),
        "expected a guarded boxed clone to specialize:\n{clone}"
    );
    assert_eq!(
        clone
            .matches("call double @perry_fn_spec_self_recursion_bigint_ts__f$spec_b_b(double")
            .count(),
        0,
        "BigInt-capable arithmetic must not bypass the Number guard:\n{clone}"
    );
    assert_eq!(
        clone
            .matches("call double @perry_fn_spec_self_recursion_bigint_ts__f(double")
            .count(),
        1,
        "the unproven recursive edge must retain the public guarded ABI:\n{clone}"
    );
}
