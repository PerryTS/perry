//! Unit tests for the #7469 / #7512 dead-default-field-init predicate.
//!
//! The whole point of these is the pair
//! `synthesized_and_user_ctor_prologues_agree` pins: the two HIR spellings of
//! `this.f = <param>` must be treated identically. #7469 shipped matching only
//! the SYNTHESIZED one, which is why the elision was measured working on an
//! object literal and was structurally unreachable for the hand-written class
//! its own changelog claimed to cover (#7512).

use super::*;
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Function, Param};

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn func(params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id: 0,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params,
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
    }
}

fn class(fields: Vec<ClassField>, constructor: Option<Function>) -> Class {
    Class {
        id: 0,
        name: "Node".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
    }
}

/// The shape a USER writes. `this.v = v` lowers to `Expr::PutValueSet`
/// (`perry-hir/src/lower/lower_expr/assignment.rs`), never
/// `Expr::PropertySet` — no source syntax produces the latter.
fn user_this_assign(property: &str, param_id: u32) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::This),
        key: Box::new(Expr::String(property.to_string())),
        value: Box::new(Expr::LocalGet(param_id)),
        receiver: Box::new(Expr::This),
        strict: true,
    })
}

/// The shape the COMPILER synthesizes for an anon-shape object-literal
/// constructor (`lower/context.rs::mint_anon_shape_class`).
fn synthesized_this_assign(property: &str, param_id: u32) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::This),
        property: property.to_string(),
        value: Box::new(Expr::LocalGet(param_id)),
    })
}

fn sorted(set: std::collections::HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort();
    v
}

/// #7512: a hand-written `constructor(v, w) { this.v = v; this.w = w }` must
/// qualify. Before the fix the predicate matched only `Expr::PropertySet`, so
/// a declared class emitted two extra class-field-set IC diamonds per
/// construction that the equivalent `{v, w}` literal did not — making the more
/// statically-known construction form the slower one.
#[test]
fn user_written_ctor_prologue_qualifies() {
    let c = class(
        vec![field("v"), field("w")],
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![user_this_assign("v", 1), user_this_assign("w", 2)],
        )),
    );
    assert_eq!(
        sorted(ctor_prologue_param_assigned_fields(&c)),
        vec!["v".to_string(), "w".to_string()]
    );
}

/// The regression pin for the ordering #7512 is about: the object-literal form
/// and the class form of the same program must elide the same field set. If
/// this ever diverges again, one construction form is silently paying
/// bookkeeping the other does not.
#[test]
fn synthesized_and_user_ctor_prologues_agree() {
    let synthesized = class(
        vec![field("v"), field("w")],
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![
                synthesized_this_assign("v", 1),
                synthesized_this_assign("w", 2),
            ],
        )),
    );
    let user = class(
        vec![field("v"), field("w")],
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![user_this_assign("v", 1), user_this_assign("w", 2)],
        )),
    );
    let synthesized_fields = sorted(ctor_prologue_param_assigned_fields(&synthesized));
    assert_eq!(synthesized_fields, vec!["v".to_string(), "w".to_string()]);
    assert_eq!(
        synthesized_fields,
        sorted(ctor_prologue_param_assigned_fields(&user))
    );
}

/// The prologue is the MAXIMAL LEADING run: a statement that is not a plain
/// `this.f = <param>` ends it, and every field assigned after it keeps its
/// default `undefined` write.
#[test]
fn prologue_stops_at_the_first_non_matching_statement() {
    let c = class(
        vec![field("v"), field("w")],
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![
                user_this_assign("v", 1),
                // A call can allocate, throw, and observe `this`, so `w`'s
                // default write is not provably dead.
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::LocalGet(9)),
                    args: Vec::new(),
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
                user_this_assign("w", 2),
            ],
        )),
    );
    assert_eq!(
        sorted(ctor_prologue_param_assigned_fields(&c)),
        vec!["v".to_string()]
    );
}

/// A non-parameter RHS is not covered by the "cannot throw, allocate, or
/// observe `this`" argument, so it does not open a prologue.
#[test]
fn non_param_rhs_does_not_qualify() {
    let c = class(
        vec![field("v")],
        Some(func(
            vec![param(1, "v")],
            vec![Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::This),
                key: Box::new(Expr::String("v".to_string())),
                // A body local, not a constructor parameter.
                value: Box::new(Expr::LocalGet(77)),
                receiver: Box::new(Expr::This),
                strict: true,
            })],
        )),
    );
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// A computed key (`this[k] = p`) evaluates an arbitrary expression to produce
/// the key, so it neither names a field statically nor is effect-free.
#[test]
fn computed_key_put_value_set_does_not_qualify() {
    let c = class(
        vec![field("v")],
        Some(func(
            vec![param(1, "v")],
            vec![Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::This),
                key: Box::new(Expr::LocalGet(5)),
                value: Box::new(Expr::LocalGet(1)),
                receiver: Box::new(Expr::This),
                strict: true,
            })],
        )),
    );
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// `PutValueSet` carries the object in both `target` and `receiver` and
/// codegen evaluates both; a receiver that is not `this` is a different
/// operation and must not be read as a prologue field write.
#[test]
fn non_this_receiver_does_not_qualify() {
    let c = class(
        vec![field("v")],
        Some(func(
            vec![param(1, "v")],
            vec![Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::This),
                key: Box::new(Expr::String("v".to_string())),
                value: Box::new(Expr::LocalGet(1)),
                receiver: Box::new(Expr::LocalGet(4)),
                strict: true,
            })],
        )),
    );
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// A same-named setter swallows the prologue store, leaving the elided
/// `undefined` as the only write that ever reached the slot.
#[test]
fn setter_shadowed_field_refuses_the_elision() {
    let mut c = class(
        vec![field("v"), field("w")],
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![user_this_assign("v", 1), user_this_assign("w", 2)],
        )),
    );
    c.setters
        .push(("v".to_string(), func(vec![param(3, "x")], Vec::new())));
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// A parameter default evaluates before the prologue and can, in the general
/// lowering, observe `this`.
#[test]
fn param_default_refuses_the_elision() {
    let mut params = vec![param(1, "v"), param(2, "w")];
    params[1].default = Some(Expr::Number(0.0));
    let c = class(
        vec![field("v"), field("w")],
        Some(func(
            params,
            vec![user_this_assign("v", 1), user_this_assign("w", 2)],
        )),
    );
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// A field carrying an initializer expression means the init phase runs user
/// code that may legally read an earlier field.
#[test]
fn field_initializer_refuses_the_elision() {
    let mut fields = vec![field("v"), field("w")];
    fields[1].init = Some(Expr::Number(1.0));
    let c = class(
        fields,
        Some(func(
            vec![param(1, "v"), param(2, "w")],
            vec![user_this_assign("v", 1), user_this_assign("w", 2)],
        )),
    );
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}

/// A derived class has its own `super()` machinery between the field-init
/// phase and the constructor body.
#[test]
fn derived_class_refuses_the_elision() {
    let mut c = class(
        vec![field("v")],
        Some(func(vec![param(1, "v")], vec![user_this_assign("v", 1)])),
    );
    c.extends_name = Some("Base".to_string());
    assert!(ctor_prologue_param_assigned_fields(&c).is_empty());
}
