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
use perry_hir::{Class, ClassField, Function, Module, ModuleInitKind, Param};

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

// ───────────────────────────────────────────────────────────────────────────
// #7512-followup: the SAME predicate across an inheritance chain.
//
// The single-class rule bails to the empty set on any heritage, so a subclass
// instance never got an at-allocation typed-shape declaration and every
// raw-f64 store in every constructor on its chain — the BASE class's own
// `this.x = x` included — missed `GC_OBJ_TYPED_LAYOUT_INTACT` and fell back to
// the by-name `js_put_value_set`. Counted on `shapes.ts`: 528 000 by-name
// stores, and a two-class probe measured 2.0x against the flattened class.
// ───────────────────────────────────────────────────────────────────────────

fn named_class(
    name: &str,
    extends: Option<&str>,
    fields: Vec<ClassField>,
    ctor: Option<Function>,
) -> Class {
    let mut c = class(fields, ctor);
    c.name = name.to_string();
    c.extends_name = extends.map(str::to_string);
    c
}

fn super_call(args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::SuperCall(args))
}

fn chain_of(classes: &[Class]) -> std::collections::HashMap<String, &Class> {
    classes.iter().map(|c| (c.name.clone(), c)).collect()
}

fn chain_sets(
    map: &std::collections::HashMap<String, &Class>,
    leaf: &str,
) -> Option<Vec<(String, Vec<String>)>> {
    chain_prologue_assigned_fields(map, leaf)
        .map(|v| v.into_iter().map(|(n, s)| (n, sorted(s))).collect())
}

/// The `shapes.ts` shape: `Rect extends Base`, both constructors opening with
/// a plain prologue, the derived one after a `super(...)` whose arguments are
/// plain parameters.
#[test]
fn chain_prologue_covers_base_and_derived() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(vec![param(1, "x")], vec![user_this_assign("x", 1)])),
    );
    let derived = named_class(
        "Derived",
        Some("Base"),
        vec![field("w")],
        Some(func(
            vec![param(2, "x"), param(3, "w")],
            vec![
                super_call(vec![Expr::LocalGet(2)]),
                user_this_assign("w", 3),
            ],
        )),
    );
    let all = vec![base, derived];
    let map = chain_of(&all);
    assert_eq!(
        chain_sets(&map, "Derived"),
        Some(vec![
            ("Base".to_string(), vec!["x".to_string()]),
            ("Derived".to_string(), vec!["w".to_string()]),
        ])
    );
    let chain = chain_prologue_assigned_fields(&map, "Derived").unwrap();
    assert!(crate::typed_shape::class_chain_layout_declarable_at_allocation(&map, &chain));
}

/// A FIELDLESS subclass assigns nothing, and that is a QUALIFIED answer, not a
/// disqualification. The pre-followup API conflated "disqualified" and
/// "qualified but assigns nothing" into one empty set, which is exactly what
/// made a chain unanalysable a class at a time. `Marker extends Shape` in
/// `shapes.ts` is this case.
#[test]
fn fieldless_subclass_is_qualified_with_an_empty_set() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(vec![param(1, "x")], vec![user_this_assign("x", 1)])),
    );
    let marker = named_class(
        "Marker",
        Some("Base"),
        vec![],
        Some(func(
            vec![param(2, "x")],
            vec![super_call(vec![Expr::LocalGet(2)])],
        )),
    );
    let all = vec![base, marker];
    let map = chain_of(&all);
    assert_eq!(
        chain_sets(&map, "Marker"),
        Some(vec![
            ("Base".to_string(), vec!["x".to_string()]),
            ("Marker".to_string(), vec![]),
        ])
    );
    let chain = chain_prologue_assigned_fields(&map, "Marker").unwrap();
    assert!(crate::typed_shape::class_chain_layout_declarable_at_allocation(&map, &chain));
}

/// `Shape.made = Shape.made + 1` — a static bump AFTER the prologue run and
/// BEFORE the subclass's own field writes. Admitted because `this` appears
/// nowhere in it, which is what proves it cannot read a raw-f64 slot that is
/// still holding the allocator's `undefined` fill.
#[test]
fn this_free_trailing_statement_does_not_disqualify() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(
            vec![param(1, "x")],
            vec![
                user_this_assign("x", 1),
                Stmt::Expr(Expr::Binary {
                    op: perry_hir::BinaryOp::Add,
                    left: Box::new(Expr::Number(1.0)),
                    right: Box::new(Expr::Number(2.0)),
                }),
            ],
        )),
    );
    let all = vec![base];
    let map = chain_of(&all);
    assert_eq!(
        chain_sets(&map, "Base"),
        Some(vec![("Base".to_string(), vec!["x".to_string()])])
    );
}

/// ...but a trailing statement that DOES mention `this` disqualifies the whole
/// chain. This is the soundness pin: in `Base extends nothing`, `Derived`'s
/// `this.w` is still unwritten when `Base`'s body finishes, so a `this` read
/// there would see `undefined`'s NaN-box bits through a declared raw-f64 mask
/// and yield `NaN`.
#[test]
fn trailing_statement_mentioning_this_disqualifies() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(
            vec![param(1, "x")],
            vec![
                user_this_assign("x", 1),
                Stmt::Expr(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "w".to_string(),
                    byte_offset: 0,
                }),
            ],
        )),
    );
    let all = vec![base];
    let map = chain_of(&all);
    assert_eq!(chain_sets(&map, "Base"), None);
}

/// A raw-f64 field nobody's prologue assigns leaves a slot that would be read
/// as a double while it still holds `undefined`. The chain must be refused.
#[test]
fn uncovered_raw_f64_field_refuses_the_declaration() {
    let base = named_class(
        "Base",
        None,
        vec![field("x"), field("never_assigned")],
        Some(func(vec![param(1, "x")], vec![user_this_assign("x", 1)])),
    );
    let derived = named_class(
        "Derived",
        Some("Base"),
        vec![],
        Some(func(
            vec![param(2, "x")],
            vec![super_call(vec![Expr::LocalGet(2)])],
        )),
    );
    let all = vec![base, derived];
    let map = chain_of(&all);
    let chain = chain_prologue_assigned_fields(&map, "Derived").unwrap();
    assert!(!crate::typed_shape::class_chain_layout_declarable_at_allocation(&map, &chain));
}

/// A derived constructor that runs anything before `super(...)` may observe
/// arbitrary state; refuse it rather than reason about it.
#[test]
fn statement_before_super_disqualifies() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(vec![param(1, "x")], vec![user_this_assign("x", 1)])),
    );
    let derived = named_class(
        "Derived",
        Some("Base"),
        vec![field("w")],
        Some(func(
            vec![param(2, "x"), param(3, "w")],
            vec![
                Stmt::Expr(Expr::Number(1.0)),
                super_call(vec![Expr::LocalGet(2)]),
                user_this_assign("w", 3),
            ],
        )),
    );
    let all = vec![base, derived];
    let map = chain_of(&all);
    assert_eq!(chain_sets(&map, "Derived"), None);
}

/// A `super(...)` argument that could observe `this` hands the parent
/// constructor the half-built instance.
#[test]
fn super_argument_mentioning_this_disqualifies() {
    let base = named_class(
        "Base",
        None,
        vec![field("x")],
        Some(func(vec![param(1, "x")], vec![user_this_assign("x", 1)])),
    );
    let derived = named_class(
        "Derived",
        Some("Base"),
        vec![field("w")],
        Some(func(
            vec![param(2, "x"), param(3, "w")],
            vec![super_call(vec![Expr::This]), user_this_assign("w", 3)],
        )),
    );
    let all = vec![base, derived];
    let map = chain_of(&all);
    assert_eq!(chain_sets(&map, "Derived"), None);
}

/// A named parent this module cannot resolve is a constructor we cannot
/// analyse — refuse rather than assume it is inert.
#[test]
fn unresolvable_parent_disqualifies() {
    let derived = named_class(
        "Derived",
        Some("SomewhereElse"),
        vec![field("w")],
        Some(func(vec![param(3, "w")], vec![user_this_assign("w", 3)])),
    );
    let all = vec![derived];
    let map = chain_of(&all);
    assert_eq!(chain_sets(&map, "Derived"), None);
}

/// The single-class predicate must be UNCHANGED by the followup: a class with
/// heritage still gets the empty set from it, so every existing caller keeps
/// its old answer and only the new chain form widens anything.
#[test]
fn single_class_predicate_still_refuses_heritage() {
    let derived = named_class(
        "Derived",
        Some("Base"),
        vec![field("w")],
        Some(func(vec![param(3, "w")], vec![user_this_assign("w", 3)])),
    );
    assert!(ctor_prologue_param_assigned_fields(&derived).is_empty());
}

fn captures_this_arrow_field() -> ClassField {
    ClassField {
        name: "createEntity".to_string(),
        key_expr: None,
        ty: Type::Any,
        init: Some(Expr::Closure {
            func_id: 8693,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::This),
                property: "value".to_string(),
                byte_offset: 0,
            }))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: true,
            captures_new_target: false,
            enclosing_class: Some("ArrowRegistry".to_string()),
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn captures_this_field_ir() -> String {
    let mut registry = class(
        vec![
            ClassField {
                name: "value".to_string(),
                key_expr: None,
                ty: Type::Number,
                init: Some(Expr::Number(1.0)),
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            },
            captures_this_arrow_field(),
        ],
        None,
    );
    registry.id = 8693;
    registry.name = "ArrowRegistry".to_string();

    let mut module = Module::new("issue_8693_arrow_field.ts");
    module.classes = vec![registry];
    module.init = vec![Stmt::Expr(Expr::New {
        class_name: "ArrowRegistry".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })];
    module.init_kind = ModuleInitKind::Eager;
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(crate::compile_module(&module, opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

/// #8693: a captures-`this` arrow field on a fresh ordinary class instance
/// must populate the field already present in the allocation's class-key
/// shape. Full `DefineOwnProperty` marks the receiver dynamically shaped, so
/// every exact-shape method guard inside the arrow would miss forever (the
/// perform-ecs `createEntity = (...) => this.addComponentsToEntity(...)` case).
#[test]
fn captures_this_arrow_field_preserves_the_predeclared_class_shape() {
    let ir = captures_this_field_ir();
    assert!(
        ir.contains("call void @js_object_set_field_by_name("),
        "the arrow field must fill its existing own slot:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_class_field_add("),
        "the ordinary arrow field must not dynamically reshape its receiver:\n{ir}"
    );
}
