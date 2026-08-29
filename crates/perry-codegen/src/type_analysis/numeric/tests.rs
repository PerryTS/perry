//! #6511 — cargo-test-visible coverage for the Math.*-result arithmetic
//! routing (the integration twin lives in
//! `tests/native_proof_regressions/math_mul_fastpath.rs` and only runs on
//! nightly/tag workflows). A multiply of `Math.*` results must stay on the
//! inline `fmul` fast path; a possibly-object operand must keep the
//! BigInt-aware `js_dynamic_mul` routing from #5970.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UpdateOp,
};

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
        namespace_member_nested: Vec::new(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
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

fn probe_module(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params,
            return_type: Type::Number,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
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

fn emitted_ir(module: Module) -> String {
    String::from_utf8(compile_module(&module, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

fn number_let(id: u32, name: &str, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable,
        init: Some(init),
    }
}

fn mul(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[test]
fn math_result_multiply_stays_inline_fmul() {
    // The #6511 repro's accumulator-loop shape, with a call-free MathSin
    // operand (`Math.sin(i)`, not the repro's `i * 0.001`) so the only
    // `fmul` in the function is the Math-result multiply under test:
    // `for (i = 0; i < 64; i++) acc += Math.sqrt(i) * Math.sin(i);`
    let ir = emitted_ir(probe_module(
        "math_result_multiply_unit.ts",
        Vec::new(),
        vec![
            number_let(1, "acc", true, Expr::Integer(0)),
            number_let(3, "iterations", false, Expr::Integer(64)),
            Stmt::For {
                init: Some(Box::new(number_let(2, "i", true, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::LocalGet(3)),
                }),
                update: Some(Expr::Update {
                    id: 2,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(1)),
                        right: Box::new(mul(
                            Expr::MathSqrt(Box::new(Expr::LocalGet(2))),
                            Expr::MathSin(Box::new(Expr::LocalGet(2))),
                        )),
                    }),
                ))],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));
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

// ---------------------------------------------------------------------------
// #8105 — a REASSIGNED numeric accumulator must reach the inline fast path.
//
// Every other `LocalGet` numeric proof needs the local to be write-once
// (`stable_local_type_proof` answers `None` once it is reassigned) or is an
// integer-range fact, so `15_mandelbrot`'s `let x = 0.0; … x = xtemp;` had no
// numeric proof at all and every `x * x` bailed to `js_dynamic_mul` — 1.71 G
// instructions for the benchmark, against 0.21 G once the proof lands.
//
// These assert on emitted IR rather than on the predicate, and they come in a
// PAIR: the positive case alone would pass against an analysis that admits
// every local, so the negative case pins the discriminating quantity.
// ---------------------------------------------------------------------------

/// Build the `15_mandelbrot` inner-loop shape over two reassigned locals.
///
/// `let x = 0.0; let y = 0.0; while (i < n) { const t = x * x - y * y; y = 2.0
/// * x * y; x = t; i++; } return x;`
fn mandelbrot_shaped_body(seed_x: Expr, seed_y: Expr) -> Vec<Stmt> {
    vec![
        number_let(10, "x", true, seed_x),
        number_let(11, "y", true, seed_y),
        number_let(12, "i", true, Expr::Integer(0)),
        number_let(13, "n", false, Expr::Integer(64)),
        Stmt::While {
            condition: Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(12)),
                right: Box::new(Expr::LocalGet(13)),
            },
            body: vec![
                number_let(
                    14,
                    "t",
                    false,
                    Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(mul(Expr::LocalGet(10), Expr::LocalGet(10))),
                        right: Box::new(mul(Expr::LocalGet(11), Expr::LocalGet(11))),
                    },
                ),
                Stmt::Expr(Expr::LocalSet(
                    11,
                    Box::new(mul(
                        mul(Expr::Number(2.0), Expr::LocalGet(10)),
                        Expr::LocalGet(11),
                    )),
                )),
                Stmt::Expr(Expr::LocalSet(10, Box::new(Expr::LocalGet(14)))),
                Stmt::Expr(Expr::Update {
                    id: 12,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
            ],
        },
        Stmt::Return(Some(Expr::LocalGet(10))),
    ]
}

#[test]
fn reassigned_number_accumulator_multiply_is_an_inline_fmul() {
    // Both accumulators are seeded from a Number literal and every later write
    // is arithmetic over the same set, so the number-by-construction fixpoint
    // admits them and the multiplies stay inline.
    let ir = emitted_ir(probe_module(
        "reassigned_numeric_accumulator_unit.ts",
        Vec::new(),
        mandelbrot_shaped_body(Expr::Number(0.0), Expr::Number(0.0)),
    ));
    assert!(
        ir.contains("fmul double"),
        "a multiply of reassigned number-by-construction locals must emit an \
         inline fmul:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_dynamic_mul"),
        "a reassigned local whose every write is number-producing must not \
         route through the BigInt-aware dynamic multiply:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_number_coerce"),
        "the proof is a canonical double at rest, so no residual coercion is \
         needed:\n{ir}"
    );
}

#[test]
fn a_reassigned_local_seeded_from_a_parameter_keeps_the_dynamic_helper() {
    // The SABOTAGE arm. Identical body, but `x` is seeded from an `Any`
    // parameter — an unconstrained incoming value that could be a boxed
    // BigInt. The fixpoint must drop it (a parameter is never a candidate and
    // cannot be chased), and the multiply must keep #5970's routing.
    //
    // Without this, the positive test above would also pass against an
    // analysis that admits every reassigned local, which is precisely the
    // wrong-code shape #7773 shipped.
    let ir = emitted_ir(probe_module(
        "reassigned_from_param_unit.ts",
        vec![Param {
            id: 2,
            name: "seed".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        mandelbrot_shaped_body(Expr::LocalGet(2), Expr::Number(0.0)),
    ));
    assert!(
        ir.contains("call double @js_dynamic_mul"),
        "a local seeded from an unconstrained parameter must keep the \
         BigInt-aware dynamic multiply:\n{ir}"
    );
}

#[test]
fn a_reassigned_local_written_from_a_string_keeps_the_dynamic_helper() {
    // Second sabotage arm: the seed is a Number, but a later write stores a
    // string. `+` on a string operand concatenates, so the local is NOT a
    // Number by construction and the fixpoint must drop it.
    let mut body = mandelbrot_shaped_body(Expr::Number(0.0), Expr::Number(0.0));
    // Insert `x = "oops";` between the declarations and the loop.
    body.insert(
        4,
        Stmt::Expr(Expr::LocalSet(
            10,
            Box::new(Expr::String("oops".to_string())),
        )),
    );
    let ir = emitted_ir(probe_module(
        "reassigned_with_string_write_unit.ts",
        Vec::new(),
        body,
    ));
    assert!(
        ir.contains("call double @js_dynamic_mul"),
        "one non-Number write must drop the whole local from the fact:\n{ir}"
    );
}

#[test]
fn dynamic_operand_multiply_keeps_bigint_aware_helper() {
    // #5970's correctness routing must survive: an operand that may be an
    // object (possible boxed BigInt / BigInt-returning valueOf) still goes
    // through the ToNumeric-running dynamic helper.
    let ir = emitted_ir(probe_module(
        "math_dynamic_operand_multiply_unit.ts",
        vec![Param {
            id: 2,
            name: "x".to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        vec![Stmt::Return(Some(mul(
            Expr::MathSqrt(Box::new(Expr::Integer(4))),
            Expr::LocalGet(2),
        )))],
    ));
    assert!(
        ir.contains("call double @js_dynamic_mul"),
        "a possibly-object operand must keep the BigInt-aware dynamic \
         multiply routing:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// #7404 — the `%` integer fast path must fire for locals that are
// integer-valued within i64 range but NOT provably i32-range.
//
// These assert on the emitted IR rather than on a predicate, because the whole
// failure mode being fixed was a gate that was live but asking the wrong
// question: a test that only checked "nothing threw" would have passed against
// the broken compiler.
// ---------------------------------------------------------------------------

fn mod_(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Mod,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `let a = 12345678; for (…) { acc += a % 1000; a = a + 1; }`
///
/// `a` is mutated by an unbounded `+ 1` chain, so it is (correctly) NOT in the
/// i32-range `integer_locals` set — before #7404 this fell through to
/// `frem double`, i.e. a `bl _fmod` libm call on AArch64.
#[test]
fn i64_range_local_reaches_the_integer_modulo_fast_path() {
    let ir = emitted_ir(probe_module(
        "mod_i64_local_unit.ts",
        Vec::new(),
        vec![
            number_let(1, "acc", true, Expr::Integer(0)),
            number_let(2, "a", true, Expr::Integer(12345678)),
            Stmt::For {
                init: Some(Box::new(number_let(3, "i", true, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::Integer(64)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(
                        1,
                        Box::new(add(
                            Expr::LocalGet(1),
                            mod_(Expr::LocalGet(2), Expr::Integer(1000)),
                        )),
                    )),
                    Stmt::Expr(Expr::LocalSet(
                        2,
                        Box::new(add(Expr::LocalGet(2), Expr::Integer(1))),
                    )),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));
    assert!(
        ir.contains("srem i64"),
        "`a % 1000` for an i64-range increment counter must lower to srem, \
         not a frem/fmod libm call:\n{ir}"
    );
}

/// The dividend may come from the i64-range set; the **divisor** may not.
///
/// `srem(x, 0)` is UB in LLVM while JS requires NaN, and the lowering's
/// zero guard only recognises a literal `0`. A counter that walks through zero
/// (`d = d - 1`) is exactly what the i64-range set admits, so it must stay on
/// `frem`.
#[test]
fn i64_range_local_is_refused_as_a_modulo_divisor() {
    let ir = emitted_ir(probe_module(
        "mod_i64_divisor_unit.ts",
        Vec::new(),
        vec![
            number_let(1, "acc", true, Expr::Integer(0)),
            number_let(2, "d", true, Expr::Integer(10)),
            Stmt::For {
                init: Some(Box::new(number_let(3, "i", true, Expr::Integer(0)))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(3)),
                    right: Box::new(Expr::Integer(64)),
                }),
                update: Some(Expr::Update {
                    id: 3,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(
                        1,
                        Box::new(add(
                            Expr::LocalGet(1),
                            mod_(Expr::Integer(1000), Expr::LocalGet(2)),
                        )),
                    )),
                    Stmt::Expr(Expr::LocalSet(
                        2,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Sub,
                            left: Box::new(Expr::LocalGet(2)),
                            right: Box::new(Expr::Integer(1)),
                        }),
                    )),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    ));
    assert!(
        !ir.contains("srem i64"),
        "a divisor that can walk through zero must NOT reach srem \
         (srem by 0 is UB; JS requires NaN):\n{ir}"
    );
}

// ── #7592: `s.charCodeAt(i)` in a hash loop ─────────────────────────────
//
// `honest_bench`'s `json_pipeline` FNV-1a phase hashes a 68 MB string one
// `charCodeAt` at a time. Its leaf profile was 85% opaque runtime calls:
// `js_string_char_code_at` 31.5%, `js_dynamic_bitxor` 31.0%,
// `js_string_index_to_i32` 13.1%, `js_get_string_pointer_unified` 9.0% —
// only 15% was the JS loop. Two independent defects produced that, and each
// assertion below pins exactly one of them.

fn typed_param(id: u32, name: &str, ty: Type) -> Param {
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

/// `recv.charCodeAt(index)`
fn char_code_at(recv: Expr, index: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(recv),
            property: "charCodeAt".to_string(),
            byte_offset: 0,
        }),
        args: vec![index],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// `for (let i = 0; i < 64; i++) h = (h ^ recv.charCodeAt(i)) | 0;`
fn hash_loop_ir(param_ty: Type) -> String {
    let recv = Expr::LocalGet(1);
    let (params, mut body) = if matches!(param_ty, Type::String) {
        (
            Vec::new(),
            vec![Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String(
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-".to_string(),
                )),
            }],
        )
    } else {
        (vec![typed_param(1, "s", param_ty)], Vec::new())
    };
    body.extend([
        number_let(10, "h", true, Expr::Integer(0)),
        Stmt::For {
            init: Some(Box::new(number_let(11, "i", true, Expr::Integer(0)))),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(11)),
                right: Box::new(Expr::Integer(64)),
            }),
            update: Some(Expr::Update {
                id: 11,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::LocalSet(
                10,
                Box::new(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left: Box::new(Expr::LocalGet(10)),
                        right: Box::new(char_code_at(recv, Expr::LocalGet(11))),
                    }),
                    right: Box::new(Expr::Integer(0)),
                }),
            ))],
        },
        Stmt::Return(Some(Expr::LocalGet(10))),
    ]);
    emitted_ir(probe_module("char_code_at_unit.ts", params, body))
}

#[test]
fn char_code_at_on_a_proven_string_receiver_is_statically_numeric() {
    // Defect 1: `is_numeric_expr` had no arm for a String-method call, so
    // `h ^ s.charCodeAt(i)` failed `expr/binary.rs`'s "both operands are
    // statically primitive" test and every iteration paid a
    // `js_dynamic_bitxor` FFI call to compute an integer xor.
    let ir = hash_loop_ir(Type::String);
    assert!(
        !ir.contains("call double @js_dynamic_bitxor"),
        "a xor against String.prototype.charCodeAt must not route through \
         the BigInt-aware dynamic helper — charCodeAt is a Number:\n{ir}"
    );
    assert!(
        ir.contains("xor i32"),
        "the xor must lower inline once both operands are proven \
         non-BigInt primitives:\n{ir}"
    );
}

#[test]
fn char_code_at_on_a_proven_string_receiver_emits_the_inline_ascii_read() {
    // Defect 2: even with the receiver handle resolved, each character cost
    // two more opaque calls (`js_string_index_to_i32` +
    // `js_string_char_code_at`), which also pinned the loop-invariant header
    // loads inside the loop because LICM cannot hoist across an opaque call.
    let ir = hash_loop_ir(Type::String);
    assert!(
        ir.contains("cca.fast"),
        "a runtime-proven string receiver must get the inline ASCII charCodeAt fast \
         path:\n{ir}"
    );
    assert!(
        ir.contains("load i8"),
        "the ASCII fast path must read the character as a single byte \
         load:\n{ir}"
    );
    // The slow arm is still emitted — it is what services SSO receivers,
    // non-ASCII payloads, out-of-range and non-numeric indices. Its presence
    // is the proof that the fast path did NOT replace the correct lowering,
    // only shortcut it.
    assert!(
        ir.contains("call double @js_string_char_code_at")
            && ir.contains("call i32 @js_string_index_to_i32"),
        "the inline path must keep the runtime helpers as its fallback \
         arm:\n{ir}"
    );
}

#[test]
fn char_code_at_on_an_unproven_receiver_keeps_the_runtime_lowering() {
    // The negative control. An `any`-typed receiver may be a user object with
    // its own `charCodeAt`, so neither the static Number claim nor the inline
    // header read is admissible — and this assertion is what makes the two
    // tests above meaningful rather than tautological (both would pass on a
    // build that fired the fast path unconditionally).
    let ir = hash_loop_ir(Type::Any);
    assert!(
        !ir.contains("cca.fast"),
        "an unproven receiver must not read a StringHeader inline:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_bitxor"),
        "an unproven receiver's method result may still be a BigInt, so the \
         xor must keep the dynamic helper:\n{ir}"
    );
}

#[test]
fn only_number_returning_string_methods_are_claimed_numeric() {
    // `codePointAt` returns `undefined` (a NaN-BOX tag) for an out-of-range
    // index and `at`/`charAt` return strings, so claiming them numeric would
    // hand a tagged value to an `fadd`/inline-xor. Pinning the exact set here
    // means widening it is a deliberate edit, not a copy-paste from
    // `is_known_string_method_name`.
    const CLAIMED: [&str; 5] = [
        "charCodeAt",
        "indexOf",
        "lastIndexOf",
        "search",
        "localeCompare",
    ];
    for name in CLAIMED {
        assert!(
            super::string_method_returns_number(name),
            "{name} lowers to a raw double and must be claimed numeric"
        );
    }
    for name in [
        "codePointAt",
        "at",
        "charAt",
        "startsWith",
        "endsWith",
        "includes",
        "slice",
        "split",
        "match",
    ] {
        assert!(
            !super::string_method_returns_number(name),
            "{name} does not always evaluate to a Number and must not be claimed"
        );
    }
    // The claim mirrors `lower_call/property_get.rs`'s static-String routing
    // condition; an admitted name that is ALSO array-only would never reach
    // `lower_string_method`, and the claim would be about a call that lowers
    // somewhere else entirely.
    for name in CLAIMED {
        assert!(
            !crate::lower_call::property_get::is_array_only_method_name(name),
            "{name} must not be array-only, or the routing mirror is wrong"
        );
        assert!(
            crate::lower_string_method::is_known_string_method_name(name),
            "{name} must be a known String method, or it never routes to \
             lower_string_method"
        );
    }
}

/// #7796 — an element read at a NON-numeric index is not a numeric read.
///
/// `a[Symbol.iterator]` on a `number[]` is a property read on the array
/// object, and it answers with a function. Typing the local that holds it as
/// `number` made the truthiness test lower to `fcmp one %v, 0.0` — and every
/// NaN-boxed pointer IS a NaN, so that comparison is false for every function,
/// object and string alive. `if (f)` took the false branch on a value that
/// `typeof` called a function and `Boolean()` called true.
mod symbol_keyed_element_reads {
    use super::*;

    /// Slice out the probe function so an assertion cannot be satisfied by
    /// some unrelated part of the module.
    fn probe_body(ir: &str) -> String {
        let start = ir
            .find("__probe(")
            .map(|i| ir[..i].rfind("define").expect("define before probe"))
            .expect("probe function must be emitted");
        let end = ir[start..].find("\n}").expect("probe must terminate") + start;
        ir[start..end].to_string()
    }

    fn array_of_numbers(id: u32) -> Stmt {
        Stmt::Let {
            id,
            name: "a".to_string(),
            ty: Type::Array(Box::new(Type::Number)),
            mutable: false,
            init: Some(Expr::Array(vec![Expr::Integer(1)])),
        }
    }

    fn truthy_return(local: u32) -> Stmt {
        Stmt::Return(Some(Expr::Conditional {
            condition: Box::new(Expr::LocalGet(local)),
            then_expr: Box::new(Expr::Integer(1)),
            else_expr: Box::new(Expr::Integer(0)),
        }))
    }

    #[test]
    fn a_symbol_indexed_element_is_tested_with_js_is_truthy() {
        // const a: number[] = [1];
        // const sym = Symbol.iterator;
        // const f = a[sym];
        // return f ? 1 : 0;
        let ir = emitted_ir(probe_module(
            "symbol_keyed_element_read.ts",
            Vec::new(),
            vec![
                array_of_numbers(1),
                Stmt::Let {
                    id: 2,
                    name: "sym".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::SymbolFor(Box::new(Expr::String(
                        "@@__perry_wk_iterator".to_string(),
                    )))),
                },
                Stmt::Let {
                    id: 3,
                    name: "f".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::LocalGet(2)),
                    }),
                },
                truthy_return(3),
            ],
        ));
        let body = probe_body(&ir);
        assert!(
            body.contains("js_is_truthy"),
            "a symbol-keyed read must go through the general truthiness \
             helper, or a NaN-boxed function reads as false:\n{body}"
        );
        assert!(
            fcmp_one_only_under_the_plain_number_guard(&body),
            "the numeric fast path must not fire for a non-numeric index:\n{body}"
        );
    }

    /// The dynamic truthiness lowering decides a plain (untagged, non-NaN)
    /// double inline with `fcmp one` — but only inside its `truthy.num` block,
    /// after the bit test that proves the value is a number. An `fcmp one`
    /// anywhere else is the unguarded numeric claim these tests forbid.
    fn fcmp_one_only_under_the_plain_number_guard(body: &str) -> bool {
        let mut label = String::new();
        for line in body.lines() {
            let trimmed = line.trim_start();
            if !line.starts_with(' ') && trimmed.ends_with(':') {
                label = trimmed.trim_end_matches(':').to_string();
            } else if trimmed.contains("fcmp one") && !label.starts_with("truthy.num") {
                return false;
            }
        }
        true
    }

    #[test]
    fn a_numeric_index_keeps_the_array_fast_path_but_not_a_truthiness_claim() {
        // A numeric index preserves the guarded array read, but its boxed
        // fallback means the result binding still needs runtime truthiness.
        let ir = emitted_ir(probe_module(
            "numeric_element_read.ts",
            Vec::new(),
            vec![
                array_of_numbers(1),
                Stmt::Let {
                    id: 2,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: false,
                    init: Some(Expr::Integer(0)),
                },
                Stmt::Let {
                    id: 3,
                    name: "v".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::LocalGet(2)),
                    }),
                },
                truthy_return(3),
            ],
        ));
        let body = probe_body(&ir);
        assert!(
            body.contains("arr.guard.deref") && body.contains("arr.fast"),
            "a numeric index must keep the guarded array read:\n{body}"
        );
        assert!(
            body.contains("js_is_truthy"),
            "the result binding must use runtime truthiness:\n{body}"
        );
        assert!(
            fcmp_one_only_under_the_plain_number_guard(&body),
            "the read's boxed fallback must not become a numeric proof:\n{body}"
        );
    }
}
