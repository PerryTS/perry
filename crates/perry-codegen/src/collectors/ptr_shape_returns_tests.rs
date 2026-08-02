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
use perry_hir::{ClassField, Function, Param};

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

/// A callee that names nothing statically is not seeded. #7170 R1 widened the
/// resolution to a `LocalGet` whose binding provably names one closure literal
/// module-wide, and local 31 here is bound by nothing at all — so it resolves
/// to `None` and the seed is not taken, exactly as before R1.
///
/// Sabotage: make `callee_names_one_function`'s `LocalGet` arm return a fixed
/// `FuncId` instead of consulting `closure_binding_func` and this fails.
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
                // A local the module never binds — nothing to resolve to.
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

// ── #7170 R1: the mechanism inside Perry's own CommonJS IIFE ───────────────
//
// `cjs_wrap` emits every CommonJS module body inside `const _cjs = (function
// () { … })();`, so a module-level `function` declaration never reaches
// `hir.functions` — it lowers to `Stmt::Let { init: Expr::Closure }` inside
// that IIFE, and a call to it to `Call { callee: LocalGet(id) }`. Both halves
// of #7107 missed it, and #7170 §2 measured 91.6% of dependency-JS allocation
// sites in `closure` regions as a result.
//
// The shapes below are transcribed from `--print-hir` of the §6 `p8_iife`
// probe, `PreallocateBoxes` included: a hoisted inner `function` referenced
// from a sibling closure is box-backed by construction
// (`lower_decl/block.rs`), so a proof that refused boxed callees would refuse
// the entire population this exists for.

/// `Stmt::Let { id, init: Expr::Closure { func_id, body } }` — how a `function`
/// declaration inside a function body lowers.
fn let_closure(id: u32, name: &str, func_id: FuncId, body: Vec<Stmt>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Closure {
            func_id,
            params: Vec::new(),
            return_type: Type::Any,
            body,
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }),
    }
}

/// `const <bind> = <callee>()` followed by `<bind>.x = 1`, the caller shape the
/// seed has to reach. The field store is deliberate: without it
/// `escape_news.rs` deletes the object outright and the promotion is
/// `unconsumed — scalar_replaced` (#7170 §6.1).
fn call_and_store(bind: u32, callee: Expr) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id: bind,
            name: "p".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::Call {
                callee: Box::new(callee),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        },
        store_x(bind),
    ]
}

/// The `p8_iife` module: `const _cjs = (function () { function mk() { return
/// new C(); } function run() { const p = mk(); p.x = 1; return p; } return
/// run(); })();`, with `mk`'s binding box-backed exactly as the lowering
/// emits it.
///
/// `extra_iife_stmts` is spliced in after the two declarations so a test can
/// add the one thing that must break the proof.
fn iife_module(extra_iife_stmts: Vec<Stmt>) -> Module {
    let mut iife_body = vec![
        Stmt::PreallocateBoxes(vec![1]),
        let_closure(1, "mk", 1, vec![Stmt::Return(Some(new_c()))]),
        let_closure(2, "run", 3, run_body()),
    ];
    iife_body.extend(extra_iife_stmts);
    iife_body.push(Stmt::Return(Some(Expr::Call {
        callee: Box::new(Expr::LocalGet(2)),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    })));

    let mut hir = Module::new("t");
    hir.classes.push(class_c());
    hir.init = vec![Stmt::Let {
        id: 0,
        name: "_cjs".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Call {
            callee: Box::new(match let_closure(99, "iife", 0, iife_body) {
                Stmt::Let { init: Some(e), .. } => e,
                _ => unreachable!(),
            }),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    }];
    hir
}

/// `run`'s body: `const p = mk(); p.x = 1; return p;`
fn run_body() -> Vec<Stmt> {
    let mut b = call_and_store(9, Expr::LocalGet(1));
    b.push(Stmt::Return(Some(Expr::LocalGet(9))));
    b
}

/// End to end, and the whole point of R1: the producer half reaches a closure
/// and the consumer half resolves a `LocalGet` callee to it.
///
/// Sabotage, each alone: drop the `for_each_module_closure` loop in
/// `collect_return_shape_functions` (the fact disappears); make
/// `callee_names_one_function` accept only `Expr::FuncRef` (the seed
/// disappears). Either one takes this red while every #7107 test stays green,
/// which is exactly the state `main` is in.
#[test]
fn a_function_declared_inside_the_cjs_iife_is_a_producer_and_its_caller_is_seeded() {
    let hir = iife_module(Vec::new());
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(
        facts.return_shape_class(1),
        Some("C"),
        "a closure literal must be able to carry a return-shape fact"
    );
    assert_eq!(
        facts.closure_binding_func(1),
        Some(1),
        "`mk`'s binding must resolve to the closure it is bound to"
    );

    let c = class_c();
    let classes = classes_of(&c);
    assert!(
        promote(&run_body(), &classes, &facts).contains_key(&9),
        "`const p = mk()` inside the IIFE must be a Ptr<Shape> candidate"
    );
}

/// The `PreallocateBoxes` binding is not incidental: it is what the real
/// lowering emits for a hoisted `function` referenced from a sibling closure,
/// and it is the entire dependency-JS population. A binding proof that refused
/// box-backed callees would be green on every hand-written fixture and dead on
/// real code.
#[test]
fn a_box_backed_callee_binding_is_still_resolved() {
    let hir = iife_module(Vec::new());
    assert!(
        matches!(
            first_iife_stmt(&hir),
            Some(Stmt::PreallocateBoxes(ids)) if ids.contains(&1)
        ),
        "fixture premise: `mk`'s binding is box-backed"
    );
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(facts.closure_binding_func(1), Some(1));
}

fn first_iife_stmt(hir: &Module) -> Option<&Stmt> {
    let Some(Stmt::Let {
        init: Some(Expr::Call { callee, .. }),
        ..
    }) = hir.init.first()
    else {
        return None;
    };
    let Expr::Closure { body, .. } = callee.as_ref() else {
        return None;
    };
    body.first()
}

/// Assert that adding `extra` to the IIFE body kills the binding proof, and
/// that the seed it kills was really there without it.
fn binding_is_killed_by(extra: Vec<Stmt>, what: &str) {
    let hir = iife_module(extra);
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(
        facts.closure_binding_func(1),
        None,
        "{what} must disqualify the callee binding"
    );
    let c = class_c();
    let classes = classes_of(&c);
    assert!(
        !promote(&run_body(), &classes, &facts).contains_key(&9),
        "{what} must also stop the caller-side seed"
    );
}

/// A reassignment ANYWHERE in the module — including inside a sibling closure,
/// which is where a CommonJS module actually puts them — means the callee no
/// longer names one body.
///
/// Sabotage: drop the `!scan.writes.contains(id)` conjunct in
/// `single_binding_closure_locals` and this fails.
#[test]
fn a_reassigned_callee_binding_is_not_resolved() {
    binding_is_killed_by(
        vec![Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Undefined)))],
        "a bare reassignment",
    );
    // …and the same write hidden inside a closure body, which is the position
    // a per-region scan would miss.
    binding_is_killed_by(
        vec![Stmt::Expr(Expr::Closure {
            func_id: 77,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Undefined)))],
            captures: vec![1],
            mutable_captures: vec![1],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        })],
        "a reassignment inside a sibling closure",
    );
}

/// Two `Stmt::Let`s on one id (the `var` re-declaration shape) means the
/// binding is not unique, so the callee is whichever ran last.
///
/// Sabotage: drop the `let_counts == 1` conjunct and this fails.
#[test]
fn a_twice_bound_callee_binding_is_not_resolved() {
    binding_is_killed_by(
        vec![let_closure(1, "mk", 42, vec![Stmt::Return(Some(new_c()))])],
        "a second binding of the same id",
    );
}

/// `with (o) { mk = v }` stores into the LOCAL when `o` does not bind the name.
/// The id lives in `WithSetFallback`, not in a child expression, so the
/// module-wide walker could not see it before R1 added the arm.
///
/// Sabotage: delete the `Expr::WithSet` arm in
/// `spec_abi_sites.rs::record_expr_use` and this fails.
#[test]
fn a_with_statement_write_disqualifies_the_callee_binding() {
    binding_is_killed_by(
        vec![Stmt::Expr(Expr::WithSet {
            object: Box::new(Expr::Undefined),
            property: "mk".to_string(),
            value: Box::new(Expr::Undefined),
            fallback: perry_hir::WithSetFallback::Local(1),
            strict: false,
        })],
        "a `with` fallback store",
    );
}

/// A `catch (mk)` clause rebinds the id for the duration of the handler, and
/// `let_counts` cannot see it.
///
/// Sabotage: drop the `c.param` recording in `spec_abi_sites.rs`'s `Stmt::Try`
/// arm and this fails.
#[test]
fn a_catch_bound_callee_id_is_not_resolved() {
    binding_is_killed_by(
        vec![Stmt::Try {
            body: Vec::new(),
            catch: Some(perry_hir::CatchClause {
                param: Some((1, "mk".to_string())),
                body: Vec::new(),
            }),
            finally: None,
        }],
        "a catch binding on the same id",
    );
}

/// A parameter is written by the CALLER, which neither `let_counts` nor
/// `writes` records. `var` hoisting can reuse a parameter's id for a body
/// `var`, so a single-`Let`-and-no-writes id can still have held an argument
/// before that `Let` ran.
///
/// Sabotage: drop `record_param_bindings` / the closure-param recording and
/// this fails.
#[test]
fn a_callee_id_that_is_also_a_parameter_is_not_resolved() {
    let mut hir = iife_module(Vec::new());
    let mut f = function(60, "outer", vec![Stmt::Return(Some(new_c()))]);
    f.params = vec![Param {
        id: 1,
        name: "mk".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }];
    hir.functions.push(f);
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(facts.closure_binding_func(1), None);
}

/// A callee bound to something that is not a closure literal resolves to
/// nothing — the map is a whitelist, so widening the seed set cannot make one
/// of these appear.
#[test]
fn a_non_closure_binding_is_not_resolved() {
    let hir = iife_module(Vec::new());
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(
        facts.closure_binding_func(0),
        None,
        "`_cjs` is bound to a Call, not a Closure"
    );
    assert_eq!(facts.closure_binding_func(9), None, "`p` likewise");
}

/// Producer-side context exclusions, in the closure spellings.
///
/// `is_async` and `is_generator` live on the closure; the CPS-rewritten async
/// closure CLEARS `is_async` and is identified only by
/// `Module::async_step_closures`, so the flag alone would let it through.
///
/// Sabotage: drop any one conjunct of `boxed_or_resumable` in the closure arm
/// and the matching case here fails.
#[test]
fn an_async_or_generator_closure_producer_gets_no_fact() {
    for (label, mutate) in [
        (
            "async",
            Box::new(|hir: &mut Module| set_mk_flag(hir, true, false)) as Box<dyn Fn(&mut Module)>,
        ),
        (
            "generator",
            Box::new(|hir: &mut Module| set_mk_flag(hir, false, true)),
        ),
        (
            "async-step",
            Box::new(|hir: &mut Module| {
                hir.async_step_closures.insert(1);
            }),
        ),
    ] {
        let mut hir = iife_module(Vec::new());
        mutate(&mut hir);
        let facts = super::super::collect_module_dispatch_facts(&hir);
        assert_eq!(
            facts.return_shape_class(1),
            None,
            "a {label} closure producer must carry no return-shape fact"
        );
    }
    // The control: untouched, the same fixture DOES carry the fact, so none of
    // the three above is passing because the fixture stopped working.
    let facts = super::super::collect_module_dispatch_facts(&iife_module(Vec::new()));
    assert_eq!(facts.return_shape_class(1), Some("C"));
}

fn set_mk_flag(hir: &mut Module, async_: bool, generator: bool) {
    let Some(Stmt::Let {
        init: Some(Expr::Call { callee, .. }),
        ..
    }) = hir.init.first_mut()
    else {
        panic!("fixture shape");
    };
    let Expr::Closure { body, .. } = callee.as_mut() else {
        panic!("fixture shape");
    };
    for s in body.iter_mut() {
        if let Stmt::Let {
            id: 1,
            init:
                Some(Expr::Closure {
                    is_async,
                    is_generator,
                    ..
                }),
            ..
        } = s
        {
            *is_async = async_;
            *is_generator = generator;
            return;
        }
    }
    panic!("fixture shape: no `mk` binding");
}

/// Freshness still has to be discharged through the wrapper: a closure that
/// hands back something it did not allocate carries no fact, exactly as the
/// `hir.functions` arm requires.
///
/// Sabotage: skip the `collect_shape_proven_ptr_locals` body proof for the
/// closure arm and this fails.
#[test]
fn a_closure_producer_returning_a_cached_value_gets_no_fact() {
    let mut hir = iife_module(Vec::new());
    let Some(Stmt::Let {
        init: Some(Expr::Call { callee, .. }),
        ..
    }) = hir.init.first_mut()
    else {
        panic!("fixture shape");
    };
    let Expr::Closure { body, .. } = callee.as_mut() else {
        panic!("fixture shape");
    };
    for s in body.iter_mut() {
        if let Stmt::Let {
            id: 1,
            init: Some(Expr::Closure { body, .. }),
            ..
        } = s
        {
            // `return CACHE` — a local this body never allocated.
            *body = vec![Stmt::Return(Some(Expr::LocalGet(500)))];
        }
    }
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(facts.return_shape_class(1), None);
}

/// The recursion invariant R1 inherits: `collect_return_shape_functions`
/// re-enters `collect_shape_proven_ptr_locals` over each producer body while
/// `return_shape_functions` is still empty. `closure_bindings` IS populated by
/// then, so the seed can now RESOLVE a callee during that re-entry — and must
/// still take no seed, because the class map it then consults is empty.
///
/// `mk` calling `mk2` and `mk2` calling `mk` is the shape that would diverge.
#[test]
fn mutually_calling_closure_producers_terminate() {
    let mut hir = Module::new("t");
    hir.classes.push(class_c());
    let mut inner = call_and_store(20, Expr::LocalGet(11));
    inner.push(Stmt::Return(Some(new_c())));
    let mut inner2 = call_and_store(21, Expr::LocalGet(10));
    inner2.push(Stmt::Return(Some(new_c())));
    hir.init = vec![
        let_closure(10, "mk", 1, inner),
        let_closure(11, "mk2", 2, inner2),
    ];
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(facts.return_shape_class(1), Some("C"));
    assert_eq!(facts.return_shape_class(2), Some("C"));
}

/// #7170 R1, key uniqueness. `return_shape_functions` is keyed by raw `FuncId`
/// and read after every transform. `monomorph::MonomorphizationContext::new`
/// seeds its fresh ids at `max(hir.functions ids) + 1000` computed over
/// `hir.functions` ONLY, so a module with few generic functions and many
/// closures can hand a specialization the id of an existing closure; a pass
/// that clones a body without renumbering does the same thing to two closures.
///
/// A fact attributed to the wrong body is a guard-free load at the wrong
/// offsets. Both directions must therefore lose the key, not resolve it by
/// walk order.
///
/// Sabotage: restore "first occurrence wins" (`if !seen.insert(func_id)
/// { return; }`) and both halves fail.
#[test]
fn a_contested_func_id_carries_no_fact() {
    // (a) two closures wearing one id, only the FIRST of which is a producer.
    let mut hir = Module::new("t");
    hir.classes.push(class_c());
    hir.init = vec![
        let_closure(10, "mk", 1, vec![Stmt::Return(Some(new_c()))]),
        let_closure(11, "other", 1, vec![Stmt::Return(Some(Expr::Number(1.0)))]),
    ];
    let facts = super::super::collect_module_dispatch_facts(&hir);
    assert_eq!(
        facts.return_shape_class(1),
        None,
        "a FuncId two closure bodies claim cannot carry a fact"
    );

    // The control: the same module with distinct ids does carry it, so (a) is
    // not passing because the fixture stopped producing facts.
    let mut ok = Module::new("t");
    ok.classes.push(class_c());
    ok.init = vec![
        let_closure(10, "mk", 1, vec![Stmt::Return(Some(new_c()))]),
        let_closure(11, "other", 2, vec![Stmt::Return(Some(Expr::Number(1.0)))]),
    ];
    assert_eq!(
        super::super::collect_module_dispatch_facts(&ok).return_shape_class(1),
        Some("C")
    );

    // (b) a `hir.functions` entry and a closure wearing one id — the
    // monomorph-collision shape. The FUNCTION's fact goes too: once the id is
    // ambiguous neither body is attributable.
    let mut collide = Module::new("t");
    collide.classes.push(class_c());
    collide.functions = vec![function(1, "fn_mk", vec![Stmt::Return(Some(new_c()))])];
    collide.init = vec![let_closure(10, "mk", 1, vec![Stmt::Return(Some(new_c()))])];
    assert_eq!(
        super::super::collect_module_dispatch_facts(&collide).return_shape_class(1),
        None,
        "a FuncId a function and a closure both claim cannot carry a fact"
    );

    // Control for (b): the function alone keeps its #7107 fact.
    let mut alone = Module::new("t");
    alone.classes.push(class_c());
    alone.functions = vec![function(1, "fn_mk", vec![Stmt::Return(Some(new_c()))])];
    assert_eq!(
        super::super::collect_module_dispatch_facts(&alone).return_shape_class(1),
        Some("C")
    );
}
