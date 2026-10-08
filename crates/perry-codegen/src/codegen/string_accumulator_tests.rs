//! Untyped string accumulators (`collectors::string_accumulator_locals`).
//!
//! TypeScript's `createTextWriter` keeps its output in `var output;`, assigns
//! it `""` in `reset()`, and grows it with `output += s` from sibling
//! closures. The binding's declared type is `any`, so before this the
//! declared-`string` gate never selected the in-place append and every
//! `output += s` copied the whole accumulator: on a three-transpile tsc
//! workload that was 6.5 GB of born-tenured large strings and 236 of its 237
//! full collections.
//!
//! The tests come in pairs: an untyped self-append that is given a string
//! literal is selected, a numeric one is not; an ordinary read of the
//! accumulator demotes the unique owner, a `.length` read does not.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Stmt};

const ACC: u32 = 10;

fn ir_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        ..Default::default()
    }
}

fn probe(body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
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

/// A module whose init declares the untyped binding `ACC = init` and whose
/// `probe` function runs `body` against it (a module global is a persistent
/// binding, like a closure capture or a box).
fn module(init: Expr, body: Vec<Stmt>) -> Module {
    Module {
        name: "string_accumulator.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![probe(body)],
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init_is_strict: false,
        init: vec![Stmt::Let {
            id: ACC,
            name: "output".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(init),
        }],
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
        class_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn probe_ir(module: Module) -> String {
    let ir = String::from_utf8(compile_module(&module, ir_opts()).unwrap()).expect("UTF-8 IR");
    let marker = "@perry_fn_string_accumulator_ts__probe(";
    let start = ir
        .match_indices("define ")
        .find_map(|(index, _)| {
            let line_end = ir[index..].find('\n').map(|offset| index + offset)?;
            ir[index..line_end].contains(marker).then_some(index)
        })
        .expect("probe body");
    let end = ir[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 3)
        .unwrap_or(ir.len());
    ir[start..end].to_string()
}

fn self_append(rhs: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        ACC,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(ACC)),
            right: Box::new(rhs),
        }),
    ))
}

const APPEND: &str = "call i64 @js_string_append_known_heap(";
const DEMOTE: &str = "call void @js_string_addref_if_heap_string(";

#[test]
fn an_untyped_accumulator_given_a_string_selects_the_in_place_append() {
    let ir = probe_ir(module(
        Expr::String(String::new()),
        vec![
            self_append(Expr::String("tok".to_string())),
            Stmt::Return(None),
        ],
    ));
    assert!(
        ir.contains(APPEND),
        "`output += s` on an untyped string accumulator must reach the amortized append:\n{ir}"
    );
}

#[test]
fn an_untyped_numeric_self_add_is_not_an_accumulator() {
    let ir = probe_ir(module(
        Expr::Number(0.0),
        vec![self_append(Expr::Number(1.0)), Stmt::Return(None)],
    ));
    assert!(
        !ir.contains(APPEND),
        "a self-add never given a string must keep its ordinary `+` lowering:\n{ir}"
    );
}

#[test]
fn an_ordinary_read_of_the_accumulator_demotes_the_unique_owner() {
    let ir = probe_ir(module(
        Expr::String(String::new()),
        vec![
            self_append(Expr::String("tok".to_string())),
            Stmt::Return(Some(Expr::LocalGet(ACC))),
        ],
    ));
    assert!(
        ir.contains(DEMOTE),
        "a read that can alias the accumulator must demote it, or a later in-place \
         append would rewrite the alias:\n{ir}"
    );
}

#[test]
fn a_length_read_of_the_accumulator_does_not_demote() {
    let ir = probe_ir(module(
        Expr::String(String::new()),
        vec![
            self_append(Expr::String("tok".to_string())),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(ACC)),
                property: "length".to_string(),
                byte_offset: 0,
            })),
        ],
    ));
    assert!(
        !ir.contains(DEMOTE),
        "`output.length` cannot hand the string anywhere; demoting there makes the \
         next append copy the whole accumulator:\n{ir}"
    );
}
