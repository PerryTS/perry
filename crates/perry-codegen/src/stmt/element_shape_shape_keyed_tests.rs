//! #10123: the SHAPE-keyed element-shape versioned loop clone — the arm that
//! serves `JSON.parse`'d record arrays, where the element identity is a
//! runtime ShapeId rather than a compile-time class.
//!
//! A child module of `element_shape_loop_tests` (see the `mod` declaration
//! there), so `use super::*` brings that file's helpers and the class-arm
//! fixtures the negatives here compare against.

use super::*;

// ---------------------------------------------------------------------------
// #10123 — SHAPE-KEYED clones over an untyped (`any`) record array.
//
// This is the `JSON.parse` shape: `function run(rows: any, count: number)`,
// with the array's element identity known only at run time. Four things had to
// change together for it to fire, and every one of them is asserted below,
// because each alone still compiles and still prints the right answer:
//
//   1. the preheader asks `js_array_ensure_element_shape_ordinary` (the
//      class-keyed query returns 0 for a class-0 record array, so a clone
//      built on it would never be entered);
//   2. it resolves each tracked property to an inline slot with
//      `js_shape_ordinary_inline_slot_for_key` (there is no class to bake a
//      packed index from);
//   3. the residual per-element mask DROPS the typed-layout bit — a parsed
//      record never has it, so the class-keyed mask would side-exit on the
//      FIRST element of every loop while every label assertion still passed;
//   4. the loaded word is tag-tested as a Number, because a shape says which
//      slot holds `id` and nothing about what is in it.
// ---------------------------------------------------------------------------

const ROWS_ID: u32 = 31;
const COUNT_ID: u32 = 32;
const MOD_ID: u32 = 33;
const U_SUM_ID: u32 = 34;
const U_COUNTER_ID: u32 = 35;
const U_INDEX_ID: u32 = 36;

/// `rows[<index>].<prop>`
fn untyped_elem_field(index: Expr, prop: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ROWS_ID)),
            index: Box::new(index),
        }),
        property: prop.to_string(),
        byte_offset: 0,
    }
}

/// `sum = sum + <value>`
fn untyped_accumulate(value: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        U_SUM_ID,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(U_SUM_ID)),
            right: Box::new(value),
        }),
    ))
}

/// `const index = i % n;`
fn derived_index_stmt(modulus: Expr) -> Stmt {
    Stmt::Let {
        id: U_INDEX_ID,
        name: "index".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Binary {
            op: BinaryOp::Mod,
            left: Box::new(Expr::LocalGet(U_COUNTER_ID)),
            right: Box::new(modulus),
        }),
    }
}

/// The benchmark's own function, parametrized:
///
/// ```text
/// function run(rows: <rows_ty>, count: number, n: number): number {
///     let sum: <sum_ty> = 0;
///     for (let i = 0; i < count; i++) <body>
///     return sum;
/// }
/// ```
fn untyped_param_module(rows_ty: Type, sum_ty: Type, body: Vec<Stmt>) -> Module {
    let mut m = Module::new("element_shape_loop.ts");
    m.functions = vec![perry_hir::Function {
        id: 901,
        name: "run".to_string(),
        type_params: Vec::new(),
        params: vec![
            perry_hir::Param {
                id: ROWS_ID,
                name: "rows".to_string(),
                ty: rows_ty,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            perry_hir::Param {
                id: COUNT_ID,
                name: "count".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            perry_hir::Param {
                id: MOD_ID,
                name: "n".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: U_SUM_ID,
                name: "sum".to_string(),
                ty: sum_ty,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: U_COUNTER_ID,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(U_COUNTER_ID)),
                    right: Box::new(Expr::LocalGet(COUNT_ID)),
                }),
                update: Some(Expr::Update {
                    id: U_COUNTER_ID,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(U_SUM_ID))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// The four shape-keyed obligations, asserted together.
fn assert_shape_keyed_clone(ir: &str, what: &str) {
    assert_clone_fires_call_free(ir, what);
    assert!(
        ir.contains("call i32 @js_array_ensure_element_shape_ordinary"),
        "{what}: the preheader must ask the SHAPE query — the class-keyed one \
         answers 0 for a class-0 record array, so a clone guarded on it could \
         never be entered"
    );
    assert!(
        !ir.contains("call i32 @js_array_ensure_element_shape("),
        "{what}: an untyped array has no class to compare against"
    );
    assert!(
        ir.contains("call i32 @js_shape_ordinary_inline_slot_for_key"),
        "{what}: each tracked property's inline slot must be resolved once in \
         the preheader; there is no class to bake a packed index from"
    );
    let fast = fast_clone_slice(ir);
    assert!(
        fast.contains("134250751"),
        "{what}: the fast clone must use the shape-keyed residual mask \
         (0x0800_80FF); emitted:\n{fast}"
    );
    assert!(
        !fast.contains("402686207"),
        "{what}: the class-keyed mask requires GC_OBJ_TYPED_LAYOUT_INTACT, \
         which a JSON record NEVER has — using it here would side-exit on the \
         first element of every loop while every label assertion still passed; \
         emitted:\n{fast}"
    );
    assert!(
        fast.contains("element_shape.number"),
        "{what}: a shape says which slot holds the field, not what is in it — \
         the loaded word must be tag-tested as a Number before it is consumed \
         as a raw double; emitted:\n{fast}"
    );
    assert!(
        fast.contains("getelementptr double"),
        "{what}: the read must still be a bare offset load"
    );
}

#[test]
fn an_untyped_record_array_gets_a_shape_keyed_clone() {
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(U_COUNTER_ID),
            "v",
        ))],
    ));
    assert_shape_keyed_clone(&ir, "counter-indexed untyped array");
}

#[test]
fn a_constant_index_gets_a_shape_keyed_clone_with_a_hoisted_bounds_check() {
    // `for (let i = 0; i < count; i++) sum += rows[7].id` — the benchmark's
    // `repeat` mode. The trip count says NOTHING about index 7, so the
    // preheader owes its own `length > 7`, and the clone owes no per-read
    // bounds test in exchange.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::Integer(7),
            "v",
        ))],
    ));
    assert_shape_keyed_clone(&ir, "constant index");
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("icmp ugt i32") && deref.contains(", 7"),
        "the preheader must prove `length > 7` before the clone is reachable; \
         emitted:\n{deref}"
    );
    // THE LIVENESS ASSERTION. The trip-count obligation `length >= bound` is
    // the counter-index arm's, and emitting it here made the whole clone
    // UNENTERABLE for its own benchmark: `for (i = 0; i < 1000000; i++) sum +=
    // rows[7].id` over a 7,600-element array asks the preheader to prove
    // `length >= 1000000`. The `cond_br` into the fast clone was emitted, every
    // census assertion passed, and the loop ran the slow clone a million times.
    // The counter indexes nothing here, so the verified prefix has nothing to
    // say about the trip count.
    assert!(
        !deref.contains("icmp uge i32"),
        "a constant index must NOT carry the counter arm's `length >= bound` \
         obligation — the counter indexes nothing, and requiring it makes the \
         clone unenterable whenever the loop runs more times than the array is \
         long; emitted:\n{deref}"
    );
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains("icmp ult i32") && !fast.contains("icmp ugt i32"),
        "the bounds obligation is discharged ONCE in the preheader; a per-read \
         test in the clone would mean it was not; emitted:\n{fast}"
    );
}

/// The counter-index arm must KEEP the obligation the test above forbids: it
/// is the only thing that makes `arr[j]` in bounds for every `j < bound`.
#[test]
fn a_counter_index_keeps_the_trip_count_obligation() {
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(U_COUNTER_ID),
            "v",
        ))],
    ));
    assert_shape_keyed_clone(&ir, "counter index");
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("icmp uge i32"),
        "`arr[j]` is in bounds only because the verified prefix covers the \
         whole trip count; emitted:\n{deref}"
    );
}

#[test]
fn a_derived_modulo_index_gets_a_shape_keyed_clone_with_an_srem_in_the_body() {
    // `const index = i % n; sum += rows[index].id` — the benchmark's
    // `sequential` mode, and the shape every wrap-around pass over a record
    // array is written in. The generic `%` lowering is a runtime call, which
    // inside this clone would DELETE it (#7690), so the `Let` must become one
    // `srem`.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![
            derived_index_stmt(Expr::LocalGet(MOD_ID)),
            untyped_accumulate(untyped_elem_field(Expr::LocalGet(U_INDEX_ID), "v")),
        ],
    ));
    assert_shape_keyed_clone(&ir, "derived modulo index");
    let fast = fast_clone_slice(&ir);
    assert!(
        fast.contains("srem i32"),
        "the derived index must be one `srem` inside the clone; emitted:\n{fast}"
    );
    assert!(
        ir.contains("element_shape.loop.modulus.range"),
        "the modulus must be materialized as a validated i32 in the preheader \
         — `srem` by zero is UB and `x % 0` is NaN in JS"
    );
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert!(
        deref.contains("icmp uge i32"),
        "the preheader must prove `modulus <= length`, which is what makes \
         every derived index in bounds with no per-read test; emitted:\n{deref}"
    );
    // Same liveness assertion as the constant-index case: the ONE `icmp uge`
    // in this block must be the modulus obligation, not the counter arm's
    // `length >= bound`. `for (i = 0; i < 1000000; i++) { const d = i % n; … }`
    // over a short array is the benchmark's `sequential` mode, and demanding
    // `length >= 1000000` made the clone unenterable there.
    assert_eq!(
        deref.matches("icmp uge i32").count(),
        1,
        "a derived index owes exactly ONE length comparison (`modulus <= \
         length`); a second one is the counter arm's trip-count obligation, \
         which makes the clone unenterable whenever the loop runs more times \
         than the array is long; emitted:\n{deref}"
    );
}

#[test]
fn a_shape_keyed_clone_admits_an_untyped_accumulator() {
    // `let sum = 0; sum += rows[i].id` widens `sum` to `Any` in HIR exactly
    // when the element read is untyped — which is every program this clone
    // exists for. The preheader's tag check on the accumulator is what makes
    // the numeric fact sound, and it is emitted either way.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Any,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(U_COUNTER_ID),
            "v",
        ))],
    ));
    assert_shape_keyed_clone(&ir, "untyped accumulator");
}

#[test]
fn a_class_typed_array_still_takes_the_class_arm() {
    // #10123 must not steal the loops #7480 already owns: a declared element
    // class still compares a class id and bakes a packed slot index.
    let ir = emit(&element_shape_module(
        vec![accumulate_stmt(
            SUM_ID,
            ARRAY_ID,
            Expr::LocalGet(COUNTER_ID),
        )],
        None,
    ));
    assert_fast_clone_is_entered(&ir);
    assert!(
        ir.contains("call i32 @js_array_ensure_element_shape("),
        "a declared element class must keep the class-keyed query"
    );
    // CALLS, not declarations: both shape-keyed helpers are declared in every
    // module by `runtime_decls`, so a bare substring search would be vacuous.
    assert!(
        !ir.contains("call i32 @js_array_ensure_element_shape_ordinary")
            && !ir.contains("call i32 @js_shape_ordinary_inline_slot_for_key"),
        "a declared element class must not pay the shape-keyed runtime queries"
    );
    let fast = fast_clone_slice(&ir);
    assert!(
        fast.contains("402686207") && !fast.contains("134250751"),
        "the class arm keeps the typed-layout conjunct in its residual mask"
    );
}

// ---------------------------------------------------------------------------
// #10123 SABOTAGE — shapes the shape-keyed arm must decline.
// ---------------------------------------------------------------------------

#[test]
fn a_body_mixing_index_forms_declines() {
    // Each index form carries its OWN preheader bounds obligation, and the
    // fact records exactly one. A body reading both `rows[i]` and `rows[7]`
    // would have one of them discharged and the other not.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(untyped_elem_field(Expr::LocalGet(U_COUNTER_ID), "v")),
            right: Box::new(untyped_elem_field(Expr::Integer(7), "w")),
        })],
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a body mixing index forms must decline the clone"
    );
}

#[test]
fn a_modulo_index_with_the_operands_swapped_declines() {
    // `const index = n % i` is not a bounded index at all — it is bounded by
    // the COUNTER, which grows. Only `counter % modulus` is admitted.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![
            Stmt::Let {
                id: U_INDEX_ID,
                name: "index".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Mod,
                    left: Box::new(Expr::LocalGet(MOD_ID)),
                    right: Box::new(Expr::LocalGet(U_COUNTER_ID)),
                }),
            },
            untyped_accumulate(untyped_elem_field(Expr::LocalGet(U_INDEX_ID), "v")),
        ],
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "`modulus % counter` must decline the clone"
    );
}

#[test]
fn an_untracked_local_index_declines() {
    // A local that is neither the counter nor the body's own derived binding
    // has no range the preheader proved anything about.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(MOD_ID),
            "v",
        ))],
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "an arbitrary local index must decline the clone"
    );
}

#[test]
fn the_shape_keyed_arm_denies_only_proto() {
    // #10185 narrowed this list. The CLASS arm still denies every name with a
    // dedicated branch in the property dispatch, because its read bakes in a
    // compile-time packed slot. The SHAPE arm bakes in nothing: the preheader
    // asks the runtime for that exact key's inline slot in that exact ordinary
    // ShapeId and declines on `-1`, and the residual pins
    // `obj_type == GC_TYPE_OBJECT` with no per-object descriptors — so every
    // receiver whose builtin branch could answer a name differently (a
    // function's `name`, an array's `length`, a Map's `size`) is already
    // excluded, and for a plain record an own data property shadows the
    // prototype name it collides with. Denying the whole list cost the access
    // benchmark its `fields` shape outright, for a field called `name`.
    for property in ["length", "name", "constructor", "size", "message"] {
        let ir = emit(&untyped_param_module(
            Type::Any,
            Type::Number,
            vec![untyped_accumulate(untyped_elem_field(
                Expr::LocalGet(U_COUNTER_ID),
                property,
            ))],
        ));
        assert!(
            ir.contains("element_shape.loop.fast.preheader"),
            "`{property}` is an ordinary own inline slot on a parsed record; \
             the shape-keyed arm must serve it"
        );
    }
    // `__proto__` stays denied — not because of JavaScript (node gives
    // `JSON.parse('{\"__proto__\":1}')` an own data property and reads `1`
    // back), but because Perry's generic property path may special-case the
    // name ahead of own-property lookup, and the clone must agree with the
    // path it is a clone of.
    let ir = emit(&untyped_param_module(
        Type::Any,
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(U_COUNTER_ID),
            "__proto__",
        ))],
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "`__proto__` must decline the shape-keyed clone"
    );
}

#[test]
fn the_class_keyed_arm_keeps_the_full_property_denylist() {
    // The narrowing above is shape-arm-only: a class-keyed read bakes in a
    // packed slot index while the surrounding lowering may route the name
    // somewhere else entirely, which is what the list has always protected.
    let ir = emit(&element_shape_module(
        vec![Stmt::Expr(Expr::LocalSet(
            SUM_ID,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(SUM_ID)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(ARRAY_ID)),
                        index: Box::new(Expr::LocalGet(COUNTER_ID)),
                    }),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            }),
        ))],
        None,
    ));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a denylisted property must still decline the CLASS-keyed clone"
    );
}

#[test]
fn a_declared_but_unresolvable_element_type_still_declines() {
    // The shape-keyed arm is entered only by a receiver that declares NO
    // element type. `Widget[]` where `Widget` names no module class is a
    // declined CLASS resolution, not an untyped receiver: the annotation is a
    // layout claim this arm would be second-guessing.
    let mut m = untyped_param_module(
        Type::Array(Box::new(Type::Named("Widget".to_string()))),
        Type::Number,
        vec![untyped_accumulate(untyped_elem_field(
            Expr::LocalGet(U_COUNTER_ID),
            "v",
        ))],
    );
    m.classes = Vec::new();
    let ir = emit(&m);
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "an unresolvable declared element type must decline both arms"
    );
}

/// #10185's `fields` and `random` shapes — a child module so it inherits both
/// this file's shape-keyed assertions and `element_shape_loop_tests`'s slicing
/// helpers.
#[path = "element_shape_fields_random_tests.rs"]
mod fields_random;
