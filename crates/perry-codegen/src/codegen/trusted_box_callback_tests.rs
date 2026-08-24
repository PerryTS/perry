//! Compiler-private box access is emitted only for an exact direct arrow
//! literal; the public closure body and indirect closure values stay checked.

use crate::{compile_module, CompileOptions};
use perry_hir::types::{FunctionType, Type};
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Param, Stmt, UpdateOp};
use std::collections::{HashMap, HashSet};

use super::closure_collect::{select_trusted_box_closures, select_versioned_loop_callbacks};

const COUNT: u32 = 10;
const CALLBACK: u32 = 20;
const CALLBACK_FUNC: u32 = 99;

fn param(id: u32, name: &str, ty: Type) -> Param {
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

fn callback_type() -> Type {
    Type::Function(FunctionType {
        params: Vec::new(),
        return_type: Box::new(Type::Void),
        is_async: false,
        is_generator: false,
    })
}

fn consume_function() -> Function {
    Function {
        id: 1,
        name: "consume".to_string(),
        type_params: Vec::new(),
        params: vec![param(2, "callback", callback_type())],
        return_type: Type::Void,
        body: Vec::new(),
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

fn outer_function(direct_literal: bool) -> Function {
    let mut body = vec![Stmt::Let {
        id: COUNT,
        name: "count".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    }];
    let call_arg = if direct_literal {
        callback()
    } else {
        body.push(Stmt::Let {
            id: CALLBACK,
            name: "callback".to_string(),
            ty: callback_type(),
            mutable: false,
            init: Some(callback()),
        });
        Expr::LocalGet(CALLBACK)
    };
    body.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![call_arg],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    Function {
        id: 3,
        name: "outer".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
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

fn callback() -> Expr {
    callback_with(CALLBACK_FUNC, Vec::new(), callback_body())
}

fn callback_body() -> Vec<Stmt> {
    vec![Stmt::Expr(Expr::Update {
        id: COUNT,
        op: UpdateOp::Increment,
        prefix: false,
    })]
}

fn callback_with(func_id: u32, params: Vec<Param>, body: Vec<Stmt>) -> Expr {
    Expr::Closure {
        func_id,
        params,
        return_type: Type::Void,
        body,
        captures: vec![COUNT],
        mutable_captures: vec![COUNT],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

fn versioned_callback(func_id: u32) -> Expr {
    const ENTITY: u32 = 29;
    const POS: u32 = 30;
    const VEL: u32 = 31;
    let property = |id, name: &str| Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: name.to_string(),
        byte_offset: 0,
    };
    callback_with(
        func_id,
        vec![
            param(ENTITY, "entity", Type::Any),
            param(POS, "pos", Type::Any),
            param(VEL, "vel", Type::Any),
        ],
        vec![Stmt::Expr(Expr::LocalSet(
            COUNT,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(COUNT)),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(property(POS, "x")),
                    right: Box::new(property(VEL, "vx")),
                }),
            }),
        ))],
    )
}

fn select(closures: Vec<(u32, Expr)>, direct: impl IntoIterator<Item = u32>) -> HashSet<u32> {
    select_trusted_box_closures(
        &closures,
        &direct.into_iter().collect(),
        &HashSet::from([COUNT]),
        &HashMap::new(),
        &HashSet::new(),
    )
    .into_keys()
    .collect()
}

fn emit(direct_literal: bool) -> String {
    let mut module = Module::new("trusted_box_callback.ts");
    module.init_kind = ModuleInitKind::Eager;
    module.functions = vec![consume_function(), outer_function(direct_literal)];
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(3)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }));

    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn emit_versioned_callback() -> String {
    const VERSIONED_FUNC: u32 = 100;
    let mut outer = outer_function(true);
    let call = outer.body.last_mut().expect("outer call exists");
    let Stmt::Expr(Expr::Call { args, .. }) = call else {
        panic!("outer tail is a call");
    };
    args[0] = versioned_callback(VERSIONED_FUNC);

    let mut module = Module::new("versioned_loop_callback.ts");
    module.init_kind = ModuleInitKind::Eager;
    module.functions = vec![consume_function(), outer];
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(3)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body(ir: &str, symbol: &str) -> String {
    let start = ir
        .lines()
        .position(|line| line.starts_with("define") && line.contains(&format!("@{symbol}(")))
        .unwrap_or_else(|| panic!("missing definition for {symbol}:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn direct_arrow_gets_a_private_body_but_keeps_the_public_validation_path() {
    let ir = emit(true);
    let public = function_body(&ir, "perry_closure_trusted_box_callback_ts__99");
    let trusted = function_body(
        &ir,
        "perry_closure_trusted_box_callback_ts__99$trusted_boxes",
    );

    assert!(public.contains("@js_box_get_bits("), "{public}");
    assert!(public.contains("@js_box_set_bits("), "{public}");
    assert!(!public.contains("@js_box_get_bits_trusted("), "{public}");
    assert!(
        !public.contains("@js_box_set_bits_trusted_no_barrier("),
        "{public}"
    );

    // The exact clone reads its immutable raw-box capture pointer once from
    // the closure entry layout, then directly accesses the non-moving cell.
    // The trusted getter remains only as the cold TDZ/suppression fallback;
    // normal writes need no helper at all.
    assert!(trusted.contains("getelementptr i8, ptr"), "{trusted}");
    assert!(trusted.contains(", i64 16"), "{trusted}");
    assert!(trusted.contains("inttoptr i64"), "{trusted}");
    assert!(trusted.contains("load i64, ptr"), "{trusted}");
    assert!(trusted.contains("store i64"), "{trusted}");
    assert!(trusted.contains(crate::nanbox::TAG_TDZ_I64), "{trusted}");
    assert!(trusted.contains("trusted_box.tdz"), "{trusted}");
    assert!(trusted.contains("@js_box_get_bits_trusted("), "{trusted}");
    assert!(
        !trusted.contains("@js_box_set_bits_trusted_no_barrier("),
        "{trusted}"
    );
    assert!(
        !trusted.contains("@js_closure_get_capture_bits("),
        "{trusted}"
    );
    assert!(!trusted.contains("@js_box_get_bits("));
    assert!(!trusted.contains("@js_box_set_bits("));
    assert!(trusted.contains("@js_write_barrier("));

    assert!(ir.contains(
        "@js_register_closure_trusted_direct(ptr @perry_closure_trusted_box_callback_ts__99, ptr @perry_closure_trusted_box_callback_ts__99$trusted_boxes, i32 1, i64 1)"
    ));
}

#[test]
fn additive_property_callback_gets_a_cold_deopting_private_body() {
    let ir = emit_versioned_callback();
    let special = function_body(
        &ir,
        "perry_closure_versioned_loop_callback_ts__100$trusted_boxes$versioned_loop",
    );
    assert!(
        special.lines().next().is_some_and(|line| {
            !line.contains("ptr %versioned_loop_deopt") && line.contains(" internal ")
        }),
        "the private ABI must reuse the unused first callback argument:\n{special}"
    );
    assert!(
        special.contains("pic.miss.call")
            && special.contains("js_object_get_field_ic_miss")
            && special.contains("guarded_add.dynamic")
            && special.contains("versioned_callback.deopt.mark"),
        "both observable cold arms must poison the loop before fallback:\n{special}"
    );
    assert!(ir.contains(
        "@js_register_closure_versioned_loop_direct(ptr @perry_closure_versioned_loop_callback_ts__100, ptr @perry_closure_versioned_loop_callback_ts__100$trusted_boxes$versioned_loop, i32 1, i64 1)"
    ));
}

#[test]
fn versioned_callback_selector_rejects_calls_and_heap_writes() {
    const VERSIONED_FUNC: u32 = 100;
    let eligible = versioned_callback(VERSIONED_FUNC);
    let closures = vec![(VERSIONED_FUNC, eligible.clone())];
    let direct = HashSet::from([VERSIONED_FUNC]);
    let boxed = HashSet::from([COUNT]);
    let globals = HashMap::new();
    let trusted =
        select_trusted_box_closures(&closures, &direct, &boxed, &globals, &HashSet::new());
    assert!(
        select_versioned_loop_callbacks(&closures, &trusted, &boxed, &globals)
            .contains(&VERSIONED_FUNC)
    );

    for rejected_body in [
        vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(30)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })],
        vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(30)),
            property: "x".to_string(),
            value: Box::new(Expr::Integer(1)),
        })],
    ] {
        let rejected = callback_with(
            VERSIONED_FUNC,
            vec![param(30, "value", Type::Any)],
            rejected_body,
        );
        let closures = vec![(VERSIONED_FUNC, rejected)];
        assert!(select_versioned_loop_callbacks(&closures, &trusted, &boxed, &globals).is_empty());
    }

    let used_first_param = callback_with(
        VERSIONED_FUNC,
        vec![param(30, "value", Type::Any)],
        vec![Stmt::Expr(Expr::LocalSet(
            COUNT,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(COUNT)),
                right: Box::new(Expr::LocalGet(30)),
            }),
        ))],
    );
    let closures = vec![(VERSIONED_FUNC, used_first_param)];
    let trusted =
        select_trusted_box_closures(&closures, &direct, &boxed, &globals, &HashSet::new());
    assert!(select_versioned_loop_callbacks(&closures, &trusted, &boxed, &globals).is_empty());
}

#[test]
fn closure_first_stored_as_a_value_does_not_get_a_trusted_body() {
    let ir = emit(false);
    assert!(!ir.contains("$trusted_boxes"));
    assert!(!ir.contains("call void @js_register_closure_trusted_direct("));
}

#[test]
fn default_and_rest_parameter_callbacks_stay_on_the_public_body() {
    let mut default_param = param(30, "value", Type::Number);
    default_param.default = Some(Expr::Integer(1));
    let mut rest_param = param(31, "values", Type::Array(Box::new(Type::Number)));
    rest_param.is_rest = true;

    let closures = vec![
        (
            100,
            callback_with(100, vec![default_param], callback_body()),
        ),
        (101, callback_with(101, vec![rest_param], callback_body())),
    ];
    assert!(select(closures, [100, 101]).is_empty());
}

#[test]
fn oversized_callback_body_is_not_cloned() {
    // Each expression statement contributes two HIR nodes, so this is over
    // the 64-node clone budget even before considering any future overhead.
    let body = (0..33).map(|_| Stmt::Expr(Expr::LocalGet(COUNT))).collect();
    assert!(select(vec![(102, callback_with(102, Vec::new(), body))], [102]).is_empty());
}

#[test]
fn module_clone_budget_selects_only_the_sixteen_cheapest_candidates() {
    let closures: Vec<_> = (1..=17)
        .map(|func_id| (func_id, callback_with(func_id, Vec::new(), callback_body())))
        .collect();
    let selected = select(closures, 1..=17);

    assert_eq!(selected.len(), 16);
    assert!((1..=16).all(|func_id| selected.contains(&func_id)));
    assert!(!selected.contains(&17));
}
