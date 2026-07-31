//! Unit tests for the repsel profitability model (#7128).
//!
//! The load-bearing assertion is `mandelbrot_iter_is_refused`: it reproduces
//! the exact HIR shape that measured **+14.87% instructions retired** on a
//! quiet Raspberry Pi 5, and it fails against a compiler without this module.
//! Every other test exists to pin the rule's *boundaries*, because a refusal
//! rule that over-fires is a silent coverage loss with no symptom — so each
//! conjunct of the rule has a test that removing the conjunct turns red.

use super::*;
use perry_hir::types::Type as HirType;
use perry_hir::{CompareOp, LogicalOp, UpdateOp};

fn let_mut(id: u32, init: Option<Expr>) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: HirType::Number,
        mutable: true,
        init,
    }
}

fn get(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn set(id: u32, rhs: Expr) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
}

fn inc(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Increment,
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

fn cmp(l: Expr, r: Expr) -> Expr {
    Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn index(object: Expr, idx: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(object),
        index: Box::new(idx),
    }
}

fn call(args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::FuncRef(0)),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

fn for_loop(init: Stmt, condition: Expr, update: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::For {
        init: Some(Box::new(init)),
        condition: Some(condition),
        update: Some(update),
        body,
    }
}

/// No local anywhere holds parallel-shadow i32 storage — the state every
/// #7110-admitted counter is in, and the one that makes the model's verdict
/// the deciding one.
fn no_i32_storage() -> (HashSet<u32>, HashSet<u32>, HashSet<u32>, HashSet<u32>) {
    (
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
        HashSet::new(),
    )
}

fn run(stmts: &[Stmt], index_used: &HashSet<u32>) -> HashSet<u32> {
    let (_, strictly_bounded, unsigned, int_valued_ta) = no_i32_storage();
    collect_unprofitable_canonical_i32_locals(
        stmts,
        &I32StorageFacts {
            index_used,
            strictly_bounded: &strictly_bounded,
            unsigned: &unsigned,
            int_valued_ta: &int_valued_ta,
        },
    )
}

/// ```text
/// let totalIter = 0;
/// for (let py = 0; py < 800; py++) {
///   const cy = (py - 400.0) * 4.0;
///   for (let px = 0; px < 800; px++) {
///     const cx = (px - 400.0) * 4.0;
///     let iter = 0;
///     while (fp && iter < 100) { iter = iter + 1; }
///     totalIter = totalIter + iter;
///   }
/// }
/// ```
///
/// `benchmarks/suite/15_mandelbrot.ts`, reduced. `iter`, `px` and `py` are all
/// proven-integer, all admitted by #7110's interval proof, and all three are
/// consumed only as doubles inside a loop. This is the +14.87% regression.
#[test]
fn mandelbrot_iter_is_refused() {
    // 1 = totalIter, 2 = py, 3 = px, 4 = cx, 5 = iter, 6 = cy
    let inner_while = Stmt::While {
        condition: Expr::Logical {
            op: LogicalOp::And,
            left: Box::new(cmp(Expr::Number(1.0), Expr::Number(4.0))),
            right: Box::new(cmp(get(5), Expr::Integer(100))),
        },
        body: vec![set(5, bin(BinaryOp::Add, get(5), Expr::Integer(1)))],
    };
    let px_body = vec![
        // const cx = (px - 400.0) * 4.0  →  px read in a double chain
        Stmt::Let {
            id: 4,
            name: "cx".into(),
            ty: HirType::Number,
            mutable: false,
            init: Some(bin(
                BinaryOp::Mul,
                bin(BinaryOp::Sub, get(3), Expr::Number(400.0)),
                Expr::Number(4.0),
            )),
        },
        let_mut(5, Some(Expr::Integer(0))),
        inner_while,
        // totalIter = totalIter + iter  →  iter read into a boxed accumulator
        set(1, bin(BinaryOp::Add, get(1), get(5))),
    ];
    // The py loop carries its own `const cy = (py - 400.0) * 4.0`, exactly as
    // the source does. Leaving it out is how the first version of this test
    // passed for `px` and `iter` while `py` stayed promoted — the reduction
    // was wrong, not the model.
    let py_body = vec![
        Stmt::Let {
            id: 6,
            name: "cy".into(),
            ty: HirType::Number,
            mutable: false,
            init: Some(bin(
                BinaryOp::Mul,
                bin(BinaryOp::Sub, get(2), Expr::Number(400.0)),
                Expr::Number(4.0),
            )),
        },
        for_loop(
            let_mut(3, Some(Expr::Integer(0))),
            cmp(get(3), Expr::Integer(800)),
            inc(3),
            px_body,
        ),
    ];
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        for_loop(
            let_mut(2, Some(Expr::Integer(0))),
            cmp(get(2), Expr::Integer(800)),
            inc(2),
            py_body,
        ),
    ];

    let out = run(&stmts, &HashSet::new());
    assert!(out.contains(&5), "iter must be refused: {out:?}");
    assert!(out.contains(&3), "px must be refused: {out:?}");
    assert!(out.contains(&2), "py must be refused: {out:?}");
}

/// `benchmarks/suite/11_prime_sieve.ts`, reduced — the −1.05% win. One array
/// index is enough to pay for the representation, so the counter must survive
/// even though `count = count + 1` never reads it.
#[test]
fn index_used_counter_survives() {
    // 1 = sieve, 2 = i
    let stmts = vec![
        let_mut(1, None),
        for_loop(
            let_mut(2, Some(Expr::Integer(0))),
            cmp(get(2), Expr::Integer(1_000_000)),
            inc(2),
            vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(get(1)),
                index: Box::new(get(2)),
                value: Box::new(Expr::Bool(false)),
            })],
        ),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&2), "index-used counter refused: {out:?}");
}

/// A counter whose only in-loop double consumer sits *behind* an index is
/// still profitable: `sum = sum + arr[i]` reads `i` in an index position.
#[test]
fn index_behind_a_boxed_accumulator_survives() {
    // 1 = arr, 2 = sum, 3 = i
    let stmts = vec![
        let_mut(1, None),
        let_mut(2, Some(Expr::Integer(0))),
        for_loop(
            let_mut(3, Some(Expr::Integer(0))),
            cmp(get(3), Expr::Integer(100)),
            inc(3),
            vec![set(2, bin(BinaryOp::Add, get(2), index(get(1), get(3))))],
        ),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&3), "indexed counter refused: {out:?}");
}

/// `benchmarks/suite/02_loop_overhead.ts` and `08_string_concat.ts`: a counter
/// whose every read is a guard. Nothing converts, so nothing is refused —
/// this is the shape #7110 exists for and the one the −4.12% `Str` workload
/// carries.
#[test]
fn bare_guard_only_counter_survives() {
    let stmts = vec![for_loop(
        let_mut(1, Some(Expr::Integer(0))),
        cmp(get(1), Expr::Integer(100_000)),
        inc(1),
        vec![],
    )];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "guard-only counter refused: {out:?}");
}

/// `for (let i = 0; i < n; i++)` with a `number` parameter bound. The guard is
/// neutral on BOTH sides by construction; classifying it as a double consumer
/// would refuse the most common loop in JavaScript.
#[test]
fn guard_against_a_non_integer_bound_is_not_a_cost() {
    // 9 = a parameter (never declared, never in any integer set)
    let stmts = vec![for_loop(
        let_mut(1, Some(Expr::Integer(0))),
        cmp(get(1), get(9)),
        inc(1),
        vec![],
    )];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "param-bounded counter refused: {out:?}");
}

/// `iter = iter + 1` writes the value back into its own slot, which is
/// representation-preserving. Without the self-target exemption every `while`
/// counter in the corpus would be refused — including
/// `fixture_loop_bounded_i32.ts`'s `iterate()`, whose census floor is the
/// liveness proof for #7110 itself.
#[test]
fn self_step_is_not_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(100)),
            body: vec![set(1, bin(BinaryOp::Add, get(1), Expr::Integer(1)))],
        },
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "self-stepping counter refused: {out:?}");
}

/// A conversion outside every loop runs once. `return iter` (and the
/// `console.log` at the end of every benchmark) must not refuse a counter —
/// `fixture_loop_bounded_i32.ts`'s `iterate()` returns its counter.
#[test]
fn double_use_outside_a_loop_is_not_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(100)),
            body: vec![Stmt::Expr(inc(1))],
        },
        Stmt::Return(Some(get(1))),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "post-loop return refused: {out:?}");
}

/// A single-assignment local is loop-invariant at every read, so LICM hoists
/// any conversion out of the loop. `const WIDTH = 800` in `15_mandelbrot` is
/// read as a double twice per inner iteration and still costs nothing.
#[test]
fn write_once_local_is_out_of_scope() {
    let stmts = vec![
        Stmt::Let {
            id: 1,
            name: "WIDTH".into(),
            ty: HirType::Number,
            mutable: false,
            init: Some(Expr::Integer(800)),
        },
        for_loop(
            let_mut(2, Some(Expr::Integer(0))),
            cmp(get(2), Expr::Integer(800)),
            inc(2),
            vec![Stmt::Expr(bin(BinaryOp::Div, get(1), Expr::Number(2.0)))],
        ),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "write-once const refused: {out:?}");
}

/// `benchmarks/suite/14_closure.ts` / `07_object_create.ts`: Perry's calling
/// convention passes NaN-boxed doubles, so an argument position converts.
#[test]
fn call_argument_is_a_cost() {
    let stmts = vec![for_loop(
        let_mut(1, Some(Expr::Integer(0))),
        cmp(get(1), Expr::Integer(100)),
        inc(1),
        vec![Stmt::Expr(call(vec![get(1)]))],
    )];
    let out = run(&stmts, &HashSet::new());
    assert!(out.contains(&1), "call-argument counter kept: {out:?}");
}

/// `benchmarks/suite/06_math_intensive.ts`: `result = result + (1.0 / i)`.
/// `/` is floating-point in JS whatever its operands are.
#[test]
fn float_divide_operand_is_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Number(1.0))),
        for_loop(
            let_mut(2, Some(Expr::Integer(1))),
            cmp(get(2), Expr::Integer(50_000_000)),
            inc(2),
            vec![set(
                1,
                bin(
                    BinaryOp::Add,
                    get(1),
                    bin(BinaryOp::Div, Expr::Number(1.0), get(2)),
                ),
            )],
        ),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(out.contains(&2), "fdiv operand kept: {out:?}");
}

/// Bitwise operands are ToInt32-coerced by the language, so an i32 slot feeds
/// them with no conversion — `17_loop_data_dependent`'s `x[i & 63]` shape and
/// every FNV/xorshift mixer in the corpus.
#[test]
fn bitwise_use_is_a_benefit() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        for_loop(
            let_mut(2, Some(Expr::Integer(0))),
            cmp(get(2), Expr::Integer(100)),
            inc(2),
            vec![
                set(1, bin(BinaryOp::BitXor, get(1), get(2))),
                Stmt::Expr(call(vec![get(2)])),
            ],
        ),
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(
        !out.contains(&2),
        "bitwise-consumed counter refused: {out:?}"
    );
}

/// Writing into a local that already holds parallel-shadow i32 storage is an
/// i32-consuming use: `j = j + i` in `11_prime_sieve` keeps `i` profitable.
#[test]
fn write_into_an_i32_storage_local_is_a_benefit() {
    let index_used: HashSet<u32> = [2u32].into_iter().collect();
    let stmts = vec![
        let_mut(2, Some(Expr::Integer(0))),
        for_loop(
            let_mut(1, Some(Expr::Integer(0))),
            cmp(get(1), Expr::Integer(100)),
            inc(1),
            vec![
                set(2, bin(BinaryOp::Add, get(2), get(1))),
                Stmt::Expr(call(vec![get(1)])),
            ],
        ),
    ];
    let out = run(&stmts, &index_used);
    assert!(
        !out.contains(&1),
        "i32-store-consumed counter refused: {out:?}"
    );
}

/// `Math.imul` is the one call form that takes i32 operands directly.
#[test]
fn math_imul_operand_is_a_benefit() {
    let stmts = vec![for_loop(
        let_mut(1, Some(Expr::Integer(0))),
        cmp(get(1), Expr::Integer(100)),
        inc(1),
        vec![
            Stmt::Expr(Expr::MathImul(Box::new(get(1)), Box::new(Expr::Integer(3)))),
            Stmt::Expr(call(vec![get(1)])),
        ],
    )];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "Math.imul operand refused: {out:?}");
}

/// `seed = (Math.imul(seed, K) + C) & 0x7fffffff` — a local whose ONLY read is
/// inside its own assignment, in an i32-consuming position. The self-target
/// exemption must suppress the COST half and not the BENEFIT half:
/// `17_loop_data_dependent`'s LCG seed and every FNV/xorshift mixer in the
/// corpus have exactly this shape, and also divide the value out as a double
/// once per iteration.
#[test]
fn a_self_write_in_an_i32_position_is_still_a_benefit() {
    // 1 = seed (already holds parallel-shadow i32 storage via its `&` write)
    let strictly: HashSet<u32> = [1u32].into_iter().collect();
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(42))),
        for_loop(
            let_mut(2, Some(Expr::Integer(0))),
            cmp(get(2), Expr::Integer(64)),
            inc(2),
            vec![
                set(
                    1,
                    bin(
                        BinaryOp::BitAnd,
                        Expr::MathImul(Box::new(get(1)), Box::new(Expr::Integer(1103515245))),
                        Expr::Integer(0x7fff_ffff),
                    ),
                ),
                Stmt::Expr(call(vec![bin(
                    BinaryOp::Div,
                    get(1),
                    Expr::Number(2147483647.0),
                )])),
            ],
        ),
    ];
    let (_, _, unsigned, int_valued_ta) = no_i32_storage();
    let empty = HashSet::new();
    let out = collect_unprofitable_canonical_i32_locals(
        &stmts,
        &I32StorageFacts {
            index_used: &empty,
            strictly_bounded: &strictly,
            unsigned: &unsigned,
            int_valued_ta: &int_valued_ta,
        },
    );
    assert!(!out.contains(&1), "self-written i32 mixer refused: {out:?}");
}

/// `x = x / 2` — a self-write whose VALUE is a double. The exemption is about
/// the representation flow, not the syntactic shape `t = … t …`: `/` is
/// floating-point in JS whatever its operands, so an i32 slot for `x` would
/// `sitofp` out and `fptosi` back on every iteration and buy nothing. Raised
/// by CodeRabbit on #7132; it fails against the first version of the
/// exemption, which keyed on the shape.
#[test]
fn self_divide_is_still_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(64))),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(1)),
            body: vec![set(1, bin(BinaryOp::Div, get(1), Expr::Number(2.0)))],
        },
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(
        out.contains(&1),
        "self-divided counter must still be refused: {out:?}"
    );
}

/// `x = f(x)` — the same point through the other code path. Perry's calling
/// convention passes NaN-boxed doubles, so the argument position materializes
/// even though the value comes straight back into `x`'s own slot.
#[test]
fn self_referencing_call_argument_is_still_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(100)),
            body: vec![set(1, call(vec![get(1)]))],
        },
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(
        out.contains(&1),
        "self-referencing call argument must still be refused: {out:?}"
    );
}

/// …and `x = new C(x)` likewise, since `new` has its own arm.
#[test]
fn self_referencing_new_argument_is_still_a_cost() {
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(100)),
            body: vec![set(
                1,
                Expr::New {
                    class_name: "C".into(),
                    args: vec![get(1)],
                    type_args: vec![],
                    byte_offset: 0,
                },
            )],
        },
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(
        out.contains(&1),
        "self-referencing new argument must still be refused: {out:?}"
    );
}

/// The exemption still holds through a representation-PRESERVING chain that
/// the model does not otherwise model: `t = -(t + other)` keeps `t` in its own
/// slot. The second local is the anti-vacuity control — it is read in the same
/// expression and is NOT a self-write, so it must be refused; a green verdict
/// for `t` therefore cannot come from the walk stopping early.
#[test]
fn a_self_write_through_a_preserving_chain_is_still_not_a_cost() {
    // 1 = t (self-written), 2 = other (read in the same expression)
    let stmts = vec![
        let_mut(1, Some(Expr::Integer(0))),
        let_mut(2, Some(Expr::Integer(0))),
        Stmt::Expr(inc(2)),
        Stmt::While {
            condition: cmp(get(1), Expr::Integer(100)),
            body: vec![set(
                1,
                Expr::Unary {
                    op: perry_hir::UnaryOp::Neg,
                    operand: Box::new(bin(BinaryOp::Add, get(1), get(2))),
                },
            )],
        },
    ];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "preserving self-write refused: {out:?}");
    assert!(out.contains(&2), "control local not refused: {out:?}");
}

/// An expression form the model does not understand contributes neither side.
/// A refusal rule must under-approximate the cost: a missed refusal is today's
/// behaviour, a spurious one is a lost promotion.
#[test]
fn unmodelled_forms_are_neutral() {
    let stmts = vec![for_loop(
        let_mut(1, Some(Expr::Integer(0))),
        cmp(get(1), Expr::Integer(100)),
        inc(1),
        vec![Stmt::Expr(Expr::TypeOf(Box::new(get(1))))],
    )];
    let out = run(&stmts, &HashSet::new());
    assert!(!out.contains(&1), "typeof-read counter refused: {out:?}");
}
