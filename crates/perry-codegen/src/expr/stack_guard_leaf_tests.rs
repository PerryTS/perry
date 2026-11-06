//! A body none of whose calls can run JS drops its entry stack check
//! (`stack_guard::finish_stack_guard`); any body that can recurse keeps it.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};

fn function(id: u32, name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn call(id: u32) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::FuncRef(id)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

/// The body of `@perry_fn_stack_guard_leaf_ts__<name>`.
fn body_of<'a>(ir: &'a str, name: &str) -> &'a str {
    let header = format!("@perry_fn_stack_guard_leaf_ts__{name}(");
    let start = ir
        .lines()
        .position(|l| l.starts_with("define") && l.contains(&header))
        .unwrap_or_else(|| panic!("no {name} in:\n{ir}"));
    let offset: usize = ir.lines().take(start).map(|l| l.len() + 1).sum();
    let rest = &ir[offset..];
    &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
}

#[test]
fn a_leaf_drops_its_check_and_a_recursive_body_keeps_it() {
    let mut m = Module::new("stack_guard_leaf.ts");
    m.functions = vec![
        function(1, "leaf", vec![Stmt::Return(Some(Expr::Number(1.0)))]),
        // The self-call sits in the body's first block, the block the check
        // branches to.
        function(2, "rec", vec![Stmt::Return(Some(call(2)))]),
        function(3, "outer", vec![Stmt::Return(Some(call(1)))]),
    ];
    m.init = vec![Stmt::Expr(call(2)), Stmt::Expr(call(3))];
    m.init_kind = ModuleInitKind::Eager;
    let ir = String::from_utf8(
        compile_module(&m, super::class_field_barrier_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        !body_of(&ir, "leaf").contains("@js_stack_overflow"),
        "a body that calls nothing needs no check:\n{}",
        body_of(&ir, "leaf")
    );
    assert!(
        body_of(&ir, "rec").contains("@js_stack_overflow"),
        "a self-recursive body keeps its check:\n{}",
        body_of(&ir, "rec")
    );
    assert!(
        body_of(&ir, "outer").contains("@js_stack_overflow"),
        "a call to user code keeps the check:\n{}",
        body_of(&ir, "outer")
    );
}

/// A body whose only user call sits on one branch checks in front of that
/// branch's block instead of at entry: the other path runs no check.
#[test]
fn a_call_on_one_branch_moves_the_check_in_front_of_its_block() {
    let mut m = Module::new("stack_guard_leaf.ts");
    let mut branchy = function(
        2,
        "branchy",
        vec![
            Stmt::If {
                condition: Expr::LocalGet(10),
                then_branch: vec![Stmt::Return(Some(call(1)))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::Number(2.0))),
        ],
    );
    branchy.params = vec![perry_hir::Param {
        id: 10,
        name: "flag".to_string(),
        ty: Type::Boolean,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }];
    m.functions = vec![
        // Recursive, so the call to it can run JS.
        function(1, "rec", vec![Stmt::Return(Some(call(1)))]),
        branchy,
    ];
    m.init = vec![Stmt::Expr(Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::FuncRef(2)),
        args: vec![Expr::Bool(true)],
        type_args: Vec::new(),
        byte_offset: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    let ir = String::from_utf8(
        compile_module(&m, super::class_field_barrier_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    let body = body_of(&ir, "branchy$generic");
    let entry: Vec<&str> = body
        .lines()
        .skip(1)
        .take_while(|l| !l.ends_with(':') || l.starts_with("entry"))
        .collect();
    assert!(
        !entry.iter().any(|l| l.contains("PERRY_HOT_TSD_KEY") || l.contains("read_register")),
        "the entry block runs no check:\n{body}"
    );
    assert!(body.contains("@js_stack_overflow"), "the calling arm keeps one:\n{body}");
    // The check is laid out just before the block holding the call.
    let lines: Vec<&str> = body.lines().collect();
    let check = lines
        .iter()
        .position(|l| l.starts_with("stack_guard.call."))
        .unwrap_or_else(|| panic!("no moved check:\n{body}"));
    let call_line = lines
        .iter()
        .position(|l| l.contains("@perry_fn_stack_guard_leaf_ts__rec"))
        .unwrap_or_else(|| panic!("no call:\n{body}"));
    assert!(check < call_line, "the check precedes the call:\n{body}");
}
