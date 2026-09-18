//! #10446: the static-type `Map` / `Set` fast paths must check their receiver.
//!
//! Codegen picks `js_map_get` / `js_set_add` from the receiver's DECLARED
//! type and hands them the unboxed 48-bit payload. A declaration is not a
//! runtime fact: `undefined` unboxes to the address `0x1`, which those helpers
//! dereferenced (SIGSEGV, no JS stack, uncatchable — mongodb 7.5.0 died that
//! way inside `MongoError.addErrorLabel`).
//!
//! This is an IR census, and both halves matter.
//!
//! The positive half asserts the guard is LIVE: the object-tag compare, the
//! throw block, and the diverging call are all emitted, so a guard that is
//! silently never emitted fails here rather than in a segfault months later.
//!
//! The negative half asserts the guard is still a GUARD and not a detour: the
//! fast path keeps calling the same runtime helper it always did, and the
//! throwing arm ends in `unreachable` so the check adds no reachable call — and
//! therefore no collection point — between the receiver and its use.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};

/// The emitted blocks that exist only when the guard was lowered.
const THROW_BLOCK: &str = "collection_recv.throw";
const OK_BLOCK: &str = "collection_recv.ok";
/// `POINTER_TAG >> 48` — the one compare the fast path pays.
const OBJECT_TAG_TOP16: &str = "32765";
/// The CALL, not the module's unconditional `declare` of the same symbol.
const THROW_CALL: &str = "call void @js_throw_collection_receiver_type_error(";

fn ir_opts() -> CompileOptions {
    CompileOptions {
        is_entry_module: true,
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    }
}

fn map_type() -> Type {
    Type::Generic {
        base: "Map".to_string(),
        type_args: vec![Type::String, Type::Number],
    }
}

fn set_type() -> Type {
    Type::Generic {
        base: "Set".to_string(),
        type_args: vec![Type::String],
    }
}

fn module_with(body: Vec<Stmt>) -> Module {
    Module {
        name: "typed_collection_receiver_guard.ts".to_string(),
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
            params: Vec::new(),
            return_type: Type::Void,
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
        init_is_strict: false,
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
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

fn ir_for(body: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module_with(body), ir_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// Every emitted call to `name`, with the instruction that follows it.
fn call_lines_with_successor<'a>(ir: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let lines: Vec<&str> = ir.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(name))
        .map(|(idx, line)| (*line, lines.get(idx + 1).copied().unwrap_or("")))
        .collect()
}

#[test]
fn map_get_guards_its_receiver_before_the_runtime_call() {
    let ir = ir_for(vec![
        Stmt::Let {
            id: 1,
            name: "m".to_string(),
            ty: map_type(),
            mutable: false,
            init: Some(Expr::MapNew),
        },
        Stmt::Expr(Expr::MapGet {
            map: Box::new(Expr::LocalGet(1)),
            key: Box::new(Expr::String("k".to_string())),
        }),
    ]);

    assert!(
        ir.contains(THROW_BLOCK) && ir.contains(OK_BLOCK),
        "the receiver guard's blocks must be emitted:\n{ir}"
    );
    assert!(
        ir.contains(OBJECT_TAG_TOP16),
        "the fast path must test the object tag ({OBJECT_TAG_TOP16}):\n{ir}"
    );
    // The guard is a guard: the ordinary lowering still reaches the helper.
    assert!(
        ir.contains("@js_map_get("),
        "the guarded fast path must still call js_map_get:\n{ir}"
    );

    let throws = call_lines_with_successor(&ir, THROW_CALL);
    assert!(
        !throws.is_empty(),
        "the miss path must call the diverging helper:\n{ir}"
    );
    for (call, next) in throws {
        assert!(
            next.trim() == "unreachable",
            "the throw helper diverges, so its block must end there \
             (call: {call}, next: {next})"
        );
    }
}

#[test]
fn set_add_guards_its_receiver_before_the_runtime_call() {
    let ir = ir_for(vec![
        Stmt::Let {
            id: 1,
            name: "s".to_string(),
            ty: set_type(),
            mutable: false,
            init: Some(Expr::SetNew),
        },
        Stmt::Expr(Expr::SetAdd {
            set_id: 1,
            value: Box::new(Expr::String("x".to_string())),
        }),
    ]);

    assert!(
        ir.contains(THROW_BLOCK) && ir.contains(THROW_CALL),
        "set.add must guard its receiver:\n{ir}"
    );
    assert!(
        ir.contains("@js_set_add"),
        "the guarded fast path must still call a js_set_add helper:\n{ir}"
    );
}

#[test]
fn a_guardless_collection_free_body_emits_no_guard() {
    // The control: nothing about the guard is unconditional module prelude.
    let ir = ir_for(vec![Stmt::Expr(Expr::Integer(1))]);
    assert!(
        !ir.contains(THROW_BLOCK) && !ir.contains(THROW_CALL),
        "a body with no collection receiver must emit no guard:\n{ir}"
    );
}
