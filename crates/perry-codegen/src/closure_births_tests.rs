//! A run of closure declarations bound in scope slots is born from one table.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

/// `function fK() { return f1 ?? … ?? fN; }` for every K in `ids`: each body
/// reads every sibling, so every binding is captured before its declaration
/// runs, as hoisted function declarations are.
fn hoisted_run(ids: &[u32]) -> Vec<Stmt> {
    let mut body = vec![Stmt::PreallocateBoxes(ids.to_vec())];
    for &id in ids {
        let reads = ids
            .iter()
            .map(|&other| Stmt::Expr(Expr::LocalGet(other)))
            .collect::<Vec<_>>();
        body.push(Stmt::Let {
            id,
            name: format!("f{id}"),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Closure {
                func_id: 100 + id,
                params: Vec::new(),
                return_type: Type::Any,
                body: reads,
                captures: ids.to_vec(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: false,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        });
    }
    body.push(Stmt::Return(Some(Expr::LocalGet(ids[0]))));
    body
}

pub(crate) fn function_ir(ids: &[u32]) -> String {
    let mut module = Module::new("closure_births_probe.ts");
    module.functions.push(Function {
        id: 1,
        name: "probe1".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: hoisted_run(ids),
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    crate::scope_env::group_scope_boxes(&mut module);
    let options = crate::CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..crate::CompileOptions::default()
    };
    String::from_utf8(crate::compile_module(&module, options).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

#[test]
fn a_run_of_scope_slot_closure_declarations_is_one_table_walk() {
    let ids: Vec<u32> = (1..=10).collect();
    let ir = function_ir(&ids);
    let fn_ir =
        crate::testing::root_slots::function_slice(&ir, "perry_fn_closure_births_probe_ts__probe1");
    assert_eq!(
        fn_ir
            .matches("call void @js_closure_births_to_scope(")
            .count(),
        1,
        "one walk for the whole run:\n{fn_ir}"
    );
    assert!(
        !fn_ir.contains("@js_closure_alloc_small(") && !fn_ir.contains("@js_closure_alloc_fixed("),
        "no inline birth is left in the run:\n{fn_ir}"
    );
    let table = ir
        .lines()
        .find(|line| line.starts_with("@perry_births_") && !line.contains("_captures ="))
        .expect("the birth table is emitted");
    assert!(
        table.contains("[10 x { ptr, i32, i32, i32, i32 }]"),
        "{table}"
    );
    // Every body, in declaration order.
    let mut at = 0;
    for id in &ids {
        let info = format!("@perry_closure_closure_births_probe_ts__{}$info", 100 + id);
        let found = table[at..]
            .find(&info)
            .unwrap_or_else(|| panic!("{info} in order: {table}"));
        at += found + info.len();
    }
}

#[test]
fn a_short_run_stays_inline() {
    let ids: Vec<u32> = (1..crate::closure_births::MIN_RUN as u32).collect();
    let ir = function_ir(&ids);
    let fn_ir =
        crate::testing::root_slots::function_slice(&ir, "perry_fn_closure_births_probe_ts__probe1");
    assert!(!fn_ir.contains("@js_closure_births_to_scope("), "{fn_ir}");
    assert_eq!(
        fn_ir.matches("@js_closure_alloc_small(").count(),
        ids.len(),
        "{fn_ir}"
    );
}
