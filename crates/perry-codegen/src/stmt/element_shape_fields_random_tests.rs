//! #10185: the `fields` and `random` access shapes — IR census for the
//! element-shape clone's loop-carried index, its multi-statement accumulator
//! fold, and its two non-numeric reads.
//!
//! A child module of `element_shape_shape_keyed_tests` (see the `mod`
//! declaration there), so `use super::*` brings both that file's shape-keyed
//! assertions and `element_shape_loop_tests`'s slicing helpers.
//!
//! Every positive here is paired with a sabotage case, and the two liveness
//! assertions #10171 was short — the preheader's EXACT comparison set, and that
//! the residual check really is shared rather than repeated — are the ones that
//! would have caught a clone that is emitted, branched into, and never entered
//! or never actually cheaper.

use super::*;

const ROWS2_ID: u32 = 40;
const COUNT2_ID: u32 = 41;
const MOD2_ID: u32 = 42;
const SUM2_ID: u32 = 43;
const COUNTER2_ID: u32 = 44;
const CURSOR_ID: u32 = 45;
const ALIAS_ID: u32 = 46;
const OTHER_ID: u32 = 47;

/// `rows[<index>].<prop>`
fn rows_field(index: Expr, prop: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ROWS2_ID)),
            index: Box::new(index),
        }),
        property: prop.to_string(),
        byte_offset: 0,
    }
}

/// `sum = sum + <value>`
fn add_to_sum(value: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        SUM2_ID,
        Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(SUM2_ID)),
            right: Box::new(value),
        }),
    ))
}

fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// `cursor = <affine> % n`
fn cursor_update(affine: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(
        CURSOR_ID,
        Box::new(binary(BinaryOp::Mod, affine, Expr::LocalGet(MOD2_ID))),
    ))
}

/// The benchmark's own recurrence, `(cursor * 17 + 7)`.
fn benchmark_affine() -> Expr {
    binary(
        BinaryOp::Add,
        binary(BinaryOp::Mul, Expr::LocalGet(CURSOR_ID), Expr::Integer(17)),
        Expr::Integer(7),
    )
}

/// `const index = cursor;`
fn cursor_alias() -> Stmt {
    Stmt::Let {
        id: ALIAS_ID,
        name: "index".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::LocalGet(CURSOR_ID)),
    }
}

/// `const index = i % n;`
fn modulo_index() -> Stmt {
    Stmt::Let {
        id: ALIAS_ID,
        name: "index".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(binary(
            BinaryOp::Mod,
            Expr::LocalGet(COUNTER2_ID),
            Expr::LocalGet(MOD2_ID),
        )),
    }
}

/// ```text
/// function run(rows: any, count: number, n: number): number {
///     let sum = 0;
///     let cursor = 0;
///     for (let i = 0; i < count; i++) <body>
///     return sum;
/// }
/// ```
fn access_module(body: Vec<Stmt>) -> Module {
    let mut m = Module::new("element_shape_loop.ts");
    let param = |id: u32, name: &str, ty: Type| perry_hir::Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    m.functions = vec![perry_hir::Function {
        id: 902,
        name: "run".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(ROWS2_ID, "rows", Type::Any),
            param(COUNT2_ID, "count", Type::Number),
            param(MOD2_ID, "n", Type::Number),
        ],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: SUM2_ID,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::Let {
                id: CURSOR_ID,
                name: "cursor".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: COUNTER2_ID,
                    name: "i".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(COUNTER2_ID)),
                    right: Box::new(Expr::LocalGet(COUNT2_ID)),
                }),
                update: Some(Expr::Update {
                    id: COUNTER2_ID,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(SUM2_ID))),
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

/// The benchmark's `random` body, with and without the alias spelling.
fn random_body(with_alias: bool) -> Vec<Stmt> {
    let index = if with_alias { ALIAS_ID } else { CURSOR_ID };
    let mut body = vec![cursor_update(benchmark_affine())];
    if with_alias {
        body.push(cursor_alias());
    }
    body.push(add_to_sum(rows_field(Expr::LocalGet(index), "id")));
    body
}

/// The benchmark's `fields` body: three accumulator statements over ONE
/// element, one through a string and one through a boolean.
fn fields_body() -> Vec<Stmt> {
    vec![
        modulo_index(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
        add_to_sum(Expr::PropertyGet {
            object: Box::new(rows_field(Expr::LocalGet(ALIAS_ID), "name")),
            property: "length".to_string(),
            byte_offset: 0,
        }),
        add_to_sum(Expr::Conditional {
            condition: Box::new(rows_field(Expr::LocalGet(ALIAS_ID), "active")),
            then_expr: Box::new(Expr::Integer(1)),
            else_expr: Box::new(Expr::Integer(0)),
        }),
    ]
}

/// How many side exits the clone can take per iteration — one branch per
/// residual / tag test that leaves the clone.
///
/// A side exit targets the slow preheader directly, or — once the clone keeps
/// its accumulator or counter in native storage (`stmt/element_shape_native.rs`)
/// — the write-back trampoline that publishes them and then enters the slow
/// preheader. Both spellings are one exit; counting only the first would make
/// every assertion below read zero for a clone that exits through the second.
fn side_exit_count(fast: &str) -> usize {
    fast.matches("label %element_shape.loop.slow.preheader")
        .count()
        + fast.matches("label %element_shape.loop.side_exit").count()
}

/// The registers `block` defines with a `sitofp i32 … to double`.
fn sitofp_results(block: &str) -> Vec<&str> {
    block
        .lines()
        .filter(|l| l.contains(" = sitofp i32 "))
        .filter_map(|l| l.trim_start().split(" = ").next())
        .collect()
}

/// The LAST block in `fast` whose label starts with `prefix`, body only.
///
/// Used to name the block a commit must live in. Reaching a block named after a
/// passed tag test IS the proof that every side exit of the iteration is behind
/// it — which is stronger than any textual before/after comparison, because the
/// emitted block ORDER is not the execution order (the loop's `update` block is
/// printed before the body blocks that branch into it).
fn last_block_with_prefix<'a>(fast: &'a str, prefix: &str) -> &'a str {
    let start = fast
        .rmatch_indices('\n')
        .map(|(idx, _)| idx + 1)
        .find(|&idx| {
            fast[idx..].starts_with(prefix)
                && fast[idx..]
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim_end()
                    .ends_with(':')
        })
        .unwrap_or_else(|| panic!("no block named `{prefix}*` in:\n{fast}"));
    let body = &fast[start..];
    let end = body.find("\n\n").unwrap_or(body.len());
    &body[..end]
}

/// Count the `store` instructions one iteration of the clone executes.
fn store_count(fast: &str) -> usize {
    fast.lines()
        .filter(|l| l.trim_start().starts_with("store "))
        .count()
}

// ---------------------------------------------------------------------------
// #10185 — the LOOP-CARRIED index (`random`).
// ---------------------------------------------------------------------------

#[test]
fn a_carried_recurrence_gets_a_shape_keyed_clone() {
    let ir = emit(&access_module(random_body(true)));
    assert_shape_keyed_clone(&ir, "carried index with an alias");
    let fast = fast_clone_slice(&ir);
    assert!(
        fast.contains("srem i64") && fast.contains("mul i64"),
        "the recurrence must be the i64 chain, not an `frem` (a libm call on \
         aarch64, which would delete the clone); emitted:\n{fast}"
    );
    assert!(
        !fast.contains("frem"),
        "an `frem` in the clone is the whole reason this form needed its own \
         lowering; emitted:\n{fast}"
    );
    assert!(
        ir.contains("element_shape.loop.carried.range"),
        "the carried ENTRY value must be materialized as a validated \
         non-negative i32 — the i64 bound and the `srem` sign argument are \
         both stated over `0..=i32::MAX`"
    );
    assert!(
        ir.contains("element_shape.loop.modulus.range"),
        "the modulus must be materialized: `srem` by zero is UB and `x % 0` \
         is NaN in JS"
    );
}

#[test]
fn a_carried_recurrence_matches_without_the_alias() {
    // `rows[cursor]` directly — the alias is sugar, not part of the form.
    let ir = emit(&access_module(random_body(false)));
    assert_shape_keyed_clone(&ir, "carried index, no alias");
    let fast = fast_clone_slice(&ir);
    assert!(fast.contains("srem i64"), "emitted:\n{fast}");
}

/// THE correctness assertion for this form.
///
/// The residual side exit resumes the CURRENT iteration in the slow clone,
/// which re-runs the whole body — the recurrence included. So the write-back to
/// the real binding must come after EVERY exit the iteration can take, or the
/// recurrence is applied twice and every later index is silently wrong (still
/// in bounds, still a valid record, just not the one JavaScript names).
#[test]
fn the_carried_write_back_follows_every_side_exit() {
    let ir = emit(&access_module(random_body(true)));
    let fast = fast_clone_slice(&ir);
    // Every side exit this iteration can take is a tag test on a loaded word,
    // and the LAST of them is the `id` read's Number test. Its admitted arm —
    // `element_shape.number.*` — is therefore reachable only once the whole
    // iteration is past every exit, so a commit inside it is provably last.
    let committed = last_block_with_prefix(&fast, "element_shape.number");
    assert!(
        committed.contains("sitofp i32") && committed.contains("store double"),
        "the carried write-back must live past every side exit; a write-back \
         before one makes the slow clone apply the recurrence twice for that \
         iteration, and every later index is silently wrong. \
         final block:\n{committed}\nfull clone:\n{fast}"
    );
    // The accumulator's store is ALSO a `store double` in this block now that
    // it lands in the clone's f64 alloca (`stmt/element_shape_native.rs`), so
    // the publication is counted by what it stores: the carried i32 converted
    // for its real slot.
    let published = sitofp_results(committed);
    assert_eq!(
        published.len(),
        1,
        "the carried value must be converted for publication exactly once per \
         iteration; final block:\n{committed}"
    );
    assert_eq!(
        committed
            .matches(&format!("store double {}, ", published[0]))
            .count(),
        1,
        "the carried value must be published exactly once per iteration; \
         final block:\n{committed}"
    );
    assert_eq!(
        store_count(&fast),
        // the carried i32 slot, the prefetched element handle, the
        // accumulator, the carried write-back, and the counter's increment
        5,
        "a second write to the carried binding would double-apply the \
         recurrence on a side exit; emitted:\n{fast}"
    );
}

/// The same liveness assertion #10123's derived index carries: the ONE length
/// comparison the preheader owes is `modulus <= length`, NOT the counter arm's
/// `length >= bound`. Demanding the latter makes the clone unenterable whenever
/// the loop runs more times than the array is long — which is every access
/// benchmark.
#[test]
fn a_carried_index_owes_exactly_the_modulus_obligation() {
    let ir = emit(&access_module(random_body(true)));
    let deref = block_slice(&ir, "element_shape.loop.preheader.deref");
    assert_eq!(
        deref.matches("icmp uge i32").count(),
        1,
        "a carried index owes exactly ONE length comparison (`modulus <= \
         length`); emitted:\n{deref}"
    );
    let fast = fast_clone_slice(&ir);
    assert!(
        !fast.contains("icmp ult i32") && !fast.contains("icmp ugt i32"),
        "the bounds obligation is discharged ONCE in the preheader; a per-read \
         test in the clone would mean it was not; emitted:\n{fast}"
    );
}

#[test]
fn a_negative_coefficient_declines() {
    // `(cursor * 17 - 7) % n` can produce a negative dividend, and JS `%`
    // returns a NEGATIVE remainder for one — an out-of-bounds subscript the
    // preheader's `m <= length` says nothing about.
    let body = vec![
        cursor_update(binary(
            BinaryOp::Sub,
            binary(BinaryOp::Mul, Expr::LocalGet(CURSOR_ID), Expr::Integer(17)),
            Expr::Integer(7),
        )),
        cursor_alias(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a recurrence whose dividend can go negative must decline"
    );
}

#[test]
fn a_non_affine_recurrence_declines() {
    // `(cursor * cursor) % n` is not affine, so no (a, b) pair describes it and
    // no magnitude bound can be proven.
    let body = vec![
        cursor_update(binary(
            BinaryOp::Mul,
            Expr::LocalGet(CURSOR_ID),
            Expr::LocalGet(CURSOR_ID),
        )),
        cursor_alias(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a non-affine recurrence must decline"
    );
}

#[test]
fn a_recurrence_that_leaves_exact_double_range_declines() {
    // `cursor * 1e9 + 1` fits i64 comfortably and does NOT fit the f64 that
    // JavaScript actually evaluates it in, so the i64 chain and the program
    // would disagree. "Fits i64" was the original design note's obligation; it
    // is necessary and not sufficient.
    let body = vec![
        cursor_update(binary(
            BinaryOp::Add,
            binary(
                BinaryOp::Mul,
                Expr::LocalGet(CURSOR_ID),
                Expr::Integer(1_000_000_000),
            ),
            Expr::Integer(1),
        )),
        cursor_alias(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a recurrence whose intermediates pass 2^53 must decline: JS evaluates \
         it in doubles and the clone's i64 chain would not agree"
    );
}

#[test]
fn a_bare_read_of_the_carried_local_declines() {
    // Inside the clone the real slot is one iteration behind until the trailing
    // commit, so a bare `cursor` in the accumulator would read a stale value.
    let body = vec![
        cursor_update(benchmark_affine()),
        cursor_alias(),
        add_to_sum(binary(
            BinaryOp::Add,
            rows_field(Expr::LocalGet(ALIAS_ID), "id"),
            Expr::LocalGet(CURSOR_ID),
        )),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a bare read of the carried local must decline the clone"
    );
}

#[test]
fn a_recurrence_over_a_second_variable_declines() {
    // `(cursor + other) % n` is not a function of `cursor` alone; `other` is
    // not materialized, bounded, or proven loop-invariant.
    let body = vec![
        cursor_update(binary(
            BinaryOp::Add,
            Expr::LocalGet(CURSOR_ID),
            Expr::LocalGet(OTHER_ID),
        )),
        cursor_alias(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "a recurrence reading a second variable must decline"
    );
}

// ---------------------------------------------------------------------------
// #10185 — the MULTI-STATEMENT body, the string `.length` and the ternary
// (`fields`).
// ---------------------------------------------------------------------------

#[test]
fn the_fields_body_gets_a_shape_keyed_clone() {
    let ir = emit(&access_module(fields_body()));
    assert!(
        fast_clone_slice(&ir).contains("sso.utf16"),
        "the entered, call-free clone must count non-ASCII SSO code units"
    );
    assert_shape_keyed_clone(&ir, "three reads of one element");
    let fast = fast_clone_slice(&ir);
    assert!(
        fast.contains("element_shape.strlen.heap") && fast.contains("element_shape.strlen.sso"),
        "a string `.length` must decode BOTH representations inline — a call \
         to the runtime would delete the clone; emitted:\n{fast}"
    );
    assert!(
        fast.contains("select i1"),
        "the ternary must become a select between the two constants; \
         emitted:\n{fast}"
    );
    assert!(
        fast.contains("9222246136947933188") && fast.contains("9222246136947933187"),
        "the ternary must test the two boolean singletons by exact NaN-box bit \
         pattern; guessing JS truthiness in the clone would be a miscompile; \
         emitted:\n{fast}"
    );
}

/// THE cost assertion. Three reads of `rows[index]` are three loads of the same
/// element and three repeats of the residual header/ShapeId check unless the
/// prologue shares them — and a clone that is entered but pays 3x the guard is
/// exactly the "emitted, branched into, and not actually cheaper" failure the
/// IR census exists to catch.
#[test]
fn three_reads_of_one_element_share_one_residual_check() {
    let ir = emit(&access_module(fields_body()));
    let fast = fast_clone_slice(&ir);
    assert_eq!(
        fast.matches("134250751").count(),
        1,
        "the shape-keyed residual mask must appear ONCE per iteration, not \
         once per read; emitted:\n{fast}"
    );
    assert_eq!(
        fast.matches("element_shape.load").count(),
        // one block definition plus the `br` that targets it
        2,
        "there must be exactly one residual-check block in the iteration; \
         emitted:\n{fast}"
    );
    // Four tag/residual tests, not twelve: the residual once, then one
    // representation test per read.
    assert_eq!(
        side_exit_count(&fast),
        4,
        "one residual check plus one tag test per read; emitted:\n{fast}"
    );
}

/// THE correctness assertion for the multi-statement body. `sum += a; sum += b`
/// commits `sum` twice; if the second read's tag test side-exits, the slow
/// clone re-runs the iteration and applies `a` a second time. The fold makes
/// the whole iteration commit once, after every exit.
#[test]
fn the_accumulator_commits_once_after_every_side_exit() {
    let ir = emit(&access_module(fields_body()));
    let fast = fast_clone_slice(&ir);
    assert_eq!(
        fast.matches("fadd double").count(),
        3,
        "the fold keeps all three additions in source order — it changes when \
         the result is STORED, never what is computed; emitted:\n{fast}"
    );
    assert_eq!(
        store_count(&fast),
        // the derived index, the prefetched element handle, the accumulator,
        // and the counter's own increment
        4,
        "three accumulator statements must fold to ONE accumulator store; a \
         second one means a side exit from a later read can leave an earlier \
         read already applied when the slow clone re-runs the iteration; \
         emitted:\n{fast}"
    );
    // The one store lives in the block the LAST tag test branches into, so it
    // is behind every side exit of the iteration.
    let committed = last_block_with_prefix(&fast, "element_shape.bool");
    assert!(
        committed.contains("store "),
        "the accumulator store must live past every side exit; final \
         block:\n{committed}\nfull clone:\n{fast}"
    );
}

#[test]
fn an_addend_that_reads_the_accumulator_declines() {
    // `sum += rows[i].id; sum += sum` — folding would substitute the PRE-first
    // -statement value of `sum` into the second addend.
    let body = vec![
        modulo_index(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
        add_to_sum(Expr::LocalGet(SUM2_ID)),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "an addend reading the accumulator must decline the fold, and with it \
         the clone"
    );
}

#[test]
fn two_accumulators_decline() {
    let body = vec![
        modulo_index(),
        add_to_sum(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
        Stmt::Expr(Expr::LocalSet(
            CURSOR_ID,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(CURSOR_ID)),
                right: Box::new(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
            }),
        )),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "the body must write exactly one accumulator"
    );
}

#[test]
fn a_ternary_with_a_non_constant_arm_declines() {
    let body = vec![
        modulo_index(),
        add_to_sum(Expr::Conditional {
            condition: Box::new(rows_field(Expr::LocalGet(ALIAS_ID), "active")),
            then_expr: Box::new(rows_field(Expr::LocalGet(ALIAS_ID), "id")),
            else_expr: Box::new(Expr::Integer(0)),
        }),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "only integer-constant ternary arms are admitted"
    );
}

#[test]
fn a_ternary_on_something_other_than_an_element_read_declines() {
    let body = vec![
        modulo_index(),
        add_to_sum(Expr::Conditional {
            condition: Box::new(Expr::LocalGet(COUNTER2_ID)),
            then_expr: Box::new(Expr::Integer(1)),
            else_expr: Box::new(Expr::Integer(0)),
        }),
    ];
    let ir = emit(&access_module(body));
    assert!(
        !ir.contains("element_shape.loop.fast.preheader"),
        "the clone lowers a boolean-singleton test, not general JS truthiness"
    );
}

/// The two non-numeric reads are SHAPE-keyed only: a class-keyed clone's slot
/// holds a raw `double`, so there is no NaN-box tag to test and both readers
/// would be decoding a double's bit pattern.
#[test]
fn a_class_typed_array_declines_the_string_length_read() {
    let ir = emit(&element_shape_module(
        vec![Stmt::Expr(Expr::LocalSet(
            SUM_ID,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(SUM_ID)),
                right: Box::new(Expr::PropertyGet {
                    object: Box::new(elem_field(ARRAY_ID, Expr::LocalGet(COUNTER_ID))),
                    property: "length".to_string(),
                    byte_offset: 0,
                }),
            }),
        ))],
        None,
    ));
    assert!(
        !ir.contains("element_shape.strlen"),
        "the class-keyed arm reads raw doubles; there is no tag to test"
    );
}

/// The accumulator and counter in their native domains
/// (`stmt/element_shape_native.rs`) — a child module so it reuses this file's
/// `random`/`fields` fixtures and exit-counting helpers.
#[path = "element_shape_native_tests.rs"]
mod native_domains;
