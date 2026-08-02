//! #7238 — a `number`-typed function body must not be re-emitted in integer
//! registers behind an `fptosi`/`sitofp` shim.
//!
//! `emit_i64_specializations` used to do exactly that for any function whose
//! return type and every parameter type was `number` and whose body was
//! Add/Sub/Mul/Compare/Conditional over locals, params, integer literals and
//! self-calls. Two halves of the contract went unproven:
//!
//! * **Argument domain.** A `number` parameter is an IEEE-754 double. The
//!   wrapper's `fptosi double %arg to i64` truncated a fractional argument on
//!   entry, and `sitofp` on the way out could not represent a fractional
//!   result at all.
//! * **Magnitude.** i64 `add`/`sub`/`mul` are exact; JS rounds to the nearest
//!   double at *every* operator. They agree only while each intermediate
//!   satisfies `|v| <= 2^53`.
//!
//! Neither is statically provable for the self-recursive bodies the pass
//! existed to serve — a parameter fed by its own recursive call argument has no
//! bound, so #7237's `i32_chain_magnitude_bits` has no bounded leaf to measure
//! from. The pass was removed; these tests are the guard against reintroducing
//! it, and against losing the sound specializations that now claim the same
//! functions.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: crate::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn number_param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn number_fn(id: u32, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

fn module_with(functions: Vec<Function>) -> Module {
    Module {
        name: "number_exactness.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions,
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
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn emitted_ir(functions: Vec<Function>) -> String {
    String::from_utf8(compile_module(&module_with(functions), ir_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// Slice out the `define`d function whose signature line contains `marker`.
fn function_ir<'a>(ir: &'a str, marker: &str) -> Option<&'a str> {
    let start = ir
        .match_indices("define ")
        .find(|(i, _)| {
            let line_end = ir[*i..].find('\n').map(|n| i + n).unwrap_or(ir.len());
            ir[*i..line_end].contains(marker)
        })
        .map(|(i, _)| i)?;
    let end = ir[start..].find("\n}")? + start;
    Some(&ir[start..end])
}

/// `function fib(n: number): number { if (n <= 1) return n; return fib(n-1) + fib(n-2); }`
fn fib_fn() -> Function {
    let call = |k: i64| Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::Integer(k)),
        }],
        type_args: Vec::new(),
        byte_offset: 0,
    };
    number_fn(
        1,
        "fib",
        vec![number_param(10, "n")],
        vec![
            Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Le,
                    left: Box::new(Expr::LocalGet(10)),
                    right: Box::new(Expr::Integer(1)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::LocalGet(10)))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(call(1)),
                right: Box::new(call(2)),
            })),
        ],
    )
}

/// `function grow(n: number, acc: number): number {
///    return n === 0 ? acc : grow(n - 1, acc * 3 + 1); }`
/// — the issue's overflow repro: `grow(40, 1)` wrapped past 2^63 and went
/// negative under the exact i64 chain.
fn grow_fn() -> Function {
    number_fn(
        1,
        "grow",
        vec![number_param(10, "n"), number_param(11, "acc")],
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(Expr::LocalGet(11)),
            else_expr: Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![
                    Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(Expr::LocalGet(10)),
                        right: Box::new(Expr::Integer(1)),
                    },
                    Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::Binary {
                            op: BinaryOp::Mul,
                            left: Box::new(Expr::LocalGet(11)),
                            right: Box::new(Expr::Integer(3)),
                        }),
                        right: Box::new(Expr::Integer(1)),
                    },
                ],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        }))],
    )
}

/// `function add(a: number, b: number): number { return a + b; }` — the
/// straight-line shape the pass also claimed, which is now free to take the
/// sound typed-f64 clone instead.
fn add_fn() -> Function {
    number_fn(
        1,
        "add",
        vec![number_param(10, "a"), number_param(11, "b")],
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::LocalGet(11)),
        }))],
    )
}

const NO_I64_BODY: &str = "no `number` function may be re-emitted as an i64 body";

#[test]
fn self_recursive_number_function_gets_no_i64_body() {
    for f in [fib_fn(), grow_fn()] {
        let name = f.name.clone();
        let ir = emitted_ir(vec![f]);
        assert!(
            !ir.contains(&format!("{name}_i64")),
            "{NO_I64_BODY}, but `{name}` still has one:\n{ir}"
        );
        assert!(
            !ir.contains("alwaysinline") || !ir.contains("define i64 @perry_fn_number_exactness"),
            "{NO_I64_BODY}, but an i64 user-function body was emitted:\n{ir}"
        );
    }
}

#[test]
fn self_recursive_number_function_keeps_a_double_body() {
    for f in [fib_fn(), grow_fn()] {
        let name = f.name.clone();
        let ir = emitted_ir(vec![f]);
        let symbol = format!("@perry_fn_number_exactness_ts__{name}(");
        let body = function_ir(&ir, &symbol)
            .unwrap_or_else(|| panic!("public f64 body for `{name}` must be emitted:\n{ir}"));
        // The removed wrapper was exactly `fptosi` → `call i64` → `sitofp`,
        // with no other instruction. Its argument truncation is the second of
        // the two defects and is what this asserts is gone.
        assert!(
            !body.contains("fptosi double %arg"),
            "`{name}`'s public body must not truncate its arguments on entry:\n{body}"
        );
        assert!(
            body.contains("call double @perry_fn_number_exactness_ts__"),
            "`{name}` must still recurse through a double-typed body:\n{body}"
        );
    }
}

/// The pass suppressed the ordinary f64 body *and* the typed-ABI clone
/// families (`typed_f64_functions` and friends were retained minus the
/// i64-specialized set). Removing it hands these functions back to the sound
/// specializer, so coverage moves rather than disappears.
#[test]
fn straight_line_number_function_takes_the_typed_f64_clone() {
    let ir = emitted_ir(vec![add_fn()]);
    assert!(!ir.contains("add_i64"), "{NO_I64_BODY}:\n{ir}");
    assert!(
        ir.contains("__typed_f64"),
        "the typed-f64 clone must now be reachable for a plain `a + b`:\n{ir}"
    );
}

/// A fractional `Number` literal in the body was already rejected by the old
/// gate (#6221). It must stay rejected — and now for the whole class of
/// reasons, not just that one literal.
#[test]
fn fractional_literal_body_stays_on_the_double_path() {
    let f = number_fn(
        1,
        "halfDown",
        vec![number_param(10, "n")],
        vec![Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Integer(0)),
            }),
            then_expr: Box::new(Expr::Number(0.5)),
            else_expr: Box::new(Expr::Call {
                callee: Box::new(Expr::FuncRef(1)),
                args: vec![Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::LocalGet(10)),
                    right: Box::new(Expr::Integer(1)),
                }],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        }))],
    );
    let ir = emitted_ir(vec![f]);
    assert!(!ir.contains("halfDown_i64"), "{NO_I64_BODY}:\n{ir}");
}
