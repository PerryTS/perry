use super::*;
use perry_hir::types::Type;

fn param(id: LocalId, name: &str, default: Option<Expr>) -> Param {
    Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: FuncId, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.into(),
        type_params: vec![],
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn call(id: FuncId, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(id)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn optional(id: LocalId, value: Expr, body: Expr) -> Expr {
    Expr::ScopedTemp {
        id,
        value: Box::new(value),
        body: Box::new(Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::LooseEq,
                left: Box::new(Expr::LocalGet(id)),
                right: Box::new(Expr::Null),
            }),
            then_expr: Box::new(Expr::Undefined),
            else_expr: Box::new(body),
        }),
    }
}

fn property(id: LocalId) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: "value".into(),
    }
}

fn run_js(module: Module, checks: &str) {
    let js = JsEmitter::new("scoped_temp", false).emit_module(&module);
    // Scoped captures do not synthesize runtime closures or expression IIFEs.
    assert!(!js.contains("=>"), "{js}");
    assert!(!js.contains("(function"), "{js}");
    let result = std::process::Command::new("node")
        .arg("-e")
        .arg(format!("{js}\n{checks}"))
        .output()
        .expect("Node is required for JS backend semantics tests");
    assert!(
        result.status.success(),
        "{}\n{js}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn scoped_defaults_preserve_order_tdz_arity_and_skipped_initializers() {
    let mut module = Module::new("scoped_temp");
    module.globals.push(Global {
        id: 90,
        name: "outer".into(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Number(41.0)),
    });
    module.functions.push(function(
        1,
        "pick",
        vec![
            param(
                1,
                "first",
                Some(optional(100, call(99, vec![]), property(100))),
            ),
            param(2, "second", Some(Expr::LocalGet(1))),
        ],
        vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::LocalGet(2)),
        }))],
    ));
    module.functions.push(function(
        2,
        "later",
        vec![
            param(
                3,
                "early",
                Some(optional(101, call(99, vec![]), Expr::LocalGet(4))),
            ),
            param(4, "late", Some(Expr::Number(9.0))),
        ],
        vec![Stmt::Return(Some(Expr::LocalGet(3)))],
    ));
    module.functions.push(function(
        3,
        "shadow",
        vec![param(
            5,
            "selected",
            Some(optional(102, call(99, vec![]), Expr::GlobalGet(90))),
        )],
        vec![
            Stmt::Let {
                id: 200,
                name: "outer".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Number(99.0)),
            },
            Stmt::Return(Some(Expr::LocalGet(5))),
        ],
    ));
    run_js(
        module,
        r#"
const assert = require('node:assert/strict');
let calls = 0;
function _f99() { calls++; return {value: 7}; }
assert.equal(pick.length, 0);
assert.equal(pick(), 14); assert.equal(calls, 1);
assert.equal(pick(3), 6); assert.equal(calls, 1);
assert.equal(pick(undefined, 4), 11); assert.equal(calls, 2);
assert.throws(() => later(), ReferenceError); assert.equal(calls, 3);
assert.equal(later(8), 8); assert.equal(calls, 3);
assert.equal(shadow(), 41); assert.equal(calls, 4);
"#,
    );
}

#[test]
fn scoped_temps_are_private_to_recursive_activations() {
    let mut module = Module::new("scoped_temp");
    let recurse = call(
        1,
        vec![Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Number(1.0)),
        }],
    );
    let body = Expr::Conditional {
        condition: Box::new(Expr::Compare {
            op: CompareOp::Le,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Number(0.0)),
        }),
        then_expr: Box::new(Expr::Number(0.0)),
        else_expr: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(recurse),
            right: Box::new(Expr::LocalGet(100)),
        }),
    };
    module.functions.push(function(
        1,
        "sum",
        vec![param(1, "n", None)],
        vec![Stmt::Return(Some(Expr::ScopedTemp {
            id: 100,
            value: Box::new(Expr::LocalGet(1)),
            body: Box::new(body),
        }))],
    ));
    run_js(module, "require('node:assert/strict').equal(sum(4), 10);");
}

#[test]
fn generator_defaults_keep_creation_time_and_historical_complex_expansion() {
    let mut module = Module::new("scoped_temp");
    let mut simple = function(
        1,
        "simple",
        vec![param(
            1,
            "selected",
            Some(optional(100, call(99, vec![]), property(100))),
        )],
        vec![Stmt::Return(Some(Expr::LocalGet(1)))],
    );
    simple.is_generator = true;
    module.functions.push(simple);
    let mut complex = function(
        2,
        "complex",
        vec![param(
            2,
            "selected",
            Some(optional(
                101,
                call(99, vec![]),
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(property(101)),
                    right: Box::new(property(101)),
                },
            )),
        )],
        vec![Stmt::Return(Some(Expr::LocalGet(2)))],
    );
    complex.is_generator = true;
    module.functions.push(complex);
    run_js(
        module,
        r#"
const assert = require('node:assert/strict');
let calls = 0;
function _f99() { calls++; return {value: 7}; }
const a = simple(); assert.equal(calls, 1); assert.equal(a.next().value, 7);
const b = complex(); assert.equal(calls, 4); assert.equal(b.next().value, 14);
"#,
    );
}

#[test]
fn constructor_scoped_temps_survive_recursive_field_initialization_and_module_temps_clear() {
    let mut module = Module::new("scoped_temp");
    module.classes.push(Class {
        id: 1,
        name: "Box".into(),
        type_params: vec![],
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "result".into(),
            key_expr: None,
            ty: Type::Any,
            init: Some(optional(
                100,
                call(99, vec![]),
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(call(98, vec![])),
                    right: Box::new(property(100)),
                },
            )),
            is_private: false,
            is_readonly: false,
            decorators: vec![],
        }],
        constructor: None,
        methods: vec![],
        getters: vec![],
        setters: vec![],
        static_accessor_names: vec![],
        static_accessor_fn_ids: vec![],
        static_fields: vec![],
        static_methods: vec![],
        computed_members: vec![],
        decorators: vec![],
        is_exported: false,
        aliases: vec![],
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    });
    module.init.push(Stmt::Let {
        id: 200,
        name: "moduleValue".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(optional(
            1000,
            Expr::Object(vec![("value".into(), Expr::Number(17.0))]),
            property(1000),
        )),
    });
    run_js(
        module,
        r#"
const assert = require('node:assert/strict');
assert.equal(moduleValue, 17);
assert.equal(__perry_scoped_value_1000, undefined);
assert.equal(__perry_scoped_result_1000, undefined);
let calls = 0, nesting = false;
function _f99() { return {value: ++calls}; }
function _f98() { if (nesting) return 0; nesting = true; return new Box().result; }
assert.equal(new Box().result, 3);
assert.equal(calls, 2);
"#,
    );
}

#[test]
fn closure_scoped_temps_are_activation_local() {
    let mut module = Module::new("scoped_temp");
    module.init.push(Stmt::Let {
        id: 10,
        name: "sumClosure".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Closure {
            func_id: 10,
            params: vec![param(20, "n", None)],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::ScopedTemp {
                id: 100,
                value: Box::new(Expr::LocalGet(20)),
                body: Box::new(Expr::Conditional {
                    condition: Box::new(Expr::Compare {
                        op: CompareOp::Le,
                        left: Box::new(Expr::LocalGet(20)),
                        right: Box::new(Expr::Number(0.0)),
                    }),
                    then_expr: Box::new(Expr::Number(0.0)),
                    else_expr: Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::Call {
                            callee: Box::new(Expr::LocalGet(10)),
                            args: vec![Expr::Binary {
                                op: BinaryOp::Sub,
                                left: Box::new(Expr::LocalGet(20)),
                                right: Box::new(Expr::Number(1.0)),
                            }],
                            type_args: vec![],
                            byte_offset: 0,
                        }),
                        right: Box::new(Expr::LocalGet(100)),
                    }),
                }),
            }))],
            captures: vec![10],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
    });
    let js = JsEmitter::new("scoped_temp", false).emit_module(&module);
    assert_eq!(js.matches("=>").count(), 1, "{js}");
    let result = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(
            "{js}\nrequire('node:assert/strict').equal(sumClosure(4), 10);"
        ))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{js}",
        String::from_utf8_lossy(&result.stderr)
    );
}
