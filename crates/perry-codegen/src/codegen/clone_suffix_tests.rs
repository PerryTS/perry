//! Issue #6927 — generated clone symbols must be unforgeable by user names.
//!
//! Clone/body symbols used to be `{public}__<suffix>` (`__generic`,
//! `__typed_f64`, …). A user function literally named `add__typed_f64`
//! composed the SAME LLVM symbol as `add`'s typed clone, and
//! `deduped_function_refs` (first-define-wins, added for minified same-name
//! classes) silently dropped one of the two definitions: the user function's
//! public entry was usurped by `add`'s clone, so every indirect call through
//! its registered wrapper executed `add`'s body instead — a SILENT wrong
//! result, not a loud verifier error. (Witnessed on v0.5.1280:
//! `const g = add__typed_f64; g(2, 3)` returned 5 instead of 6.)
//!
//! The fix reserves `$` as the generated-suffix separator (`{public}$generic`,
//! `{public}$typed_f64`, `{public}$dupN`, the spec-ABI and proven-`this`
//! suffixes): `sanitize`/`sanitize_member` output is strictly `[A-Za-z0-9_]`,
//! so no user-derived public symbol can ever equal a generated one. These
//! tests pin the emitted-IR side of that contract; the mangling side is pinned
//! by `helpers::sanitize_tests`.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..CompileOptions::default()
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

/// A typed-ABI clone candidate: straight-line `return a <op> b` over two
/// `number` params, so codegen emits the public trampoline plus `$typed_f64`
/// and `$generic` bodies.
fn candidate_fn(id: u32, name: &str, op: BinaryOp) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: vec![number_param(10, "a"), number_param(11, "b")],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op,
            left: Box::new(Expr::LocalGet(10)),
            right: Box::new(Expr::LocalGet(11)),
        }))],
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
        name: "clone_suffix.ts".to_string(),
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

/// `define` lines whose declared symbol is exactly `name`.
fn define_count(ir: &str, name: &str) -> usize {
    let needle = format!("@{name}(");
    ir.lines()
        .filter(|l| l.starts_with("define") && l.contains(&needle))
        .count()
}

/// Slice out the body of the `define`d function whose signature line declares
/// exactly `name`.
fn function_body<'a>(ir: &'a str, name: &str) -> &'a str {
    let needle = format!("@{name}(");
    let start = ir
        .match_indices("define")
        .find(|(i, _)| {
            let line_end = ir[*i..].find('\n').map(|n| i + n).unwrap_or(ir.len());
            ir[*i..line_end].contains(&needle)
        })
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("no define for @{name} in module IR"));
    let end = ir[start..].find("\n}").expect("unterminated function") + start;
    &ir[start..end]
}

/// The #6927 witness family: `add` plus user functions whose names are the
/// OLD (forgeable) spellings of `add`'s clone symbols. Every public entry and
/// every clone must be a distinct symbol, each defined exactly once, with each
/// body reached from its own trampoline.
#[test]
fn user_members_named_like_clone_suffixes_keep_their_own_symbols() {
    let ir = String::from_utf8(
        compile_module(
            &module_with(vec![
                candidate_fn(1, "add", BinaryOp::Add),
                candidate_fn(2, "add__typed_f64", BinaryOp::Mul),
                candidate_fn(3, "add__generic", BinaryOp::Sub),
            ]),
            ir_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");

    // Publics and clones are all distinct symbols, each defined exactly once.
    // Pre-fix, `add`'s clone was literally `perry_fn_clone_suffix_ts__add__typed_f64`
    // — the user function's public symbol — and first-define-wins dedup
    // silently dropped the user function's own entry.
    for name in [
        "perry_fn_clone_suffix_ts__add",
        "perry_fn_clone_suffix_ts__add__typed_f64",
        "perry_fn_clone_suffix_ts__add__generic",
        "perry_fn_clone_suffix_ts__add$typed_f64",
        "perry_fn_clone_suffix_ts__add$generic",
        "perry_fn_clone_suffix_ts__add__typed_f64$typed_f64",
        "perry_fn_clone_suffix_ts__add__typed_f64$generic",
        "perry_fn_clone_suffix_ts__add__generic$typed_f64",
        "perry_fn_clone_suffix_ts__add__generic$generic",
    ] {
        assert_eq!(
            define_count(&ir, name),
            1,
            "@{name} must be defined exactly once"
        );
    }

    // Each trampoline routes to ITS OWN clones — `add`'s fast arm computes
    // a + b, the user `add__typed_f64`'s computes a * b.
    let add_public = function_body(&ir, "perry_fn_clone_suffix_ts__add");
    assert!(add_public.contains("@perry_fn_clone_suffix_ts__add$typed_f64("));
    assert!(add_public.contains("@perry_fn_clone_suffix_ts__add$generic("));
    let user_public = function_body(&ir, "perry_fn_clone_suffix_ts__add__typed_f64");
    assert!(user_public.contains("@perry_fn_clone_suffix_ts__add__typed_f64$typed_f64("));
    assert!(user_public.contains("@perry_fn_clone_suffix_ts__add__typed_f64$generic("));

    let add_clone = function_body(&ir, "perry_fn_clone_suffix_ts__add$typed_f64");
    assert!(add_clone.contains("fadd"), "add's clone body is a + b");
    let user_clone = function_body(&ir, "perry_fn_clone_suffix_ts__add__typed_f64$typed_f64");
    assert!(
        user_clone.contains("fmul"),
        "add__typed_f64's clone body is a * b"
    );
}
