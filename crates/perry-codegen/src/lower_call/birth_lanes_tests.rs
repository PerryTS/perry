//! Charter step 5, P4 option (a): the per-field `F64` birth set follows the
//! construction order (`birth_lanes::chain_birth_f64_fields`).

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Param, Stmt};

use super::birth_lanes::chain_birth_f64_fields;

fn field(name: &str, ty: Type, init: Option<Expr>) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn number_param(id: u32) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 700,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
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

fn class(
    name: &str,
    parent: Option<&str>,
    fields: Vec<ClassField>,
    constructor: Option<Function>,
) -> Class {
    Class {
        id: 500,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: parent.map(str::to_string),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor,
        methods: vec![function("m", Vec::new(), Vec::new())],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

/// `this.<f> = <value>` as user source lowers it.
fn store(f: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::This),
        key: Box::new(Expr::String(f.to_string())),
        value: Box::new(value),
        receiver: Box::new(Expr::This),
        strict: true,
    })
}

/// `this.m()`
fn call_m() -> Stmt {
    Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: "m".to_string(),
            byte_offset: 0,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })
}

/// The F64 set for `leaf` over `classes` (root first).
fn born_f64(classes: &[Class], leaf: &str) -> HashSet<String> {
    let map: HashMap<String, &Class> = classes.iter().map(|c| (c.name.clone(), c)).collect();
    let mut chain = Vec::new();
    let mut cur = map.get(leaf).copied();
    while let Some(c) = cur {
        chain.push((c.name.clone(), c.fields.clone()));
        cur = c.extends_name.as_deref().and_then(|p| map.get(p).copied());
    }
    chain.reverse();
    chain_birth_f64_fields(&map, &chain)
        .into_iter()
        .flat_map(|(_, set)| set)
        .collect()
}

fn set(names: &[&str]) -> HashSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn a_literal_initializer_is_a_first_write() {
    // `class L { a: number = 1; b: number }`: `b` is never written during
    // construction, so it must read `undefined` and stays `Any`.
    let l = class(
        "L",
        None,
        vec![
            field("a", Type::Number, Some(Expr::Integer(1))),
            field("b", Type::Number, None),
        ],
        None,
    );
    assert_eq!(born_f64(&[l], "L"), set(&["a"]));
}

#[test]
fn constructor_body_arithmetic_is_a_first_write() {
    // `class A { n: number; k: number; constructor(p1: number) {
    //   const t = p1 * 2; this.n = t + 1; this.k = -p1 } }`
    let ctor = function(
        "constructor",
        vec![number_param(1)],
        vec![
            Stmt::Let {
                id: 2,
                name: "t".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(Expr::LocalGet(1)),
                    right: Box::new(Expr::Integer(2)),
                }),
            },
            store(
                "n",
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::Integer(1)),
                },
            ),
            store(
                "k",
                Expr::Unary {
                    op: perry_hir::UnaryOp::Neg,
                    operand: Box::new(Expr::LocalGet(1)),
                },
            ),
        ],
    );
    let a = class(
        "A",
        None,
        vec![
            field("n", Type::Number, None),
            field("k", Type::Number, None),
        ],
        Some(ctor),
    );
    assert_eq!(born_f64(&[a], "A"), set(&["n", "k"]));
}

#[test]
fn a_this_read_before_the_write_keeps_the_field_any() {
    // `constructor(p1: number) { this.a = p1; this.m(); this.b = p1 }`:
    // `a` is written before the call, `b` after it.
    let ctor = function(
        "constructor",
        vec![number_param(1)],
        vec![
            store("a", Expr::LocalGet(1)),
            call_m(),
            store("b", Expr::LocalGet(1)),
        ],
    );
    let c = class(
        "C",
        None,
        vec![
            field("a", Type::Number, None),
            field("b", Type::Number, None),
        ],
        Some(ctor),
    );
    assert_eq!(born_f64(&[c], "C"), set(&["a"]));
    // An initializer that reads `this` observes every later field.
    let d = class(
        "D",
        None,
        vec![
            field(
                "a",
                Type::Any,
                Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "b".to_string(),
                    byte_offset: 0,
                }),
            ),
            field("b", Type::Number, Some(Expr::Integer(3))),
        ],
        None,
    );
    assert!(born_f64(&[d], "D").is_empty());
}

#[test]
fn a_subclass_field_read_early_through_an_override_stays_any() {
    // `class P { constructor() { this.m() } }` — `m` may be a subclass
    // override reading `this.x` before `S`'s constructor writes it.
    let p = class(
        "P",
        None,
        Vec::new(),
        Some(function("constructor", Vec::new(), vec![call_m()])),
    );
    let s_ctor = || {
        function(
            "constructor",
            vec![number_param(1)],
            vec![
                Stmt::Expr(Expr::SuperCall(Vec::new())),
                store("x", Expr::LocalGet(1)),
            ],
        )
    };
    let s = class(
        "S",
        Some("P"),
        vec![field("x", Type::Number, None)],
        Some(s_ctor()),
    );
    assert!(born_f64(&[p, s.clone()], "S").is_empty());
    // The same subclass under a parent that never touches `this`.
    let quiet = class(
        "P",
        None,
        Vec::new(),
        Some(function("constructor", Vec::new(), Vec::new())),
    );
    assert_eq!(born_f64(&[quiet, s], "S"), set(&["x"]));
}

#[test]
fn a_non_number_first_write_or_control_flow_keeps_the_field_any() {
    let ctor = function(
        "constructor",
        vec![number_param(1)],
        vec![
            store("a", Expr::String("s".to_string())),
            Stmt::If {
                condition: Expr::LocalGet(1),
                then_branch: Vec::new(),
                else_branch: None,
            },
            store("b", Expr::LocalGet(1)),
        ],
    );
    let c = class(
        "C",
        None,
        vec![
            field("a", Type::Number, None),
            field("b", Type::Number, None),
        ],
        Some(ctor),
    );
    assert!(born_f64(&[c], "C").is_empty());
}
