//! Unit tests for the monotone loop-induction i32 range proof (#7110).
//!
//! The interesting assertions are the REJECTIONS. Admitting a value that can
//! leave i32 range is a silent wrong answer (a wrapped negative printed where
//! Node prints the true integer), so every arm of the proof gets a test that
//! removing it would turn green.

use super::*;
use perry_hir::types::Type as HirType;
use perry_hir::{CatchClause, Param, SwitchCase};

fn let_mut(id: u32, init: Option<Expr>) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: HirType::Number,
        mutable: true,
        init,
    }
}

fn let_const(id: u32, value: i64) -> Stmt {
    Stmt::Let {
        id,
        name: format!("c{id}"),
        ty: HirType::Number,
        mutable: false,
        init: Some(Expr::Integer(value)),
    }
}

fn inc(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Increment,
        prefix: false,
    }
}

fn dec(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Decrement,
        prefix: false,
    }
}

fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn cmp(op: CompareOp, l: Expr, r: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn and(l: Expr, r: Expr) -> Expr {
    Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn or(l: Expr, r: Expr) -> Expr {
    Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn set(id: u32, rhs: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
}

/// `for (let <id> = <init>; <id> < <bound>; <id>++) <body>`
fn counting_for(id: u32, init: i64, bound: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(let_mut(id, Some(Expr::Integer(init))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(id), bound)),
        update: Some(inc(id)),
        body,
    }
}

fn run(stmts: &[Stmt]) -> HashSet<u32> {
    collect_loop_bounded_i32_locals(stmts, &HashMap::new())
}

/// Same, with a module-level `const` map (`compile_time_constants`).
fn run_with_module_consts(stmts: &[Stmt], consts: &[(u32, f64)]) -> HashSet<u32> {
    let map: HashMap<u32, f64> = consts.iter().copied().collect();
    collect_loop_bounded_i32_locals(stmts, &map)
}

// ---------------------------------------------------------------------------
// The shape the issue is about
// ---------------------------------------------------------------------------

#[test]
fn bare_loop_counter_with_literal_bound_is_admitted() {
    // for (let i = 0; i < 1000000; i++) { sum = sum + 1; }
    //
    // `i` is not index-used (nothing is indexed) and `i++` keeps it out of
    // `strictly_i32_bounded_locals`. This fact is the ONLY thing that can
    // admit it. Interval: [0, 999999].
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        counting_for(
            2,
            0,
            Expr::Integer(1_000_000),
            vec![set(
                1,
                bin(BinaryOp::Add, Expr::LocalGet(1), Expr::Integer(1)),
            )],
        ),
    ];
    let got = run(&stmts);
    assert!(got.contains(&2), "loop counter must be admitted: {got:?}");
}

#[test]
fn bare_accumulator_is_not_admitted() {
    // The other half of #7110, and the half that must STAY denied:
    // `sum = sum + 1` is a step, but no loop guard constrains `sum`, so
    // nothing bounds it. 13_factorial's `sum` really does reach 4.995e10.
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        counting_for(
            2,
            0,
            Expr::Integer(1_000_000),
            vec![set(
                1,
                bin(BinaryOp::Add, Expr::LocalGet(1), Expr::Integer(1)),
            )],
        ),
    ];
    let got = run(&stmts);
    assert!(
        !got.contains(&1),
        "bare accumulator must stay denied: {got:?}"
    );
}

#[test]
fn const_local_bound_resolves() {
    // const LIMIT = 100; for (let i = 0; i < LIMIT; i++) {}
    let stmts = vec![
        let_const(1, 100),
        counting_for(2, 0, Expr::LocalGet(1), vec![]),
    ];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn while_loop_guard_conjunct_admits_the_counter() {
    // The 15_mandelbrot `iter` shape: a `while` whose guard is a CONJUNCTION,
    // one arm of which bounds the counter, and whose step is a `LocalSet` Add
    // rather than `++`.
    //   let iter = 0;
    //   while (flag && iter < 100) { iter = iter + 1; }
    let stmts = vec![
        let_mut(3, Some(Expr::Integer(0))),
        Stmt::While {
            condition: and(
                Expr::Bool(true),
                cmp(CompareOp::Lt, Expr::LocalGet(3), Expr::Integer(100)),
            ),
            body: vec![set(
                3,
                bin(BinaryOp::Add, Expr::LocalGet(3), Expr::Integer(1)),
            )],
        },
    ];
    assert!(run(&stmts).contains(&3));
}

#[test]
fn disjunctive_guard_is_not_a_guard() {
    // `a || i < 100` does NOT imply `i < 100` when the body runs.
    let stmts = vec![
        let_mut(3, Some(Expr::Integer(0))),
        Stmt::While {
            condition: or(
                Expr::Bool(true),
                cmp(CompareOp::Lt, Expr::LocalGet(3), Expr::Integer(100)),
            ),
            body: vec![set(
                3,
                bin(BinaryOp::Add, Expr::LocalGet(3), Expr::Integer(1)),
            )],
        },
    ];
    assert!(!run(&stmts).contains(&3));
}

#[test]
fn reversed_operand_order_is_the_same_guard() {
    // for (let i = 0; 100 > i; i++) {}
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Gt, Expr::Integer(100), Expr::LocalGet(2))),
        update: Some(inc(2)),
        body: vec![],
    }];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn descending_counter_is_admitted() {
    // for (let i = 100; i > -5; i--) {}  → interval [-4, 100]
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(100))))),
        condition: Some(cmp(CompareOp::Gt, Expr::LocalGet(2), Expr::Integer(-5))),
        update: Some(dec(2)),
        body: vec![],
    }];
    assert!(run(&stmts).contains(&2));
}

// ---------------------------------------------------------------------------
// Overflow-boundary rejections — each one is a wrong answer if admitted
// ---------------------------------------------------------------------------

#[test]
fn le_bound_at_int32_max_would_overflow_and_is_rejected() {
    // for (let i = 0; i <= 2147483647; i++) — the counter tops out at
    // 2147483648, one past INT32_MAX. Must NOT be admitted.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(
            CompareOp::Le,
            Expr::LocalGet(2),
            Expr::Integer(i32::MAX as i64),
        )),
        update: Some(inc(2)),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn lt_bound_at_int32_max_tops_out_exactly_at_int32_max() {
    // for (let i = 0; i < 2147483647; i++) — tops out at 2147483647. Admitted;
    // this is the boundary the previous test sits one past.
    let stmts = vec![counting_for(2, 0, Expr::Integer(i32::MAX as i64), vec![])];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn two_steps_per_iteration_are_summed_into_the_bound() {
    // for (let i = 0; i < 2147483646; i++) { i = i + 1; }
    // The body step AND the update both run, so the counter tops out at
    // 2147483645 + 2 = 2147483647. Exactly INT32_MAX — admitted.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(
            CompareOp::Lt,
            Expr::LocalGet(2),
            Expr::Integer(i32::MAX as i64 - 1),
        )),
        update: Some(inc(2)),
        body: vec![set(
            2,
            bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(1)),
        )],
    }];
    assert!(run(&stmts).contains(&2));

    // One higher and the same two steps overshoot INT32_MAX. Rejected. If the
    // per-iteration total were not summed, this would (wrongly) pass.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(
            CompareOp::Lt,
            Expr::LocalGet(2),
            Expr::Integer(i32::MAX as i64),
        )),
        update: Some(inc(2)),
        body: vec![set(
            2,
            bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(1)),
        )],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn out_of_range_literal_bound_is_rejected() {
    // for (let i = 0; i < 3000000000; i++) — the bound itself is past i32.
    let stmts = vec![counting_for(2, 0, Expr::Integer(3_000_000_000), vec![])];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn out_of_range_initialiser_is_rejected() {
    // let i = 3000000000; for (; i > 0; i--) — the ENTRY value is past i32
    // even though the guard bounds the other end.
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(3_000_000_000))),
        Stmt::For {
            init: None,
            condition: Some(cmp(CompareOp::Gt, Expr::LocalGet(2), Expr::Integer(0))),
            update: Some(dec(2)),
            body: vec![],
        },
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn runtime_bound_is_not_a_constant() {
    // for (let i = 0; i < n; i++) where `n` is a mutable local: #6072's
    // runaway shape (`n = 2147483653` wraps the shadow to INT32_MIN).
    let stmts = vec![
        let_mut(1, None),
        counting_for(2, 0, Expr::LocalGet(1), vec![]),
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_written_bound_is_not_a_constant() {
    // `let LIMIT = 100; LIMIT = 4000000000;` — declared with a literal but
    // reassigned, so it cannot serve as a compile-time bound.
    let stmts = vec![
        Stmt::Let {
            id: 1,
            name: "LIMIT".into(),
            ty: HirType::Number,
            mutable: false,
            init: Some(Expr::Integer(100)),
        },
        set(1, Expr::Integer(4_000_000_000)),
        counting_for(2, 0, Expr::LocalGet(1), vec![]),
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn direction_must_agree_with_the_guard() {
    // for (let i = 0; i < 10; i--) never terminates upward-bounded: `i` runs
    // to -infinity. The guard says Inc, the step says Dec.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(dec(2)),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn non_step_write_disqualifies() {
    // for (let i = 0; i < 10; i++) { i = someCall(); }
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(inc(2)),
        body: vec![set(2, Expr::LocalGet(9))],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn multiplicative_step_is_not_a_step() {
    // `i = i * 2` grows geometrically; the guard bounds one iteration's ENTRY
    // value but not the product.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(1))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(Expr::LocalSet(
            2,
            Box::new(bin(BinaryOp::Mul, Expr::LocalGet(2), Expr::Integer(2))),
        )),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn reverse_subtraction_is_not_a_step() {
    // `i = 10 - i` flips sign each iteration; it is not monotone, so the
    // "guard dominates the step" argument does not apply.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(Expr::LocalSet(
            2,
            Box::new(bin(BinaryOp::Sub, Expr::Integer(10), Expr::LocalGet(2))),
        )),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn step_inside_a_nested_loop_is_not_bounded_by_the_outer_guard() {
    // for (let i = 0; i < 10; i++) { for (let j = 0; j < 1000000000; j++) { i = i + 1; } }
    //
    // `i` advances a BILLION times per outer iteration; the outer guard bounds
    // it only at the top of each outer iteration. If the "no intervening loop"
    // rule were dropped, this would be admitted and `i` would wrap.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(inc(2)),
        body: vec![counting_for(
            3,
            0,
            Expr::Integer(1_000_000_000),
            vec![set(
                2,
                bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(1)),
            )],
        )],
    }];
    let got = run(&stmts);
    assert!(
        !got.contains(&2),
        "outer counter must not be admitted: {got:?}"
    );
    // The inner counter is still fine — its own guard dominates its own step.
    assert!(got.contains(&3));
}

#[test]
fn do_while_first_pass_is_unguarded() {
    // `let i = 0; do { i++; } while (i < 10);` — the first `i++` runs before
    // the condition is ever evaluated, so no guard dominates it.
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::DoWhile {
            body: vec![Stmt::Expr(inc(2))],
            condition: cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10)),
        },
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_write_from_a_closure_disqualifies() {
    // The gate excludes closure-referenced locals independently, but the FACT
    // must not claim a bound it cannot prove: a closure body can run any
    // number of times, from anywhere.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(inc(2)),
        body: vec![Stmt::Expr(Expr::Closure {
            func_id: 0,
            params: Vec::<Param>::new(),
            return_type: HirType::Void,
            body: vec![Stmt::Expr(inc(2))],
            captures: vec![2],
            mutable_captures: vec![2],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        })],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_write_after_the_loop_disqualifies() {
    // for (let i = 0; i < 10; i++) {}  … then `i = i + 1` outside the loop:
    // that step has no guard at all.
    let stmts = vec![
        counting_for(2, 0, Expr::Integer(10), vec![]),
        set(2, bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(1))),
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_local_declared_twice_is_rejected() {
    // A hoisted `var` / two arms sharing an id break the single-entry-value
    // premise the interval rests on.
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![let_mut(2, Some(Expr::Integer(5)))],
            else_branch: None,
        },
        Stmt::For {
            init: None,
            condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
            update: Some(inc(2)),
            body: vec![],
        },
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_local_with_no_declaration_is_rejected() {
    // A parameter (no `Stmt::Let`) has an unknown entry value.
    let stmts = vec![Stmt::For {
        init: None,
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(7), Expr::Integer(10))),
        update: Some(inc(7)),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&7));
}

#[test]
fn a_never_stepped_local_is_not_reported() {
    // `let i = 0;` with no writes is already handled by
    // `strictly_i32_bounded_locals`; this fact only speaks about induction
    // variables, so it must not widen its own scope.
    let stmts = vec![let_mut(2, Some(Expr::Integer(0)))];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn two_guarded_loops_take_the_union_of_their_intervals() {
    // let i = 0; for (; i < 10; i++) {} for (; i < 20; i++) {}
    // Both writes are guarded; the interval is [0, 19].
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::For {
            init: None,
            condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
            update: Some(inc(2)),
            body: vec![],
        },
        Stmt::For {
            init: None,
            condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(20))),
            update: Some(inc(2)),
            body: vec![],
        },
    ];
    assert!(run(&stmts).contains(&2));

    // …but if the second loop's bound is out of range, the union is not.
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::For {
            init: None,
            condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
            update: Some(inc(2)),
            body: vec![],
        },
        Stmt::For {
            init: None,
            condition: Some(cmp(
                CompareOp::Lt,
                Expr::LocalGet(2),
                Expr::Integer(3_000_000_000),
            )),
            update: Some(inc(2)),
            body: vec![],
        },
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn step_under_an_if_inside_the_guarded_body_is_still_guarded() {
    // A conditional step still executes at most once per iteration, so the
    // per-iteration total is exact.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(inc(2)),
        body: vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: vec![set(
                2,
                bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(1)),
            )],
            else_branch: None,
        }],
    }];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn step_in_a_switch_case_and_a_finally_are_both_counted() {
    // Both are reachable at most once per iteration and both must be summed
    // into the per-iteration total, not silently skipped.
    let body = vec![
        Stmt::Switch {
            discriminant: Expr::LocalGet(2),
            cases: vec![SwitchCase {
                test: Some(Expr::Integer(0)),
                body: vec![Stmt::Expr(inc(2))],
            }],
        },
        Stmt::Try {
            body: vec![],
            catch: Some(CatchClause {
                param: None,
                body: vec![],
            }),
            finally: Some(vec![Stmt::Expr(inc(2))]),
        },
    ];
    // Three steps run per iteration (update + switch case + finally), so the
    // counter tops out at `bound - 1 + 3`. At bound == INT32_MAX - 2 that is
    // exactly INT32_MAX: admitted.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(
            CompareOp::Lt,
            Expr::LocalGet(2),
            Expr::Integer(i32::MAX as i64 - 2),
        )),
        update: Some(inc(2)),
        body: body.clone(),
    }];
    assert!(run(&stmts).contains(&2));

    // One higher overshoots by exactly one. Counting only the `update` step
    // (total 1 instead of 3) would compute 2147483646 and let this through.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(
            CompareOp::Lt,
            Expr::LocalGet(2),
            Expr::Integer(i32::MAX as i64 - 1),
        )),
        update: Some(inc(2)),
        body,
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_step_by_a_const_local_amount_is_summed_at_its_value() {
    // `for (let i = 0; i < 10; i = i + STEP)` with `const STEP = 4`.
    let stmts = vec![
        let_const(1, 4),
        Stmt::For {
            init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
            condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
            update: Some(Expr::LocalSet(
                2,
                Box::new(bin(BinaryOp::Add, Expr::LocalGet(2), Expr::LocalGet(1))),
            )),
            body: vec![],
        },
    ];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn a_negative_step_magnitude_is_rejected() {
    // `i = i + (-1)` under a `<` guard walks downwards forever.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(Expr::LocalSet(
            2,
            Box::new(bin(BinaryOp::Add, Expr::LocalGet(2), Expr::Integer(-1))),
        )),
        body: vec![],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn commuted_increment_is_a_step() {
    // `i = 1 + i` is the same step as `i = i + 1`.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(Expr::LocalSet(
            2,
            Box::new(bin(BinaryOp::Add, Expr::Integer(1), Expr::LocalGet(2))),
        )),
        body: vec![],
    }];
    assert!(run(&stmts).contains(&2));
}

#[test]
fn a_step_written_in_the_loop_condition_is_not_guarded_by_that_loop() {
    // `while (i++ < 10) {}` — the write happens while the guard is being
    // EVALUATED, so it is not dominated by it.
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::While {
            condition: cmp(CompareOp::Lt, inc(2), Expr::Integer(10)),
            body: vec![],
        },
    ];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_module_level_const_bound_resolves() {
    // `const ITERATIONS = 100000000;` at module scope, read from a function
    // body: id 1 is not declared in these stmts, only in `hir.init`.
    let stmts = vec![counting_for(2, 0, Expr::LocalGet(1), vec![])];
    assert!(!run(&stmts).contains(&2), "no bound without the const map");
    assert!(run_with_module_consts(&stmts, &[(1, 100_000_000.0)]).contains(&2));
}

#[test]
fn an_out_of_range_module_const_is_not_a_bound() {
    let stmts = vec![counting_for(2, 0, Expr::LocalGet(1), vec![])];
    assert!(!run_with_module_consts(&stmts, &[(1, 3_000_000_000.0)]).contains(&2));
}

#[test]
fn a_fractional_module_const_is_not_a_bound() {
    // `const LIMIT = 10.5` — `i` stops at 11, but the guard arithmetic
    // (`bound - 1 + step`) is integer, so refuse rather than round.
    let stmts = vec![counting_for(2, 0, Expr::LocalGet(1), vec![])];
    assert!(!run_with_module_consts(&stmts, &[(1, 10.5)]).contains(&2));
}

#[test]
fn a_with_block_fallback_write_disqualifies() {
    // `with (obj) { i = 4000000000; }` writes `i` through
    // `Expr::WithSet`'s fallback, NOT through a `LocalSet`. Without the
    // fallback arm the analysis never sees the write and admits `i`.
    let stmts = vec![Stmt::For {
        init: Some(Box::new(let_mut(2, Some(Expr::Integer(0))))),
        condition: Some(cmp(CompareOp::Lt, Expr::LocalGet(2), Expr::Integer(10))),
        update: Some(inc(2)),
        body: vec![Stmt::Expr(Expr::WithSet {
            object: Box::new(Expr::LocalGet(9)),
            property: "i".into(),
            value: Box::new(Expr::Integer(4_000_000_000)),
            fallback: perry_hir::WithSetFallback::Local(2),
            strict: false,
        })],
    }];
    assert!(!run(&stmts).contains(&2));
}

#[test]
fn a_with_block_fallback_write_also_kills_a_constant_bound() {
    // Same mechanism one level up: a `const`-looking bound that a `with`
    // block can reassign is not a compile-time constant.
    let stmts = vec![
        let_const(1, 100),
        Stmt::Expr(Expr::WithSet {
            object: Box::new(Expr::LocalGet(9)),
            property: "LIMIT".into(),
            value: Box::new(Expr::Integer(4_000_000_000)),
            fallback: perry_hir::WithSetFallback::SloppyImplicit(1),
            strict: false,
        }),
        counting_for(2, 0, Expr::LocalGet(1), vec![]),
    ];
    assert!(!run(&stmts).contains(&2));
}
