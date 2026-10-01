//! Grouping-rule, map and codegen tests for scope context objects.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

use super::{group_scope_boxes, ScopeMap};

fn let_num(id: u32, v: f64) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Number(v)),
    }
}

/// `() => { v1 = v1 + 1; …; return v_last }` over `ids`: captures and mutates.
fn mutating_closure(func_id: u32, ids: &[u32]) -> Expr {
    let mut body: Vec<Stmt> = ids
        .iter()
        .map(|&id| {
            Stmt::Expr(Expr::LocalSet(
                id,
                Box::new(Expr::Binary {
                    op: perry_hir::BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(id)),
                    right: Box::new(Expr::Number(1.0)),
                }),
            ))
        })
        .collect();
    body.push(Stmt::Return(ids.last().map(|id| Expr::LocalGet(*id))));
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body,
        captures: ids.to_vec(),
        mutable_captures: ids.to_vec(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: false,
    }
}

fn function(id: u32, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: format!("probe{id}"),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn module_with(body: Vec<Stmt>) -> Module {
    let mut module = Module::new("scope_env_probe.ts");
    module.functions.push(function(1, body));
    module
}

fn preallocs(stmts: &[Stmt]) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    super::analysis::for_each_stmt_shallow(stmts, &mut |s| {
        if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = s {
            out.push(ids.clone());
        }
    });
    out
}

/// `n` captured-and-mutated locals, all captured by ONE closure, which is
/// then called.
fn wide_body(n: u32) -> Vec<Stmt> {
    let ids: Vec<u32> = (10..10 + n).collect();
    let mut body: Vec<Stmt> = ids.iter().map(|&id| let_num(id, id as f64)).collect();
    body.push(Stmt::Let {
        id: 9,
        name: "f".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(mutating_closure(100, &ids)),
    });
    body.push(Stmt::Return(Some(Expr::LocalGet(9))));
    body
}

#[test]
fn bindings_captured_by_the_same_closures_share_one_group() {
    let mut module = module_with(wide_body(6));
    group_scope_boxes(&mut module);
    let groups = preallocs(&module.functions[0].body);
    assert_eq!(groups, vec![(10..16).collect::<Vec<u32>>()]);
    // Inserted before the earliest home, so it dominates every member.
    assert!(matches!(
        module.functions[0].body[0],
        Stmt::PreallocateBoxes(_)
    ));
}

#[test]
fn different_capturing_closure_sets_get_different_groups() {
    // x: f and g; y: g only; z: f only.
    let body = vec![
        let_num(1, 0.0),
        let_num(2, 0.0),
        let_num(3, 0.0),
        Stmt::Expr(mutating_closure(100, &[1, 3])),
        Stmt::Expr(mutating_closure(101, &[1, 2])),
    ];
    let mut module = module_with(body);
    group_scope_boxes(&mut module);
    let mut groups = preallocs(&module.functions[0].body);
    groups.sort();
    assert_eq!(groups, vec![vec![1], vec![2], vec![3]]);
}

#[test]
fn a_loop_body_group_is_allocated_inside_the_body() {
    let loop_body = vec![
        let_num(5, 1.0),
        let_num(6, 2.0),
        Stmt::Expr(mutating_closure(100, &[5, 6])),
    ];
    let body = vec![Stmt::While {
        condition: Expr::Bool(true),
        body: loop_body,
    }];
    let mut module = module_with(body);
    group_scope_boxes(&mut module);
    let Stmt::While { body, .. } = &module.functions[0].body[0] else {
        panic!("loop kept");
    };
    assert!(
        matches!(&body[0], Stmt::PreallocateBoxes(ids) if ids == &vec![5, 6]),
        "a per-iteration binding's scope object must be allocated per iteration: {body:?}"
    );
}

#[test]
fn a_reference_before_the_home_keeps_its_own_cell() {
    // `v7` is read by a closure created BEFORE its declaration: the home does
    // not dominate that reference, so it is not grouped.
    let body = vec![
        let_num(8, 0.0),
        Stmt::Expr(mutating_closure(100, &[7, 8])),
        let_num(7, 0.0),
    ];
    let mut module = module_with(body);
    group_scope_boxes(&mut module);
    let groups = preallocs(&module.functions[0].body);
    assert_eq!(groups, vec![vec![8]]);
}

#[test]
fn the_map_follows_the_group_statements() {
    let mut module = module_with(wide_body(3));
    group_scope_boxes(&mut module);
    let boxed = crate::codegen::boxed_locals::collect_module_boxed_vars(&module);
    let map = ScopeMap::build(&module, &boxed, &Default::default());
    let slots: Vec<_> = (10..13).map(|id| map.slot(id).expect("grouped")).collect();
    assert!(slots.iter().all(|s| s.rep == 10 && s.len == 3 && !s.tdz));
    assert_eq!(
        slots.iter().map(|s| s.index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(map.collapse_captures(vec![12, 5, 10, 11]), vec![10, 5]);
    // A module global never joins, even if a group statement names it.
    let globals = std::collections::HashMap::from([(11u32, "g".to_string())]);
    let map = ScopeMap::build(&module, &boxed, &globals);
    assert!(map.slot(11).is_none());
    assert_eq!(map.members(10), &[10, 12]);
}

fn compile_ir(module: &Module) -> String {
    let options = crate::CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..crate::CompileOptions::default()
    };
    String::from_utf8(crate::compile_module(module, options).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// Root slots under either lowering: native `alloca ptr addrspace(1)` or a
/// shadow-slot bind.
fn root_slots(fn_ir: &str) -> usize {
    fn_ir.matches("alloca ptr addrspace(1)").count()
        + fn_ir.matches("@js_shadow_slot_bind(").count()
}

/// The owner's cost model: a frame with N captured-and-mutated bindings pays
/// ONE root for them, not N. Measured as the root-count DIFFERENCE between a
/// 12-binding and a 2-binding frame, which cancels every root unrelated to
/// the bindings. The control arm (no grouping pass: one cell per binding, the
/// #11179 shape) must show the difference, or the test could not fail.
#[test]
fn a_scope_group_costs_one_root_not_one_per_binding() {
    let roots = |n: u32, grouped: bool| {
        let mut module = module_with(wide_body(n));
        if grouped {
            group_scope_boxes(&mut module);
        }
        let ir = compile_ir(&module);
        let fn_ir =
            crate::testing::root_slots::function_slice(&ir, "perry_fn_scope_env_probe_ts__probe1");
        assert!(
            fn_ir.contains(if grouped {
                "@js_scope_alloc("
            } else {
                "@js_box_alloc_bits("
            }),
            "the subject must be live in the artifact:\n{fn_ir}"
        );
        (root_slots(fn_ir), fn_ir.matches("@js_scope_alloc(").count())
    };
    let (grouped_wide, allocs_wide) = roots(12, true);
    let (grouped_narrow, allocs_narrow) = roots(2, true);
    assert_eq!(allocs_wide, 1, "one scope object for the whole group");
    assert_eq!(allocs_narrow, 1);
    assert_eq!(
        grouped_wide, grouped_narrow,
        "ten more grouped bindings must not add a single root"
    );
    let (cells_wide, _) = roots(12, false);
    let (cells_narrow, _) = roots(2, false);
    assert!(
        cells_wide >= cells_narrow + 10,
        "control: per-binding cells pay a root each ({cells_narrow} -> {cells_wide})"
    );
}

/// The closure side of the same model: the capturing closure holds ONE
/// capture slot for the group.
#[test]
fn a_closure_captures_a_group_through_one_slot() {
    let mut module = module_with(wide_body(8));
    group_scope_boxes(&mut module);
    let ir = compile_ir(&module);
    let fn_ir =
        crate::testing::root_slots::function_slice(&ir, "perry_fn_scope_env_probe_ts__probe1");
    // One capture slot: `js_closure_alloc*` with capture count 1.
    assert!(
        fn_ir.contains("i32 1)") || fn_ir.contains("i32 1, ptr"),
        "the closure's capture count must be 1:\n{fn_ir}"
    );
    assert_eq!(
        fn_ir.matches("@js_closure_set_box_capture_ptr(").count()
            + fn_ir.matches("@js_closure_set_capture_bits(").count(),
        1,
        "exactly one capture store for eight grouped bindings:\n{fn_ir}"
    );
}

/// Escape/mutation analysis: a binding written only BEFORE any closure that
/// captures it is created is captured by value and gets no cell at all; one
/// written after a capture is still boxed.
#[test]
fn writes_before_every_capture_need_no_cell() {
    let early_writes = vec![
        let_num(1, 0.0),
        Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Number(5.0)))),
        Stmt::Expr(Expr::Closure {
            func_id: 100,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(1)))],
            captures: vec![1],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
    ];
    assert!(!crate::boxed_vars::collect_boxed_vars(&early_writes).contains(&1));
    let mut late_write = early_writes.clone();
    late_write.push(Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Number(6.0)))));
    assert!(crate::boxed_vars::collect_boxed_vars(&late_write).contains(&1));
}
