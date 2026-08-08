//! Layer 1: rooting by construction (`docs/src/internals/rfc-rooting-by-construction.md`).
//!
//! **Status: migration under way, one module at a time.** The ledger at the
//! bottom of this file names the modules that have finished; the campaign's
//! ordering lives on the Layer 1 tracking issue.
//!
//! This file has two halves and they answer different questions.
//!
//! The **first half** ([`RootingEmitter`], [`Raw`], [`Rooted`], [`Plain`]) is the
//! RFC's design as written, against a hypothetical emitter with interior
//! mutability. It exists to settle the one question the RFC could not answer on
//! paper — *does the borrow checker actually reject the bug shape?* The
//! `compile_fail` doctests below are the answer, and `cargo test` executes them,
//! so the claim cannot rot into prose the way the RFC's own example did (its
//! constructor was `E0499`, #7459).
//!
//! The **second half** is what runs. `FnCtx` has no interior mutability, so the
//! borrow formulation cannot be built on it; the combinators there get the same
//! guarantees by never handing out an unrooted register in the first place. The
//! gap between the two is stated exactly, and honestly, where the second half
//! begins.
//!
//! # The shape it has to reject
//!
//! Every bug in the #7341 family is one sentence: *a GC-managed pointer is held
//! in a register across a point where the collector can run.* #7453 is the most
//! recent — `js_url_coerce_string` returns a raw `StringHeader`, `base` lowers
//! (arbitrary user code), a second coercion allocates, and only then is the
//! first pointer used.
//!
//! Today `LlBlock::call` takes `&mut self` but returns an owned `String`, so the
//! borrow ends at the semicolon and nothing stops that register from being used
//! ten collection points later. The entire fix is to return a value that *keeps*
//! the borrow.
//!
//! # Three types and one rule
//!
//! [`Plain`] is anything the collector does not manage — an `i32`, a length, a
//! slot index. Freely cloneable, no borrow.
//!
//! [`Raw`] is a register holding a GC-managed value that is **not** rooted. It
//! borrows the emitter immutably, and is neither `Clone` nor `Copy`.
//!
//! [`Rooted`] is a slot the collector knows about. It survives collection
//! points, and cannot be read except through [`Rooted::get`], which hands back a
//! fresh [`Raw`] — so "re-read after every collection point", which
//! `expr/temp_root.rs` can only state in prose today, becomes the only thing
//! that type-checks.
//!
//! The rule needs no new machinery. Emitting something that can collect takes
//! `&mut`, which ends every outstanding [`Raw`] borrow:
//!
//! ```compile_fail,E0499
//! # use perry_codegen::rooting::{RootingEmitter, Raw};
//! # fn demo(e: &mut RootingEmitter) {
//! // #7453's shape: coerce, then lower `base` (which can collect), then use
//! // the first pointer.
//! let url_ptr = e.emit_collecting("js_url_coerce_string");
//! let base_ptr = e.emit_collecting("js_url_coerce_string");
//! // ERROR[E0499]: `url_ptr` still borrows `e`, which is mutably borrowed above.
//! e.emit_use(&url_ptr, &base_ptr);
//! # }
//! ```
//!
//! #7192 is the same rule read from the other end — the value is materialised,
//! a call that allocates is emitted, and only *then* is the root store taken.
//! Rooting an already-stale pointer is indistinguishable from rooting a live
//! one at runtime; here it is a borrow error, because `root` consumes a handle
//! whose borrow the intervening `&mut` emission already ended:
//!
//! ```compile_fail,E0499
//! # use perry_codegen::rooting::RootingEmitter;
//! # fn demo(e: &mut RootingEmitter) {
//! let obj = e.emit_collecting("js_object_alloc");
//! e.emit_collecting("js_closure_callN");   // allocates; may move `obj`
//! // ERROR[E0499]: the root store is BELOW the collection point.
//! let _root = obj.root();
//! # }
//! ```
//!
//! The correct code is also the shortest way out of that error — root it, then
//! re-read after the window:
//!
//! ```
//! # use perry_codegen::rooting::RootingEmitter;
//! # fn demo(e: &mut RootingEmitter) {
//! let url = e.emit_collecting("js_url_coerce_string").root();
//! let base = e.emit_collecting("js_url_coerce_string").root();
//! e.emit_use(&url.get(e), &base.get(e));
//! # }
//! ```
//!
//! # What it cannot catch
//!
//! Anything not expressed through this emitter: runtime-side Rust (layer 3), a
//! raw pointer cached in a side table, or a value the collector moves that never
//! passes through a `Raw`. The RFC's "What it cannot catch" section is the
//! authority; this half does not widen it.
//!
//! And note which half these doctests are about. **They prove the DESIGN, not
//! the shipped code.** What the migrated lowerings actually get is the
//! combinator form below, which is measurably weaker — the block comment where
//! it starts records each sabotage arm and its outcome, including the two that
//! compile silently.

/// A register holding something the collector does not manage — an `i32`, a
/// length, a slot index. No borrow, freely cloneable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plain(pub String);

/// A register holding a GC-managed value that is **not** rooted.
///
/// Borrows the emitter immutably, so it cannot outlive the next emission that
/// can collect. Deliberately neither `Clone` nor `Copy`: cloning one would let a
/// copy escape the borrow that makes it safe.
#[derive(Debug)]
pub struct Raw<'e> {
    reg: String,
    /// The emitter this register was produced by. Carrying the reference here
    /// rather than taking a fresh one in [`Raw::root`] is load-bearing: the RFC
    /// spells that method `root(self, e: &mut Emitter)`, which **cannot
    /// compile** — `self` already holds a borrow of the emitter, so asking for
    /// a second one is `E0499`. Storing the shared reborrow lets `root` consume
    /// the handle without re-borrowing.
    emitter: &'e RootingEmitter,
}

impl<'e> Raw<'e> {
    /// The SSA name. Safe to read *now* — the borrow proves no collection point
    /// has intervened since it was produced.
    pub fn reg(&self) -> &str {
        &self.reg
    }

    /// Consume this register into a root. The only way to obtain a [`Rooted`],
    /// which is what forces the root to be taken *before* the window rather
    /// than after — the ordering error in #7184, #7192 and #7453 alike.
    pub fn root(self) -> Rooted {
        Rooted {
            slot: self.emitter.emit_root_store(&self.reg),
        }
    }
}

/// A slot the collector knows about. Survives collection points.
#[derive(Debug, Clone)]
pub struct Rooted {
    slot: String,
}

impl Rooted {
    /// Re-read the slot, yielding a [`Raw`] valid until the next collecting
    /// emission. There is deliberately no way to keep the result across one:
    /// a cached read is the second half of the bug, and it does not type-check.
    pub fn get<'e>(&self, e: &'e RootingEmitter) -> Raw<'e> {
        Raw {
            reg: e.emit_root_load(&self.slot),
            emitter: e,
        }
    }

    /// The slot index, for diagnostics.
    pub fn slot(&self) -> &str {
        &self.slot
    }
}

/// Prototype emitter. Records emissions instead of writing IR — the point here
/// is the *signatures*, which is what the borrow checker reads.
#[derive(Debug, Default)]
pub struct RootingEmitter {
    ops: std::cell::RefCell<Vec<String>>,
    next: std::cell::Cell<u32>,
}

impl RootingEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    fn fresh(&self) -> String {
        let n = self.next.get();
        self.next.set(n + 1);
        format!("%r{n}")
    }

    /// Emit something that **cannot** collect. Takes `&self`, so outstanding
    /// [`Raw`] handles stay valid across it.
    pub fn emit_pure(&self, op: &str) -> Plain {
        let r = self.fresh();
        self.ops.borrow_mut().push(format!("{r} = pure {op}"));
        Plain(r)
    }

    /// Emit something that **can** collect. Takes `&mut self`, which ends every
    /// outstanding [`Raw`] borrow — that single signature is the whole rule.
    pub fn emit_collecting(&mut self, callee: &str) -> Raw<'_> {
        let r = self.fresh();
        self.ops.borrow_mut().push(format!("{r} = call {callee}"));
        Raw {
            reg: r,
            emitter: self,
        }
    }

    /// Consume two live registers. Takes `&self`: using values is not a
    /// collection point, so this must not invalidate anything.
    pub fn emit_use(&self, a: &Raw<'_>, b: &Raw<'_>) -> Plain {
        let r = self.fresh();
        self.ops
            .borrow_mut()
            .push(format!("{r} = use {} {}", a.reg(), b.reg()));
        Plain(r)
    }

    fn emit_root_store(&self, reg: &str) -> String {
        let s = self.fresh();
        self.ops
            .borrow_mut()
            .push(format!("{s} = root_store {reg}"));
        s
    }

    fn emit_root_load(&self, slot: &str) -> String {
        let r = self.fresh();
        self.ops
            .borrow_mut()
            .push(format!("{r} = root_load {slot}"));
        r
    }

    /// The emitted sequence, for tests.
    pub fn ops(&self) -> Vec<String> {
        self.ops.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rooted form emits root-store, the collecting call, then root-load
    /// — root BEFORE the window, re-read AFTER it. That ordering is the fix in
    /// every #7341 bug; getting it backwards roots an already-stale pointer.
    #[test]
    fn rooting_emits_store_before_the_window_and_load_after() {
        let mut e = RootingEmitter::new();
        let url = e.emit_collecting("js_url_coerce_string").root();
        let base = e.emit_collecting("js_url_coerce_string").root();
        e.emit_use(&url.get(&e), &base.get(&e));

        let ops = e.ops();
        let store = ops.iter().position(|o| o.contains("root_store")).unwrap();
        let second_call = ops
            .iter()
            .enumerate()
            .filter(|(_, o)| o.contains("call js_url_coerce_string"))
            .nth(1)
            .unwrap()
            .0;
        let load = ops.iter().position(|o| o.contains("root_load")).unwrap();
        assert!(store < second_call, "root store must precede the window");
        assert!(load > second_call, "re-read must follow the window");
    }

    /// A `Plain` is not GC-managed, so it may cross a collection point. If this
    /// stopped compiling the types would be too strict to migrate to.
    #[test]
    fn plain_values_survive_collection_points() {
        let mut e = RootingEmitter::new();
        let len = e.emit_pure("array_length");
        let _ = e.emit_collecting("js_array_grow");
        assert_eq!(len.0, "%r0");
    }
}

// ---------------------------------------------------------------------------
// Applying the design to the REAL emitter.
//
// `FnCtx` has no interior mutability -- `ctx.block()` needs `&mut` -- so the
// borrow-carrying `Raw` above cannot be built on it directly: `root(self)`
// would need a second borrow while the handle still holds the first (the same
// E0499 the RFC's own API hits, see `Raw`'s doc).
//
// The shape that DOES work against a `&mut`-only emitter is the combinator, and
// it is the same one the runtime settled on for layer 3 (`RuntimeHandle::
// across_*`): never hand out an unrooted handle at all.
//
// HOW MUCH WEAKER, MEASURED RATHER THAN ASSERTED.
//
// The first module migrated (`expr/url_main.rs`) was sabotaged four ways, each
// reintroducing a historic bug shape, and each result recorded:
//
//   arm                                                      compiles? caught by
//   -------------------------------------------------------- --------- ---------
//   #7192 in the BORROW form (the doctests above)             NO (E0499) rustc
//   hold the `call_with_roots` result across a lowering       yes        nothing
//   the verbatim pre-#7453 code, via bare `ctx.block()`       yes        nothing
//   reach back into `expr::temp_root`                         yes        ledger test
//   hold the operand guard so it can be released on one arm   yes        ledger test
//
// So state it plainly: **on the real emitter this API does not make the bug
// fail to compile.** It removes the bug from the path of least resistance --
// there is no expression in it that yields an unrooted register, and no guard
// for a caller to mis-release -- and the ledger test denies the escape hatch.
// A lowering that reaches past the API into `ctx.block()` is exactly as
// writable as it was before.
//
// The third row is the one worth reading twice. Reintroducing #7453 verbatim
// produced IR that `gc_root_dominance_check.py` reports as CLEAN in all three
// of its modes -- dominance 0, unrooted-allocas 0, stale-registers identical to
// the control. Its `--moving-only` filter discards the window because
// `js_url_coerce_string` is absent from `POLL_CAPABLE_RUNTIME`, even though
// #7453's own fix added it to `ALLOC_RE`. Dropping that filter surfaces 11
// stale uses at `js_url_new_with_base` in the sabotaged arm and 0 in the
// migrated one, so the shape IS expressible -- the gate just cannot see it.
// Filed separately; not fixed here, because widening a gate is its own change
// with its own corpora to measure.
//
// Which is the real argument for the migration rather than for the checker:
// for the raw-register shape there is currently no automated defence at all,
// and the API is the only thing that makes the correct form the easy one.
// ---------------------------------------------------------------------------

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::LlvmType;

/// A slot the collector knows about, holding a GC-managed pointer for the
/// duration of a lowering.
///
/// There is deliberately **no way to read one into a register**. #7461 shipped
/// a `read(&self, ctx) -> String` and it reintroduced the second half of the
/// bug the slot exists to prevent: a register loaded from a root is stale the
/// moment anything else collects (#7114, #7375), and a `String` remembers
/// nothing about when it was loaded. [`call_with_roots`] fuses the re-read to
/// the use instead, so "load early, use late" is not a sequence this API can
/// express.
#[derive(Debug)]
pub(crate) struct RootedSlot {
    idx: String,
}

impl RootedSlot {
    /// Release the slot.
    ///
    /// `temp_root_truncate` is a stack CUT, not a pop: releasing a slot drops
    /// every slot acquired after it. Release in reverse acquisition order, as
    /// the un-migrated callers already had to.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        crate::expr::temp_root::temp_root_truncate(ctx, &self.idx);
    }
}

/// One argument to [`call_rooted`], [`call_with_roots`] or
/// [`call_void_with_roots`].
///
/// The split is the whole point: a `Root` is re-read from its slot at the
/// instant the call is emitted, and a `Plain` is a register the caller is
/// asserting the collector does not manage — an `i32`, a length, a literal, or
/// a value another combinator has already re-read below the last collection
/// point.
pub(crate) enum Arg<'a> {
    /// Re-read this slot immediately before the call. The register never
    /// exists as a value the caller can hold.
    Root(&'a RootedSlot),
    /// A value the collector does not manage in this window.
    Plain(LlvmType, &'a str),
}

/// Materialise each argument in order, re-reading every rooted slot.
///
/// Order matters and is asserted by the IR-identity check: the re-reads are
/// emitted left to right, immediately before the call, which is exactly the
/// sequence the hand-written `temp_root_get_i64` callers emitted.
fn materialize<'a>(ctx: &mut FnCtx<'_>, args: &'a [Arg<'a>]) -> Vec<(LlvmType, String)> {
    args.iter()
        .map(|arg| match arg {
            Arg::Root(slot) => (
                crate::types::I64,
                crate::expr::temp_root::temp_root_get_i64(ctx, &slot.idx),
            ),
            Arg::Plain(ty, reg) => (*ty, (*reg).to_string()),
        })
        .collect()
}

fn borrow_args(args: &[(LlvmType, String)]) -> Vec<(LlvmType, &str)> {
    args.iter().map(|(ty, reg)| (*ty, reg.as_str())).collect()
}

/// Emit a call that can collect and root its result in one step.
///
/// The point is what this function does NOT return: an unrooted register. A
/// caller cannot hold the result across a later collection point because it
/// never has the result -- only a slot -- which is what makes the #7453 shape
/// unwritable here rather than merely reviewable.
pub(crate) fn call_rooted(
    ctx: &mut FnCtx<'_>,
    ret_ty: LlvmType,
    callee: &str,
    args: &[Arg<'_>],
) -> RootedSlot {
    let materialized = materialize(ctx, args);
    let reg = ctx
        .block()
        .call(ret_ty, callee, &borrow_args(&materialized));
    let idx = crate::expr::temp_root::temp_root_push_i64(ctx, &reg);
    RootedSlot { idx }
}

// A `root_i64(ctx, reg) -> RootedSlot` combinator -- "root a raw pointer some
// earlier emission produced" -- was written for this slice and then deleted
// unused. It is recorded here because it is the ONE addition that would reopen
// the window the API closes: taking a bare register and rooting it puts the
// ordering back in the author's hands, which is #7192 exactly. If a later slice
// genuinely needs it (a receiver unboxed from a NaN-boxed operand is the likely
// case), it should arrive with its caller and with a written argument for why
// `call_rooted` cannot serve -- not ahead of one.

/// Emit a call whose rooted arguments are re-read as part of the emission.
///
/// Returns the call's own result register. That register is raw, and holding it
/// across a later collection point is still writable — see the module-level
/// note on what this API does not catch.
pub(crate) fn call_with_roots(
    ctx: &mut FnCtx<'_>,
    ret_ty: LlvmType,
    callee: &str,
    args: &[Arg<'_>],
) -> String {
    let materialized = materialize(ctx, args);
    ctx.block()
        .call(ret_ty, callee, &borrow_args(&materialized))
}

/// Lower `exprs` with every already-evaluated operand rooted across the
/// evaluation of the ones that follow, run `body` over the re-read values, and
/// release the group **on every path out**.
///
/// The release is the half nobody gets wrong in the happy case and everybody
/// gets wrong in a branch. #7462 placed `temp_root_release` inside one arm of
/// an `if`, so `URLSearchParams.delete(name, value)` pushed two temp roots per
/// execution and truncated none — unbounded growth inside a loop, compiled
/// without a warning. Owning the guard here rather than handing it back makes
/// "released on one arm" not a program: the caller never holds the guard, and
/// `body`'s `?` returns through the same release as its `Ok`.
pub(crate) fn with_operands_rooted<'f, R>(
    ctx: &mut FnCtx<'f>,
    exprs: &[&Expr],
    body: impl FnOnce(&mut FnCtx<'f>, &[String]) -> Result<R>,
) -> Result<R> {
    with_operands_rooted_across(ctx, exprs, &[], |_| Ok(()), |ctx, vals, ()| body(ctx, vals))
}

/// [`with_operands_rooted`], but with a caller-controlled lowering step wedged
/// between the operand group and its re-read.
///
/// `across` lowers `across_exprs` in a representation this API cannot produce —
/// today that is `expr::arrays_finds`'s index lowering, which picks between the
/// `i32` fast path (`lower_expr_as_i32`) and a `double` + `fptosi` from the
/// expression's proven integer range. Feeding those indexes to
/// [`with_operands_rooted`] instead would force every `u8[i]` back onto the
/// NaN-boxed path, which is a codegen-quality regression rather than a rooting
/// fix.
///
/// **Why the plain form cannot serve.** Its re-read point is fixed at the end of
/// the operand list, so an operand lowered before caller-controlled work is
/// re-read *above* that work and is stale again by the time the call runs — the
/// exact half-measure #7114 is. Here the group is rooted before `across` runs
/// and re-read after it, so `body` sees post-collection values.
///
/// `across_exprs` is used for one thing: deciding whether the window collects at
/// all. It is not lowered here — `across` owns that — so passing the
/// expressions rather than a `bool` keeps "does this window collect?" answered
/// by `operand_protection` like every other site, instead of by the caller.
/// When neither the later operands nor `across_exprs` can collect, nothing is
/// pushed and the emitted IR is unchanged.
///
/// The release still happens on every path out, including `across`'s `?`.
pub(crate) fn with_operands_rooted_across<'f, T, R>(
    ctx: &mut FnCtx<'f>,
    exprs: &[&Expr],
    across_exprs: &[&Expr],
    across: impl FnOnce(&mut FnCtx<'f>) -> Result<T>,
    body: impl FnOnce(&mut FnCtx<'f>, &[String], T) -> Result<R>,
) -> Result<R> {
    use crate::expr::temp_root::{any_may_trigger_gc, root_operands_begin};

    let across_collects = any_may_trigger_gc(ctx, across_exprs.iter().copied());
    let mut group = root_operands_begin(exprs.len());
    let out = (|| {
        // Incremental, one operand at a time: each is rooted BEFORE the next is
        // lowered. Rooting a finished list afterwards is worse than doing
        // nothing — it publishes an already-dangling pointer into a slot the
        // collector scans (`root_operands_begin`'s doc, #6969).
        for (i, expr) in exprs.iter().enumerate() {
            let value = crate::expr::lower_expr(ctx, expr)?;
            let collects =
                across_collects || any_may_trigger_gc(ctx, exprs[i + 1..].iter().copied());
            group.push(ctx, expr, &value, collects);
        }
        let extra = across(ctx)?;
        let values = group.reread(ctx, exprs)?;
        body(ctx, &values, extra)
    })();
    // Released after `body`'s consuming call, which itself allocates -- and on
    // every error path too, including a bail from the operand lowering itself,
    // so a lowering that fails does not leave the group pushed.
    group.release(ctx);
    out
}

// ---------------------------------------------------------------------------
// The per-module migration ledger (RFC step 3).
//
// "Migrate one family at a time [...] `#[deny]` the escape hatch per-module as
// each module finishes, so migrated code cannot regress." Rust has no attribute
// that denies calling a `pub(crate)` function from one module, so the deny is
// spelled as a test over the module's own source, inlined at COMPILE time by
// `include_str!` -- no path, no working directory, no stale checkout.
//
// `expr::temp_root` IS the escape hatch. It is the raw, order-sensitive API
// (push / get / set / truncate, guards the caller must remember to release),
// and every bug in the #7341 family was an ordering mistake against it. A
// migrated module names `crate::rooting` and nothing else.
// ---------------------------------------------------------------------------

/// Modules that have completed the Layer 1 migration, with their source
/// inlined at compile time.
///
/// Adding a line here is how a migration slice finishes. Removing one is a
/// regression, not a cleanup.
/// A module is listed here only when it is migrated **end to end**. Slice 1's
/// `lower_array_method.rs` is one file and lands whole; `expr/url_main.rs` sat
/// half-migrated from #7461 to #7617, which is the reason the rule exists. When
/// a module genuinely cannot land in one PR, the boundary goes in this comment
/// with the slice that will finish it — an unlisted module is indistinguishable
/// from an unstarted one, and that is what let the half-migration hide.
///
/// No boundary is outstanding today.
///
/// **Listing a module that never used the escape hatch passes vacuously.** That
/// is true of every module migrated so far except `expr/url_main.rs`: they named
/// no `temp_root` symbol before the migration, so
/// `migrated_modules_do_not_reach_past_the_rooting_api` went green the instant
/// the line was added. The listing only means something if the slice ALSO ran
/// the sabotage arm — inject a real, compiling `temp_root_push_*` /
/// `temp_root_truncate` pair into the migrated module, confirm the ledger test
/// goes red and names the lines, then revert. Slices 1a and 1b both did, and
/// recorded it in their PRs; a slice that skips it is adding a line that asserts
/// nothing.
#[cfg(test)]
const MIGRATED_MODULES: &[(&str, &str)] = &[
    (
        "crates/perry-codegen/src/expr/url_main.rs",
        include_str!("expr/url_main.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_array_method.rs",
        include_str!("lower_array_method.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/arrays_finds.rs",
        include_str!("expr/arrays_finds.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_methods.rs",
        include_str!("expr/array_methods.rs"),
    ),
];

/// Lines in `src` that reach past [`crate::rooting`] into the raw rooting API.
#[cfg(test)]
fn escape_hatch_uses(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("temp_root") || code.contains("rooted_handle")
        })
        .map(|(i, line)| (i + 1, line.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod migration_ledger {
    use super::{escape_hatch_uses, MIGRATED_MODULES};

    /// An empty ledger passes vacuously, which is hazard 4 in CLAUDE.md applied
    /// to this test. Assert the subject exists before asserting it is clean.
    #[test]
    fn the_ledger_is_not_empty() {
        assert!(
            !MIGRATED_MODULES.is_empty(),
            "the Layer 1 ledger is empty; a clean verdict over nothing is not a check"
        );
    }

    #[test]
    fn migrated_modules_do_not_reach_past_the_rooting_api() {
        for (path, src) in MIGRATED_MODULES {
            let hits = escape_hatch_uses(src);
            assert!(
                hits.is_empty(),
                "{path} has completed the Layer 1 migration, so it must root only \
                 through crate::rooting. Reaching back into expr::temp_root \
                 restores the ordering hazard the migration removed:\n{}",
                hits.iter()
                    .map(|(n, l)| format!("  {path}:{n}: {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    /// Sabotage duty: a ledger that cannot report a violation is documentation.
    /// Plant each escape-hatch spelling and require the checker to name it.
    #[test]
    fn the_ledger_check_still_reports_a_planted_violation() {
        let planted = "\
fn lower(ctx: &mut FnCtx<'_>) {
    let p = ctx.block().call(I64, \"js_url_coerce_string\", &[]);
    let slot = super::temp_root::temp_root_push_i64(ctx, &p);
    let h = super::temp_root::rooted_handle_begin(ctx, &p, true);
}
";
        let hits = escape_hatch_uses(planted);
        assert_eq!(
            hits.len(),
            2,
            "planted escape-hatch uses must be reported, got {hits:?}"
        );
        assert!(hits[0].1.contains("temp_root_push_i64"));
        assert!(hits[1].1.contains("rooted_handle_begin"));
    }

    /// ...and must NOT report the migrated form, or the check would make the
    /// migration impossible to finish.
    #[test]
    fn the_ledger_check_clears_the_migrated_form() {
        let clean = "\
fn lower(ctx: &mut FnCtx<'_>) {
    let slot = crate::rooting::call_rooted(ctx, I64, \"js_url_coerce_string\", &[]);
    let obj = crate::rooting::call_with_roots(ctx, I64, \"js_url_new\", &[Arg::Root(&slot)]);
    slot.release(ctx);
}
";
        assert!(escape_hatch_uses(clean).is_empty());
    }
}
