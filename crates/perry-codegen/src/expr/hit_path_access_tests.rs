//! Emission pins for the hit-path audit's array, typed-array and class-field
//! access changes. Each test names the removed out-of-line work and the
//! fallback that must remain; the behavioural half of each change is a
//! `test-files/test_gap_*` fixture compared against node.

use super::class_field_barrier_tests::block_body;
use crate::testing::root_slots::function_slice;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn function(params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// The public entry plus every clone of `probe` — a typed parameter may route
/// the body through a specialized clone or the erased `$generic` fallback.
fn probe_ir(module: &Module) -> String {
    let ir = String::from_utf8(
        compile_module(
            module,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .expect("compile access probe"),
    )
    .unwrap();
    let prefix = format!("perry_fn_{}__probe", module.name.replace('.', "_"));
    let mut body = String::new();
    for line in ir.lines() {
        if line.starts_with("define ") && line.contains(&format!("@{prefix}")) {
            let name_start = line.find('@').unwrap() + 1;
            let name_end = line[name_start..].find('(').unwrap() + name_start;
            body.push_str(function_slice(&ir, &line[name_start..name_end]));
            body.push('\n');
        }
    }
    assert!(!body.is_empty(), "no probe function in:\n{ir}");
    body
}

fn module(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Module {
    let mut m = Module::new(name);
    m.functions.push(function(params, body));
    m
}

fn named(name: &str) -> Type {
    Type::Named(name.to_string())
}

/// `probe(a: Float64Array) { return a.length }` reads the typed array's header
/// inline. The name-keyed runtime lookup (which heap-copied "length") is only
/// the fallback.
#[test]
fn declared_typed_array_length_reads_the_header_inline() {
    let ir = probe_ir(&module(
        "ta_length",
        vec![param(1, named("Float64Array"))],
        vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "length".into(),
            byte_offset: 0,
        }))],
    ));
    let arm = block_body(&ir, "plen.typed_array")
        .unwrap_or_else(|| panic!("no typed-array length arm:\n{ir}"));
    assert!(
        arm.contains("icmp eq i8") && arm.contains(", 11"),
        "the arm must test GC_TYPE_TYPED_ARRAY:\n{arm}"
    );
    assert!(
        arm.contains("@PERRY_TA_VIEW_GUARD") && arm.contains("@PERRY_TA_OWN_PROPS_PRESENT"),
        "a view or an own `length` property must keep the header read off:\n{arm}"
    );
}

/// `probe(a: number[], k: number, v: number) { a[k] = v }` — an index with no
/// static range proof — takes the guarded in-bounds store for a canonical
/// element index and calls the exact key helper only on a guard miss.
#[test]
fn unproven_numeric_index_store_has_an_inline_element_tier() {
    let ir = probe_ir(&module(
        "array_runtime_key",
        vec![
            param(1, Type::Array(Box::new(Type::Number))),
            param(2, Type::Number),
            param(3, Type::Number),
        ],
        vec![Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
            value: Box::new(Expr::LocalGet(3)),
        })],
    ));
    assert!(
        ir.contains("idxset.runtime_key.fast"),
        "the canonical element index must reach the inline store:\n{ir}"
    );
    let slow = block_body(&ir, "idxset.runtime_key.slow")
        .unwrap_or_else(|| panic!("no helper arm:\n{ir}"));
    assert!(
        slow.contains("@js_typed_feedback_array_set_index_or_string("),
        "a declined key must still reach the exact helper:\n{slow}"
    );
}
