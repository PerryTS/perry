//! Layer 1: rooting by construction (`docs/src/internals/rfc-rooting-by-construction.md`).
//!
//! **Status: migration under way, one module at a time.** The ledger in the
//! sibling `ledger.rs` names the modules that have finished; the campaign's
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

/// The raw, order-sensitive rooting API.
///
/// **PRIVATE, and that is the campaign's terminal condition** (#7615). It used
/// to be `crate::expr::temp_root`, reachable from anywhere in the crate, and
/// every bug in the #7341 family was an ordering mistake against it: a push
/// below the collection point (#7192), a truncate at the wrong slot, a release
/// on one arm of an `if` (#7462), a re-read taken above the window (#7114).
///
/// A private module inside `rooting` makes each of those unwritable outside
/// this file rather than merely uncounted — the ledger in `ledger.rs` can only
/// report what a module NAMES, and a module that cannot name it has nothing
/// to report. The accessors additionally carry an explicit
/// `pub(in crate::rooting)`, so re-opening the module in a moment of haste
/// does not silently widen them back.
///
/// Two items keep `pub(crate)` and are re-exported below, because they are not
/// accessors and make no ordering decision: [`TempRootPool`] is the
/// compile-time slot bookkeeping `FnCtx` owns, and `expr_is_inert_primitive`
/// is the "can evaluating this run user code?" predicate the loop back-edge
/// poll shares (`crate::loop_purity`).
mod temp_root;

pub(crate) use temp_root::{expr_is_inert_primitive, TempRootPool};

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{LlvmType, DOUBLE, I64};

/// How a rooted slot's contents are read back out.
///
/// A temp-root slot is representation-agnostic — `temp_root_push_double`
/// bitcasts to `i64` and pushes the same word `temp_root_push_i64` does — so
/// the *reader* decides whether the word is a raw heap pointer or a NaN-boxed
/// JS value. Before this was carried on the slot, that decision lived at each
/// call site as a choice between `temp_root_get_i64` and `temp_root_get_double`,
/// and reading a boxed slot as a pointer is a silent miscompile rather than a
/// type error. Recording it at the push makes the pair impossible to mismatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Repr {
    /// A raw heap pointer in an `i64` — what a `js_*` helper returning `I64`
    /// yields, and what `unbox_to_i64` derives.
    Ptr,
    /// A NaN-boxed JS value in a `double` — the ordinary operand currency.
    Boxed,
}

impl Repr {
    fn llvm_ty(self) -> LlvmType {
        match self {
            Repr::Ptr => I64,
            Repr::Boxed => DOUBLE,
        }
    }
}

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
#[derive(Debug, Clone)]
pub(crate) struct RootedSlot {
    idx: String,
    repr: Repr,
}

impl RootedSlot {
    /// Release the slot.
    ///
    /// `temp_root_truncate` is a stack CUT, not a pop: releasing a slot drops
    /// every slot acquired after it. Release in reverse acquisition order, as
    /// the un-migrated callers already had to.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        temp_root::temp_root_truncate(ctx, &self.idx);
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
#[derive(Clone, Copy)]
pub(crate) enum Arg<'a> {
    /// Re-read this slot immediately before the call, in the representation the
    /// slot was pushed with. The register never exists as a value the caller
    /// can hold.
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
            Arg::Root(slot) => (slot.repr.llvm_ty(), read_slot(ctx, slot)),
            Arg::Plain(ty, reg) => (*ty, (*reg).to_string()),
        })
        .collect()
}

/// The one place a rooted slot becomes a register, and it is private: every
/// public path out of it fuses the read to the emission that consumes it.
fn read_slot(ctx: &mut FnCtx<'_>, slot: &RootedSlot) -> String {
    match slot.repr {
        Repr::Ptr => temp_root::temp_root_get_i64(ctx, &slot.idx),
        Repr::Boxed => temp_root::temp_root_get_double(ctx, &slot.idx),
    }
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
    let idx = temp_root::temp_root_push_i64(ctx, &reg);
    RootedSlot {
        idx,
        repr: Repr::Ptr,
    }
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

/// [`call_with_roots`] for a `void` helper — a mutator such as
/// `js_object_set_field_by_name` or `js_map_set`, which is most of the
/// accumulator surface. Returning nothing is the point: there is no register
/// for a caller to hold, so this form cannot reopen the window at all.
pub(crate) fn call_void_with_roots(ctx: &mut FnCtx<'_>, callee: &str, args: &[Arg<'_>]) {
    let materialized = materialize(ctx, args);
    ctx.block().call_void(callee, &borrow_args(&materialized));
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
    let across_collects = temp_root::any_may_trigger_gc(ctx, across_exprs.iter().copied());
    with_operands_rooted_window(ctx, exprs, across_collects, across, body)
}

/// [`with_operands_rooted_across`] for a step that is an **emitted runtime
/// call** rather than a lowered expression.
///
/// The two forms differ only in who answers "does this window collect?", and
/// for a call there is nothing for `any_may_trigger_gc` to read — the step is
/// not an `Expr`. `re.test(s)` is the shape: the arm unconditionally emits
/// `js_jsvalue_to_string_coerce`, which allocates and, on an object argument,
/// dispatches a user `toString`. Deriving the window from the `string` operand
/// answers *false* for a plain local and drops the root, which is #7154 at that
/// exact site (`js_regexp_test` dereferencing a from-space `RegExpHeader`).
///
/// So this takes the answer rather than deriving it, and the precedent is
/// deliberate: `temp_root::guard_store_operand_across` already had to, for the
/// same reason and since #7201. What stays centralised is the part that can
/// drift — `operand_protection` still decides *how* each operand is protected
/// (root / re-derive / reuse). Only the window's extent is stated here, and it
/// is stated as "yes", the conservative answer.
///
/// Use it only when the emitted step can re-enter user code or enumerate an
/// arbitrary object's own properties. For a helper that merely allocates, the
/// project's position (#7198) is that it cannot *initiate* a moving collection,
/// so a root there would be pure cost.
pub(crate) fn with_operands_rooted_across_call<'f, T, R>(
    ctx: &mut FnCtx<'f>,
    exprs: &[&Expr],
    across: impl FnOnce(&mut FnCtx<'f>) -> Result<T>,
    body: impl FnOnce(&mut FnCtx<'f>, &[String], T) -> Result<R>,
) -> Result<R> {
    with_operands_rooted_window(ctx, exprs, true, across, body)
}

/// The one implementation behind all three `with_operands_rooted*` forms.
///
/// They differ only in how `across_collects` is obtained; keeping the lowering,
/// the re-read point and the release in a single body is what stops the family
/// from growing three subtly different orderings (the drift that produced
/// #7114).
fn with_operands_rooted_window<'f, T, R>(
    ctx: &mut FnCtx<'f>,
    exprs: &[&Expr],
    across_collects: bool,
    across: impl FnOnce(&mut FnCtx<'f>) -> Result<T>,
    body: impl FnOnce(&mut FnCtx<'f>, &[String], T) -> Result<R>,
) -> Result<R> {
    use temp_root::{any_may_trigger_gc, root_operands_begin};

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

/// Does evaluating any of these expressions reach a collection point?
///
/// Re-exported so a migrated module can answer the question a caller-supplied
/// `protect` flag needs (`{ ...a, k: f() }`, `Math.min(f(), g(), h())`) without
/// naming `expr::temp_root`. It is the same predicate `operand_protection`
/// consults, not a second copy.
pub(crate) fn any_operand_may_collect<'a>(
    ctx: &FnCtx<'_>,
    exprs: impl IntoIterator<Item = &'a Expr>,
) -> bool {
    temp_root::any_may_trigger_gc(ctx, exprs)
}

/// [`any_operand_may_collect`] for a single expression.
///
/// The per-operand form is what a group with *unequal* windows needs: in the
/// generic dynamic call the receiver is live across the callee read and every
/// argument, the callee across every argument, and argument `i` across the
/// arguments after it plus an allocating rebind. One `collects` for the whole
/// list cannot say that.
pub(crate) fn operand_may_collect(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    temp_root::expr_may_trigger_gc(ctx, expr)
}

// ---------------------------------------------------------------------------
// Split out of this file to stay under the 2000-line cap (`check_file_size.sh`).
//
// Both are pure moves. `group.rs` holds the multi-point re-read scope and the
// accumulator combinators; `ledger.rs` holds the per-module migration ledger
// and its tests. The re-exports below are explicit and by name, so every
// caller still says `crate::rooting::…` and nothing new becomes reachable.
// ---------------------------------------------------------------------------

mod group;
mod ledger;

// `ImplicitThisSave` and `NewTargetSave` are deliberately absent: no caller
// names either type — both are only ever held as an inferred local between the
// `*_save` and `*_restore` pair — so re-exporting them would be an unused
// import. They stay `pub(crate)` in `group.rs`, where the functions that
// produce them live.
pub(crate) use group::{
    implicit_this_restore, implicit_this_save, new_target_restore, new_target_save,
    open_rooted_group, with_rooted_accumulator, with_rooted_group, AccArray, EmittedValue,
    RootedAcc, RootedGroup,
};

// ---------------------------------------------------------------------------
// One receiver, materialised once for a whole call-site lowering (#10943).
// ---------------------------------------------------------------------------

thread_local! {
    /// The receiver the innermost call-site guard materialised, keyed by the
    /// identity of the receiver's HIR node.
    static MATERIALIZED_RECEIVER: std::cell::RefCell<Option<(usize, RootedSlot)>> =
        const { std::cell::RefCell::new(None) };
}

/// Materialise a call's receiver ONCE, run `body` with it available, release.
///
/// The own-override guard (#10943) needs the receiver's VALUE before it can
/// branch, and the lowerings below it are handed the same receiver EXPRESSION
/// and lower it again. Evaluating an effectful receiver twice is a wrong
/// program — `make().get(k)` must call `make()` once — so it is evaluated
/// here, held in a rooted slot for the window (everything below it allocates),
/// and RE-READ at each use rather than handed out as a register: an
/// evacuating minor rewrites the root, not the register (#7211).
///
/// `key` is the identity of the receiver's HIR node
/// (`expr as *const Expr as usize`), which is what
/// [`materialized_receiver_reread`] matches against. HIR nodes are owned by
/// the module for the whole of codegen and are never shared between call
/// sites, so the identity is exact.
///
/// Nesting is a stack: a guard inside `body` saves and restores this cell, and
/// releases its own slot first, which is the order [`RootedSlot::release`]'s
/// truncate requires.
pub(crate) fn with_materialized_receiver<R>(
    ctx: &mut FnCtx<'_>,
    key: usize,
    value: &str,
    body: impl FnOnce(&mut FnCtx<'_>) -> R,
) -> R {
    let slot = RootedSlot {
        idx: temp_root::temp_root_push_double(ctx, value),
        repr: Repr::Boxed,
    };
    let previous =
        MATERIALIZED_RECEIVER.with(|cell| cell.borrow_mut().replace((key, slot.clone())));
    let out = body(ctx);
    MATERIALIZED_RECEIVER.with(|cell| *cell.borrow_mut() = previous);
    slot.release(ctx);
    out
}

/// Re-read the materialised receiver for `key` HERE, or `None` when this node
/// is not the materialised one.
///
/// Every operand lowering in the compiler funnels through
/// `crate::expr::lower_expr` (`RootedGroup::lower`, `with_operands_rooted` and
/// the arms' direct calls all do), so consulting it there covers every way a
/// lowering below the guard can ask for the receiver.
pub(crate) fn materialized_receiver_reread(ctx: &mut FnCtx<'_>, key: usize) -> Option<String> {
    let slot = MATERIALIZED_RECEIVER.with(|cell| match &*cell.borrow() {
        Some((installed, slot)) if *installed == key => Some(slot.clone()),
        _ => None,
    })?;
    Some(read_slot(ctx, &slot))
}
