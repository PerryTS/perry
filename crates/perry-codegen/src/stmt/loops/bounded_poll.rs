//! No back-edge poll in a loop that is provably bounded and call-free.
//!
//! `emit_gc_loop_safepoint` already leaves out the poll of a body whose HIR
//! cannot allocate (`loop_purity::loop_may_allocate`). That test is made
//! before the facts choose a lowering: a body that reads `this.board` through
//! a field read call, or an element of an Array whose representation the
//! facts prove, is "a call" or "a read that may reach a getter" in the HIR and
//! a few loads in the emitted code. Such a loop polls on every iteration
//! (one volatile load, one compare, one branch), and the volatile load keeps
//! LLVM from holding anything across the back edge.
//!
//! A loop statement L loses its polls iff both hold:
//!
//! 1. **Trip count.** L has a counter `c` whose whole-program interval is
//!    `[lo, hi]` (`range_facts::facts_local_range`: every value `c` ever
//!    holds is an int32 in it), and every iteration moves `c` at least once,
//!    always in the same direction, by a nonzero integer (`trip`). The values
//!    `c` holds when successive iterations start are then distinct integers
//!    of `[lo, hi]`, and each of them passed L's test: where the test bounds
//!    `c` by a literal (`c < 128`, `c >= 0 && c < 8`, `c <= Math.min(7, x)`),
//!    they lie in that window too (`guard_window`). L runs at most as many
//!    iterations per entry as the interval and the window share integers
//!    (one more for a `do-while`, whose first iteration no test admits).
//! 2. **Body effect.** No block emitted for L (tests, body, update, nested
//!    loops, cold arms, every clone a versioning tier made) holds a call that
//!    may collect or run user code: only LLVM intrinsics, inline asm markers
//!    and the runtime helpers the linked archive's table classes `Leaf`
//!    (`gc_call_effects`: reaches no collector entry, no poll, no JS). This is
//!    read off the emitted blocks after L is lowered, so it is the lowering
//!    the facts chose, not a prediction of it. A block that ends in
//!    `unreachable` never returns to the loop and is not counted.
//!
//! The polls of L are emitted as usual, each behind its own block
//! (`gcpoll.check`); when L is closed and both hold, that block becomes a
//! plain branch to the code after the poll and the poll's blocks become
//! unreachable.
//!
//! **Why this is sound.** A poll does nothing unless a collection is pending,
//! and a collection becomes pending only in a runtime call that allocates.
//! By 2 no such call runs between L's entry and its exit, so no poll of L
//! could fire for a collection L itself deferred. A collection deferred
//! *before* L (by the code in front of it) waits for the next poll after L.
//! That wait is what 1 bounds.
//!
//! **The bound.** The poll-free stretch is limited to [`BUDGET`] back edges,
//! counted over the whole nest: a loop's weight is its trip bound times one
//! plus the weights of the poll-free loops nested in it, and a loop whose
//! weight is above the budget keeps its polls (its nested loops, each within
//! the budget, still lose theirs). 65 536 call-free iterations are well under
//! a millisecond, which is far below the time between two polls of ordinary
//! code that calls anything, so a pending collection, or any later use of
//! the poll as an interruption point, is delayed by a negligible, bounded
//! amount. A loop with no trip proof, a larger trip bound, or any collecting
//! call keeps today's polls.
//!
//! `PERRY_FACTS_BOUNDED_LOOPS=0` turns it off (today's polls, the same
//! blocks). `PERRY_LOOP_POLL_DIAG=1` prints one line per loop with a trip
//! proof (`[perry loop poll]`). Check mode (`PERRY_VERIFY_FACTS=1`) keeps
//! every poll and counts the iterations of each loop with a trip proof: one
//! more than the bound reports the false proof and exits 86
//! (`loop trip bound`). `PERRY_FACTS_SABOTAGE=looptrip` accepts a counter
//! whose other writes were not looked at.

use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp, UpdateOp};

use super::{expr_mutates_local, stmts_mutate_local};
use crate::expr::FnCtx;
use crate::gc_call_effects::{GcCallEffect, GcEffects};
use crate::inst::LlInst;
use crate::types::{DOUBLE, I32, I64};

/// The most back edges a poll-free loop nest may run per entry (module docs).
pub(crate) const BUDGET: u64 = 65_536;

/// `PERRY_FACTS_BOUNDED_LOOPS=0` keeps every poll.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_BOUNDED_LOOPS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn diag_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_LOOP_POLL_DIAG").as_deref() == Ok("1"))
}

fn sabotaged() -> bool {
    std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("looptrip")
}

/// The loop statement being lowered.
#[derive(Clone, Copy)]
pub(crate) enum Loop<'a> {
    For {
        condition: Option<&'a Expr>,
        update: Option<&'a Expr>,
        body: &'a [Stmt],
    },
    While {
        condition: &'a Expr,
        body: &'a [Stmt],
    },
    DoWhile {
        body: &'a [Stmt],
        condition: &'a Expr,
    },
}

impl<'a> Loop<'a> {
    fn kind(self) -> &'static str {
        match self {
            Loop::For { .. } => "for",
            Loop::While { .. } => "while",
            Loop::DoWhile { .. } => "do-while",
        }
    }

    fn body(self) -> &'a [Stmt] {
        match self {
            Loop::For { body, .. } | Loop::While { body, .. } | Loop::DoWhile { body, .. } => body,
        }
    }

    /// The expressions the loop evaluates outside its body, every iteration.
    fn controls(self) -> Vec<&'a Expr> {
        match self {
            Loop::For {
                condition, update, ..
            } => condition.into_iter().chain(update).collect(),
            Loop::While { condition, .. } | Loop::DoWhile { condition, .. } => vec![condition],
        }
    }
}

/// The trip proof of a loop (module docs, 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Trip {
    pub(crate) counter: u32,
    pub(crate) lo: i64,
    pub(crate) hi: i64,
    /// The most iterations per entry: the integers of `[lo, hi]` the loop's
    /// test admits (`guard_window`).
    pub(crate) bound: u64,
}

/// One emitted poll of a loop: its `gcpoll.check` block, the block after the
/// poll, and the end of the blocks the poll made.
struct Poll {
    check: usize,
    done: usize,
    end: usize,
}

struct Scope {
    kind: &'static str,
    /// The first block made for the loop.
    start: usize,
    trip: Option<Trip>,
    polls: Vec<Poll>,
    /// The weights of the poll-free loops nested in this one.
    inner: u64,
    /// Check mode: the iteration count of this entry.
    count_slot: Option<String>,
}

/// The loops being lowered, innermost last (`FnCtx::loop_polls`).
#[derive(Default)]
pub(crate) struct Scopes(Vec<Scope>);

/// Open the loop `l` (before anything of it is lowered). Returns whether
/// [`end`] has a scope to close.
pub(crate) fn begin(ctx: &mut FnCtx<'_>, l: Loop<'_>) -> bool {
    if !enabled() {
        return false;
    }
    // A loop in dead code still gets its scope: its polls are its own.
    let trip = if ctx.block().is_terminated() {
        None
    } else {
        trip(ctx, l)
    };
    let mut count_slot = None;
    if let Some(t) = trip {
        if crate::verify_facts::enabled() {
            crate::verify_facts::consumed(
                crate::verify_facts::Fact::LoopTrip,
                || serde_json::json!({ "local": t.counter, "lo": t.lo, "hi": t.hi }),
            );
            let slot = ctx.func.alloca_entry(I64);
            ctx.block().store(I64, "0", &slot);
            count_slot = Some(slot);
        }
    }
    let start = ctx.func.num_blocks();
    ctx.loop_polls.0.push(Scope {
        kind: l.kind(),
        start,
        trip,
        polls: Vec::new(),
        inner: 0,
        count_slot,
    });
    true
}

/// The innermost loop has a trip proof and its polls can be taken back: the
/// poll about to be emitted gets its own block, which is returned (and
/// entered).
pub(crate) fn poll_block(ctx: &mut FnCtx<'_>) -> Option<usize> {
    let scope = ctx.loop_polls.0.last()?;
    if scope.trip.is_none() || crate::verify_facts::enabled() {
        return None;
    }
    let check = ctx.new_block("gcpoll.check");
    let label = ctx.block_label(check);
    ctx.block().br(&label);
    ctx.current_block = check;
    Some(check)
}

/// The poll entered through `check` is emitted; the code continues in `done`.
pub(crate) fn poll_emitted(ctx: &mut FnCtx<'_>, check: usize, done: usize) {
    let end = ctx.func.num_blocks();
    if let Some(scope) = ctx.loop_polls.0.last_mut() {
        scope.polls.push(Poll { check, done, end });
    }
}

/// Check mode, at the end of the innermost loop's body: one more iteration
/// than the trip bound is a false proof.
pub(crate) fn witness(ctx: &mut FnCtx<'_>) {
    if !crate::verify_facts::enabled() || ctx.block().is_terminated() {
        return;
    }
    let Some(scope) = ctx.loop_polls.0.last() else {
        return;
    };
    let (Some(t), Some(slot)) = (scope.trip, scope.count_slot.clone()) else {
        return;
    };
    crate::verify_facts::covered(
        crate::verify_facts::Fact::LoopTrip,
        || serde_json::json!({ "local": t.counter, "lo": t.lo, "hi": t.hi }),
    );
    let blk = ctx.block();
    let n = blk.load(I64, &slot);
    let n1 = blk.add(I64, &n, "1");
    blk.store(I64, &n1, &slot);
    let inside = blk.icmp_ule(I64, &n1, &t.bound.to_string());
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("loop_trip.verify_miss");
    let next = ctx.new_block("loop_trip.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&inside, &next_l, &miss_l);
    ctx.current_block = miss;
    let seen = ctx.block().sitofp(I64, &n1, DOUBLE);
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!(
            "loop trip of local {} in [{}, {}]: at most {} iterations",
            t.counter, t.lo, t.hi, t.bound
        ),
        crate::verify_facts::Fact::LoopTrip,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &seen)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// Close the loop [`begin`] opened (after all of it is lowered): take its
/// polls back when it is bounded and call-free.
pub(crate) fn end(ctx: &mut FnCtx<'_>, opened: bool) {
    if !opened {
        return;
    }
    let Some(scope) = ctx.loop_polls.0.pop() else {
        return;
    };
    let Some(t) = scope.trip else {
        return;
    };
    if crate::verify_facts::enabled() {
        return;
    }
    let stop = ctx.func.num_blocks();
    let own = |idx: usize| {
        scope
            .polls
            .iter()
            .any(|p| idx > p.check && idx < p.end && idx != p.done)
    };
    let effects = ctx.func.gc_effects();
    let call = (scope.start..stop)
        .filter(|&idx| !own(idx))
        .find_map(|idx| collecting_call(&ctx.func.blocks()[idx], &effects));
    let weight = t.bound.saturating_mul(scope.inner.saturating_add(1));
    let verdict = match &call {
        Some(callee) => format!("kept:call:{callee}"),
        None if weight > BUDGET => "kept:budget".to_string(),
        None if scope.polls.is_empty() => "none".to_string(),
        None => "elided".to_string(),
    };
    if diag_enabled() {
        eprintln!(
            "[perry loop poll] fn={} loop={} counter={} range=[{},{}] trip<={} nest<={} polls={} verdict={}",
            ctx.func.name,
            scope.kind,
            t.counter,
            t.lo,
            t.hi,
            t.bound,
            weight,
            scope.polls.len(),
            verdict
        );
    }
    if call.is_some() || weight > BUDGET {
        return;
    }
    // Poll-free from here on: the enclosing loop counts this one's back edges.
    if let Some(parent) = ctx.loop_polls.0.last_mut() {
        parent.inner = parent.inner.saturating_add(weight);
    }
    if scope.polls.is_empty() {
        return;
    }
    crate::facts_consumed::used();
    for p in &scope.polls {
        let done = ctx.block_label(p.done);
        let blocks = ctx.func.blocks_mut();
        let check = blocks[p.check].insts_mut();
        check.clear();
        check.push(LlInst::Br {
            label: done,
            meta: None,
        });
        for idx in p.check + 1..p.end {
            if idx != p.done {
                let insts = blocks[idx].insts_mut();
                insts.clear();
                insts.push(LlInst::Unreachable);
            }
        }
    }
}

/// The first call in `block` that may collect or run user code (module
/// docs, 2), by its callee.
fn collecting_call(block: &crate::block::LlBlock, effects: &GcEffects) -> Option<String> {
    let insts = block.insts();
    if matches!(insts.last(), Some(LlInst::Unreachable)) {
        return None;
    }
    // The shadow slot stores are bounds-checked TLS stores that cannot
    // allocate or collect (`gc/roots/shadow_stack.rs`; the call-free clone
    // scans exempt the same, `LlInst::is_gc_unsafe_call`), and a body whose
    // roots lower to stack maps drops them (`gc_call_effects::effects_of`).
    let leaf = |callee: &str| {
        callee.starts_with("llvm.")
            || matches!(callee, "js_shadow_slot_set" | "js_shadow_slot_bind")
            || effects.classify_direct_callee(callee) == GcCallEffect::CannotCollect
    };
    insts.iter().find_map(|inst| match inst {
        LlInst::Call { callee, .. } => (!leaf(callee)).then(|| callee.clone()),
        LlInst::CallIndirect { .. } => Some("<indirect>".to_string()),
        LlInst::Raw(line) => raw_collecting_call(line, &leaf),
        _ => None,
    })
}

/// [`collecting_call`] for a pre-rendered line: any `call` or `invoke` in it
/// of anything but inline asm or a leaf.
fn raw_collecting_call(line: &str, leaf: &dyn Fn(&str) -> bool) -> Option<String> {
    let mut found = None;
    for op in ["call ", "invoke "] {
        for (at, _) in line.match_indices(op) {
            let rest = &line[at + op.len()..];
            // The callee is the first global or the inline asm of this call.
            let name_at = rest.find('@');
            let asm_at = rest.find("asm ");
            let callee = match (name_at, asm_at) {
                (Some(n), Some(a)) if a < n => continue,
                (None, Some(_)) => continue,
                (Some(n), _) => rest[n + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
                    .collect::<String>(),
                (None, None) => "<indirect>".to_string(),
            };
            if callee == "<indirect>" || !leaf(&callee) {
                found.get_or_insert(callee);
            }
        }
    }
    found
}

/// The direction a write moves the counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

/// A write of the counter that moves it by a nonzero integer.
#[derive(Clone, Copy)]
enum Step<'a> {
    /// By a literal: the direction is known.
    Known(Dir),
    /// By `test ? a : b` with literals of both signs: one direction for the
    /// whole loop when `test` is the same in every iteration.
    Either(&'a Expr),
}

fn literal_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n) => Some(*n),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 9.0e15 => Some(*n as i64),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => literal_int(operand).and_then(i64::checked_neg),
        Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        } => literal_int(operand),
        _ => None,
    }
}

/// The step `c + s` takes, `negated` for `c - s`.
fn step_of<'a>(s: &'a Expr, negated: bool) -> Option<Step<'a>> {
    let dir = |n: i64| {
        let up = (n > 0) != negated;
        (n != 0).then_some(if up { Dir::Up } else { Dir::Down })
    };
    if let Some(n) = literal_int(s) {
        return dir(n).map(Step::Known);
    }
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = s
    else {
        return None;
    };
    let a = dir(literal_int(then_expr)?)?;
    let b = dir(literal_int(else_expr)?)?;
    Some(if a == b {
        Step::Known(a)
    } else {
        Step::Either(condition)
    })
}

/// `e` as a write of `id` that moves it: `id++`, `id--`, `id = id + s`,
/// `id = s + id`, `id = id - s`.
fn step_write<'a>(e: &'a Expr, id: u32) -> Option<Step<'a>> {
    match e {
        Expr::Update { id: i, op, .. } if *i == id => Some(Step::Known(match op {
            UpdateOp::Increment => Dir::Up,
            UpdateOp::Decrement => Dir::Down,
        })),
        Expr::LocalSet(i, value) if *i == id => {
            let Expr::Binary { op, left, right } = value.as_ref() else {
                return None;
            };
            let is_c = |x: &Expr| matches!(x, Expr::LocalGet(l) if *l == id);
            match op {
                BinaryOp::Add if is_c(left) => step_of(right, false),
                BinaryOp::Add if is_c(right) => step_of(left, false),
                BinaryOp::Sub if is_c(left) => step_of(right, true),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The local a moving write `e` writes.
fn step_target(e: &Expr) -> Option<u32> {
    let id = match e {
        Expr::Update { id, .. } | Expr::LocalSet(id, _) => *id,
        _ => return None,
    };
    step_write(e, id).map(|_| id)
}

/// Collect the steps of every write of `id` in `e`; `false` when one of
/// them is no moving write (or sits in a closure).
fn writes_in_expr<'a>(e: &'a Expr, id: u32, out: &mut Vec<Step<'a>>) -> bool {
    match e {
        Expr::Update { id: i, .. } | Expr::LocalSet(i, _) if *i == id => {
            let Some(step) = step_write(e, id) else {
                return false;
            };
            out.push(step);
        }
        Expr::Closure { .. } => return !expr_mutates_local(e, id),
        _ => {}
    }
    let mut ok = true;
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        ok = ok && writes_in_expr(child, id, out);
    });
    ok
}

fn writes_in_stmts<'a>(stmts: &'a [Stmt], id: u32, out: &mut Vec<Step<'a>>) -> bool {
    stmts.iter().all(|s| writes_in_stmt(s, id, out))
}

fn writes_in_stmt<'a>(s: &'a Stmt, id: u32, out: &mut Vec<Step<'a>>) -> bool {
    match s {
        // A second binding of the counter is a write this proof does not read.
        Stmt::Let { id: i, .. } if *i == id => false,
        Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| writes_in_expr(e, id, out)),
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => writes_in_expr(e, id, out),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => true,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            writes_in_expr(condition, id, out)
                && writes_in_stmts(then_branch, id, out)
                && else_branch
                    .as_ref()
                    .is_none_or(|b| writes_in_stmts(b, id, out))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            writes_in_expr(condition, id, out) && writes_in_stmts(body, id, out)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_none_or(|i| writes_in_stmt(i, id, out))
                && condition
                    .as_ref()
                    .is_none_or(|e| writes_in_expr(e, id, out))
                && update.as_ref().is_none_or(|e| writes_in_expr(e, id, out))
                && writes_in_stmts(body, id, out)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            writes_in_stmts(body, id, out)
                && catch
                    .as_ref()
                    .is_none_or(|c| writes_in_stmts(&c.body, id, out))
                && finally.as_ref().is_none_or(|f| writes_in_stmts(f, id, out))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            writes_in_expr(discriminant, id, out)
                && cases.iter().all(|c| {
                    c.test.as_ref().is_none_or(|t| writes_in_expr(t, id, out))
                        && writes_in_stmts(&c.body, id, out)
                })
        }
        Stmt::Labeled { body, .. } => writes_in_stmt(body, id, out),
    }
}

/// Whether `stmts` may reach the loop's latch without running to their end:
/// a `continue` of this loop (`plain`: not inside a nested loop), or any
/// labeled jump.
fn jumps_to_latch(stmts: &[Stmt], plain: bool) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::Continue => plain,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            jumps_to_latch(then_branch, plain)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| jumps_to_latch(b, plain))
        }
        Stmt::Labeled { body, .. } => jumps_to_latch(std::slice::from_ref(body.as_ref()), plain),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            jumps_to_latch(body, false)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| jumps_to_latch(&c.body, plain)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            jumps_to_latch(body, plain)
                || catch
                    .as_ref()
                    .is_some_and(|c| jumps_to_latch(&c.body, plain))
                || finally.as_ref().is_some_and(|f| jumps_to_latch(f, plain))
        }
        _ => false,
    })
}

/// The counters every iteration of `l` moves at least once: the target of a
/// `for` update; for `while` and `do-while`, of a moving write that is the
/// body's first statement, its last one when no jump skips it, or an operand
/// of the test's comparison.
fn progress_counters(l: Loop<'_>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut add = |id: Option<u32>| {
        if let Some(id) = id {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    };
    let stmt_target = |s: Option<&Stmt>| match s {
        Some(Stmt::Expr(e)) => step_target(e),
        _ => None,
    };
    match l {
        Loop::For { update, .. } => add(update.and_then(step_target)),
        Loop::While { condition, body } | Loop::DoWhile { body, condition } => {
            add(stmt_target(body.first()));
            if !jumps_to_latch(body, true) {
                add(stmt_target(body.last()));
            }
            if let Expr::Compare { left, right, .. } = condition {
                add(step_target(left));
                add(step_target(right));
            }
        }
    }
    out
}

/// The truth of `e` is the same in every iteration of `l`: `e` reads only
/// literals and locals nothing in `l` writes, and an operator that coerces
/// its operands (a comparison, arithmetic) runs no user code
/// (`expr_is_inert_primitive`). ToBoolean of a local, `!` and `&&` / `||`
/// run none whatever the local holds.
fn stable_truth(ctx: &FnCtx<'_>, l: Loop<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        Expr::LocalGet(id) => unwritten(ctx, l, *id),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => stable_truth(ctx, l, operand),
        Expr::Logical { left, right, .. } => {
            stable_truth(ctx, l, left) && stable_truth(ctx, l, right)
        }
        Expr::Compare { .. } | Expr::Binary { .. } | Expr::Unary { .. } => {
            crate::rooting::expr_is_inert_primitive(ctx, e) && reads_unwritten(ctx, l, e)
        }
        _ => false,
    }
}

/// `e` is literals and locals nothing in `l` writes, under operators.
fn reads_unwritten(ctx: &FnCtx<'_>, l: Loop<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        Expr::LocalGet(id) => unwritten(ctx, l, *id),
        Expr::Compare { left, right, .. }
        | Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            reads_unwritten(ctx, l, left) && reads_unwritten(ctx, l, right)
        }
        Expr::Unary { operand, .. } => reads_unwritten(ctx, l, operand),
        _ => false,
    }
}

/// Nothing in `l` (and no closure) writes local `id`.
fn unwritten(ctx: &FnCtx<'_>, l: Loop<'_>, id: u32) -> bool {
    !closure_writable(ctx, id)
        && !stmts_mutate_local(l.body(), id)
        && !l.controls().iter().any(|c| expr_mutates_local(c, id))
}

/// A value every evaluation of `e` stays at or below (`upper`) or at or
/// above (`!upper`), whatever the program state: a literal, or a
/// `Math.min` / `Math.max` with a literal on the side that bounds it.
fn literal_bound(e: &Expr, upper: bool) -> Option<i64> {
    if let Some(n) = literal_int(e) {
        return Some(n);
    }
    let pick = |a: i64, b: i64| if upper { a.min(b) } else { a.max(b) };
    match (e, upper) {
        // `min(..) <= each operand`, `max(..) >= each operand`: any bounded
        // operand bounds the whole, the tightest one best.
        (Expr::MathMin(v), true) | (Expr::MathMax(v), false) => v
            .iter()
            .filter_map(|x| literal_bound(x, upper))
            .reduce(pick),
        // The other side needs every operand bounded, and takes the loosest.
        (Expr::MathMin(v), false) | (Expr::MathMax(v), true) if !v.is_empty() => v
            .iter()
            .map(|x| literal_bound(x, upper))
            .collect::<Option<Vec<i64>>>()?
            .into_iter()
            .reduce(|a, b| if upper { a.max(b) } else { a.min(b) }),
        _ => None,
    }
}

/// The window `[lo, hi]` the test `e` admits local `id` to: the bounds its
/// `&&` conjuncts put on `id` by a comparison with a literal bound
/// (`literal_bound`). An unbounded side is `i64::MIN` / `i64::MAX`.
fn guard_window(e: &Expr, id: u32) -> (i64, i64) {
    let is_c = |x: &Expr| matches!(x, Expr::LocalGet(l) if *l == id);
    match e {
        Expr::Logical {
            op: perry_hir::LogicalOp::And,
            left,
            right,
        } => {
            let (a, b) = (guard_window(left, id), guard_window(right, id));
            (a.0.max(b.0), a.1.min(b.1))
        }
        Expr::Compare { op, left, right } => {
            use perry_hir::CompareOp::{Ge, Gt, Le, Lt};
            // `c op bound`, or `bound op c` read from the other side.
            let (op, bound) = if is_c(left) {
                (*op, right)
            } else if is_c(right) {
                let mirrored = match op {
                    Lt => Gt,
                    Le => Ge,
                    Gt => Lt,
                    Ge => Le,
                    other => *other,
                };
                (mirrored, left)
            } else {
                return (i64::MIN, i64::MAX);
            };
            match op {
                Lt => literal_bound(bound, true)
                    .map_or((i64::MIN, i64::MAX), |k| (i64::MIN, k.saturating_sub(1))),
                Le => literal_bound(bound, true).map_or((i64::MIN, i64::MAX), |k| (i64::MIN, k)),
                Gt => literal_bound(bound, false)
                    .map_or((i64::MIN, i64::MAX), |k| (k.saturating_add(1), i64::MAX)),
                Ge => literal_bound(bound, false).map_or((i64::MIN, i64::MAX), |k| (k, i64::MAX)),
                _ => (i64::MIN, i64::MAX),
            }
        }
        _ => (i64::MIN, i64::MAX),
    }
}

/// The most iterations per entry of a loop whose counter `id` holds a
/// different integer of `[lo, hi]` at the start of every iteration (module
/// docs, 1).
fn trip_bound(l: Loop<'_>, id: u32, lo: i64, hi: i64) -> Option<u64> {
    let (test, unguarded) = match l {
        Loop::For { condition, .. } => (condition, 0),
        Loop::While { condition, .. } => (Some(condition), 0),
        // The first iteration runs before any test.
        Loop::DoWhile { condition, .. } => (Some(condition), 1),
    };
    // A test that itself writes the counter compares values of different
    // moments: it bounds nothing here.
    let (wlo, whi) = test
        .filter(|t| !expr_mutates_local(t, id))
        .map_or((i64::MIN, i64::MAX), |t| guard_window(t, id));
    let (a, b) = (lo.max(wlo), hi.min(whi));
    let admitted = if a > b {
        0
    } else {
        u64::try_from(b.checked_sub(a)?.checked_add(1)?).ok()?
    };
    // At least one, so that a nest's weight is never zero.
    Some(admitted.saturating_add(unguarded).max(1))
}

fn closure_writable(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.boxed_vars.contains(&id)
        || ctx.prealloc_boxes.contains(&id)
        || ctx.closure_captures.contains_key(&id)
        || ctx.module_globals.contains_key(&id)
}

/// The trip proof of `l` (module docs, 1), if it has one.
pub(crate) fn trip(ctx: &FnCtx<'_>, l: Loop<'_>) -> Option<Trip> {
    if ctx.wpfacts_off {
        return None;
    }
    progress_counters(l)
        .into_iter()
        .find_map(|id| prove(ctx, l, id))
}

fn prove(ctx: &FnCtx<'_>, l: Loop<'_>, id: u32) -> Option<Trip> {
    if closure_writable(ctx, id) {
        return None;
    }
    let range = crate::expr::facts_local_range(ctx, id)?;
    let (lo, hi) = (range.min, range.max);
    if lo > hi {
        return None;
    }
    // The sabotaged proof is the interval alone.
    let bound = if sabotaged() {
        u64::try_from(hi.checked_sub(lo)?.checked_add(1)?).ok()?
    } else {
        trip_bound(l, id, lo, hi)?
    };
    let trip = Trip {
        counter: id,
        lo,
        hi,
        bound,
    };
    if sabotaged() {
        return Some(trip);
    }
    let mut steps = Vec::new();
    let controls = l.controls();
    if !writes_in_stmts(l.body(), id, &mut steps)
        || !controls.iter().all(|c| writes_in_expr(c, id, &mut steps))
    {
        return None;
    }
    let one_way = match steps.as_slice() {
        [] => false,
        // One write: its step is the same nonzero integer in every iteration.
        [Step::Either(test)] => stable_truth(ctx, l, test),
        all => {
            let dir = |s: &Step<'_>| match s {
                Step::Known(d) => Some(*d),
                Step::Either(_) => None,
            };
            let first = dir(&all[0]);
            first.is_some() && all.iter().all(|s| dir(s) == first)
        }
    };
    one_way.then_some(trip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Expr {
        Expr::Integer(n)
    }

    fn add_assign(id: u32, step: Expr) -> Expr {
        Expr::LocalSet(
            id,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(id)),
                right: Box::new(step),
            }),
        )
    }

    fn inc(id: u32) -> Expr {
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            prefix: false,
        }
    }

    fn dirs(steps: &[Step<'_>]) -> Vec<Option<Dir>> {
        steps
            .iter()
            .map(|s| match s {
                Step::Known(d) => Some(*d),
                Step::Either(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_literal_step_has_a_direction_and_zero_has_none() {
        assert!(matches!(step_write(&inc(1), 1), Some(Step::Known(Dir::Up))));
        assert!(matches!(
            step_write(&add_assign(1, int(7)), 1),
            Some(Step::Known(Dir::Up))
        ));
        assert!(matches!(
            step_write(&add_assign(1, int(-2)), 1),
            Some(Step::Known(Dir::Down))
        ));
        assert!(step_write(&add_assign(1, int(0)), 1).is_none());
        // A step that is another local is no proof.
        assert!(step_write(&add_assign(1, Expr::LocalGet(2)), 1).is_none());
        // A plain store is no step.
        assert!(step_write(&Expr::LocalSet(1, Box::new(int(0))), 1).is_none());
        // `c = c - 3` goes down, `c = 3 - c` is no step.
        let sub = |l: Expr, r: Expr| {
            Expr::LocalSet(
                1,
                Box::new(Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(l),
                    right: Box::new(r),
                }),
            )
        };
        assert!(matches!(
            step_write(&sub(Expr::LocalGet(1), int(3)), 1),
            Some(Step::Known(Dir::Down))
        ));
        assert!(step_write(&sub(int(3), Expr::LocalGet(1)), 1).is_none());
    }

    #[test]
    fn a_selected_step_of_both_signs_is_one_direction_only_with_its_test() {
        let pick = |a: i64, b: i64| Expr::Conditional {
            condition: Box::new(Expr::LocalGet(9)),
            then_expr: Box::new(int(a)),
            else_expr: Box::new(int(b)),
        };
        assert!(matches!(
            step_write(&add_assign(1, pick(1, -1)), 1),
            Some(Step::Either(_))
        ));
        assert!(matches!(
            step_write(&add_assign(1, pick(1, 16)), 1),
            Some(Step::Known(Dir::Up))
        ));
        assert!(step_write(&add_assign(1, pick(1, 0)), 1).is_none());
    }

    #[test]
    fn every_write_of_the_counter_is_collected_and_a_reset_refuses() {
        // if (x) { c += 7; continue; }  ...  (update: c++)
        let body = vec![Stmt::If {
            condition: Expr::LocalGet(5),
            then_branch: vec![Stmt::Expr(add_assign(1, int(7))), Stmt::Continue],
            else_branch: None,
        }];
        let mut steps = Vec::new();
        assert!(writes_in_stmts(&body, 1, &mut steps));
        let update = inc(1);
        assert!(writes_in_expr(&update, 1, &mut steps));
        assert_eq!(dirs(&steps), vec![Some(Dir::Up), Some(Dir::Up)]);

        let reset = vec![Stmt::Expr(Expr::LocalSet(1, Box::new(int(0))))];
        assert!(!writes_in_stmts(&reset, 1, &mut Vec::new()));
        // A write of another local is not this counter's.
        assert!(writes_in_stmts(&reset, 2, &mut Vec::new()));
    }

    #[test]
    fn a_while_counter_moves_first_or_last_and_a_continue_skips_the_last() {
        let cond = Expr::Bool(true);
        let first = vec![Stmt::Expr(inc(1)), Stmt::Expr(Expr::LocalGet(3))];
        assert_eq!(
            progress_counters(Loop::While {
                condition: &cond,
                body: &first
            }),
            vec![1]
        );
        let last = vec![Stmt::Expr(Expr::LocalGet(3)), Stmt::Expr(inc(1))];
        assert_eq!(
            progress_counters(Loop::While {
                condition: &cond,
                body: &last
            }),
            vec![1]
        );
        let skipped = vec![
            Stmt::If {
                condition: Expr::LocalGet(3),
                then_branch: vec![Stmt::Continue],
                else_branch: None,
            },
            Stmt::Expr(inc(1)),
        ];
        assert!(progress_counters(Loop::While {
            condition: &cond,
            body: &skipped
        })
        .is_empty());
        // A `continue` of a nested loop does not skip it.
        let nested = vec![
            Stmt::While {
                condition: Expr::LocalGet(3),
                body: vec![Stmt::Continue],
            },
            Stmt::Expr(inc(1)),
        ];
        assert_eq!(
            progress_counters(Loop::While {
                condition: &cond,
                body: &nested
            }),
            vec![1]
        );
        // A `for` reads its update only.
        assert!(progress_counters(Loop::For {
            condition: None,
            update: None,
            body: &first
        })
        .is_empty());
    }

    fn cmp(op: perry_hir::CompareOp, left: Expr, right: Expr) -> Expr {
        Expr::Compare {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn and(left: Expr, right: Expr) -> Expr {
        Expr::Logical {
            op: perry_hir::LogicalOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn the_test_window_is_the_literal_bounds_of_its_conjuncts() {
        use perry_hir::CompareOp::{Ge, Gt, Le, Lt, Ne};
        let c = || Expr::LocalGet(1);
        assert_eq!(guard_window(&cmp(Lt, c(), int(128)), 1), (i64::MIN, 127));
        assert_eq!(guard_window(&cmp(Le, c(), int(7)), 1), (i64::MIN, 7));
        assert_eq!(guard_window(&cmp(Gt, c(), int(0)), 1), (1, i64::MAX));
        // `8 > c` is `c < 8`.
        assert_eq!(guard_window(&cmp(Gt, int(8), c()), 1), (i64::MIN, 7));
        assert_eq!(
            guard_window(&and(cmp(Ge, c(), int(0)), cmp(Lt, c(), int(8))), 1),
            (0, 7)
        );
        // `c <= Math.min(7, x)` is at most 7; `c <= Math.max(7, x)` is unbounded.
        let min = Expr::MathMin(vec![int(7), Expr::LocalGet(2)]);
        let max = Expr::MathMax(vec![int(7), Expr::LocalGet(2)]);
        assert_eq!(guard_window(&cmp(Le, c(), min), 1), (i64::MIN, 7));
        assert_eq!(guard_window(&cmp(Le, c(), max), 1), (i64::MIN, i64::MAX));
        // A bound that is a local, another local's test, `!==` and `||` bound nothing.
        let open = (i64::MIN, i64::MAX);
        assert_eq!(guard_window(&cmp(Lt, c(), Expr::LocalGet(2)), 1), open);
        assert_eq!(guard_window(&cmp(Lt, Expr::LocalGet(2), int(8)), 1), open);
        assert_eq!(guard_window(&cmp(Ne, c(), int(8)), 1), open);
        let or = Expr::Logical {
            op: perry_hir::LogicalOp::Or,
            left: Box::new(cmp(Lt, c(), int(8))),
            right: Box::new(cmp(Lt, c(), int(4))),
        };
        assert_eq!(guard_window(&or, 1), open);
    }

    #[test]
    fn the_trip_bound_is_the_interval_inside_the_window() {
        use perry_hir::CompareOp::{Ge, Lt};
        let body: Vec<Stmt> = Vec::new();
        let test = cmp(Lt, Expr::LocalGet(1), int(128));
        let for_ = |condition| Loop::For {
            condition,
            update: None,
            body: &body,
        };
        // [0, 65535] under `c < 128`: 128 iterations.
        assert_eq!(trip_bound(for_(Some(&test)), 1, 0, 65535), Some(128));
        // No test: the interval.
        assert_eq!(trip_bound(for_(None), 1, 0, 65535), Some(65536));
        // The interval is inside the window.
        assert_eq!(trip_bound(for_(Some(&test)), 1, 3, 18), Some(16));
        // An empty meet still weighs one.
        assert_eq!(trip_bound(for_(Some(&test)), 1, 200, 300), Some(1));
        let both = and(
            cmp(Ge, Expr::LocalGet(1), int(0)),
            cmp(Lt, Expr::LocalGet(1), int(8)),
        );
        assert_eq!(trip_bound(for_(Some(&both)), 1, -1, 127), Some(8));
        // A test that moves the counter bounds nothing.
        let moving = and(
            cmp(Lt, Expr::LocalGet(1), int(8)),
            cmp(Lt, inc(1), int(100)),
        );
        assert_eq!(trip_bound(for_(Some(&moving)), 1, 0, 255), Some(256));
        // A `do-while` runs once before its test.
        assert_eq!(
            trip_bound(
                Loop::DoWhile {
                    body: &body,
                    condition: &both
                },
                1,
                -1,
                127
            ),
            Some(9)
        );
    }

    #[test]
    fn a_rendered_call_line_names_its_callee_and_asm_is_no_call() {
        let leaf = |name: &str| name == "js_leaf" || name.starts_with("llvm.");
        let call = |line: &str| raw_collecting_call(line, &leaf);
        assert_eq!(
            call("  %r1 = call double @js_get(double %r0)").as_deref(),
            Some("js_get")
        );
        assert_eq!(
            call(
                "  %r1 = invoke double @perry_fn_m__f$sig(double %r0) to label %a unwind label %b"
            )
            .as_deref(),
            Some("perry_fn_m__f$sig")
        );
        assert_eq!(call("  call void @js_leaf()"), None);
        assert_eq!(call("  %r2 = call double @llvm.fabs.f64(double %r1)"), None);
        assert_eq!(call("  call void asm sideeffect \"\", \"\"()"), None);
        assert_eq!(
            call("  %r3 = call double %fp(double %r1)").as_deref(),
            Some("<indirect>")
        );
        assert_eq!(call("  %r4 = load i32, ptr @PERRY_GC_POLL_ARMED"), None);
    }
}
