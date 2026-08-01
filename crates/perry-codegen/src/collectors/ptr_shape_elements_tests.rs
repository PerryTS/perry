//! #7034 §3 array-element shape facts: both halves of the proof, and the
//! cases that must NOT get one.
//!
//! Every positive test here fails against the pre-#7034-§3 collector (the
//! local was denied with rule 2 "stored into a container", or the `A[i]`
//! binding was never a candidate at all), and **every conjunct of the rule
//! has its own disjoint red set** — each negative test names, in its doc, the
//! single guard whose deletion makes it fail.

use super::*;
use crate::collectors::PtrShapeLocal;
use perry_hir::types::Type;
use perry_hir::{ClassField, CompareOp, Function, Module, UpdateOp};

// ── Fixture builders ───────────────────────────────────────────────────────

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
        fields: vec![field("x"), field("y")],
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

fn class_d() -> Class {
    let mut d = class_c();
    d.id = 1;
    d.name = "D".to_string();
    d
}

fn new_of(name: &str) -> Expr {
    Expr::New {
        class_name: name.to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

fn new_c() -> Expr {
    new_of("C")
}

/// `const <name> = new C();`
fn let_c(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Named("C".to_string()),
        mutable: false,
        init: Some(new_c()),
    }
}

/// `const <name>: C[] = [];`
fn let_arr(id: u32, name: &str) -> Stmt {
    let_arr_ty(
        id,
        name,
        Type::Array(Box::new(Type::Named("C".to_string()))),
    )
}

fn let_arr_ty(id: u32, name: &str, ty: Type) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: false,
        init: Some(Expr::Array(Vec::new())),
    }
}

fn push(array_id: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::ArrayPush {
        array_id,
        value: Box::new(value),
    })
}

fn read_x(id: u32) -> Stmt {
    Stmt::Expr(Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: "x".to_string(),
        byte_offset: 0,
    })
}

fn store_x(id: u32) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::LocalGet(id)),
        property: "x".to_string(),
        value: Box::new(Expr::Number(1.0)),
    })
}

/// `for (let <idx> = 0; <idx> < <arr>.length; <idx>++) { body }`
fn bounded_loop(idx: u32, arr: u32, body: Vec<Stmt>) -> Stmt {
    bounded_loop_cond(
        idx,
        Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(idx)),
            right: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(arr)),
                property: "length".to_string(),
                byte_offset: 0,
            }),
        },
        body,
    )
}

fn bounded_loop_cond(idx: u32, condition: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: idx,
            name: format!("i{idx}"),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        })),
        condition: Some(condition),
        update: Some(Expr::Update {
            id: idx,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body,
    }
}

/// `const <name> = <arr>[<idx>];`
fn let_elem(id: u32, name: &str, arr: u32, idx: u32) -> Stmt {
    let_elem_ty(id, name, arr, idx, Type::Named("C".to_string()))
}

fn let_elem_ty(id: u32, name: &str, arr: u32, idx: u32, ty: Type) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: false,
        init: Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(arr)),
            index: Box::new(Expr::LocalGet(idx)),
        }),
    }
}

fn classes_of<'a>(cs: &'a [Class]) -> HashMap<String, &'a Class> {
    cs.iter().map(|c| (c.name.clone(), c)).collect()
}

/// Real module dispatch facts, so the rule-5 barrier flags are the real ones.
///
/// `classes` is the test's OWN class list, not a pristine `class_c()`. A test
/// that mutates its class (an accessor, a computed member) and then hands the
/// mutation only to `chain_admissible` while the dispatch facts were built
/// from a different class is the classic vacuous-pass shape: it can go green
/// on a rule other than the one under test. `ptr_shape_returns_tests.rs` grew
/// `facts_for_classes` for exactly this reason.
fn facts_for(classes: &HashMap<String, &Class>, functions: Vec<Function>) -> ModuleDispatchFacts {
    let mut hir = Module::new("t");
    let mut names: Vec<&String> = classes.keys().collect();
    names.sort();
    for n in names {
        hir.classes.push(classes[n].clone());
    }
    hir.functions = functions;
    super::super::collect_module_dispatch_facts(&hir)
}

fn elements(stmts: &[Stmt], classes: &HashMap<String, &Class>) -> ElementShapeFacts {
    elements_with(stmts, classes, &facts_for(classes, Vec::new()))
}

fn elements_with(
    stmts: &[Stmt],
    classes: &HashMap<String, &Class>,
    facts: &ModuleDispatchFacts,
) -> ElementShapeFacts {
    collect_element_shape_facts(stmts, &HashSet::new(), &HashMap::new(), classes, facts)
}

/// The full Phase 3b verdict, element facts included — what codegen sees.
fn promote(stmts: &[Stmt], classes: &HashMap<String, &Class>) -> HashMap<u32, PtrShapeLocal> {
    let facts = facts_for(classes, Vec::new());
    let els = elements_with(stmts, classes, &facts);
    super::super::ptr_shape::collect_shape_proven_ptr_locals(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &facts,
        &HashSet::new(),
        &els,
    )
}

// ── The producer half ──────────────────────────────────────────────────────

/// `const a = []; const o = new C(); o.x = 1; a.push(o);` — the single most
/// common record-producing idiom there is. Denied by rule 2 before #7034 §3.
///
/// Sabotage: delete the `push_is_contained` arm in `ptr_shape.rs`'s
/// `Expr::ArrayPush` and this fails.
#[test]
fn pushed_local_is_promoted() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        store_x(2),
        push(1, Expr::LocalGet(2)),
    ];
    let promoted = promote(&stmts, &classes);
    let fact = promoted
        .get(&2)
        .expect("a local whose only escape is a push into a proven array must promote");
    assert_eq!(fact.class_name, "C");
    assert!(
        fact.numeric_fields.is_empty(),
        "an element-group member must never claim numeric fields: a sibling's \
         store through the array is a reachable store this proof cannot see"
    );
}

/// The array is proven even when it is returned — #7034 §4's terminator
/// exemption, applied to the array rather than to the record.
///
/// Sabotage: delete the `Stmt::Return` arm in `ArrayWalk` and this fails.
#[test]
fn returned_array_is_still_proven() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Return(Some(Expr::LocalGet(1))),
    ];
    assert!(promote(&stmts, &classes).contains_key(&2));
}

/// **E3.** The array passed as a call argument can be reshaped by the callee,
/// so nothing about its elements is provable.
///
/// Sabotage: make `ArrayWalk`'s `Expr::LocalGet` arm a no-op and this fails.
#[test]
fn array_escaping_as_a_call_argument_denies_the_push() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(9)),
            args: vec![Expr::LocalGet(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    assert!(
        !promote(&stmts, &classes).contains_key(&2),
        "an array that escapes to an opaque callee proves nothing"
    );
}

/// **E3.** A closure that captures the array can reshape its elements at an
/// unbounded later time.
///
/// Sabotage: delete `ArrayWalk`'s `Expr::Closure` arm and this fails.
#[test]
fn array_captured_by_a_closure_denies_the_push() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Expr(Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Any,
            // Deliberately NOT referenced in the body: the guard under test
            // is the capture LIST, and a body reference would deny through
            // the ordinary bare-reference arm instead, making this vacuous.
            body: Vec::new(),
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
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E2.** `pop`/`shift`/`splice`/`unshift` are the mutators that make the
/// array non-dense (or that re-index it), which is what E5's in-bounds
/// argument rests on.
///
/// Sabotage: drop the `ArrayPop`/`ArrayShift` arm and this fails.
#[test]
fn a_shrinking_mutator_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Expr(Expr::ArrayPop(1)),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E2.** `A[k] = v` can write any value at any index, including past the
/// end (which punches holes).
///
/// Sabotage: delete `ArrayWalk`'s `Expr::IndexSet` arm and this fails.
#[test]
fn an_indexed_store_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Number(9.0)),
            value: Box::new(Expr::Number(1.0)),
        }),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E2.** Two classes in one array is exactly the polymorphism the fact
/// claims does not happen.
///
/// Sabotage: delete the `class_name` agreement check and this fails.
#[test]
fn mixed_element_classes_deny_the_array() {
    let cs = [class_c(), class_d()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        push(1, new_of("D")),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E2.** A value pushed into two arrays belongs to two element groups, and
/// the one-group soundness argument ("every reference to this object is a
/// member of this group") no longer holds.
///
/// Sabotage: delete the `push_value_counts` check and this fails.
#[test]
fn a_local_pushed_into_two_arrays_is_not_exempt() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "a"),
        let_arr(3, "b"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        push(3, Expr::LocalGet(2)),
    ];
    // Asserted on the FACTS, not on promotion: without the guard, `pushed`
    // keeps whichever group was recorded last, so exactly one of these two
    // would come back true — and a `promote()` assertion would pass or fail
    // on `HashMap` iteration order, i.e. be vacuous half the time.
    let facts = elements(&stmts, &classes);
    assert!(
        !facts.push_is_contained(2, 1) && !facts.push_is_contained(2, 3),
        "a value in two element groups must be exempt in neither"
    );
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E1.** A non-empty array literal can carry elisions, whose slots read
/// back as `undefined`.
///
/// Sabotage: relax `items.is_empty()` and this fails.
#[test]
fn a_non_empty_array_literal_is_not_a_seed() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        Stmt::Let {
            id: 1,
            name: "rows".to_string(),
            ty: Type::Array(Box::new(Type::Named("C".to_string()))),
            mutable: false,
            init: Some(Expr::Array(vec![Expr::Undefined])),
        },
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

/// **E1.** A `let` array can be rebound to a different array between the
/// push and a read.
///
/// Sabotage: relax the `mutable: false` requirement and this fails.
#[test]
fn a_reassignable_array_binding_is_not_a_seed() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        Stmt::Let {
            id: 1,
            name: "rows".to_string(),
            ty: Type::Array(Box::new(Type::Named("C".to_string()))),
            mutable: true,
            init: Some(Expr::Array(Vec::new())),
        },
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&2));
}

// ── The reader half ────────────────────────────────────────────────────────

/// `for (let i = 0; i < a.length; i++) { const r = a[i]; r.x; }` — the read
/// form `for…of` also desugars to. `r` becomes a rule-1 seed.
///
/// Sabotage: delete the `element_seeded` arm in `ptr_shape.rs`'s `Stmt::Let`,
/// or the `ReadSite` push in `ArrayWalk`, and this fails.
#[test]
fn bounded_element_read_is_provenance() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        bounded_loop(5, 1, vec![let_elem(6, "r", 1, 5), read_x(6)]),
    ];
    let promoted = promote(&stmts, &classes);
    let fact = promoted
        .get(&6)
        .expect("an in-bounds `a[i]` binding must be a Ptr<Shape> candidate");
    assert_eq!(fact.class_name, "C");
    assert!(
        fact.numeric_fields.is_empty(),
        "an element read is aliased through the array by construction"
    );
}

/// **E5 — the conjunct that separates this pass from a wrong one.** With an
/// unbounded index, `a[i]` can be `undefined`, and a guard-free fixed-offset
/// load would mask a NaN-boxed `undefined` into a wild pointer.
///
/// Sabotage: accept any `IndexGet` in the `Stmt::Let` arm (drop the
/// `self.bounded` membership test) and this fails.
#[test]
fn unbounded_element_read_is_not_provenance() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        // `const j = 99; const r = rows[j];` — a local index, so the shape of
        // the read is identical to the licensed one and the ONLY thing
        // denying it is the absence of a bounding loop.
        Stmt::Let {
            id: 5,
            name: "j".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::Number(99.0)),
        },
        let_elem(6, "r", 1, 5),
        // Rule 2 never walks `r`, so this undeclared write is invisible —
        // which is exactly why the unlicensed read has to void the array.
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(6)),
            property: "extra".to_string(),
            value: Box::new(Expr::Number(1.0)),
        }),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        !promoted.contains_key(&6),
        "an out-of-bounds read yields `undefined`, not an instance of C"
    );
    assert!(
        !promoted.contains_key(&2),
        "and the unlicensed read voids the array, so the producer goes too"
    );
}

/// **E5.** A loop bounded by something OTHER than this array's length proves
/// nothing about this array's indices.
///
/// Sabotage: stop comparing the condition's `.length` receiver against the
/// read's array root and this fails.
#[test]
fn a_loop_bounded_by_another_length_does_not_license_the_read() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_arr(2, "other"),
        push(1, new_c()),
        push(2, new_c()),
        bounded_loop(5, 2, vec![let_elem(6, "r", 1, 5), read_x(6)]),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **E5.** `i < notAnArray.length` bounds `i` by something that is not this
/// array at all. Exactly one array exists in this fixture, so the guard under
/// test — resolving the condition's `.length` receiver to the read's array
/// ROOT — is the only thing that can deny it.
///
/// Sabotage: replace `self.root_of(*a)?` in `bounded_induction` with any
/// tracked root and this fails.
#[test]
fn a_loop_bounded_by_a_non_array_length_does_not_license_the_read() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        Stmt::Let {
            id: 8,
            name: "other".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::String("xyz".to_string())),
        },
        bounded_loop_cond(
            5,
            Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(8)),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            },
            vec![let_elem(6, "r", 1, 5), read_x(6)],
        ),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **E5.** A constant bound is not the array's length — the array may be
/// shorter.
///
/// Sabotage: accept any `Compare { Lt }` condition and this fails.
#[test]
fn a_constant_bound_does_not_license_the_read() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        bounded_loop_cond(
            5,
            Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(5)),
                right: Box::new(Expr::Number(10.0)),
            },
            vec![let_elem(6, "r", 1, 5), read_x(6)],
        ),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **E5.** An induction variable reassigned in the body is no longer bounded
/// by the loop condition at the read.
///
/// Sabotage: delete the `idx_writes == 2` check and this fails.
#[test]
fn an_index_reassigned_in_the_body_does_not_license_the_read() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        bounded_loop(
            5,
            1,
            vec![
                Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::Number(999.0)))),
                let_elem(6, "r", 1, 5),
                read_x(6),
            ],
        ),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **Group integrity.** One member adding an undeclared property reshapes the
/// objects every other member reads, so the whole group is dropped — not just
/// the offender.
///
/// Sabotage: delete the group-integrity filter at the end of
/// `collect_shape_proven_ptr_locals` and this fails: the producer keeps a
/// promotion whose objects the reader has just reshaped.
#[test]
fn one_member_failing_containment_voids_the_group() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        bounded_loop(
            5,
            1,
            vec![
                let_elem(6, "r", 1, 5),
                // `r.extra = 1` — not a declared field of C, so this is a
                // shape transition on an object `row` also references.
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(6)),
                    property: "extra".to_string(),
                    value: Box::new(Expr::Number(1.0)),
                }),
            ],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        !promoted.contains_key(&6),
        "the offending member is denied by rule 2"
    );
    assert!(
        !promoted.contains_key(&2),
        "and so is every sibling — the objects it reads have been reshaped"
    );
}

/// **Group integrity, the other direction.** A group whose members are all
/// contained keeps every one of them, so the filter is not simply "drop
/// everything".
#[test]
fn a_clean_group_keeps_every_member() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        store_x(2),
        push(1, Expr::LocalGet(2)),
        bounded_loop(5, 1, vec![let_elem(6, "r", 1, 5), read_x(6), store_x(6)]),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(promoted.contains_key(&2), "producer");
    assert!(promoted.contains_key(&6), "reader");
}

/// **E3, the ELEMENT half.** `f(A[i])` hands an element to an opaque callee,
/// which can add a property to an object that a licensed `const s = A[k]`
/// reads guard-free. A read cannot transition a shape, but the REFERENCE it
/// produces can be used to.
///
/// Sabotage: make `ArrayWalk`'s `Expr::IndexGet` arm skip `self.disq` and this
/// fails — with a wrong answer under an aliasing mutation, not a compile
/// error.
#[test]
fn an_element_passed_to_a_callee_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        bounded_loop(
            5,
            1,
            vec![
                let_elem(6, "r", 1, 5),
                read_x(6),
                // `f(rows[i])` — the element escapes, the array does not.
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::FuncRef(9)),
                    args: vec![Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::LocalGet(5)),
                    }],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            ],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        !promoted.contains_key(&6) && !promoted.contains_key(&2),
        "an element handed to an opaque callee must void the whole group"
    );
}

/// **E3, the ELEMENT half, unlicensed binding.** `const r = A[0]` binds an
/// element to a local at a site E5 does not license, so rule 2 never walks
/// `r` — and `r.extra = 1` would reshape an object the licensed members read.
///
/// Sabotage: restore the old "an unbounded index read does not disqualify the
/// array" early return in `walk_stmt` and this fails.
#[test]
fn an_unlicensed_element_binding_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        // `const stray = rows[0];` — literal index, no bounding loop.
        Stmt::Let {
            id: 7,
            name: "stray".to_string(),
            ty: Type::Named("C".to_string()),
            mutable: false,
            init: Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Number(0.0)),
            }),
        },
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(7)),
            property: "extra".to_string(),
            value: Box::new(Expr::Number(1.0)),
        }),
        bounded_loop(5, 1, vec![let_elem(6, "r", 1, 5), read_x(6)]),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        !promoted.contains_key(&6) && !promoted.contains_key(&2),
        "an element bound outside a licensed site must void the whole group"
    );
}

/// The array ALIAS path, directly. `for (const r of A)` does not desugar to
/// `A[__idx]` — it binds `const __arr_N = A` first and indexes THAT
/// (`lower/stmt_loops.rs`), so every claim this pass makes about the iterator
/// form runs through the alias edge. The gap test covers it end-to-end; this
/// covers it where a break would be attributable.
///
/// Sabotage: drop the `collect_alias_edges` fixpoint in
/// `collect_element_shape_facts` and this fails while every direct-index test
/// stays green.
#[test]
fn reads_through_an_array_alias_are_licensed() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        // `const __arr = rows;` — the for-of desugar's alias binding.
        Stmt::Let {
            id: 3,
            name: "__arr_3".to_string(),
            ty: Type::Array(Box::new(Type::Named("C".to_string()))),
            mutable: false,
            init: Some(Expr::LocalGet(1)),
        },
        // `for (let i = 0; i < __arr.length; i++) { const r = __arr[i]; … }`
        bounded_loop(5, 3, vec![let_elem(6, "r", 3, 5), read_x(6)]),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        promoted.contains_key(&6),
        "an element read through an array alias must be licensed — this is the \
         `for…of` form"
    );
    assert!(promoted.contains_key(&2), "and the producer with it");
}

// ── CodeRabbit review reproducers (PR #7149) ───────────────────────────────

/// **CodeRabbit 🔴 #1** (`ptr_shape_elements.rs:710`, review of `816a5a3`):
/// a property store through `A[i]` was admitted for ANY property name, so
/// `rows[i].extra = 1` added an own property while licensed reads kept doing
/// guard-free fixed-offset loads on the transitioned object.
///
/// The reviewer's reproducer, verbatim in HIR form. The hazard is real; the
/// mechanism it named (`element_access_is_admissible`) no longer exists —
/// the same commit the review was posted against deleted it, so a
/// `PropertySet` whose receiver is an `IndexGet` now reaches the
/// `Expr::IndexGet` arm and disqualifies the array outright. This test is the
/// standing proof of that, and it is what stops a future "admit declared-field
/// element stores" widening from re-introducing the hole silently.
#[test]
fn a_property_store_through_an_element_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        // `for (…) rows[i].extra = 1;`
        bounded_loop(
            5,
            1,
            vec![Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(5)),
                }),
                property: "extra".to_string(),
                value: Box::new(Expr::Number(1.0)),
            })],
        ),
        // `for (…) { const r = rows[i]; use(r.x); }`
        bounded_loop(8, 1, vec![let_elem(6, "r", 1, 8), read_x(6)]),
    ];
    assert!(
        !promote(&stmts, &classes).contains_key(&6),
        "an undeclared-property store through an element must void the array"
    );
    assert!(
        elements(&stmts, &classes).is_empty(),
        "and no element fact may survive it"
    );
}

/// Same hazard through `A[i].f++`, which is a different HIR node
/// (`PropertyUpdate`) and therefore a different arm.
#[test]
fn a_property_update_through_an_element_denies_the_array() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        bounded_loop(
            5,
            1,
            vec![Stmt::Expr(Expr::PropertyUpdate {
                object: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::LocalGet(5)),
                }),
                property: "extra".to_string(),
                op: perry_hir::BinaryOp::Add,
                prefix: false,
            })],
        ),
        bounded_loop(8, 1, vec![let_elem(6, "r", 1, 8), read_x(6)]),
    ];
    assert!(elements(&stmts, &classes).is_empty());
}

/// **CodeRabbit 🔴 #2** (`ptr_shape.rs:562`): group integrity removed only the
/// ids `group_members()` reports, but the insert loop above it gives every
/// ALIAS of a promoted root the same `PtrShapeLocal` fact. An alias holds the
/// same object, so it kept a guard-free proof of a shape a sibling had just
/// transitioned.
///
/// The reviewer's reproducer. Sabotage: drop the alias closure from the
/// group-integrity filter in `collect_shape_proven_ptr_locals`.
#[test]
fn group_integrity_drops_the_aliases_of_a_dropped_member() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        // `const a = row;` — an alias, which the ptr_shape alias pre-pass
        // tracks and hands the same fact to.
        Stmt::Let {
            id: 3,
            name: "a".to_string(),
            ty: Type::Named("C".to_string()),
            mutable: false,
            init: Some(Expr::LocalGet(2)),
        },
        push(1, Expr::LocalGet(2)),
        bounded_loop(
            5,
            1,
            vec![
                let_elem(6, "r", 1, 5),
                // Fails rule 2: an undeclared property on a group member.
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(6)),
                    property: "extra".to_string(),
                    value: Box::new(Expr::Number(1.0)),
                }),
            ],
        ),
        read_x(3),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(!promoted.contains_key(&6), "the offender");
    assert!(!promoted.contains_key(&2), "the group member");
    assert!(
        !promoted.contains_key(&3),
        "and its ALIAS, which holds the same object and would otherwise keep          a guard-free proof of a shape that has been transitioned"
    );
}

/// **CodeRabbit 🟠 #3** (`ptr_shape_elements.rs:727`): pushing a tracked array
/// into another array made the OUTER array `PushValue::Other` (disqualified)
/// but never disqualified the INNER one, because the arm skips `walk_expr` for
/// `Expr::LocalGet` values. The inner array stays reachable and mutable
/// through the outer one, and `outer[0][0] = new Other()` is an `IndexSet` on
/// an `IndexGet` — an expression this walk does not track.
///
/// The reviewer's reproducer. Sabotage: delete the `disq(*v)` in the
/// `Expr::ArrayPush` arm.
#[test]
fn an_array_pushed_into_another_array_is_disqualified() {
    let cs = [class_c(), class_d()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "inner"),
        push(1, new_c()),
        let_arr_ty(
            2,
            "outer",
            Type::Array(Box::new(Type::Array(Box::new(Type::Named(
                "C".to_string(),
            ))))),
        ),
        // `outer.push(inner)`
        push(2, Expr::LocalGet(1)),
        // `outer[0][0] = new D();` — reaches `inner`'s element through an
        // expression neither walk tracks.
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(2)),
                index: Box::new(Expr::Number(0.0)),
            }),
            index: Box::new(Expr::Number(0.0)),
            value: Box::new(new_of("D")),
        }),
        bounded_loop(5, 1, vec![let_elem(6, "r", 1, 5), read_x(6)]),
    ];
    let facts = elements(&stmts, &classes);
    assert!(
        facts.element_read_class(6).is_none(),
        "an array stored as an element of another array is aliased through it"
    );
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **CodeRabbit 🟡 #6** (`is_empty` covered only `arrays`). The other three
/// maps are kept consistent with `arrays` by construction today — `pushed` is
/// `retain`ed against it, `element_reads` only inserts for a proven root, and
/// an empty `arrays` returns `default()` — but that is an invariant no type
/// enforces, and `is_empty()` gates every consumer. Assert it directly so a
/// future edit that populates one map without the other is caught here rather
/// than by a wrong answer.
#[test]
fn is_empty_covers_every_fact_map() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    // A region with an array that fails E3 (call-argument escape): every map
    // must come back empty together.
    let denied = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(9)),
            args: vec![Expr::LocalGet(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    let facts = elements(&denied, &classes);
    assert!(facts.is_empty());
    assert!(
        facts.debug_all_maps_empty(),
        "is_empty must imply ALL maps empty"
    );

    // And the converse: a proven region is not `is_empty`.
    let proven = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    let facts = elements(&proven, &classes);
    assert!(!facts.is_empty());
    assert!(!facts.debug_all_maps_empty());
}

// ── GC rooting obligations ─────────────────────────────────────────────────

/// **GC (#7019).** `collect_pointer_typed_locals` infers an `IndexGet`'s type
/// from the object's: a local declared `number[]` yields `Number`, which is
/// "definitely not a pointer", so the binding gets NO shadow slot. Perry does
/// not validate annotations, so a `number[]` that this pass proved holds `C`
/// instances would leave a promoted element in an unrooted alloca and an
/// evacuating minor would move the object without rewriting it.
///
/// Sabotage: make `array_type_keeps_element_slot` return `true` and this
/// fails — with a silent wrong answer under `PERRY_GC_FORCE_EVACUATE`, not a
/// compile error.
#[test]
fn a_number_typed_array_annotation_denies_the_fact() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr_ty(1, "rows", Type::Array(Box::new(Type::Number))),
        push(1, new_c()),
        bounded_loop(5, 1, vec![let_elem(6, "r", 1, 5), read_x(6)]),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

/// **GC (#7019), the binding's own annotation.** Same hazard one level down:
/// a `const r: number = a[i]` binding is dropped from the shadow stack by its
/// declared type alone.
///
/// Sabotage: delete the `elem_let_ty` / `is_definitely_non_pointer_type`
/// check on the read site and this fails.
#[test]
fn a_number_typed_element_binding_denies_the_fact() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        push(1, new_c()),
        bounded_loop(
            5,
            1,
            vec![let_elem_ty(6, "r", 1, 5, Type::Number), read_x(6)],
        ),
    ];
    assert!(!promote(&stmts, &classes).contains_key(&6));
}

// ── Rules 4 and 5 still apply ──────────────────────────────────────────────

/// **E4/rule 5.** One `Object.defineProperty` anywhere in the module still
/// kills every `Ptr<Shape>` promotion, elements included.
///
/// Sabotage: drop the `has_shape_barrier_sites` bail in
/// `collect_element_shape_facts` and this test still passes (ptr_shape bails
/// too) — which is why the assertion is on the ELEMENT facts directly.
#[test]
fn the_module_barrier_still_denies_element_facts() {
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let barrier = Function {
        id: 3,
        name: "b".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::ObjectDefineProperty(
            Box::new(Expr::LocalGet(90)),
            Box::new(Expr::String("k".to_string())),
            Box::new(Expr::Undefined),
        ))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let facts = facts_for(&classes, vec![barrier]);
    assert!(facts.has_shape_barrier_sites(), "fixture must be a barrier");
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    assert!(elements_with(&stmts, &classes, &facts).is_empty());
}

/// **E4.** An element class Phase 3b would not admit on its own is not
/// admitted through an array either.
///
/// Sabotage: delete the `chain_admissible` call and this fails.
#[test]
fn an_inadmissible_element_class_denies_the_array() {
    let mut c = class_c();
    // NOT "x"/"y": those are declared fields, and a name that is both would
    // let this pass on field/method ambiguity rather than on the accessor
    // rule it is written for.
    c.getters = vec![(
        "derived".to_string(),
        perry_hir::Function {
            id: 50,
            name: "derived".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        },
    )];
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    assert!(elements(&stmts, &classes).is_empty());
}

/// The gate is a bisection knob for the whole of Phase 3b, and it must take
/// this analysis with it.
///
/// Sabotage: delete the `ptr_shape_locals_enabled()` bail and this fails.
#[test]
fn the_env_gate_disables_element_facts() {
    // `ptr_shape_locals_enabled` caches in a `OnceLock`, so this asserts the
    // call is PRESENT rather than flipping the env (which a parallel test
    // would race on): with the gate on, the same fixture is non-empty.
    let c = class_c();
    let cs = [c];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "rows"),
        let_c(2, "row"),
        push(1, Expr::LocalGet(2)),
    ];
    assert_eq!(
        elements(&stmts, &classes).is_empty(),
        !ptr_shape_locals_enabled(),
        "element facts must track PERRY_PTR_SHAPE_LOCALS exactly"
    );
}
