//! Hit-path lowering census: operations that used to call a runtime helper on
//! every execution are decided inline, and each helper survives only on the arm
//! that genuinely needs it.
//!
//! Every positive assertion is paired with the fallback that must remain, so a
//! fast path that widened past its exact predicate — or one that stopped being
//! reached — fails here rather than only in the gap suite.

use crate::temp_root_coverage::main_ir_for;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, Param, Stmt, SwitchCase, UnaryOp, UpdateOp,
};

const X: u32 = 1;
const Y: u32 = 2;
const R: u32 = 3;

fn let_any(id: u32, name: &str, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

/// `let x: any = undefined; let y: any = undefined; <stmts>` as module init.
fn init_ir(name: &str, stmts: Vec<Stmt>) -> String {
    let mut init = vec![
        let_any(X, "x", Expr::Undefined),
        let_any(Y, "y", Expr::Undefined),
    ];
    init.extend(stmts);
    main_ir_for(name, init)
}

fn typeof_cmp(op: CompareOp, operand: Expr, literal: &str) -> Stmt {
    let_any(
        R,
        "r",
        Expr::Compare {
            op,
            left: Box::new(Expr::TypeOf(Box::new(operand))),
            right: Box::new(Expr::String(literal.to_string())),
        },
    )
}

/// Body of the block whose label starts with `label` (up to the next label).
fn block_body(ir: &str, label: &str) -> String {
    super::class_field_barrier_tests::block_body(ir, label)
        .unwrap_or_else(|| panic!("no block {label} in:\n{ir}"))
}

fn function_slice<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let needle = format!("@{symbol}(");
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let line_end = ir[*index..].find('\n').map_or(ir.len(), |o| index + o);
            ir[*index..line_end].contains(&needle)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function {symbol}:\n{ir}"));
    let end = ir[start..].find("\n}").map_or(ir.len(), |o| start + o);
    &ir[start..end]
}

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: u32, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// Compile `functions` with one unknown call each from module init, so every
/// function gets a public entry that unknown callers reach.
fn module_ir(module_name: &str, functions: Vec<Function>) -> String {
    let mut module = Module::new(module_name);
    for f in &functions {
        module.init.push(Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(f.id)),
            args: f.params.iter().map(|_| Expr::Undefined).collect(),
            type_args: Vec::new(),
            byte_offset: 0,
        }));
    }
    module.functions = functions;
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

// ---------------------------------------------------------------------------
// typeof
// ---------------------------------------------------------------------------

#[test]
fn typeof_string_undefined_and_boolean_are_decided_without_the_classifier() {
    for (i, literal) in ["string", "undefined", "boolean"].into_iter().enumerate() {
        let ir = init_ir(
            &format!("typeof_exact_{i}"),
            vec![typeof_cmp(CompareOp::Eq, Expr::LocalGet(X), literal)],
        );
        assert!(!ir.contains("@js_value_typeof_tag("), "{literal}:\n{ir}");
        assert!(!ir.contains("@js_value_typeof("), "{literal}:\n{ir}");
        assert!(!ir.contains("@js_string_equals("), "{literal}:\n{ir}");
    }
}

#[test]
fn typeof_object_function_symbol_bigint_ask_the_integer_classifier_once() {
    for (i, literal) in ["object", "function", "symbol", "bigint"]
        .into_iter()
        .enumerate()
    {
        let ir = init_ir(
            &format!("typeof_classifier_{i}"),
            vec![typeof_cmp(CompareOp::Ne, Expr::LocalGet(X), literal)],
        );
        assert_eq!(
            ir.matches("call i32 @js_value_typeof_tag(").count(),
            1,
            "{literal}: exactly one classifier call:\n{ir}"
        );
        assert!(!ir.contains("@js_value_typeof("), "{literal}:\n{ir}");
        assert!(!ir.contains("@js_string_equals("), "{literal}:\n{ir}");
    }
}

#[test]
fn typeof_of_a_property_read_and_loose_equality_use_the_tag_route() {
    let property = Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(X)),
        property: "a".to_string(),
        byte_offset: 0,
    };
    let ir = init_ir(
        "typeof_property_read",
        vec![typeof_cmp(CompareOp::LooseEq, property, "string")],
    );
    assert!(!ir.contains("@js_value_typeof("), "{ir}");
    assert!(!ir.contains("@js_loose_eq("), "{ir}");
}

// ---------------------------------------------------------------------------
// switch
// ---------------------------------------------------------------------------

fn switch_on(discriminant: Expr, tests: Vec<Expr>) -> Stmt {
    let mut cases: Vec<SwitchCase> = tests
        .into_iter()
        .enumerate()
        .map(|(i, test)| SwitchCase {
            test: Some(test),
            body: vec![
                Stmt::Expr(Expr::LocalSet(R, Box::new(Expr::Number(i as f64)))),
                Stmt::Break,
            ],
        })
        .collect();
    cases.push(SwitchCase {
        test: None,
        body: vec![Stmt::Expr(Expr::LocalSet(R, Box::new(Expr::Number(-1.0))))],
    });
    Stmt::Switch {
        discriminant,
        cases,
    }
}

#[test]
fn a_short_numeric_switch_dispatches_by_fcmp() {
    let ir = init_ir(
        "switch_numeric_short",
        vec![
            let_any(R, "r", Expr::Undefined),
            switch_on(
                Expr::LocalGet(X),
                vec![Expr::Integer(0), Expr::Integer(1), Expr::Integer(2)],
            ),
        ],
    );
    assert!(ir.contains("switch.bst"), "{ir}");
    assert!(!ir.contains("@js_switch_strict_equals("), "{ir}");
}

#[test]
fn literal_cases_are_tested_inline_and_other_cases_keep_the_helper() {
    let ir = init_ir(
        "switch_string_literals",
        vec![
            let_any(R, "r", Expr::Undefined),
            switch_on(
                Expr::LocalGet(X),
                vec![
                    Expr::String("alpha".to_string()),
                    Expr::String("b".to_string()),
                    Expr::Integer(3),
                    Expr::Bool(true),
                    Expr::Null,
                ],
            ),
        ],
    );
    assert!(ir.contains("streqlit.tag"), "{ir}");
    assert!(!ir.contains("@js_switch_strict_equals("), "{ir}");

    let ir = init_ir(
        "switch_non_literal_case",
        vec![
            let_any(R, "r", Expr::Undefined),
            switch_on(
                Expr::LocalGet(X),
                vec![Expr::String("alpha".to_string()), Expr::LocalGet(Y)],
            ),
        ],
    );
    assert_eq!(
        ir.matches("call i32 @js_switch_strict_equals(").count(),
        1,
        "only the non-literal case keeps the runtime helper:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// Math.max / Math.min
// ---------------------------------------------------------------------------

#[test]
fn two_argument_math_max_min_use_intrinsics_behind_a_plain_double_test() {
    let ir = init_ir(
        "math_minmax_any",
        vec![
            let_any(
                R,
                "r",
                Expr::MathMax(vec![Expr::LocalGet(X), Expr::LocalGet(Y)]),
            ),
            let_any(
                10,
                "s",
                Expr::MathMin(vec![Expr::LocalGet(X), Expr::LocalGet(Y)]),
            ),
        ],
    );
    assert!(
        block_body(&ir, "math.minmax.fast.").contains("@llvm.maximum.f64(")
            || ir.contains("@llvm.maximum.f64("),
        "{ir}"
    );
    assert!(ir.contains("@llvm.minimum.f64("), "{ir}");
    assert!(
        ir.contains("@js_math_max2(") && ir.contains("@js_math_min2("),
        "tagged operands must keep the coercing helpers:\n{ir}"
    );

    let ir = init_ir(
        "math_max_numbers",
        vec![let_any(
            R,
            "r",
            Expr::MathMax(vec![Expr::Number(1.5), Expr::Number(-0.0)]),
        )],
    );
    assert!(
        !ir.contains("@js_math_max2("),
        "proven numbers need no helper:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// module-global update
// ---------------------------------------------------------------------------

#[test]
fn increment_has_a_call_free_double_arm() {
    let ir = init_ir(
        "local_update",
        vec![Stmt::Expr(Expr::Update {
            id: X,
            op: UpdateOp::Increment,
            prefix: false,
        })],
    );
    let fast = block_body(&ir, "update.num.fast.");
    assert!(
        !fast.contains("call "),
        "the double arm must not call:\n{ir}"
    );
    let slow = block_body(&ir, "update.num.slow.");
    assert!(slow.contains("@js_to_numeric("), "{ir}");
    assert!(slow.contains("@js_numeric_step("), "{ir}");
}

#[test]
fn module_global_increment_gates_its_root_barrier() {
    let mut module = Module::new("global_update.ts");
    module.init.push(let_any(X, "g", Expr::Undefined));
    let bump = function(
        1,
        "bump",
        Vec::new(),
        vec![Stmt::Expr(Expr::Update {
            id: X,
            op: UpdateOp::Increment,
            prefix: true,
        })],
    );
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    module.functions.push(bump);
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let body = function_slice(&ir, "perry_fn_global_update_ts__bump");
    let fast = block_body(body, "update.num.fast.");
    assert!(
        !fast.contains("call "),
        "the double arm must not call:\n{body}"
    );
    assert!(
        body.contains("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT")
            && body.contains("@js_write_barrier_root_nanbox("),
        "the coercing arm's root store keeps its gated barrier:\n{body}"
    );
}

// ---------------------------------------------------------------------------
// typed and specialized public entries
// ---------------------------------------------------------------------------

#[test]
fn typed_public_entries_guard_inline_and_test_plain_doubles_first() {
    let add = function(
        1,
        "add",
        vec![param(10, Type::Number), param(11, Type::Number)],
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::LocalGet(11)),
        }))],
    );
    let not = function(
        2,
        "not",
        vec![param(20, Type::Boolean)],
        vec![Stmt::Return(Some(Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::LocalGet(20)),
        }))],
    );
    let ir = module_ir("typed_entry.ts", vec![add, not]);
    let add_entry = function_slice(&ir, "perry_fn_typed_entry_ts__add");
    assert!(!add_entry.contains("call i32 @js_typed_"), "{add_entry}");
    assert!(
        add_entry.contains("9221401712017801215"),
        "tier 1 must be the plain-double compare:\n{add_entry}"
    );
    assert!(add_entry.contains("_public.int32"), "{add_entry}");
    assert_eq!(
        add_entry.matches("add$spec_").count() + add_entry.matches("add$typed_").count(),
        1,
        "both tiers share the single clone call site:\n{add_entry}"
    );
    let not_entry = function_slice(&ir, "perry_fn_typed_entry_ts__not");
    assert!(!not_entry.contains("@js_typed_i1_arg"), "{not_entry}");
}

#[test]
fn a_guarded_boolean_parameter_tests_truthiness_by_tag_identity() {
    let pick = function(
        1,
        "pick",
        vec![param(10, Type::Any), param(11, Type::Boolean)],
        vec![
            Stmt::If {
                condition: Expr::LocalGet(11),
                then_branch: vec![Stmt::Return(Some(Expr::LocalGet(10)))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Number(0.0))),
        ],
    );
    let ir = module_ir("bool_truthy.ts", vec![pick]);
    let spec = function_slice(&ir, "perry_fn_bool_truthy_ts__pick$spec_b_b");
    assert!(!spec.contains("truthy.tag"), "{spec}");
    assert!(!spec.contains("@js_is_truthy("), "{spec}");
    let generic = function_slice(&ir, "perry_fn_bool_truthy_ts__pick$generic");
    assert!(
        generic.contains("truthy.tag"),
        "the unguarded body keeps the total predicate:\n{generic}"
    );
}

#[test]
fn a_clone_identical_to_its_generic_body_drops_the_entry_guard() {
    // `o.label` on an object-typed parameter: the class-field/IC read needs no
    // parameter proof, so the guarded clone lowers to the generic body.
    let payload = Type::Object(perry_hir::types::ObjectType {
        name: None,
        properties: std::collections::HashMap::from([(
            "label".to_string(),
            perry_hir::types::PropertyInfo {
                ty: Type::Number,
                optional: false,
                readonly: false,
            },
        )]),
        property_order: Some(vec!["label".to_string()]),
        index_signature: None,
    });
    let read = function(
        1,
        "read",
        vec![param(10, payload)],
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(10)),
            property: "label".to_string(),
            byte_offset: 0,
        }))],
    );
    let ir = module_ir("vacuous_guard.ts", vec![read]);
    let entry = function_slice(&ir, "perry_fn_vacuous_guard_ts__read");
    assert!(!entry.contains("@js_param_type_guard("), "{entry}");
    assert!(entry.contains("read$generic("), "{entry}");
}
