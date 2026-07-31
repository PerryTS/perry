//! #7034 §4 return-shape facts: both halves of the proof, and the cases that
//! must NOT get one.
//!
//! Every positive test here fails against the pre-#7034 collector (the local
//! was denied with rule 2 / the call was never a candidate at all), and every
//! negative test fails if the corresponding guard is deleted — the guards are
//! named in each test's doc so the sabotage is reproducible.

use super::*;
use crate::collectors::PtrShapeLocal;
use perry_hir::types::{FuncId, Type};
use perry_hir::{ClassField, Param};

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

fn class_c() -> Class {
    Class {
        id: 0,
        name: "C".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("x")],
        constructor: None,
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
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
    }
}

fn new_c() -> Expr {
    Expr::New {
        class_name: "C".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn let_c(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(new_c()),
    }
}

fn store_x(id: u32) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::LocalGet(id)),
        property: "x".to_string(),
        value: Box::new(Expr::Number(1.0)),
    })
}

fn function(id: FuncId, name: &str, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
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

/// A module with class `C` and the given functions, run through the real
/// `collect_module_dispatch_facts` so the barrier flags are the real ones.
fn facts_for(functions: Vec<Function>) -> (ModuleDispatchFacts, Class) {
    facts_for_classes(Vec::new(), functions)
}

/// `facts_for` plus extra declared classes. Needed whenever a test's intended
/// guard could be masked by "the class is not declared in this module", which
/// denies a fact for a different reason and would make the test vacuous.
fn facts_for_classes(extra: Vec<Class>, functions: Vec<Function>) -> (ModuleDispatchFacts, Class) {
    let mut hir = Module::new("t");
    hir.classes.push(class_c());
    hir.classes.extend(extra);
    hir.functions = functions;
    (super::super::collect_module_dispatch_facts(&hir), class_c())
}

/// A second admissible class, identical to `C` but for its identity.
fn class_d() -> Class {
    let mut d = class_c();
    d.id = 1;
    d.name = "D".to_string();
    d
}

fn classes_of(c: &Class) -> HashMap<String, &Class> {
    let mut m = HashMap::new();
    m.insert("C".to_string(), c);
    m
}

fn promote(
    stmts: &[Stmt],
    classes: &HashMap<String, &Class>,
    facts: &ModuleDispatchFacts,
) -> HashMap<u32, PtrShapeLocal> {
    super::super::ptr_shape::collect_shape_proven_ptr_locals(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        facts,
        &HashSet::new(),
        &super::super::ptr_shape_elements::collect_element_shape_facts(
            stmts,
            &HashSet::new(),
            &HashMap::new(),
            classes,
            facts,
        ),
    )
}

// ── Half 1: the producer-side return exemption ─────────────────────────────

/// `const o = new C(); o.x = 1; return o;` — the accumulator idiom. Before
/// #7034 §4 this was denied with rule 2 ("returned from this function").
///
/// Sabotage: delete the `!self.in_closure` / `Expr::LocalGet` arm in
/// `ptr_shape.rs`'s `Stmt::Return` and this fails.
#[test]
fn returned_local_is_promoted() {
    let c = class_c();
    let classes = classes_of(&c);
    let (facts, _) = facts_for(Vec::new());
    let stmts = vec![
        let_c(1, "acc"),
        store_x(1),
        Stmt::Return(Some(Expr::LocalGet(1))),
    ];
    let promoted = promote(&stmts, &classes, &facts);
    assert!(
        promoted.contains_key(&1),
        "a contained local whose only escape is `return o` must be promoted"
    );
}

/// The exemption is for the BARE form only. `return { wrapper: o }` (here the
/// generic container shape: an array literal) still embeds the object in a
/// value whose other references the walk has not bounded.
///
/// Sabotage: widen the `Stmt::Return` arm to exempt any return and this fails.
#[test]
fn return_of_a_container_holding_the_local_still_escapes() {
    let c = class_c();
    let classes = classes_of(&c);
    let (facts, _) = facts_for(Vec::new());
    let stmts = vec![
        let_c(1, "wrapped"),
        store_x(1),
        Stmt::Return(Some(Expr::Array(vec![Expr::LocalGet(1)]))),
    ];
    assert!(
        !promote(&stmts, &classes, &facts).contains_key(&1),
        "`return [o]` must still disqualify — the array outlives the frame"
    );
}

/// A `return o` inside a CLOSURE body is not a terminator for the enclosing
/// function's local: the closure can be invoked at an unbounded later time,
/// after the enclosing body has gone on using `o`.
///
/// Sabotage: drop the `in_closure` tracking and this fails.
#[test]
fn return_inside_a_closure_body_is_not_exempt() {
    let c = class_c();
    let classes = classes_of(&c);
    let (facts, _) = facts_for(Vec::new());
    let stmts = vec![
        let_c(1, "escapes"),
        Stmt::Expr(Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Any,
            // Deliberately NOT in `captures`: the point of the guard is to
            // hold even where capture analysis has not marked the reference.
            body: vec![Stmt::Return(Some(Expr::LocalGet(1)))],
            captures: Vec::new(),
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
    assert!(
        !promote(&stmts, &classes, &facts).contains_key(&1),
        "a closure body's `return o` must not license the enclosing local"
    );
}

// ── Half 2: the caller-side fact ───────────────────────────────────────────

/// The producer proven above becomes a fact, and `const r = producer()` is
/// then a rule-1 seed: `r.x` in the caller lowers guard-free.
///
/// Sabotage: return `None` from `producer_return_class`, or drop the
/// `return_seeded` arm in `ptr_shape.rs`'s `Stmt::Let`, and this fails.
#[test]
fn call_to_a_return_shape_producer_is_provenance() {
    let producer = function(
        7,
        "make",
        vec![
            let_c(1, "acc"),
            store_x(1),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );
    let (facts, c) = facts_for(vec![producer]);
    assert_eq!(
        facts.return_shape_class(7),
        Some("C"),
        "the producer must carry a return-shape fact"
    );

    let classes = classes_of(&c);
    let caller = vec![
        Stmt::Let {
            id: 20,
            name: "r".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(7)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        },
        store_x(20),
    ];
    let promoted = promote(&caller, &classes, &facts);
    let fact = promoted
        .get(&20)
        .expect("the call result must be a Ptr<Shape> candidate");
    assert_eq!(fact.class_name, "C");
    assert!(
        fact.numeric_fields.is_empty(),
        "a call-seeded candidate must never claim numeric fields: the \
         producer's own stores are outside this region"
    );
}

/// `return new C()` directly is fresh by construction — no body proof needed.
#[test]
fn direct_new_return_is_a_fact() {
    let (facts, _) = facts_for(vec![function(3, "mk", vec![Stmt::Return(Some(new_c()))])]);
    assert_eq!(facts.return_shape_class(3), Some("C"));
}

/// A producer that can fall off the end returns `undefined` on that path; a
/// caller treating the result as a proven `C` would load a field off it.
///
/// Sabotage: delete the `f.body.last()` check and this fails.
#[test]
fn producer_that_can_fall_through_gets_no_fact() {
    let (facts, _) = facts_for(vec![function(
        4,
        "maybe",
        vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![Stmt::Return(Some(new_c()))],
            else_branch: None,
        }],
    )]);
    assert_eq!(
        facts.return_shape_class(4),
        None,
        "an implicit `return undefined` path must deny the fact"
    );
}

/// A bare `return;` is the same hazard, spelled explicitly.
#[test]
fn bare_return_denies_the_fact() {
    let (facts, _) = facts_for(vec![function(
        5,
        "maybe2",
        vec![
            Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Return(None)],
                else_branch: None,
            },
            Stmt::Return(Some(new_c())),
        ],
    )]);
    assert_eq!(facts.return_shape_class(5), None);
}

/// The object must be FRESH. A producer that hands back a value it was given
/// (or read from anywhere else) is aliased, so no fact — this is the
/// `function get() { return CACHE; }` hazard in its smallest form: the
/// returned local is also passed to another function.
///
/// Sabotage: skip the `collect_shape_proven_ptr_locals` body proof and accept
/// any `return <local bound to a new>` — this fails.
#[test]
fn producer_whose_local_also_escapes_gets_no_fact() {
    let (facts, _) = facts_for(vec![function(
        6,
        "leaky",
        vec![
            let_c(1, "o"),
            // `stash(o)` — an alias the caller-side proof could not bound.
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::LocalGet(50)),
                args: vec![Expr::LocalGet(1)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    )]);
    assert_eq!(
        facts.return_shape_class(6),
        None,
        "a producer that also leaks the object must not carry a fact"
    );
}

/// Returns that disagree on the class carry no fact.
///
/// `D` is DECLARED in the module on purpose. With it undeclared the fact would
/// also be denied — by "the class is not in this module's table" — so the test
/// would pass with the agreement check deleted and would be asserting nothing.
///
/// Sabotage: change the `Some(_) => return None` arm in `producer_return_class`
/// to accept a disagreeing class and this fails.
#[test]
fn disagreeing_return_classes_get_no_fact() {
    let other = Expr::New {
        class_name: "D".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    };
    let (facts, _) = facts_for_classes(
        vec![class_d()],
        vec![function(
            8,
            "two",
            vec![
                Stmt::If {
                    condition: Expr::Bool(true),
                    then_branch: vec![Stmt::Return(Some(new_c()))],
                    else_branch: None,
                },
                Stmt::Return(Some(other)),
            ],
        )],
    );
    assert_eq!(facts.return_shape_class(8), None);

    // The control: the SAME module shape with both returns agreeing on `D`
    // does carry a fact. Without this, "denied" could still be an artifact of
    // the fixture rather than of the disagreement.
    let (facts, _) = facts_for_classes(
        vec![class_d()],
        vec![function(
            9,
            "one",
            vec![Stmt::Return(Some(Expr::New {
                class_name: "D".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }))],
        )],
    );
    assert_eq!(facts.return_shape_class(9), Some("D"));
}

/// An async producer's locals are boxed into one shared cell by the
/// async-to-generator transform, and its `return` is not a terminator in the
/// sense rule 2 needs.
#[test]
fn async_producer_gets_no_fact() {
    let mut f = function(9, "amk", vec![Stmt::Return(Some(new_c()))]);
    f.is_async = true;
    let (facts, _) = facts_for(vec![f]);
    assert_eq!(facts.return_shape_class(9), None);
}

/// Rule 5: any §5.2 barrier in the module denies every return-shape fact,
/// exactly as it denies every local.
#[test]
fn module_barrier_denies_every_fact() {
    let mut hir = Module::new("t");
    hir.classes.push(class_c());
    hir.functions = vec![function(10, "mk", vec![Stmt::Return(Some(new_c()))])];
    // `delete o.x` anywhere in the module is a shape barrier.
    hir.init = vec![Stmt::Expr(Expr::Delete(Box::new(Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(1)),
        property: "x".to_string(),
        byte_offset: 0,
    })))];
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert!(facts.has_shape_barrier_sites());
    assert_eq!(facts.return_shape_class(10), None);
}

/// Only a direct `Expr::FuncRef` callee names a statically-known function.
/// A computed callee could be rebound between the fact and the call.
#[test]
fn indirect_callee_is_not_seeded() {
    let (facts, c) = facts_for(vec![function(11, "mk", vec![Stmt::Return(Some(new_c()))])]);
    assert_eq!(facts.return_shape_class(11), Some("C"));
    let classes = classes_of(&c);
    let caller = vec![
        Stmt::Let {
            id: 30,
            name: "r".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                // A closure value in a local, not a FuncRef.
                callee: Box::new(Expr::LocalGet(31)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        },
        store_x(30),
    ];
    assert!(
        !promote(&caller, &classes, &facts).contains_key(&30),
        "an indirect call must not be a provenance seed"
    );
}

/// A call-seeded candidate is still subject to rules 2-4: escaping it in the
/// caller denies it exactly as escaping a `new`-seeded one does.
#[test]
fn call_seeded_candidate_still_obeys_containment() {
    let (facts, c) = facts_for(vec![function(12, "mk", vec![Stmt::Return(Some(new_c()))])]);
    let classes = classes_of(&c);
    let caller = vec![
        Stmt::Let {
            id: 40,
            name: "r".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(Expr::FuncRef(12)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        },
        // `r.nope` is not a declared field of `C`.
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(40)),
            property: "nope".to_string(),
            value: Box::new(Expr::Number(1.0)),
        }),
    ];
    assert!(
        !promote(&caller, &classes, &facts).contains_key(&40),
        "an undeclared-property write must deny a call-seeded candidate too"
    );
}

/// A `Param`-carrying producer is fine; this pins that the fact does not
/// accidentally depend on an empty parameter list.
#[test]
fn producer_with_params_still_gets_a_fact() {
    let mut f = function(13, "mk1", vec![Stmt::Return(Some(new_c()))]);
    f.params = vec![Param {
        id: 100,
        name: "n".to_string(),
        ty: Type::Number,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }];
    let (facts, _) = facts_for(vec![f]);
    assert_eq!(facts.return_shape_class(13), Some("C"));
}

/// GC guard. A producer whose declared return type is definitely-non-pointer
/// would leave the caller's binding without a shadow-stack slot
/// (`collect_pointer_typed_locals` drops it), so a promoted `Ptr<Shape>` local
/// there would be an UNROOTED object pointer under an evacuating minor.
/// Perry does not check annotations, so the fact must stand down.
///
/// Sabotage: delete the `is_definitely_non_pointer(&f.return_type)` check and
/// this fails.
#[test]
fn non_pointer_return_type_denies_the_fact() {
    for ty in [
        Type::Number,
        Type::Int32,
        Type::Boolean,
        Type::Void,
        Type::Union(vec![Type::Number, Type::Boolean]),
    ] {
        let mut f = function(14, "lying", vec![Stmt::Return(Some(new_c()))]);
        f.return_type = ty.clone();
        let (facts, _) = facts_for(vec![f]);
        assert_eq!(
            facts.return_shape_class(14),
            None,
            "a `{ty:?}`-annotated producer must not carry a return-shape fact"
        );
    }
    // The complement: a union that CAN hold a pointer keeps the slot, so the
    // guard must not over-reject.
    let mut f = function(15, "honest", vec![Stmt::Return(Some(new_c()))]);
    f.return_type = Type::Union(vec![Type::Named("C".to_string()), Type::Void]);
    let (facts, _) = facts_for(vec![f]);
    assert_eq!(facts.return_shape_class(15), Some("C"));
}

/// The proof's inputs must not be able to differ between the module pre-pass
/// (which sees `collect_boxed_vars(&f.body)` and an empty module-global map)
/// and the per-region pass (which sees the module-wide union). A local boxed
/// by a closure capture inside the producer must be rejected by BOTH.
#[test]
fn boxed_producer_local_gets_no_fact() {
    let body = vec![
        let_c(1, "o"),
        // A closure that mutably captures `o` — boxes it.
        Stmt::Expr(Expr::Closure {
            func_id: 98,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![store_x(1)],
            captures: Vec::new(),
            mutable_captures: vec![1],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
        Stmt::Return(Some(Expr::LocalGet(1))),
    ];
    let (facts, _) = facts_for(vec![function(16, "boxy", body)]);
    assert_eq!(facts.return_shape_class(16), None);
}
