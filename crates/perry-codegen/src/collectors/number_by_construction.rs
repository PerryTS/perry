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
    // #10777: shape-proven receivers and the property names numeric on all of
    // them, from `collect_shape_proven_ptr_locals`. Empty reproduces the
    // pre-fix behaviour exactly.
    shape_members: &HashSet<u32>,
    shape_numeric_fields: &HashSet<String>,
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
        shape_members,
        shape_numeric_fields,
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
    let numberish = spec_numeric_params.clone();
    let invalid = {
        let mut flow = NumberAtReadFlow {
            candidates: &candidates,
            types: &types,
            ta_lens: &ta_lens,
            numeric_index_locals: spec_numeric_params,
            invalid: HashSet::new(),
            targets: Vec::new(),
            pending_labels: Vec::new(),
            finally_stack: Vec::new(),
        };
        flow.stmts(stmts, &mut FlowState::live(numberish));
        flow.invalid
    };

    candidates
        .into_iter()
        .filter(|id| !invalid.contains(id))
        .collect()
}

/// Flow state of [`NumberAtReadFlow`]: the locals that hold a Number on every
/// path reaching this point. A state is dead after `break`/`continue`/
/// `return`/`throw`; joins ignore dead states. A dead state keeps the set it
/// had at the jump so reads in the unreachable rest of the block are still
/// judged conservatively.
#[derive(Clone, PartialEq, Eq)]
struct FlowState {
    set: HashSet<u32>,
    live: bool,
}

impl FlowState {
    fn live(set: HashSet<u32>) -> Self {
        Self { set, live: true }
    }

    fn dead_copy(&self) -> Self {
        Self {
            set: self.set.clone(),
            live: false,
        }
    }

    /// Merge `other` into `self` at a join point.
    fn meet(&mut self, other: &FlowState) {
        if !other.live {
            return;
        }
        if !self.live {
            *self = other.clone();
            return;
        }
        self.set.retain(|id| other.set.contains(id));
    }

    fn meet_opt(&mut self, other: &Option<FlowState>) {
        if let Some(other) = other {
            self.meet(other);
        }
    }
}

fn meet_into(slot: &mut Option<FlowState>, state: &FlowState) {
    match slot {
        Some(existing) => existing.meet(state),
        None => *slot = Some(state.clone()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JumpTargetKind {
    /// Target of `break`, `continue` and their labeled forms. The HIR run-once
    /// `L: do { .. } while (false)` of a labeled block is one too, as codegen
    /// binds it.
    Loop,
    /// Target of unlabeled `break`.
    Switch,
    /// A labeled statement that is neither: target of `break label` only.
    Block,
}

/// A statement `break`/`continue` can jump to: the states of the jumps that
/// reach it join its end (`breaks`) or its next iteration (`continues`).
struct JumpTarget {
    kind: JumpTargetKind,
    labels: Vec<String>,
    breaks: Option<FlowState>,
    continues: Option<FlowState>,
    /// `finally_stack` depth when the target was entered: a jump to it runs
    /// every finally pushed since.
    finally_depth: usize,
}

struct NumberAtReadFlow<'a, 'h> {
    candidates: &'a HashSet<u32>,
    types: &'a HashMap<u32, HirType>,
    ta_lens: &'a HashMap<u32, i64>,
    numeric_index_locals: &'a HashSet<u32>,
    invalid: HashSet<u32>,
    targets: Vec<JumpTarget>,
    /// Labels of a `Stmt::Labeled` chain waiting for the loop/switch it names.
    pending_labels: Vec<String>,
    /// Finally blocks of the enclosing `try` statements (innermost last).
    finally_stack: Vec<&'h [Stmt]>,
}

impl<'h> NumberAtReadFlow<'_, 'h> {
    fn stmts(&mut self, stmts: &'h [Stmt], state: &mut FlowState) {
        for stmt in stmts {
            self.stmt(stmt, state);
        }
    }

    fn stmt(&mut self, stmt: &'h Stmt, state: &mut FlowState) {
        // Labels attach to the statement directly under the `Labeled`.
        let labels = if matches!(stmt, Stmt::Labeled { .. }) {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending_labels)
        };
        match stmt {
            Stmt::Let { id, init, .. } => match init {
                Some(rhs) => self.write(*id, rhs, &mut state.set),
                None => {
                    state.set.remove(id);
                }
            },
            Stmt::Expr(expr) => self.expr(expr, &mut state.set, true),
            Stmt::Throw(expr) => {
                self.expr(expr, &mut state.set, true);
                *state = state.dead_copy();
            }
            Stmt::Return(value) => {
                if let Some(expr) = value {
                    self.expr(expr, &mut state.set, false);
                }
                self.run_finallies(0, state);
                *state = state.dead_copy();
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, &mut state.set, false);
                let mut then_state = state.clone();
                self.stmts(then_branch, &mut then_state);
                let mut else_state = state.clone();
                if let Some(else_branch) = else_branch {
                    self.stmts(else_branch, &mut else_state);
                }
                then_state.meet(&else_state);
                if !then_state.live {
                    // Both arms jumped: keep the jump state of the then arm for
                    // the unreachable rest.
                    then_state.set.retain(|id| else_state.set.contains(id));
                }
                *state = then_state;
            }
            Stmt::While { condition, body } => {
                let always = matches!(condition, Expr::Bool(true));
                *state = self.run_loop(labels, state, |flow, head| {
                    let mut s = head.clone();
                    flow.expr(condition, &mut s.set, false);
                    let exit = if always { s.dead_copy() } else { s.clone() };
                    flow.stmts(body, &mut s);
                    let continues = flow.targets.last_mut().and_then(|t| t.continues.take());
                    s.meet_opt(&continues);
                    (exit, s)
                });
            }
            Stmt::DoWhile { body, condition } => {
                // `L: do { .. } while (false)` is the lowered labeled block:
                // the body runs once and there is no back edge.
                let once = matches!(condition, Expr::Bool(false));
                *state = self.run_loop(labels, state, |flow, head| {
                    let mut s = head.clone();
                    flow.stmts(body, &mut s);
                    let continues = flow.targets.last_mut().and_then(|t| t.continues.take());
                    s.meet_opt(&continues);
                    flow.expr(condition, &mut s.set, false);
                    let back = if once { s.dead_copy() } else { s.clone() };
                    (s, back)
                });
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init, state);
                }
                *state = self.run_loop(labels, state, |flow, head| {
                    let mut s = head.clone();
                    if let Some(condition) = condition {
                        flow.expr(condition, &mut s.set, false);
                    }
                    let exit = if condition.is_none() || matches!(condition, Some(Expr::Bool(true)))
                    {
                        s.dead_copy()
                    } else {
                        s.clone()
                    };
                    flow.stmts(body, &mut s);
                    let continues = flow.targets.last_mut().and_then(|t| t.continues.take());
                    s.meet_opt(&continues);
                    if let Some(update) = update {
                        flow.expr(update, &mut s.set, false);
                    }
                    (exit, s)
                });
            }
            Stmt::Labeled { label, body } => {
                if matches!(
                    **body,
                    Stmt::Labeled { .. }
                        | Stmt::While { .. }
                        | Stmt::DoWhile { .. }
                        | Stmt::For { .. }
                        | Stmt::Switch { .. }
                ) {
                    self.pending_labels.push(label.clone());
                    self.stmt(body, state);
                    self.pending_labels.clear();
                } else {
                    let mut block_labels = std::mem::take(&mut self.pending_labels);
                    block_labels.push(label.clone());
                    self.push_target(JumpTargetKind::Block, block_labels);
                    self.stmt(body, state);
                    let target = self.targets.pop().expect("labeled block target");
                    state.meet_opt(&target.breaks);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                let entry = state.clone();
                if let Some(finally) = finally {
                    self.finally_stack.push(finally);
                }
                let mut try_state = state.clone();
                self.stmts(body, &mut try_state);
                let mut merged = try_state;
                if let Some(catch) = catch {
                    // The catch is entered from the entry state, as before.
                    let mut catch_state = FlowState::live(entry.set.clone());
                    self.stmts(&catch.body, &mut catch_state);
                    merged.meet(&catch_state);
                    if !merged.live {
                        merged.set.retain(|id| catch_state.set.contains(id));
                    }
                }
                // An exception escaping body or catch also runs the finally;
                // its own state is at most the entry state.
                merged.set.retain(|id| entry.set.contains(id));
                if let Some(finally) = finally {
                    self.finally_stack.pop();
                    let was_live = merged.live;
                    let mut finally_state = FlowState::live(merged.set.clone());
                    self.stmts(finally, &mut finally_state);
                    if !finally_state.live {
                        merged = finally_state;
                    } else {
                        merged = FlowState {
                            set: finally_state.set,
                            live: was_live,
                        };
                    }
                }
                *state = merged;
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, &mut state.set, false);
                // Tests run in source order until one matches; a case body is
                // entered with the state after its own test or by fallthrough,
                // the default with the state after every test.
                let mut tests = state.clone();
                let mut after_test = Vec::with_capacity(cases.len());
                for case in cases {
                    if let Some(test) = &case.test {
                        self.expr(test, &mut tests.set, false);
                    }
                    after_test.push(tests.clone());
                }
                let has_default = cases.iter().any(|case| case.test.is_none());
                self.push_target(JumpTargetKind::Switch, labels);
                let mut fall = FlowState {
                    set: state.set.clone(),
                    live: false,
                };
                for (case, test_state) in cases.iter().zip(after_test) {
                    let mut entered = if case.test.is_some() {
                        test_state
                    } else {
                        tests.clone()
                    };
                    entered.meet(&fall);
                    self.stmts(&case.body, &mut entered);
                    fall = entered;
                }
                let target = self.targets.pop().expect("switch target");
                if !has_default {
                    fall.meet(&tests);
                }
                fall.meet_opt(&target.breaks);
                *state = fall;
            }
            Stmt::Break => {
                let index = self
                    .targets
                    .iter()
                    .rposition(|t| t.kind != JumpTargetKind::Block);
                self.jump(index, false, state);
            }
            Stmt::Continue => {
                let index = self
                    .targets
                    .iter()
                    .rposition(|t| t.kind == JumpTargetKind::Loop);
                self.jump(index, true, state);
            }
            Stmt::LabeledBreak(label) => {
                // Codegen falls back to the innermost loop/switch for a label
                // it cannot resolve; mirror that.
                let index = self
                    .targets
                    .iter()
                    .rposition(|t| t.labels.iter().any(|l| l == label))
                    .or_else(|| {
                        self.targets
                            .iter()
                            .rposition(|t| t.kind != JumpTargetKind::Block)
                    });
                self.jump(index, false, state);
            }
            Stmt::LabeledContinue(label) => {
                let index = self
                    .targets
                    .iter()
                    .rposition(|t| {
                        t.kind == JumpTargetKind::Loop && t.labels.iter().any(|l| l == label)
                    })
                    .or_else(|| {
                        self.targets
                            .iter()
                            .rposition(|t| t.kind == JumpTargetKind::Loop)
                    });
                self.jump(index, true, state);
            }
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn push_target(&mut self, kind: JumpTargetKind, labels: Vec<String>) {
        self.targets.push(JumpTarget {
            kind,
            labels,
            breaks: None,
            continues: None,
            finally_depth: self.finally_stack.len(),
        });
    }

    /// A jump to `targets[index]`: run the finally blocks it leaves, join its
    /// state into the target, and continue dead.
    fn jump(&mut self, index: Option<usize>, is_continue: bool, state: &mut FlowState) {
        if !state.live {
            return;
        }
        if let Some(index) = index {
            let mut jumped = state.clone();
            self.run_finallies(self.targets[index].finally_depth, &mut jumped);
            if jumped.live {
                let target = &mut self.targets[index];
                let slot = if is_continue {
                    &mut target.continues
                } else {
                    &mut target.breaks
                };
                meet_into(slot, &jumped);
            }
        }
        *state = state.dead_copy();
    }

    /// Walk the finally blocks above `depth` (innermost first) on a jump path.
    /// A finally walked here sees the enclosing targets as they are, so its
    /// own jumps resolve as they would at run time.
    fn run_finallies(&mut self, depth: usize, state: &mut FlowState) {
        let pending: Vec<&'h [Stmt]> = self.finally_stack[depth..].iter().rev().copied().collect();
        let saved = std::mem::take(&mut self.finally_stack);
        for (i, finally) in pending.into_iter().enumerate() {
            if !state.live {
                break;
            }
            // While a finally runs, only the try statements around it are open.
            self.finally_stack = saved[..saved.len() - 1 - i].to_vec();
            self.stmts(finally, state);
        }
        self.finally_stack = saved;
    }

    /// Run a loop to its fixpoint. `iteration` walks one iteration from the
    /// head state and returns (normal exit, back edge). The head is the meet
    /// of the entry and the back edge; sets only shrink, so this converges.
    fn run_loop(
        &mut self,
        labels: Vec<String>,
        entry: &FlowState,
        mut iteration: impl FnMut(&mut Self, &FlowState) -> (FlowState, FlowState),
    ) -> FlowState {
        let mut head = entry.clone();
        loop {
            self.push_target(JumpTargetKind::Loop, labels.clone());
            let (mut exit, back) = iteration(self, &head);
            let target = self.targets.pop().expect("loop target");
            let mut next = entry.clone();
            next.meet(&back);
            if next == head {
                exit.meet_opt(&target.breaks);
                if !exit.live {
                    // No path leaves the loop normally: keep a conservative
                    // set for the unreachable code after it.
                    exit.set.retain(|id| head.set.contains(id));
                }
                return exit;
            }
            head = next;
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
            byte_offset: 0,
            byte_end: 0,
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

    fn labeled_block(label: &str, body: Vec<Stmt>) -> Stmt {
        Stmt::Labeled {
            label: label.to_string(),
            body: Box::new(Stmt::DoWhile {
                body,
                condition: Expr::Bool(false),
            }),
        }
    }

    fn if_then(then_branch: Vec<Stmt>) -> Stmt {
        Stmt::If {
            condition: Expr::LocalGet(L),
            then_branch,
            else_branch: None,
        }
    }

    /// #12662: `blk: { if (k) break blk; n = 1; } return n` reads undefined
    /// on the break path.
    #[test]
    fn break_out_of_labeled_block_joins_the_block_end() {
        let stmts = vec![
            let_n(),
            labeled_block(
                "blk",
                vec![
                    if_then(vec![Stmt::LabeledBreak("blk".to_string())]),
                    set_n(Expr::Integer(1)),
                ],
            ),
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(!run(&stmts).contains(&N));
    }

    /// The same shape with the write before the break is a Number on every path.
    #[test]
    fn labeled_block_written_on_every_path_is_number() {
        let stmts = vec![
            let_n(),
            labeled_block(
                "blk",
                vec![
                    set_n(Expr::Integer(1)),
                    if_then(vec![Stmt::LabeledBreak("blk".to_string())]),
                    set_n(Expr::Integer(2)),
                ],
            ),
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(run(&stmts).contains(&N));
    }

    /// A real do-while: an unlabeled break skipping the write reaches the read.
    #[test]
    fn break_out_of_do_while_joins_the_loop_end() {
        let stmts = vec![
            let_n(),
            Stmt::DoWhile {
                body: vec![if_then(vec![Stmt::Break]), set_n(Expr::Integer(1))],
                condition: Expr::LocalGet(L),
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(!run(&stmts).contains(&N));
    }

    /// `n = 0; while (c) { n = undefined; if (k) break; n = 1; } return n`:
    /// the loop entry and back edge hold a Number, the break path does not.
    #[test]
    fn break_after_undefined_write_reaches_the_loop_exit() {
        let stmts = vec![
            let_n(),
            set_n(Expr::Integer(0)),
            Stmt::While {
                condition: Expr::LocalGet(L),
                body: vec![
                    set_n(Expr::Undefined),
                    if_then(vec![Stmt::Break]),
                    set_n(Expr::Integer(1)),
                ],
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(!run(&stmts).contains(&N));
    }

    /// `switch (k) { case 1: blk: { if (c) break; n = 1; } n = 2; break; default: n = 3; }`
    /// after the lowering fix: the unlabeled break is `break <switch label>`.
    #[test]
    fn labeled_break_to_a_labeled_switch_skips_the_case_tail() {
        let stmts = vec![
            let_n(),
            Stmt::Labeled {
                label: "sw".to_string(),
                body: Box::new(Stmt::Switch {
                    discriminant: Expr::LocalGet(L),
                    cases: vec![
                        perry_hir::SwitchCase {
                            test: Some(Expr::Integer(1)),
                            body: vec![
                                labeled_block(
                                    "blk",
                                    vec![
                                        if_then(vec![Stmt::LabeledBreak("sw".to_string())]),
                                        set_n(Expr::Integer(1)),
                                    ],
                                ),
                                set_n(Expr::Integer(2)),
                                Stmt::Break,
                            ],
                        },
                        perry_hir::SwitchCase {
                            test: None,
                            body: vec![set_n(Expr::Integer(3))],
                        },
                    ],
                }),
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(!run(&stmts).contains(&N));
    }

    /// A loop the body always leaves through `break` after writing.
    #[test]
    fn infinite_loop_exit_is_the_break_state() {
        let stmts = vec![
            let_n(),
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![set_n(Expr::Integer(1)), if_then(vec![Stmt::Break])],
            },
            Stmt::Return(Some(Expr::LocalGet(N))),
        ];
        assert!(run(&stmts).contains(&N));
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
                        byte_offset: 0,
                        byte_end: 0,
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
            &HashSet::new(),
            &HashSet::new(),
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

// ── #10777: shape inputs for the function-scope walk ──────────────────────

/// Turn the receiver proofs into the `(members, numeric_fields)` pair the
/// function-scope fixpoint needs.
///
/// ## Why an INTERSECTION, and why that is sound
///
/// `expr_numeric_by_construction` takes ONE `numeric_fields` set for ONE
/// receiver class, because its other caller proves one receiver at a time. A
/// function-scope walk may see several shape-proven receivers of different
/// classes, and the arm it feeds asks only "is `members.contains(recv)` and
/// `numeric_fields.contains(prop)`" — it does not re-check which receiver the
/// property belongs to.
///
/// So the set passed must be numeric on **every** admitted receiver, which is
/// the intersection: if `prop` is numeric on all of them, it is numeric on
/// whichever one the expression names. Under-approximates when receivers
/// disagree; exact for a single shape-proven receiver.
///
/// A union would be a WRONG ANSWER, not a weaker one: `a` numeric on `C` and
/// not on `D` would license a bare `fadd` on `D.a`.
pub(crate) fn shape_numeric_inputs(
    shape_proven: &HashMap<u32, crate::collectors::ptr_shape::PtrShapeLocal>,
) -> (HashSet<u32>, HashSet<String>) {
    if shape_proven.is_empty() {
        return (HashSet::new(), HashSet::new());
    }
    let mut members: HashSet<u32> = HashSet::new();
    let mut fields: Option<HashSet<String>> = None;
    for (id, fact) in shape_proven {
        members.insert(*id);
        fields = Some(match fields {
            None => fact.numeric_fields.clone(),
            Some(acc) => acc
                .intersection(&fact.numeric_fields)
                .cloned()
                .collect::<HashSet<String>>(),
        });
    }
    let fields = fields.unwrap_or_default();
    if fields.is_empty() {
        return (HashSet::new(), HashSet::new());
    }
    (members, fields)
}

#[cfg(test)]
mod shape_input_tests {
    use super::*;
    use crate::collectors::ptr_shape::PtrShapeLocal;

    fn property_local(id: u32, receiver: u32, property: &str) -> Stmt {
        Stmt::Let {
            id,
            name: format!("value_{id}"),
            ty: HirType::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::LocalGet(receiver)),
                property: property.to_string(),
                byte_offset: 0,
            }),
        }
    }

    fn numeric_locals(
        stmts: &[Stmt],
        members: &HashSet<u32>,
        fields: &HashSet<String>,
    ) -> HashSet<u32> {
        super::super::ptr_shape::collect_numeric_by_construction_locals_for_type_analysis(
            stmts,
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
            members,
            fields,
        )
    }

    #[test]
    fn shape_inputs_intersect_numeric_fields_before_proving_property_locals() {
        let shape_proven = HashMap::from([
            (
                10,
                PtrShapeLocal {
                    family: None,
                    class_name: "First".to_string(),
                    numeric_fields: HashSet::from(["shared".to_string(), "first_only".to_string()]),
                    report_name: None,
                },
            ),
            (
                11,
                PtrShapeLocal {
                    family: None,
                    class_name: "Second".to_string(),
                    numeric_fields: HashSet::from(["shared".to_string()]),
                    report_name: None,
                },
            ),
        ]);
        let stmts = [
            property_local(20, 10, "shared"),
            property_local(21, 11, "shared"),
            property_local(22, 11, "first_only"),
            property_local(23, 10, "first_only"),
            property_local(24, 12, "shared"),
        ];

        let (members, fields) = shape_numeric_inputs(&shape_proven);
        let numeric = numeric_locals(&stmts, &members, &fields);
        assert!(numeric.contains(&20), "shared field on First is numeric");
        assert!(numeric.contains(&21), "shared field on Second is numeric");
        assert!(
            !numeric.contains(&22),
            "union would unsoundly admit Second.first_only"
        );
        assert!(
            !numeric.contains(&23),
            "function-wide inputs must be safe for both receivers"
        );
        assert!(
            !numeric.contains(&24),
            "an unproven receiver is not a numeric input"
        );
    }
}
