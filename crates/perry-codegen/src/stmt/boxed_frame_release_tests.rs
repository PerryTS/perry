//! #10464: an ordinary frame releases the box cells it minted.
//!
//! Each fixture pairs the released shape with the holder the runtime cannot
//! count, so the assertions discriminate in both directions: dropping the
//! frame release loses the positive assertions, releasing too much trips the
//! negative ones.

use perry_hir::types::Type;
use perry_hir::{ArgumentsObjectMeta, Expr, Function, Module, Param, Stmt};

const RELEASE: &str = "call void @js_box_scope_release(i64 ";

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn closure(func_id: u32, body: Vec<Stmt>, captures: Vec<u32>) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body,
        mutable_captures: captures.clone(),
        captures,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

fn let_stmt(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

fn function(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 1,
        name: name.into(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
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

fn function_ir(f: Function) -> String {
    let mut module = Module::new("boxed_frame_release.ts");
    let needle = format!("__{}(", f.name);
    module.functions.push(f);
    let ir = String::from_utf8(
        crate::compile_module(&module, super::prealloc_module_global_tests::ir_opts()).unwrap(),
    )
    .unwrap();
    ir.split("\ndefine ")
        .find(|block| block.lines().next().is_some_and(|l| l.contains(&needle)))
        .unwrap_or_else(|| panic!("no define for {needle}:\n{ir}"))
        .to_string()
}

/// Every `ret` of the frame is preceded by one release per slot it names.
fn releases_before_each_ret(ir: &str) -> Vec<usize> {
    let lines: Vec<&str> = ir.lines().collect();
    let mut counts = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("ret ") {
            let mut n = 0;
            for prev in lines[..i].iter().rev() {
                let t = prev.trim_start();
                if t.starts_with("call void @js_shadow_frame_pop") || t.contains("= load i64, ptr")
                {
                    continue;
                }
                if t.starts_with(RELEASE) {
                    n += 1;
                    continue;
                }
                break;
            }
            counts.push(n);
        }
    }
    counts
}

#[test]
fn captured_reassigned_let_is_released_at_every_return() {
    // function counter(flag) { let n = 0; const inc = () => { n = 1 };
    //   if (flag) return inc; return n; }
    let body = vec![
        let_stmt(10, Expr::Integer(0)),
        let_stmt(
            11,
            closure(
                2,
                vec![Stmt::Expr(Expr::LocalSet(10, Box::new(Expr::Integer(1))))],
                vec![10],
            ),
        ),
        Stmt::If {
            condition: Expr::LocalGet(1),
            then_branch: vec![Stmt::Return(Some(Expr::LocalGet(11)))],
            else_branch: None,
        },
        Stmt::Return(Some(Expr::LocalGet(10))),
    ];
    let ir = function_ir(function("counter", vec![param(1, "flag")], body));
    assert!(
        ir.contains("call i64 @js_box_alloc_bits("),
        "premise: n is boxed\n{ir}"
    );
    assert!(
        ir.contains("js_closure_set_box_capture_ptr"),
        "premise: counted edge\n{ir}"
    );
    let per_ret = releases_before_each_ret(&ir);
    assert!(per_ret.len() >= 2, "both returns must be lowered:\n{ir}");
    assert!(
        per_ret.iter().all(|n| *n == 1),
        "each return releases the single frame-owned cell ({per_ret:?}):\n{ir}"
    );
    // Outside a loop the declaration runs once: no previous-iteration release.
    let alloc = ir.find("call i64 @js_box_alloc_bits(").unwrap();
    assert!(
        !ir[..alloc].contains(RELEASE),
        "no release before a one-shot mint:\n{ir}"
    );
}

#[test]
fn loop_declaration_releases_the_previous_iterations_cell_before_minting() {
    // while (flag) { let x = 0; keep = () => { x = 1 }; }
    let body = vec![
        Stmt::While {
            condition: Expr::LocalGet(1),
            body: vec![
                let_stmt(20, Expr::Integer(0)),
                let_stmt(
                    21,
                    closure(
                        3,
                        vec![Stmt::Expr(Expr::LocalSet(20, Box::new(Expr::Integer(1))))],
                        vec![20],
                    ),
                ),
            ],
        },
        Stmt::Return(Some(Expr::Undefined)),
    ];
    let ir = function_ir(function("looped", vec![param(1, "flag")], body));
    let lines: Vec<&str> = ir.lines().map(str::trim_start).collect();
    let alloc = lines
        .iter()
        .position(|l| l.contains("call i64 @js_box_alloc_bits("))
        .expect("premise: x is boxed");
    let cell = lines[alloc].split(" = ").next().unwrap();
    let slot = lines[alloc..]
        .iter()
        .find_map(|l| l.strip_prefix(&format!("store i64 {cell}, ptr ")))
        .expect("the minted cell is stored in its slot");
    let previous = lines[..alloc]
        .iter()
        .rev()
        .take(4)
        .find_map(|l| l.strip_suffix(&format!(" = load i64, ptr {slot}")))
        .unwrap_or_else(|| panic!("the slot's previous cell is loaded before minting:\n{ir}"));
    assert!(
        lines[..alloc]
            .iter()
            .rev()
            .take(4)
            .any(|l| l.starts_with(&format!("{RELEASE}{previous})"))),
        "and released before the next iteration's cell is minted:\n{ir}"
    );
    assert!(
        releases_before_each_ret(&ir).iter().all(|n| *n == 1),
        "{ir}"
    );
}

#[test]
fn mapped_arguments_parameter_is_never_released_by_its_frame() {
    // Sloppy `function f(a, b) { const g = () => { a = 1; b = 2 }; return arguments; }`
    // with only `a` mapped: the Arguments object aliases a's cell raw.
    let mut arguments = param(3, "arguments");
    arguments.arguments_object = Some(ArgumentsObjectMeta {
        strict: false,
        simple_parameters: true,
        mapped_parameter_ids: vec![(0, 1)],
        restricted_callee: false,
    });
    let body = vec![
        let_stmt(
            30,
            closure(
                4,
                vec![
                    Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Integer(1)))),
                    Stmt::Expr(Expr::LocalSet(2, Box::new(Expr::Integer(2)))),
                ],
                vec![1, 2],
            ),
        ),
        Stmt::Return(Some(Expr::LocalGet(3))),
    ];
    let mut f = function(
        "sloppy",
        vec![param(1, "a"), param(2, "b"), arguments],
        body,
    );
    f.is_strict = false;
    let ir = function_ir(f);
    assert!(
        ir.contains("js_arguments_object_map_index"),
        "premise: a is mapped\n{ir}"
    );
    assert_eq!(
        ir.matches("call i64 @js_box_alloc_bits(").count(),
        2,
        "premise: both parameters are boxed:\n{ir}"
    );
    let per_ret = releases_before_each_ret(&ir);
    assert!(
        !per_ret.is_empty() && per_ret.iter().all(|n| *n == 1),
        "only the unmapped parameter is released ({per_ret:?}):\n{ir}"
    );
}

#[test]
fn plain_async_step_counts_only_enclosing_cells_and_frame_keeps_its_own() {
    // An activation frame: OWN is named by the step closure's terminal
    // `ReleaseBoxes`, OUTER is an ordinary captured-and-reassigned local.
    const OWN: u32 = 40;
    const OUTER: u32 = 41;
    let step = closure(
        5,
        vec![
            Stmt::Expr(Expr::LocalSet(OUTER, Box::new(Expr::Integer(1)))),
            Stmt::Expr(Expr::LocalSet(OWN, Box::new(Expr::Integer(2)))),
            Stmt::ReleaseBoxes(vec![OWN]),
            Stmt::Return(Some(Expr::Undefined)),
        ],
        vec![OWN, OUTER],
    );
    let body = vec![
        Stmt::PreallocateBoxes(vec![OWN]),
        let_stmt(OUTER, Expr::Integer(0)),
        let_stmt(42, step),
        Stmt::Return(Some(Expr::LocalGet(42))),
    ];
    let ir = function_ir(function("activation", Vec::new(), body));
    assert_eq!(
        ir.matches("call i64 @js_box_alloc_bits(").count(),
        2,
        "premise: both cells are minted by this frame:\n{ir}"
    );
    assert_eq!(
        ir.matches("call void @js_closure_set_box_capture_ptr(")
            .count(),
        1,
        "the enclosing cell is a counted edge, the activation's own is not:\n{ir}"
    );
    let per_ret = releases_before_each_ret(&ir);
    assert!(
        !per_ret.is_empty() && per_ret.iter().all(|n| *n == 1),
        "the frame releases OUTER only; OWN belongs to the activation ({per_ret:?}):\n{ir}"
    );
}
