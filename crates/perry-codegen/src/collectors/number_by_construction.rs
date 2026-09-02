//! #8105: locals that hold a JS **Number by construction**, exposed to
//! `type_analysis::is_numeric_expr`.
//!
//! ## The gap this closes
//!
//! Every numeric proof `is_numeric_expr` had for a bare `LocalGet` was either
//! an integer-range fact (`integer_locals`, `unsigned_i32_locals`,
//! `int_valued_i64_locals`) or `FnCtx::stable_local_type_proof`, which answers
//! `None` the moment the local is written a second time. A plain fractional
//! accumulator therefore had **no** numeric proof at all:
//!
//! ```ignore
//! let x = 0.0;
//! let y = 0.0;
//! while (x * x + y * y <= 4.0 && iter < MAX_ITER) {
//!     const xtemp = x * x - y * y + cx;
//!     y = 2.0 * x * y + cy;
//!     x = xtemp;
//! }
//! ```
//!
//! `x` and `y` are reassigned, so `expr/binary.rs`'s "both operands are
//! statically primitive" test failed and **every multiply** bailed to the
//! BigInt-aware `js_dynamic_mul` — six opaque calls per iteration of
//! `benchmarks/suite/15_mandelbrot.ts`'s inner loop. `+` and `-` stayed inline
//! because a `Binary { op: Mul, .. }` operand IS numeric by the recursive rule,
//! which is why the symptom was "only the multiplies escape".
//!
//! ## The proof
//!
//! [`collect_number_by_construction_locals`] is the existing
//! `collectors/ptr_shape_numeric.rs` locals fixpoint (#7770), which is already
//! trusted for a strictly harder claim: it licenses a bare `load double` on a
//! proven numeric FIELD with no coercion and no value check. It admits a local
//! only when its `let` initialiser **and every later write** is an expression
//! the spec guarantees evaluates to a Number, judged structurally — declared
//! types are never evidence (Perry does not enforce annotations, #7773).
//!
//! A JS Number's Perry representation IS its raw double (numbers carry no
//! NaN-box tag), so "the value is a Number" and "the f64 in this slot is a
//! canonical double" are the same statement. That is exactly what the numeric
//! fast path needs for both of its steps: skipping the dynamic helper, and
//! skipping the residual `js_number_coerce` in
//! `binary.rs::operand_needs_residual_coerce`.
//!
//! ## Why the candidate set is fail-closed
//!
//! Candidates are the ids with a `Stmt::Let` in the scanned body, so a
//! PARAMETER is never admitted (its incoming value is unconstrained, and a
//! read can precede the first assignment), and neither is a local captured
//! from an enclosing scope (its writes live outside this walk). Closure-boxed
//! locals and module globals are excluded outright. A `let x;` with no
//! initialiser is `undefined`, which is not a Number, and drops the local.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{Expr, Param, Stmt};

/// `PERRY_NUMBER_BY_CONSTRUCTION` gate. Enabled by default; `=0`/`off`/`false`
/// empties the fact, reverting every reassigned numeric accumulator to the
/// BigInt-aware dynamic-helper routing (pre-#8105 behaviour). Kept as an env
/// flag for A/B bisection, consistent with the sibling codegen fast paths
/// (`PERRY_INLINE_NONBIGINT_BITWISE`, `PERRY_PTR_SHAPE_LOCALS`); both the
/// build-level probe and the object cache key it, so a warm cache cannot serve
/// an object built under the other setting.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_NUMBER_BY_CONSTRUCTION").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Typed-array/`Uint8Array` class names whose elements are Numbers.
///
/// The BigInt kinds are deliberately absent: `BigInt64Array`/`BigUint64Array`
/// elements are BigInts, not Numbers, and `+` on one throws when mixed. Kept
/// as a name list rather than reusing a kind table because
/// `module_global_proven_types` records the CLASS NAME the initializer
/// constructed.
fn class_name_is_number_valued_view(name: &str) -> bool {
    matches!(
        name,
        "Uint8Array"
            | "Uint8ClampedArray"
            | "Int8Array"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "Buffer"
    )
}

/// Locals whose every write is number-producing by construction.
///
/// Deliberately independent of `PERRY_PTR_SHAPE_LOCALS`: the fact is about
/// locals, not about shape promotion, so it must not appear and disappear with
/// the repsel gate. The `const_local_inits` chase the `Ptr<Shape>` pass feeds
/// the same fixpoint is a strictly additional edge — every single-`Let` const
/// it resolves is itself a candidate here, so the fixpoint reaches the same
/// verdict without it.
pub(crate) fn collect_number_by_construction_locals(
    stmts: &[Stmt],
    params: &[Param],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    binding_types: &HashMap<u32, HirType>,
    spec_ta_lens: &HashMap<u32, i64>,
    spec_numeric_params: &HashSet<u32>,
    not_bigint_locals: &HashSet<u32>,
    module_global_proven_types: &HashMap<u32, HirType>,
) -> HashSet<u32> {
    if !enabled() {
        return HashSet::new();
    }
    // #8619: spec-ABI `TaPtr` parameters are proven to permanently hold one
    // specific NUMERIC-kind, non-view typed array — `spec_ta_lens` is keyed
    // exactly by those params (its only source is `SpecParamRep::TaPtr`, which
    // `collectors::spec_abi_sites` admits only for `spec_ta_kind_is_numeric`
    // kinds; the BigInt kinds are never TaPtr). A read `arr[numeric_index]` off
    // one is therefore a Number (in-bounds) or `undefined` (OOB), never a
    // pointer/string, so the fixpoint may treat it like a compiler-visible
    // local typed-view constructor on one side of `+` (where `undefined`
    // becomes the Number NaN rather than selecting string concatenation).
    let mut numeric_ta_views: HashSet<u32> = spec_ta_lens.keys().copied().collect();
    // #9363: a MODULE-GLOBAL typed array carries the same construction proof a
    // body-local `const view = new Uint8Array(n)` does, and for the same
    // reason: `module_global_proven_types` is derived from the initializer
    // expression (`Expr::Uint8ArrayNew` / `TypedArrayNew`) on a single-`Let`,
    // never-reassigned binding — it is not a declared type, which this
    // collector correctly refuses to treat as evidence (#7773).
    //
    // Without this the fixpoint saw `const buf = new Uint8Array(N)` at module
    // scope and answered "unknown receiver", so `acc += buf[i]` inside a
    // function lost the accumulator's Number-by-construction proof. The cost
    // was not the read: the missing proof made the `+` non-inert, which made
    // `loop_purity::loop_may_allocate` answer `true`, which kept a per-
    // iteration `load volatile @PERRY_GC_POLL_ARMED` in the inner loop —
    // blocking vectorization and pinning the accumulator in memory — and
    // routed the add through the rooted `guarded_add` diamond. Measured 444 ms
    // vs 94 ms for the identical loop over a body-local receiver.
    numeric_ta_views.extend(
        module_global_proven_types
            .iter()
            .filter(|(_, ty)| matches!(ty, HirType::Named(name) if class_name_is_number_valued_view(name)))
            .map(|(id, _)| *id),
    );
    let mut numeric = super::ptr_shape::collect_numeric_by_construction_locals_for_type_analysis(
        stmts,
        boxed_vars,
        module_globals,
        not_bigint_locals,
        &HashMap::new(),
        &numeric_ta_views,
    );
    numeric.extend(collect_number_at_read_after_undefined(
        stmts,
        params,
        boxed_vars,
        module_globals,
        binding_types,
        spec_ta_lens,
        spec_numeric_params,
    ));
    numeric
}

/// Locals whose only non-Number state is `undefined`, and whose every read is
/// dominated by a write that establishes a genuine Number.
///
/// TypeScript lowers `let n: number;` to an explicit `Undefined` seed.  The
/// whole-write proof above must reject that seed, but rejecting the local at
/// every later read is unnecessarily strong when control flow overwrites it
/// first on every path.  This pass supplies the missing flow leg without
/// trusting the declaration: writes qualify only through runtime-established
/// facts (literals, Number-producing operations, or an in-bounds int typed-
/// array read backed by a specialized-entry/constructor length proof).
///
/// A non-Number write other than `undefined` poisons the candidate even when
/// it is overwritten before a read.  That keeps the broader consumers of
/// `number_by_construction_locals` sound: between reads they may also use the
/// fact to omit pointer rooting, and `undefined` is non-pointer while a string
/// or object is not.
fn collect_number_at_read_after_undefined(
    stmts: &[Stmt],
    params: &[Param],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    binding_types: &HashMap<u32, HirType>,
    spec_ta_lens: &HashMap<u32, i64>,
    spec_numeric_params: &HashSet<u32>,
) -> HashSet<u32> {
    let mut writes: HashMap<u32, Vec<Option<&Expr>>> = HashMap::new();
    let mut let_bound = HashSet::new();
    super::not_bigint_locals::collect_writes(stmts, &mut writes, &mut let_bound);

    let candidates: HashSet<u32> = let_bound
        .into_iter()
        .filter(|id| !boxed_vars.contains(id) && !module_globals.contains_key(id))
        .filter(|id| {
            writes.get(id).is_some_and(|local_writes| {
                local_writes
                    .iter()
                    .any(|write| matches!(write, None | Some(Expr::Undefined)))
            })
        })
        .collect();
    if candidates.is_empty() {
        return candidates;
    }

    let mut types = binding_types.clone();
    for param in params {
        types.entry(param.id).or_insert_with(|| param.ty.clone());
    }
    let mut ta_lens = super::integer_locals::collect_const_int_ta_views(stmts);
    for (id, len) in spec_ta_lens {
        ta_lens.entry(*id).or_insert(*len);
    }

    // Only raw numeric parameters proven by the specialized entry seed the
    // initial flow state.  Declared `number` parameters remain untrusted in a
    // generic body.  The same set is the conservative numeric-key evidence
    // passed to the shared typed-array write predicate.
    let mut numberish = spec_numeric_params.clone();
    let invalid = {
        let mut flow = NumberAtReadFlow {
            candidates: &candidates,
            types: &types,
            ta_lens: &ta_lens,
            numeric_index_locals: spec_numeric_params,
            invalid: HashSet::new(),
        };
        flow.stmts(stmts, &mut numberish);
        flow.invalid
    };

    candidates
        .into_iter()
        .filter(|id| !invalid.contains(id))
        .collect()
}

struct NumberAtReadFlow<'a> {
    candidates: &'a HashSet<u32>,
    types: &'a HashMap<u32, HirType>,
    ta_lens: &'a HashMap<u32, i64>,
    numeric_index_locals: &'a HashSet<u32>,
    invalid: HashSet<u32>,
}

impl NumberAtReadFlow<'_> {
    fn stmts(&mut self, stmts: &[Stmt], numberish: &mut HashSet<u32>) {
        for stmt in stmts {
            self.stmt(stmt, numberish);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, numberish: &mut HashSet<u32>) {
        match stmt {
            Stmt::Let { id, init, .. } => match init {
                Some(rhs) => self.write(*id, rhs, numberish),
                None => {
                    numberish.remove(id);
                }
            },
            Stmt::Expr(expr) | Stmt::Throw(expr) => self.expr(expr, numberish, true),
            Stmt::Return(Some(expr)) => self.expr(expr, numberish, false),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, numberish, false);
                let mut then_state = numberish.clone();
                self.stmts(then_branch, &mut then_state);
                let mut else_state = numberish.clone();
                if let Some(else_branch) = else_branch {
                    self.stmts(else_branch, &mut else_state);
                }
                *numberish = then_state.intersection(&else_state).copied().collect();
            }
            Stmt::While { condition, body } => {
                let entry = numberish.clone();
                self.expr(condition, numberish, false);
                let mut backedge = numberish.clone();
                self.stmts(body, &mut backedge);
                // Re-check the condition under the back-edge state.  This is
                // what prevents a body write from invalidating a value read
                // by the next iteration's condition without being noticed.
                self.expr(condition, &mut backedge, false);
                *numberish = entry.intersection(&backedge).copied().collect();
            }
            Stmt::DoWhile { body, condition } => {
                let entry = numberish.clone();
                let mut backedge = numberish.clone();
                self.stmts(body, &mut backedge);
                self.expr(condition, &mut backedge, false);
                // A second conservative body walk covers later iterations
                // whose entry state is the meet of the first entry/back-edge.
                let mut loop_entry: HashSet<u32> = entry.intersection(&backedge).copied().collect();
                self.stmts(body, &mut loop_entry);
                self.expr(condition, &mut loop_entry, false);
                *numberish = backedge.intersection(&loop_entry).copied().collect();
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init, numberish);
                }
                let entry = numberish.clone();
                if let Some(condition) = condition {
                    self.expr(condition, numberish, false);
                }
                let mut backedge = numberish.clone();
                self.stmts(body, &mut backedge);
                if let Some(update) = update {
                    self.expr(update, &mut backedge, false);
                }
                if let Some(condition) = condition {
                    self.expr(condition, &mut backedge, false);
                }
                let mut loop_entry: HashSet<u32> = entry.intersection(&backedge).copied().collect();
                self.stmts(body, &mut loop_entry);
                if let Some(update) = update {
                    self.expr(update, &mut loop_entry, false);
                }
                *numberish = entry.intersection(&loop_entry).copied().collect();
            }
            Stmt::Labeled { body, .. } => self.stmt(body, numberish),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let entry = numberish.clone();
                let mut try_state = numberish.clone();
                self.stmts(body, &mut try_state);
                let mut merged: HashSet<u32> = entry.intersection(&try_state).copied().collect();
                if let Some(catch) = catch {
                    let mut catch_state = entry.clone();
                    self.stmts(&catch.body, &mut catch_state);
                    merged.retain(|id| catch_state.contains(id));
                }
                if let Some(finally) = finally {
                    self.stmts(finally, &mut merged);
                }
                *numberish = merged;
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, numberish, false);
                let entry = numberish.clone();
                let mut merged = entry.clone();
                for case in cases {
                    if let Some(test) = &case.test {
                        let mut test_state = entry.clone();
                        self.expr(test, &mut test_state, false);
                        merged.retain(|id| test_state.contains(id));
                    }
                    let mut case_state = entry.clone();
                    self.stmts(&case.body, &mut case_state);
                    merged.retain(|id| case_state.contains(id));
                }
                *numberish = merged;
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::Return(None)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn write(&mut self, target: u32, rhs: &Expr, numberish: &mut HashSet<u32>) {
        self.expr(rhs, numberish, false);
        if matches!(rhs, Expr::Undefined) {
            numberish.remove(&target);
            return;
        }
        if super::int_valued_ta_locals::write_establishes_number(
            rhs,
            self.types,
            self.ta_lens,
            numberish,
            self.numeric_index_locals,
        ) {
            numberish.insert(target);
        } else {
            numberish.remove(&target);
            if self.candidates.contains(&target) {
                self.invalid.insert(target);
            }
        }
    }

    fn expr(&mut self, expr: &Expr, numberish: &mut HashSet<u32>, root_store: bool) {
        match expr {
            Expr::LocalGet(id) => {
                if self.candidates.contains(id) && !numberish.contains(id) {
                    self.invalid.insert(*id);
                }
            }
            Expr::LocalSet(id, rhs) if root_store => self.write(*id, rhs, numberish),
            Expr::LocalSet(id, rhs) => {
                // Conditional/short-circuit expression writes need their own
                // CFG before they can establish dominance.  Decline them.
                self.expr(rhs, numberish, false);
                numberish.remove(id);
                if self.candidates.contains(id) {
                    self.invalid.insert(*id);
                }
            }
            Expr::Update { id, .. } => {
                if self.candidates.contains(id) && !numberish.contains(id) {
                    self.invalid.insert(*id);
                }
                if !numberish.contains(id) {
                    numberish.remove(id);
                }
            }
            Expr::Closure { .. } => {
                // Captured mutable locals are excluded through `boxed_vars`.
                // The HIR walker intentionally keeps closure bodies separate.
            }
            _ => perry_hir::walker::walk_expr_children(expr, &mut |child| {
                self.expr(child, numberish, false)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{BinaryOp, CompareOp};

    const N: u32 = 1;
    const S: u32 = 2;
    const L: u32 = 3;

    fn let_n() -> Stmt {
        Stmt::Let {
            id: N,
            name: "n".to_string(),
            ty: HirType::Number,
            mutable: true,
            init: Some(Expr::Undefined),
        }
    }

    fn set_n(rhs: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(N, Box::new(rhs)))
    }

    fn s_read(index: Expr) -> Expr {
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(S)),
            index: Box::new(index),
        }
    }

    fn add(left: Expr, right: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn run(stmts: &[Stmt]) -> HashSet<u32> {
        let params = [Param {
            id: S,
            name: "S".to_string(),
            ty: HirType::Named("Int32Array".to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }];
        collect_number_at_read_after_undefined(
            stmts,
            &params,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::from([(S, 1024)]),
            &HashSet::new(),
        )
    }

    fn masked_s_read() -> Expr {
        s_read(Expr::Binary {
            op: BinaryOp::UShr,
            left: Box::new(Expr::LocalGet(L)),
            right: Box::new(Expr::Integer(24)),
        })
    }

    #[test]
    fn undefined_seed_reset_before_each_loop_read_is_number_at_reads() {
        let stmts = vec![
            let_n(),
            Stmt::While {
                condition: Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::Integer(0)),
                    right: Box::new(Expr::Integer(1)),
                },
                body: vec![
                    set_n(masked_s_read()),
                    set_n(add(Expr::LocalGet(N), s_read(Expr::Integer(256)))),
                    Stmt::Expr(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left: Box::new(Expr::LocalGet(N)),
                        right: Box::new(Expr::Integer(7)),
                    }),
                ],
            },
        ];

        assert!(run(&stmts).contains(&N));
    }

    #[test]
    fn read_before_first_numeric_write_keeps_dynamic_type() {
        let stmts = vec![
            let_n(),
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![set_n(add(Expr::LocalGet(N), Expr::Integer(1)))],
            },
        ];

        assert!(!run(&stmts).contains(&N));
    }

    #[test]
    fn zero_trip_loop_does_not_dominate_a_later_read() {
        let stmts = vec![
            let_n(),
            Stmt::While {
                condition: Expr::Bool(false),
                body: vec![set_n(Expr::Integer(1))],
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];

        assert!(!run(&stmts).contains(&N));
    }

    #[test]
    fn both_branch_writes_dominate_the_join_read() {
        let stmts = vec![
            let_n(),
            Stmt::If {
                condition: Expr::LocalGet(99),
                then_branch: vec![set_n(Expr::Integer(1))],
                else_branch: Some(vec![set_n(Expr::Number(2.5))]),
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];

        assert!(run(&stmts).contains(&N));
    }

    #[test]
    fn missing_branch_write_keeps_join_read_dynamic() {
        let stmts = vec![
            let_n(),
            Stmt::If {
                condition: Expr::LocalGet(99),
                then_branch: vec![set_n(Expr::Integer(1))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];

        assert!(!run(&stmts).contains(&N));
    }

    #[test]
    fn pointer_write_poisons_rooting_fact_even_when_overwritten() {
        let stmts = vec![
            let_n(),
            set_n(Expr::String("temporary".to_string())),
            set_n(Expr::Integer(1)),
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];

        assert!(!run(&stmts).contains(&N));
    }

    // #8619: a spec-ABI `TaPtr` parameter is proven to permanently hold one
    // specific NUMERIC-kind, non-view typed array, so `arr[numeric_index]` is a
    // Number (in-bounds) or `undefined` (OOB) — never a pointer. The
    // number-by-construction fixpoint must therefore admit a fresh
    // read-derived local `let x = arr[i] + 1.0` (whose value is a genuine
    // Number, `NaN` at worst) and cascade to the loop-carried accumulator
    // `s = s + x`, so both drop their GC-root slot and their arithmetic stays
    // an inline `fadd` instead of `js_dynamic_string_or_number_add`.
    fn ta_view_stmts(arr: u32, s_id: u32, x_id: u32) -> Vec<Stmt> {
        vec![
            Stmt::Let {
                id: s_id,
                name: "s".to_string(),
                ty: HirType::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Let {
                id: x_id,
                name: "x".to_string(),
                ty: HirType::Number,
                mutable: false,
                init: Some(add(
                    Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(arr)),
                        index: Box::new(Expr::Integer(0)),
                    },
                    Expr::Number(1.0),
                )),
            },
            Stmt::Expr(Expr::LocalSet(
                s_id,
                Box::new(add(Expr::LocalGet(s_id), Expr::LocalGet(x_id))),
            )),
        ]
    }

    fn run_fixpoint(stmts: &[Stmt], ta_views: &HashSet<u32>) -> HashSet<u32> {
        crate::collectors::ptr_shape::collect_numeric_by_construction_locals_for_type_analysis(
            stmts,
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
            ta_views,
        )
    }

    #[test]
    fn spec_ta_param_view_admits_read_derived_number_locals() {
        let (arr, s_id, x_id) = (10u32, 20u32, 21u32);
        let stmts = ta_view_stmts(arr, s_id, x_id);

        let with = run_fixpoint(&stmts, &HashSet::from([arr]));
        assert!(
            with.contains(&x_id),
            "fresh `arr[i] + 1.0` local must be Number by construction"
        );
        assert!(
            with.contains(&s_id),
            "accumulator must cascade to Number by construction"
        );
    }

    #[test]
    fn ta_read_without_spec_proof_stays_dynamic() {
        // Same body, but the receiver is NOT a spec-proven typed array: the read
        // could be a string/property access on an arbitrary receiver, so neither
        // local may be un-rooted.
        let (arr, s_id, x_id) = (10u32, 20u32, 21u32);
        let stmts = ta_view_stmts(arr, s_id, x_id);

        let without = run_fixpoint(&stmts, &HashSet::new());
        assert!(!without.contains(&x_id));
        assert!(!without.contains(&s_id));
    }

    // The HIR lowers `px[i]` on a binding it knows is a `Uint8Array` to the
    // dedicated `Uint8ArrayGet` node, not `IndexGet`. The Add rule's
    // number-or-`undefined` view read must recognise both spellings, or the
    // canonical `const px = new Uint8Array(n); ... acc += px[i]` accumulator
    // (bench_int_arithmetic) never leaves the dynamic-add tier.
    fn uint8array_get_stmts(px: u32, i: u32, acc: u32) -> Vec<Stmt> {
        vec![
            Stmt::Let {
                id: px,
                name: "px".to_string(),
                ty: HirType::Named("Uint8Array".to_string()),
                mutable: false,
                init: Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(16))))),
            },
            Stmt::Let {
                id: i,
                name: "i".to_string(),
                ty: HirType::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::Let {
                id: acc,
                name: "acc".to_string(),
                ty: HirType::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Expr(Expr::LocalSet(
                acc,
                Box::new(add(
                    Expr::LocalGet(acc),
                    Expr::Uint8ArrayGet {
                        array: Box::new(Expr::LocalGet(px)),
                        index: Box::new(Expr::LocalGet(i)),
                    },
                )),
            )),
        ]
    }
    #[test]
    fn uint8array_get_node_admits_the_accumulator_like_index_get() {
        let (px, i, acc) = (10u32, 11u32, 12u32);
        let stmts = uint8array_get_stmts(px, i, acc);
        let numeric = run_fixpoint(&stmts, &HashSet::new());
        assert!(
            numeric.contains(&acc),
            "`acc += px[i]` on a const Uint8Array binding must be Number by construction"
        );
    }
    #[test]
    fn uint8array_get_node_with_string_write_stays_dynamic() {
        // One non-numeric write to the accumulator must still reject it.
        let (px, i, acc) = (10u32, 11u32, 12u32);
        let mut stmts = uint8array_get_stmts(px, i, acc);
        stmts.push(Stmt::Expr(Expr::LocalSet(
            acc,
            Box::new(add(Expr::LocalGet(acc), Expr::String("x".to_string()))),
        )));
        let numeric = run_fixpoint(&stmts, &HashSet::new());
        assert!(!numeric.contains(&acc));
    }
}
