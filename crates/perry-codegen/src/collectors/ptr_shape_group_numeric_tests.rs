//! #7770: the group-wide numeric-field proof and the numeric-by-construction
//! locals that feed it.
//!
//! Every positive test fails against the pre-#7770 collector (element-group
//! members stood down to zero numeric fields), and every negative test names
//! the store channel whose poisoning must drop the claim — the direction the
//! feature can be quietly wrong in, since a wrongly-claimed field's read is a
//! bare raw `load double` with no value check.

use super::*;
use crate::collectors::PtrShapeLocal;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, ClassField, CompareOp, Function, Module, Param, UpdateOp};

// ── Fixture builders ───────────────────────────────────────────────────────

/// Constructor parameter ids — deliberately far from the function-local ids
/// the tests use, mirroring the module-wide id allocator.
const CTOR_PX: u32 = 100;
const CTOR_PY: u32 = 101;
/// `Base`'s ctor param and `D`'s super-feeding param (the subclass tests).
const CTOR_PZ: u32 = 102;
const CTOR_PZ2: u32 = 103;
/// Method parameter id for `setX(v)`.
const METH_PV: u32 = 110;

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

fn num_param(id: u32, name: &str) -> Param {
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

fn this_store(property: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::This),
        property: property.to_string(),
        value: Box::new(value),
    })
}

/// `class P { x: number; y: number; constructor(x, y) { this.x = x; this.y = y; } }`
fn class_p() -> Class {
    Class {
        id: 0,
        name: "P".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("x"), field("y")],
        constructor: Some(Function {
            id: 900,
            name: "constructor".to_string(),
            type_params: Vec::new(),
            params: vec![num_param(CTOR_PX, "x"), num_param(CTOR_PY, "y")],
            return_type: Type::Void,
            body: vec![
                this_store("x", Expr::LocalGet(CTOR_PX)),
                this_store("y", Expr::LocalGet(CTOR_PY)),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }),
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

/// `class Q` = `class P` plus `setX(v: number) { this.x = v; }`.
fn class_q() -> Class {
    let mut q = class_p();
    q.name = "Q".to_string();
    q.methods = vec![Function {
        id: 901,
        name: "setX".to_string(),
        type_params: Vec::new(),
        params: vec![num_param(METH_PV, "v")],
        return_type: Type::Void,
        body: vec![this_store("x", Expr::LocalGet(METH_PV))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    q
}

fn new_of(class_name: &str, args: Vec<Expr>) -> Expr {
    Expr::New {
        class_name: class_name.to_string(),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

/// `const <id>: <C>[] = [];`
fn let_arr(id: u32, class_name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: format!("a{id}"),
        ty: Type::Array(Box::new(Type::Named(class_name.to_string()))),
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

/// `for (let <idx> = 0; <idx> < <bound>; <idx>++) { body }`
fn counted_loop(idx: u32, bound: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: idx,
            name: format!("i{idx}"),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(idx)),
            right: Box::new(bound),
        }),
        update: Some(Expr::Update {
            id: idx,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body,
    }
}

fn arr_len(arr: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(arr)),
        property: "length".to_string(),
        byte_offset: 0,
    }
}

/// `const <id> = <arr>[<idx>];` typed by the array's element class.
fn let_elem(id: u32, arr: u32, idx: u32, class_name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: format!("r{id}"),
        ty: Type::Named(class_name.to_string()),
        mutable: false,
        init: Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(arr)),
            index: Box::new(Expr::LocalGet(idx)),
        }),
    }
}

fn read_field(id: u32, property: &str) -> Stmt {
    Stmt::Expr(Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: property.to_string(),
        byte_offset: 0,
    })
}

fn store_field(id: u32, property: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::LocalGet(id)),
        property: property.to_string(),
        value: Box::new(value),
    })
}

/// `i + 1` over the loop counter.
fn counter_plus_one(idx: u32) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(idx)),
        right: Box::new(Expr::Integer(1)),
    }
}

fn classes_of<'a>(cs: &'a [Class]) -> HashMap<String, &'a Class> {
    cs.iter().map(|c| (c.name.clone(), c)).collect()
}

fn facts_for(classes: &HashMap<String, &Class>) -> ModuleDispatchFacts {
    let mut hir = Module::new("t");
    let mut names: Vec<&String> = classes.keys().collect();
    names.sort();
    for n in names {
        hir.classes.push(classes[n].clone());
    }
    super::super::collect_module_dispatch_facts(&hir)
}

/// The full Phase 3b verdict, element facts included — what codegen sees.
fn promote(stmts: &[Stmt], classes: &HashMap<String, &Class>) -> HashMap<u32, PtrShapeLocal> {
    let facts = facts_for(classes);
    let els = super::super::ptr_shape_elements::collect_element_shape_facts(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &facts,
    );
    collect_shape_proven_ptr_locals(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &facts,
        &HashSet::new(),
        &els,
    )
}

fn promote_with_element_fields(
    stmts: &[Stmt],
    classes: &HashMap<String, &Class>,
) -> (HashMap<u32, PtrShapeLocal>, HashMap<u32, HashSet<String>>) {
    promote_with_element_fields_and_numeric_params(stmts, classes, &HashSet::new())
}

fn promote_with_element_fields_and_numeric_params(
    stmts: &[Stmt],
    classes: &HashMap<String, &Class>,
    numeric_param_seeds: &HashSet<u32>,
) -> (HashMap<u32, PtrShapeLocal>, HashMap<u32, HashSet<String>>) {
    let facts = facts_for(classes);
    let els = super::super::ptr_shape_elements::collect_element_shape_facts(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &facts,
    );
    collect_shape_proven_ptr_locals_and_element_fields(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        &facts,
        &HashSet::new(),
        &els,
        numeric_param_seeds,
    )
}

fn read_direct_field(array_id: u32, index_id: u32, property: &str) -> Stmt {
    Stmt::Expr(Expr::PropertyGet {
        object: Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(array_id)),
            index: Box::new(Expr::LocalGet(index_id)),
        }),
        property: property.to_string(),
        byte_offset: 0,
    })
}

fn names(set: &[&str]) -> HashSet<String> {
    set.iter().map(|s| s.to_string()).collect()
}

// ── The group proof, positive direction ────────────────────────────────────

/// #7770's headline shape — the issue's reproducer: inline `new P(i, i + 1)`
/// pushes under a counting loop, licensed `const r = a[i]` reads. The loop
/// counter is numeric by construction, so the meet proves both fields.
///
/// Sabotage: gut `collect_numeric_by_construction_locals` (the counter arg
/// stops resolving) or `prove_group_numeric_fields` (members stand down) and
/// this fails.
#[test]
fn inline_push_loop_counter_args_prove_numeric_fields() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        counted_loop(
            2,
            Expr::Number(10.0),
            vec![push(
                1,
                new_of("P", vec![Expr::LocalGet(2), counter_plus_one(2)]),
            )],
        ),
        counted_loop(
            5,
            arr_len(1),
            vec![let_elem(6, 1, 5, "P"), read_field(6, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    let fact = promoted.get(&6).expect("the element read must promote");
    assert_eq!(fact.class_name, "P");
    assert_eq!(
        fact.numeric_fields,
        names(&["x", "y"]),
        "loop-counter constructor args are numeric by construction"
    );
}

/// `churn` reads fields directly from `a[i]`, so no element local exists to
/// carry the group verdict. The root-keyed result must still expose the
/// complete numeric layout to the versioned-loop matcher.
#[test]
fn direct_field_only_group_exports_numeric_element_layout() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        counted_loop(
            2,
            Expr::Number(10.0),
            vec![push(
                1,
                new_of("P", vec![Expr::LocalGet(2), counter_plus_one(2)]),
            )],
        ),
        counted_loop(5, arr_len(1), vec![read_direct_field(1, 5, "x")]),
    ];

    let (promoted, fields) = promote_with_element_fields(&stmts, &classes);
    assert!(
        promoted.is_empty(),
        "the direct read creates no element local"
    );
    assert_eq!(fields.get(&1), Some(&names(&["x", "y"])));
}

/// `churn` constructs `new Pair(base + i, i)` inside a specialized clone.
/// Only that clone's runtime parameter guard may seed `base` as numeric; the
/// public fallback must not trust the source annotation alone.
#[test]
fn guarded_numeric_parameter_proves_derived_constructor_argument() {
    const BASE: u32 = 9;
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        counted_loop(
            2,
            Expr::Number(10.0),
            vec![push(
                1,
                new_of(
                    "P",
                    vec![
                        Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(BASE)),
                            right: Box::new(Expr::LocalGet(2)),
                        },
                        Expr::LocalGet(2),
                    ],
                ),
            )],
        ),
        counted_loop(5, arr_len(1), vec![read_direct_field(1, 5, "x")]),
    ];

    let (_, generic_fields) = promote_with_element_fields(&stmts, &classes);
    assert_eq!(generic_fields.get(&1), Some(&names(&["y"])));

    let numeric_params = HashSet::from([BASE]);
    let (_, specialized_fields) =
        promote_with_element_fields_and_numeric_params(&stmts, &classes, &numeric_params);
    assert_eq!(specialized_fields.get(&1), Some(&names(&["x", "y"])));
}

#[test]
fn non_numeric_constructor_argument_denies_that_element_field_layout() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        push(
            1,
            new_of(
                "P",
                vec![Expr::String("not a number".to_string()), Expr::Number(2.0)],
            ),
        ),
        counted_loop(5, arr_len(1), vec![read_direct_field(1, 5, "x")]),
    ];

    let (_, fields) = promote_with_element_fields(&stmts, &classes);
    assert_eq!(fields.get(&1), Some(&names(&["y"])));
}

/// A producer local with numeric args joins the meet and proves.
#[test]
fn producer_local_new_args_prove_numeric_fields() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        Stmt::Let {
            id: 2,
            name: "p".to_string(),
            ty: Type::Named("P".to_string()),
            mutable: false,
            init: Some(new_of("P", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        },
        push(1, Expr::LocalGet(2)),
        store_field(2, "y", Expr::Number(3.5)),
        counted_loop(
            5,
            arr_len(1),
            vec![let_elem(6, 1, 5, "P"), read_field(6, "y")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert_eq!(
        promoted.get(&2).expect("producer promotes").numeric_fields,
        names(&["x", "y"])
    );
    assert_eq!(
        promoted.get(&6).expect("reader promotes").numeric_fields,
        names(&["x", "y"]),
        "producer stores and args are part of the same group universe"
    );
}

// ── The group proof, negative directions (one per store channel) ───────────

/// A SIBLING member's string store drops the field for EVERY member — the
/// exact hole the pre-#7770 stand-down existed to avoid.
///
/// Sabotage: key the proof on one member's `field_stores` instead of the
/// group union and this fails.
#[test]
fn sibling_string_store_drops_the_field_group_wide() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        counted_loop(
            2,
            Expr::Number(3.0),
            vec![push(
                1,
                new_of("P", vec![Expr::LocalGet(2), Expr::LocalGet(2)]),
            )],
        ),
        // Loop A: a member that poisons `x`.
        counted_loop(
            5,
            arr_len(1),
            vec![
                let_elem(6, 1, 5, "P"),
                store_field(6, "x", Expr::String("s".to_string())),
            ],
        ),
        // Loop B: a different member that only reads.
        counted_loop(
            8,
            arr_len(1),
            vec![let_elem(9, 1, 8, "P"), read_field(9, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    for id in [6u32, 9u32] {
        assert_eq!(
            promoted
                .get(&id)
                .expect("members still promote")
                .numeric_fields,
            names(&["y"]),
            "the poisoned field must drop for member {id}, the healthy one must stay"
        );
    }
}

/// The MEET over push sites: one non-numeric constructor argument at any
/// site drops the parameter's field for the whole group.
#[test]
fn mixed_push_site_meet_drops_the_field() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        push(1, new_of("P", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        push(
            1,
            new_of("P", vec![Expr::String("s".to_string()), Expr::Number(3.0)]),
        ),
        counted_loop(
            5,
            arr_len(1),
            vec![let_elem(6, 1, 5, "P"), read_field(6, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert_eq!(
        promoted.get(&6).expect("reader promotes").numeric_fields,
        names(&["y"]),
        "one string site must veto `x` for every member"
    );
}

/// A producer local's `new` args join the same meet.
#[test]
fn producer_new_args_join_the_meet() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        Stmt::Let {
            id: 2,
            name: "p".to_string(),
            ty: Type::Named("P".to_string()),
            mutable: false,
            init: Some(new_of(
                "P",
                vec![Expr::String("s".to_string()), Expr::Number(1.0)],
            )),
        },
        push(1, Expr::LocalGet(2)),
        counted_loop(
            5,
            arr_len(1),
            vec![let_elem(6, 1, 5, "P"), read_field(6, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert_eq!(
        promoted.get(&6).expect("reader promotes").numeric_fields,
        names(&["y"])
    );
}

/// A method invoked on any member resolves its parameter through the merged
/// group call sites: one string argument drops the stored-to field.
#[test]
fn method_site_string_arg_drops_the_field() {
    let cs = [class_q()];
    let classes = classes_of(&cs);
    let call_set_x = |recv: u32, arg: Expr| {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(recv)),
                property: "setX".to_string(),
                byte_offset: 0,
            }),
            args: vec![arg],
            type_args: Vec::new(),
            byte_offset: 0,
        })
    };
    let stmts = vec![
        let_arr(1, "Q"),
        push(1, new_of("Q", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        counted_loop(
            5,
            arr_len(1),
            vec![
                let_elem(6, 1, 5, "Q"),
                call_set_x(6, Expr::String("s".to_string())),
            ],
        ),
        counted_loop(
            8,
            arr_len(1),
            vec![let_elem(9, 1, 8, "Q"), read_field(9, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert_eq!(
        promoted.get(&9).expect("reader promotes").numeric_fields,
        names(&["y"]),
        "a method-mediated string store must drop `x` group-wide"
    );
    // The positive twin: a numeric argument keeps the claim.
    let stmts_ok = vec![
        let_arr(1, "Q"),
        push(1, new_of("Q", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        counted_loop(
            5,
            arr_len(1),
            vec![let_elem(6, 1, 5, "Q"), call_set_x(6, Expr::Number(7.0))],
        ),
    ];
    let promoted_ok = promote(&stmts_ok, &classes);
    assert_eq!(
        promoted_ok.get(&6).expect("member promotes").numeric_fields,
        names(&["x", "y"])
    );
}

/// `class Base { z; constructor(z) { this.z = z } }`.
fn class_base() -> Class {
    let mut b = class_p();
    b.name = "Base".to_string();
    b.fields = vec![field("z")];
    b.constructor = Some(Function {
        id: 902,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![num_param(CTOR_PZ, "z")],
        return_type: Type::Void,
        body: vec![this_store("z", Expr::LocalGet(CTOR_PZ))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    b
}

/// `class D extends Base { x; constructor(x, z) { super(z); this.x = x } }`.
fn class_d_extends_base() -> Class {
    let mut d = class_p();
    d.id = 1;
    d.name = "D".to_string();
    d.extends_name = Some("Base".to_string());
    d.fields = vec![field("x")];
    d.constructor = Some(Function {
        id: 903,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![num_param(CTOR_PX, "x"), num_param(CTOR_PZ2, "z")],
        return_type: Type::Void,
        body: vec![
            Stmt::Expr(Expr::SuperCall(vec![Expr::LocalGet(CTOR_PZ2)])),
            this_store("x", Expr::LocalGet(CTOR_PX)),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    d
}

/// The super()-argument resolution path under the group MEET — the one place
/// a parent-constructor parameter environment is derived from MULTIPLE
/// provenance `new`s. A wrong index or an unresolved caller env here would
/// grant an unsound claim on `z` (a bare raw load), so both directions get a
/// red test: all-numeric sites prove BOTH the derived and the inherited
/// field; one string at the super-feeding position drops exactly `z`,
/// group-wide, while `x` survives.
#[test]
fn super_chain_params_resolve_under_the_group_meet() {
    let cs = [class_base(), class_d_extends_base()];
    let classes = classes_of(&cs);
    let read_loop = |idx: u32, r: u32| {
        counted_loop(
            idx,
            arr_len(1),
            vec![let_elem(r, 1, idx, "D"), read_field(r, "z")],
        )
    };
    let stmts_ok = vec![
        let_arr(1, "D"),
        counted_loop(
            2,
            Expr::Number(4.0),
            vec![push(
                1,
                new_of("D", vec![Expr::LocalGet(2), counter_plus_one(2)]),
            )],
        ),
        read_loop(5, 6),
    ];
    let promoted_ok = promote(&stmts_ok, &classes);
    assert_eq!(
        promoted_ok.get(&6).expect("reader promotes").numeric_fields,
        names(&["x", "z"]),
        "super(z) must resolve Base's parameter through the caller env"
    );

    let stmts_poison = vec![
        let_arr(1, "D"),
        push(1, new_of("D", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        push(
            1,
            new_of("D", vec![Expr::Number(3.0), Expr::String("s".to_string())]),
        ),
        read_loop(5, 6),
    ];
    let promoted_poison = promote(&stmts_poison, &classes);
    assert_eq!(
        promoted_poison
            .get(&6)
            .expect("reader promotes")
            .numeric_fields,
        names(&["x"]),
        "one string at the super-feeding position must drop `z` for the whole \
         group and leave `x` standing"
    );
}

/// The group claim dies with the group: an undeclared-property store on one
/// member removes every member's FACT, claim included.
#[test]
fn group_claim_dies_with_the_group() {
    let cs = [class_p()];
    let classes = classes_of(&cs);
    let stmts = vec![
        let_arr(1, "P"),
        push(1, new_of("P", vec![Expr::Number(1.0), Expr::Number(2.0)])),
        counted_loop(
            5,
            arr_len(1),
            vec![
                let_elem(6, 1, 5, "P"),
                store_field(6, "extra", Expr::Number(1.0)),
            ],
        ),
        counted_loop(
            8,
            arr_len(1),
            vec![let_elem(9, 1, 8, "P"), read_field(9, "x")],
        ),
    ];
    let promoted = promote(&stmts, &classes);
    assert!(
        !promoted.contains_key(&6) && !promoted.contains_key(&9),
        "group integrity drops every member, so no claim can outlive a reshape"
    );
}

// ── Numeric-by-construction locals ─────────────────────────────────────────

fn numeric_locals_of(stmts: &[Stmt]) -> HashSet<u32> {
    numeric::collect_numeric_by_construction_locals(
        stmts,
        &HashSet::new(),
        &HashMap::new(),
        &HashSet::new(),
        &HashMap::new(),
        &HashSet::new(),
    )
}

/// The loop counter: `let i = 0` + `i++` and nothing else — the shape the
/// provenance `new C(i, i + 1)` needs.
#[test]
fn loop_counter_is_numeric_by_construction() {
    let stmts = vec![counted_loop(2, Expr::Number(10.0), Vec::new())];
    assert!(numeric_locals_of(&stmts).contains(&2));
}

/// A self-referencing accumulator converges on the optimistic assumption.
#[test]
fn numeric_accumulator_is_numeric_by_construction() {
    let stmts = vec![
        Stmt::Let {
            id: 3,
            name: "acc".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::Expr(Expr::LocalSet(
            3,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(3)),
                right: Box::new(Expr::Number(1.0)),
            }),
        )),
    ];
    assert!(numeric_locals_of(&stmts).contains(&3));
}

#[test]
fn bigint_typed_view_addition_is_not_numeric_by_construction() {
    for (offset, kind) in [
        perry_hir::TYPED_ARRAY_KIND_BIGINT64,
        perry_hir::TYPED_ARRAY_KIND_BIGUINT64,
    ]
    .into_iter()
    .enumerate()
    {
        let view_id = 30 + offset as u32 * 2;
        let sum_id = view_id + 1;
        let stmts = vec![
            Stmt::Let {
                id: view_id,
                name: format!("bigint_view_{offset}"),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::TypedArrayNew {
                    kind,
                    arg: Some(Box::new(Expr::Integer(1))),
                }),
            },
            Stmt::Let {
                id: sum_id,
                name: format!("mixed_sum_{offset}"),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(view_id)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                    right: Box::new(Expr::Integer(1)),
                }),
            },
        ];
        assert!(
            !numeric_locals_of(&stmts).contains(&sum_id),
            "BigInt typed-array reads must retain mixed-addition TypeError semantics"
        );
    }
}

/// The poisons, one per rule: a no-init `Let` (undefined until assigned), a
/// string write anywhere, a boxed id, and a param-like id with no `Let`.
#[test]
fn non_numeric_writes_and_bindings_are_excluded() {
    let no_init = vec![
        Stmt::Let {
            id: 4,
            name: "u".to_string(),
            ty: Type::Number,
            mutable: true,
            init: None,
        },
        Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Number(1.0)))),
    ];
    assert!(
        !numeric_locals_of(&no_init).contains(&4),
        "a no-init Let is `undefined` until assigned"
    );

    let string_write = vec![
        Stmt::Let {
            id: 5,
            name: "s".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::String("x".to_string())))),
    ];
    assert!(!numeric_locals_of(&string_write).contains(&5));

    // A closure-body write is still a write against the enclosing id.
    let closure_write = vec![
        Stmt::Let {
            id: 6,
            name: "c".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::Expr(Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::LocalSet(
                6,
                Box::new(Expr::String("x".to_string())),
            ))],
            captures: Vec::new(),
            mutable_captures: vec![6],
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
        !numeric_locals_of(&closure_write).contains(&6),
        "closure-body writes must be part of the write set"
    );

    let boxed: HashSet<u32> = [7u32].into_iter().collect();
    let boxed_local = vec![Stmt::Let {
        id: 7,
        name: "b".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Number(0.0)),
    }];
    assert!(
        !numeric::collect_numeric_by_construction_locals(
            &boxed_local,
            &boxed,
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
        )
        .contains(&7),
        "a boxed local's write set is not this region's to enumerate"
    );

    // No `Let` at all (a parameter or catch binding shape): never a candidate.
    let set_only = vec![Stmt::Expr(Expr::LocalSet(8, Box::new(Expr::Number(1.0))))];
    assert!(!numeric_locals_of(&set_only).contains(&8));
}

/// `i++` as a VALUE is `ToNumeric(old)`, numeric exactly when the operand is
/// provably not a BigInt.
#[test]
fn update_value_resolves_via_not_bigint() {
    let store_update_arg = |target: u32| {
        vec![Stmt::Let {
            id: target,
            name: "j".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Update {
                id: 20,
                op: UpdateOp::Increment,
                prefix: false,
            }),
        }]
    };
    let not_bigint: HashSet<u32> = [20u32].into_iter().collect();
    let with_fact = numeric::collect_numeric_by_construction_locals(
        &store_update_arg(21),
        &HashSet::new(),
        &HashMap::new(),
        &not_bigint,
        &HashMap::new(),
        &HashSet::new(),
    );
    assert!(with_fact.contains(&21));
    let without_fact = numeric::collect_numeric_by_construction_locals(
        &store_update_arg(22),
        &HashSet::new(),
        &HashMap::new(),
        &HashSet::new(),
        &HashMap::new(),
        &HashSet::new(),
    );
    assert!(
        !without_fact.contains(&22),
        "without the not-BigInt fact the update's value is unproven"
    );
}
