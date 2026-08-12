//! #7837 — a declared `string` is not a proof that the value is a string, so
//! it may not pick the `+` OPERATOR.
//!
//! `is_definitely_string_expr`'s `LocalGet` arm trusts `let s: string`, and
//! Perry does not enforce annotations at runtime (CLAUDE.md, Known
//! Limitations). `const s: string = (42 as any); s + 7` therefore selected the
//! one-sided concat lowering and printed `"427"` where Node prints `49`.
//!
//! The one-sided arm is the one that cannot be repaired inside the runtime:
//! codegen unboxes the string operand to a `StringHeader*` before the call, so
//! `js_string_concat_value` never sees a tag to test. The fix hands it the
//! NaN-box instead (`js_string_add_value` / `js_value_add_string`), which is
//! why these tests assert on WHICH helper is emitted.
//!
//! Every test comes in a pair: the lie must be guarded, and the neighbouring
//! shape that carries a real proof must NOT be — a fix that routed everything
//! through the dynamic helper would pass the first half and fail the second,
//! and would have cost `"item_" + i` its fused single-allocation concat.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    }
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn probe_fn(params: Vec<Param>, body: Expr) -> Function {
    Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(body))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

fn module_with(function: Function) -> Module {
    Module {
        name: "declared_string_add.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![function],
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn ir(params: Vec<Param>, body: Expr) -> String {
    let module = module_with(probe_fn(params, body));
    String::from_utf8(compile_module(&module, ir_opts()).unwrap()).expect("LLVM IR is UTF-8")
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn str_param() -> Param {
    param(1, "s", Type::String)
}

// ---------------------------------------------------------------- one-sided

#[test]
fn declared_string_on_the_left_is_guarded() {
    // `function probe(s: string) { return s + 7; }`
    let ir = ir(vec![str_param()], add(Expr::LocalGet(1), Expr::Number(7.0)));
    assert!(
        ir.contains("call double @js_string_add_value("),
        "a declared-only `string` operand must hand the NaN-box to the \
         tag-dispatching helper, or `s + 7` on a slot holding 42 prints \
         \"427\" instead of 49:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_string_concat_value("),
        "...and must NOT also emit the pre-unboxed fused concat, whose \
         `StringHeader*` argument is exactly what loses the tag:\n{ir}"
    );
}

#[test]
fn declared_string_on_the_right_is_guarded() {
    // `function probe(s: string) { return 7 + s; }`
    let ir = ir(vec![str_param()], add(Expr::Number(7.0), Expr::LocalGet(1)));
    assert!(
        ir.contains("call double @js_value_add_string("),
        "the mirrored operand order needs the mirrored guard:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_value_concat_string("),
        "the pre-unboxed fused concat must not survive alongside it:\n{ir}"
    );
}

#[test]
fn a_string_literal_operand_keeps_the_fused_concat() {
    // `function probe(n: number) { return "item_" + n; }` — the hot
    // `"prefix" + i` shape. A literal IS a proof about the bits, so `+` is
    // concat whatever `n` holds and there is nothing to test at runtime.
    let ir = ir(
        vec![param(1, "n", Type::Number)],
        add(Expr::String("item_".to_string()), Expr::LocalGet(1)),
    );
    assert!(
        ir.contains("call i64 @js_string_concat_value("),
        "a proven string must keep the fused single-allocation concat — the \
         guard is for claims, not for proofs:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_string_add_value("),
        "and must pay no tag test at all:\n{ir}"
    );
}

#[test]
fn a_coerced_operand_keeps_the_fused_concat() {
    // `String(x) + n` — `js_string_coerce` always allocates a heap
    // `StringHeader`, so this is a proof exactly like a literal.
    let ir = ir(
        vec![param(1, "n", Type::Number)],
        add(
            Expr::StringCoerce(Box::new(Expr::LocalGet(1))),
            Expr::LocalGet(1),
        ),
    );
    assert!(
        ir.contains("call i64 @js_string_concat_value(")
            && !ir.contains("call double @js_string_add_value("),
        "`String(x)` constructs a string; it is not an annotation:\n{ir}"
    );
}

#[test]
fn a_string_method_on_a_proven_receiver_keeps_the_fused_concat() {
    // `"ab".toUpperCase() + n`. The method-name arm is a proof only because
    // the RECEIVER is one — see the next test for why that matters.
    let ir = ir(
        vec![param(1, "n", Type::Number)],
        add(
            Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::String("ab".to_string())),
                    property: "toUpperCase".to_string(),
                    byte_offset: 0,
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
            Expr::LocalGet(1),
        ),
    );
    assert!(
        !ir.contains("call double @js_string_add_value("),
        "a string method on a string literal returns a string:\n{ir}"
    );
}

#[test]
fn a_string_method_name_on_an_unproven_receiver_is_guarded() {
    // `is_definitely_string_expr` matches `.slice(…)` on the METHOD NAME with
    // no look at the receiver, so `arr.slice(0) + 7` claimed a string and
    // printed "" — the array operand was decoded as an empty string. The name
    // is a guess about the receiver's type, which is the same kind of evidence
    // as an annotation.
    let ir = ir(
        vec![param(1, "a", Type::Any)],
        add(
            Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "slice".to_string(),
                    byte_offset: 0,
                }),
                args: vec![Expr::Number(0.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            },
            Expr::Number(7.0),
        ),
    );
    assert!(
        ir.contains("call double @js_string_add_value("),
        "`Array.prototype.slice` returns an array; the name proves nothing \
         about the receiver:\n{ir}"
    );
}

// -------------------------------------------------------------- chain fold

#[test]
fn a_chain_whose_head_pair_is_all_declared_does_not_fold() {
    // `s + t + "x"`. `js_string_concat_chain` formats EVERY part as a string,
    // so it reproduces the source tree only when `s + t` really concatenates.
    // With both holding numbers Node answers "141x"; the fold answers
    // "4299x".
    let ir = ir(
        vec![str_param(), param(2, "t", Type::String)],
        add(
            add(Expr::LocalGet(1), Expr::LocalGet(2)),
            Expr::String("x".to_string()),
        ),
    );
    assert!(
        !ir.contains("call i64 @js_string_concat_chain("),
        "the head pair carries no proof, so the N-way fold is unsound \
         here:\n{ir}"
    );
}

#[test]
fn a_chain_led_by_a_literal_still_folds() {
    // `"x" + s + t`. The first node concatenates whatever `s` holds, so its
    // result is a string and every later `+` concatenates too — the fold is
    // exact, and this is the CSV / log-line shape it exists for.
    let ir = ir(
        vec![str_param(), param(2, "t", Type::String)],
        add(
            add(Expr::String("x".to_string()), Expr::LocalGet(1)),
            Expr::LocalGet(2),
        ),
    );
    assert!(
        ir.contains("call i64 @js_string_concat_chain("),
        "a proven string in the head pair keeps the N-way fold:\n{ir}"
    );
}

#[test]
fn a_chain_whose_second_part_is_proven_still_folds() {
    // `s + "," + t` — the proof may sit on either side of the first node.
    let ir = ir(
        vec![str_param(), param(2, "t", Type::String)],
        add(
            add(Expr::LocalGet(1), Expr::String(",".to_string())),
            Expr::LocalGet(2),
        ),
    );
    assert!(
        ir.contains("call i64 @js_string_concat_chain("),
        "`s + \",\" + t` concatenates at every node whatever `s` holds:\n{ir}"
    );
}

// ------------------------------------------------------- untouched tiers

#[test]
fn two_numeric_operands_are_untouched() {
    // The guard must not leak into arithmetic: `a + b` on two `number`s stays
    // a bare `fadd` with no string helper anywhere near it.
    let ir = ir(
        vec![param(1, "a", Type::Number), param(2, "b", Type::Number)],
        add(Expr::LocalGet(1), Expr::LocalGet(2)),
    );
    assert!(
        !ir.contains("call double @js_string_add_value(")
            && !ir.contains("call double @js_value_add_string("),
        "numeric `+` must not acquire a string guard:\n{ir}"
    );
}
