//! IR coverage for the shared numeric guard on fully dynamic `+` trees.

use perry_hir::types::Type;
use perry_hir::{ArgumentsObjectMeta, BinaryOp, Expr, Function, Module, Param, Stmt};

use crate::temp_root_coverage::main_ir_for as ir_for;

const A: u32 = 1;
const B: u32 = 2;
const C: u32 = 3;
const RESULT: u32 = 4;

fn any_local(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(init),
    }
}

fn erased_bigint_local(id: u32, name: &str, value: &str) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::LocalSet(
            id,
            Box::new(Expr::BigInt(value.to_string())),
        )),
    ]
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn arithmetic(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn dynamic_locals() -> Vec<Stmt> {
    vec![
        any_local(A, "a", Expr::Undefined),
        any_local(B, "b", Expr::Undefined),
        any_local(C, "c", Expr::Undefined),
    ]
}

fn result(expr: Expr) -> Stmt {
    Stmt::Let {
        id: RESULT,
        name: "result".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(expr),
    }
}

#[test]
fn three_leaf_dynamic_add_tree_uses_one_shared_guard() {
    let mut body = dynamic_locals();
    body.push(result(add(
        Expr::LocalGet(A),
        add(Expr::LocalGet(B), Expr::LocalGet(C)),
    )));
    let ir = ir_for("three_leaf_dynamic_add_tree", body);

    assert_eq!(
        ir.matches("\nguarded_add.numeric.").count(),
        1,
        "the tree should have one shared numeric block:\n{ir}"
    );
    assert_eq!(
        ir.matches("fadd double").count(),
        2,
        "the fast arm must preserve both additions:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_dynamic_string_or_number_add(")
            .count(),
        2,
        "the cold arm must preserve both dynamic additions:\n{ir}"
    );
}

#[test]
fn two_leaf_dynamic_add_takes_the_guard_too() {
    // This test previously asserted the opposite, on the cost model that "a
    // single dynamic add should not pay for a separate guard diamond" — the
    // guard was thought merely to move the helper behind a branch. It does
    // more: the hot arm becomes an inline `fadd`, and the operands stop going
    // through `lower_rooted_dynamic_binary`, which roots them. Measured on
    // `s += v` with both operands numbers at runtime but neither statically
    // proven, the pair guard is worth 3.44 -> 1.33 ns/op (#9157).
    let mut body = dynamic_locals();
    body.push(result(add(Expr::LocalGet(A), Expr::LocalGet(B))));
    let ir = ir_for("two_leaf_dynamic_add", body);

    assert_eq!(
        ir.matches("\nguarded_add.numeric.").count(),
        1,
        "a two-leaf tree should get one guard diamond:\n{ir}"
    );
    assert_eq!(
        ir.matches("fadd double").count(),
        1,
        "the fast arm must perform the addition inline:\n{ir}"
    );
    assert_eq!(
        ir.matches("call double @js_dynamic_string_or_number_add(")
            .count(),
        1,
        "the cold arm must preserve exact dynamic `+` semantics:\n{ir}"
    );
}

#[test]
fn dynamic_arithmetic_results_are_guarded_before_add() {
    // #9143: for-of element bindings can be `Any` even when their runtime
    // values are BigInts. The nested arithmetic helpers preserve BigInt, so
    // their boxed results must not feed an unconditional native `fadd`.
    let mut body = erased_bigint_local(A, "a", "123456789012345678901234567890");
    body.extend(erased_bigint_local(B, "d", "1000000007"));
    let quotient = arithmetic(BinaryOp::Div, Expr::LocalGet(A), Expr::LocalGet(B));
    let product = arithmetic(BinaryOp::Mul, quotient, Expr::LocalGet(B));
    let remainder = arithmetic(BinaryOp::Mod, Expr::LocalGet(A), Expr::LocalGet(B));
    body.push(result(add(product, remainder)));
    let ir = ir_for("dynamic_bigint_identity_add", body);

    assert!(
        ir.contains("\nguarded_add.numeric."),
        "possibly-BigInt arithmetic results need a runtime number guard:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_string_or_number_add("),
        "the non-number arm must preserve BigInt addition:\n{ir}"
    );
    for helper in ["js_dynamic_div", "js_dynamic_mul", "js_dynamic_mod"] {
        assert!(
            ir.contains(&format!("call double @{helper}(")),
            "the arithmetic subtree must retain {helper}:\n{ir}"
        );
    }
}

// ---- #10904: which leaves the fold may read before an earlier conversion ----
//
// `(x + y) + z` converts `x` and `y` before the specification evaluates `z`.
// The fold reads `z` first, which is unobservable only when nothing that
// conversion can run is able to change `z`. Each fixture below is a
// left-leaning chain; what varies is the LATE leaf. The discriminator is the
// `fadd` count: a fused chain of N leaves has N - 1 of them on its fast arm,
// while a declined root lowers node by node, keeping only the inner pair's
// fused `fadd`.

const P_A: u32 = 11;
const P_B: u32 = 12;
const P_C: u32 = 13;
const P_D: u32 = 14;
const Z: u32 = 15;
const G: u32 = 16;
const ARGS: u32 = 17;

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: name.into(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// The `define` block of `f`, compiled as a module-level function so its
/// locals are stack slots and not module globals.
fn function_ir(f: Function) -> String {
    let mut module = Module::new("add_chain_order.ts");
    let needle = format!("__{}(", f.name);
    module.functions.push(f);
    let ir = String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts())
            .unwrap_or_else(|e| panic!("codegen failed: {e}")),
    )
    .expect("LLVM IR should be UTF-8");
    ir.split("\ndefine ")
        .find(|block| block.lines().next().is_some_and(|l| l.contains(&needle)))
        .unwrap_or_else(|| panic!("no define for {needle}:\n{ir}"))
        .to_string()
}

fn left_chain(leaves: Vec<Expr>) -> Expr {
    let mut it = leaves.into_iter();
    let first = it.next().expect("a chain has leaves");
    it.fold(first, add)
}

fn fadds(ir: &str) -> usize {
    ir.matches("fadd double").count()
}

fn params_abc() -> Vec<Param> {
    vec![param(P_A, "a"), param(P_B, "b"), param(P_C, "c")]
}

#[test]
fn left_chain_over_parameters_keeps_one_shared_guard() {
    // function f(a, b, c, d) { return a + b + c + d }: every late leaf is a
    // parameter nothing else can write, so the fold is unobservable.
    let chain = left_chain(vec![
        Expr::LocalGet(P_A),
        Expr::LocalGet(P_B),
        Expr::LocalGet(P_C),
        Expr::LocalGet(P_D),
    ]);
    let mut params = params_abc();
    params.push(param(P_D, "d"));
    let ir = function_ir(function(
        "params_chain",
        params,
        vec![Stmt::Return(Some(chain))],
    ));
    assert_eq!(
        ir.matches("\nguarded_add.numeric.").count(),
        1,
        "one shared guard for the whole chain:\n{ir}"
    );
    assert_eq!(
        fadds(&ir),
        3,
        "the fast arm keeps all three additions:\n{ir}"
    );
}

#[test]
fn left_chain_over_an_assigned_uncaptured_let_keeps_the_fold() {
    // function f(a, b) { let z = 1; z = 2; return a + b + z }: `let`, and
    // assigned, but only by this activation.
    let body = vec![
        Stmt::Let {
            id: Z,
            name: "z".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(1)),
        },
        Stmt::Expr(Expr::LocalSet(Z, Box::new(Expr::Integer(2)))),
        Stmt::Return(Some(left_chain(vec![
            Expr::LocalGet(P_A),
            Expr::LocalGet(P_B),
            Expr::LocalGet(Z),
        ]))),
    ];
    let ir = function_ir(function(
        "uncaptured_let",
        vec![param(P_A, "a"), param(P_B, "b")],
        body,
    ));
    assert_eq!(fadds(&ir), 2, "the chain stays fused:\n{ir}");
}

#[test]
fn left_chain_declines_when_a_late_leaf_is_a_property_read() {
    // function f(a, b, o) { return a + b + o.x }: `a`'s valueOf can assign o.x.
    let chain = left_chain(vec![
        Expr::LocalGet(P_A),
        Expr::LocalGet(P_B),
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(P_C)),
            property: "x".into(),
            byte_offset: 0,
        },
    ]);
    let ir = function_ir(function(
        "property_late",
        params_abc(),
        vec![Stmt::Return(Some(chain))],
    ));
    assert_eq!(
        fadds(&ir),
        1,
        "only the inner `a + b` may fuse; the root converts before o.x:\n{ir}"
    );
}

#[test]
fn left_chain_declines_when_a_late_leaf_is_captured_and_assigned() {
    // function f(a, b) { let z = 1; const g = () => { z = 100 }; return a + b + z }
    // `a`'s valueOf can call g. This is the must-fail control for the
    // local exemption: `z` is a local, but not one only this frame writes.
    let body = vec![
        Stmt::Let {
            id: Z,
            name: "z".into(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(1)),
        },
        Stmt::Let {
            id: G,
            name: "g".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 2,
                params: Vec::new(),
                return_type: Type::Any,
                body: vec![Stmt::Expr(Expr::LocalSet(Z, Box::new(Expr::Integer(100))))],
                captures: vec![Z],
                mutable_captures: vec![Z],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }),
        },
        Stmt::Return(Some(left_chain(vec![
            Expr::LocalGet(P_A),
            Expr::LocalGet(P_B),
            Expr::LocalGet(Z),
        ]))),
    ];
    let ir = function_ir(function(
        "captured_late",
        vec![param(P_A, "a"), param(P_B, "b")],
        body,
    ));
    assert!(
        ir.contains("call i64 @js_box_alloc_bits("),
        "premise: z is a boxed cell\n{ir}"
    );
    assert_eq!(fadds(&ir), 1, "the root must not read z early:\n{ir}");
}

#[test]
fn left_chain_declines_when_a_late_leaf_is_a_mapped_arguments_parameter() {
    // Sloppy function f(a, b, c) { return a + b + c } with `arguments` mapped
    // onto c: a valueOf holding the Arguments object can assign c through it.
    let mut arguments = param(ARGS, "arguments");
    arguments.arguments_object = Some(ArgumentsObjectMeta {
        strict: false,
        simple_parameters: true,
        mapped_parameter_ids: vec![(2, P_C)],
        restricted_callee: false,
    });
    let mut params = params_abc();
    params.push(arguments);
    let mut f = function(
        "mapped_late",
        params,
        vec![Stmt::Return(Some(left_chain(vec![
            Expr::LocalGet(P_A),
            Expr::LocalGet(P_B),
            Expr::LocalGet(P_C),
        ])))],
    );
    f.is_strict = false;
    let ir = function_ir(f);
    assert_eq!(fadds(&ir), 1, "the root must not read c early:\n{ir}");
}

#[test]
fn left_chain_over_top_level_slots_keeps_the_fold() {
    // Top-level `let a, b, c; a + b + c` with NO function referencing them.
    // perry mints a module global only for a binding some function or closure
    // references, so these stay slots in `main`: only `main` can write `c`,
    // and reading it early is unobservable.
    let mut body = dynamic_locals();
    body.push(result(left_chain(vec![
        Expr::LocalGet(A),
        Expr::LocalGet(B),
        Expr::LocalGet(C),
    ])));
    let ir = ir_for("top_level_slots", body);
    assert!(
        !ir.contains("@perry_global_"),
        "premise: a, b and c are slots in main\n{ir}"
    );
    assert_eq!(fadds(&ir), 2, "the chain stays fused:\n{ir}");
}

#[test]
fn left_chain_declines_when_a_late_leaf_is_a_module_global() {
    // Top-level `let a, b, c; function bump() { c = 100 } a + b + c`. `bump`
    // assigns the module global `c` without capturing it, so capture analysis
    // says nothing about it, and `a`'s valueOf can call `bump`.
    let mut body = dynamic_locals();
    body.push(result(left_chain(vec![
        Expr::LocalGet(A),
        Expr::LocalGet(B),
        Expr::LocalGet(C),
    ])));
    let mut module = crate::temp_root_coverage::module_with_init("module_global_late", body);
    module.functions.push(function(
        "bump",
        Vec::new(),
        vec![
            Stmt::Expr(Expr::LocalSet(C, Box::new(Expr::Integer(100)))),
            Stmt::Return(Some(Expr::Undefined)),
        ],
    ));
    let ir = String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts())
            .unwrap_or_else(|e| panic!("codegen failed: {e}")),
    )
    .expect("LLVM IR should be UTF-8");
    let main = crate::testing::root_slots::function_slice(&ir, "main");
    assert!(
        main.contains("@perry_global_"),
        "premise: c is a module global\n{main}"
    );
    assert_eq!(fadds(main), 1, "the root must not read c early:\n{main}");
}

#[test]
fn declared_number_left_chain_declines_when_a_late_leaf_is_an_element_read() {
    // function f(a: number[]) { return a[0] + a[1] + a[2] }. The element type
    // is only declared, so the tree is `both_numeric` and reaches the fold
    // through the declared-only entry, not the dynamic one. `a[0]` can still
    // hold an object whose valueOf assigns `a[2]`: on the unfixed compiler
    // this printed 6 where node prints 103.
    let mut arr = param(P_A, "a");
    arr.ty = Type::Array(Box::new(Type::Number));
    let elem = |i: i64| Expr::IndexGet {
        object: Box::new(Expr::LocalGet(P_A)),
        index: Box::new(Expr::Integer(i)),
    };
    let ir = function_ir(function(
        "declared_elements",
        vec![arr],
        vec![Stmt::Return(Some(left_chain(vec![
            elem(0),
            elem(1),
            elem(2),
        ])))],
    ));
    assert!(
        ir.contains("\nguarded_add.numeric."),
        "premise: the declared-number chain reaches the fold\n{ir}"
    );
    assert_eq!(fadds(&ir), 1, "the root must not read a[2] early:\n{ir}");
}
