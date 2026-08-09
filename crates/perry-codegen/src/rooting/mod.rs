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

/// The raw, order-sensitive rooting API.
///
/// **PRIVATE, and that is the campaign's terminal condition** (#7615). It used
/// to be `crate::expr::temp_root`, reachable from anywhere in the crate, and
/// every bug in the #7341 family was an ordering mistake against it: a push
/// below the collection point (#7192), a truncate at the wrong slot, a release
/// on one arm of an `if` (#7462), a re-read taken above the window (#7114).
///
/// A private module inside `rooting` makes each of those unwritable outside
/// this file rather than merely uncounted — the ledger below can only report
/// what a module NAMES, and a module that cannot name it has nothing to
/// report. The accessors additionally carry an explicit
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
#[derive(Debug)]
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
// The multi-point re-read scope (#7615 slice 6).
//
// WHY THE `with_operands_rooted*` FAMILY COULD NOT TAKE THE `lower_call/`
// MODULES, stated as a property of the API rather than of those files.
//
// Every form above re-reads at exactly ONE point: the end of the operand list,
// after an optional `across` step. That is the whole shape for `m.set(k, v)`
// and `u8[i]` — lower, protect, re-read once, consume once. Three lowerings in
// `lower_call/` are not that shape, and each is a different way of not being
// it:
//
//   * `lower_dynamic_closure_call` consumes the group in TWO instructions with
//     an allocating step between them. The receiver and callee feed
//     `js_closure_unbox_callee_checked_rebind`, which CLONES a `this`-capturing
//     closure and therefore allocates; the arguments feed `js_closure_callN`
//     BELOW it. One re-read point can serve one of those two and must strand
//     the other (#7154's own reasoning, `RootedOperands::reread_one`).
//   * `lower_rest_call_args_rooted` re-reads element `i` between the pushes
//     that materialise the rest array — `js_array_alloc` plus one
//     `js_array_push_f64` per element, all of which allocate — so its re-read
//     points are a LOOP, not a point.
//   * `try_lower_func_ref_call` releases ~450 lines below the lowering, in the
//     merge block of four block-splitting specialized-ABI dispatch diamonds. A
//     closure form can express that only by swallowing the dispatch chain.
//
// So the missing combinator is not "the variadic/rest shape" (slice 5's
// hypothesis, and the shape that made it visible) but the thing all three want:
// ONE temp-root scope whose contents may be re-read at ANY number of
// caller-chosen points. The rest/variadic case then falls out as a group that
// happens to hold an accumulator array as well as operands, which is why
// [`RootedGroup`] carries both rather than there being a second type for it.
//
// TWO ENTRY POINTS, AND THE ASYMMETRY IS DELIBERATE.
//
// [`with_rooted_group`] owns the release, like every other combinator here.
// [`open_rooted_group`] hands the group back, which every other combinator in
// this file deliberately refuses to do — so it needs an argument.
//
// The argument is that the two halves of a mis-managed guard are not equally
// dangerous. A release that is EARLY or MIS-ORDERED is a use-after-free: the
// slot is cut while the consumer still reads it. A release that never happens
// is over-retention — the slot stays bound for the rest of the function, the
// object stays live, and the emitted code is merely conservative (in the FFI
// fallback the runtime stack also grows, which is #7462's symptom and a real
// bug, but still not a dangling pointer).
//
// [`RootedGroup`] removes the dangerous half BY CONSTRUCTION and for both
// entry points: it is not `Clone`, `release` consumes it, and there is no way
// to obtain the slot index — so a caller cannot truncate at the wrong slot, in
// the wrong order, or twice. What escaping leaves writable is exactly the safe
// half. That is strictly better than the raw API it replaces, where the caller
// holds an `Option<String>` slot index it can truncate anywhere.
//
// Prefer [`with_rooted_group`]. Reach for [`open_rooted_group`] only where the
// release must post-dominate blocks the lowering does not lexically contain.
// ---------------------------------------------------------------------------

/// One temp-root scope: an ordered stack of rooted values — already-lowered
/// **operands** and mutable **accumulator arrays** — re-readable at any number
/// of caller-chosen points and released once, for the whole stack.
///
/// Two things it does NOT do, both on purpose:
///
///  * it never hands out a slot index, so the release cannot be mis-ordered.
///    `temp_root_truncate` is a stack CUT — truncating the wrong slot drops
///    every slot above it, which is how a receiver save becomes the number `0`
///    (`func_ref.rs`'s note on release ordering);
///  * it never lowers an operand it was not asked to. [`RootedGroup::lower`]
///    lowers, [`RootedGroup::adopt`] takes a value the caller emitted itself —
///    which the generic dynamic call needs, because its callee operand is a
///    hand-emitted by-name property read rather than `lower_expr(callee)`.
pub(crate) struct RootedGroup<'a> {
    operands: temp_root::RootedOperands,
    exprs: Vec<&'a Expr>,
    accs: Vec<String>,
    emitted: Vec<EmittedRoot>,
    /// The LOWEST slot this group pushed, of either kind. One truncate at it
    /// drops the whole scope, because a truncate is a stack cut.
    first_slot: Option<String>,
}

/// What [`RootedGroup::adopt_emitted`] recorded for one emitted value.
///
/// The `Reused` arm is the `protect == false` answer, and it exists for the
/// same reason [`RootedAcc`]'s `value` field does: a site whose window
/// provably cannot collect must keep the IR it had before it was rooted at
/// all, register numbering included. It is NOT a third protection strategy —
/// `operand_protection`'s `Reload` is still unavailable here (re-emitting the
/// producing call would call it twice) and `Reuse`-across-a-real-window is
/// still the bug. It only records that there was no window.
enum EmittedRoot {
    Rooted(RootedSlot),
    Reused(String),
}

/// A handle on one **emitted** value inside a [`RootedGroup`] —
/// see [`RootedGroup::adopt_emitted`].
///
/// Opaque and `Copy`, for the same reason [`AccArray`] is: it is not a slot
/// index, so it cannot be truncated, mis-ordered or released. The same
/// not-branded-per-group caveat applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EmittedValue(usize);

/// A handle on one accumulator array inside a [`RootedGroup`].
///
/// Opaque and `Copy`. What it buys is the half that matters: it is not a slot
/// index, so it cannot be truncated, mis-ordered or released — only handed back
/// to the group, which owns every emission that touches the slot.
///
/// It is **not branded per group**. It is a bare index into the group's own
/// list, so passing a handle to a *different* group selects that group's
/// accumulator at the same position (or panics if it has fewer). Pass a handle
/// only to the group that returned it. Branding it would need a group identity
/// this file has nowhere to get without global state, and the mistake is not
/// one any caller is positioned to make: a group is always a local, and the two
/// entry points hand it out one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AccArray(usize);

impl<'a> RootedGroup<'a> {
    fn new(capacity: usize) -> Self {
        RootedGroup {
            operands: temp_root::root_operands_begin(capacity),
            exprs: Vec::with_capacity(capacity),
            accs: Vec::new(),
            emitted: Vec::new(),
            first_slot: None,
        }
    }

    /// Record the lowest slot the group holds. Slots are handed out in
    /// increasing watermark order, so the first one recorded is the lowest and
    /// later ones must not displace it.
    fn note_slot(&mut self, slot: Option<String>) {
        if self.first_slot.is_none() {
            self.first_slot = slot;
        }
    }

    /// Lower `expr` and root it across a window the caller states.
    ///
    /// Returns the operand's index for [`RootedGroup::reread`] — and returns
    /// *only* that. The lowered register is deliberately not handed back: a
    /// caller holding it is the second half of every bug in this family, and
    /// the group is now the only way to name the value.
    pub(crate) fn lower(
        &mut self,
        ctx: &mut FnCtx<'_>,
        expr: &'a Expr,
        collects: bool,
    ) -> Result<usize> {
        let value = crate::expr::lower_expr(ctx, expr)?;
        Ok(self.adopt(ctx, expr, &value, collects))
    }

    /// Root a value the caller emitted itself.
    ///
    /// `expr` still decides the protection, so the group answers "root,
    /// re-derive, or reuse?" through `operand_protection` exactly as a lowered
    /// operand does.
    ///
    /// **Precondition.** `value` must be what lowering `expr` produces, or
    /// `expr` must not be `operand_is_reloadable` — because a `Reload` operand
    /// is re-read by *re-lowering* `expr`, and re-lowering something the caller
    /// did not lower would answer with a different value. Only `Expr::String`
    /// is reloadable, so every current caller (a `PropertyGet` callee, a
    /// receiver) satisfies this trivially; the note is here for the next one.
    pub(crate) fn adopt(
        &mut self,
        ctx: &mut FnCtx<'_>,
        expr: &'a Expr,
        value: &str,
        collects: bool,
    ) -> usize {
        self.operands.push(ctx, expr, value, collects);
        let pushed = self.operands.guard();
        self.note_slot(pushed);
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    /// Re-read operand `i` **here**, below whatever has collected since it was
    /// rooted.
    ///
    /// Mandatory rather than defensive, and it is the reason this type exists:
    /// the slot is a MUTABLE root that an evacuating cycle rewrites in place,
    /// so a register read before the cycle names from-space.
    pub(crate) fn reread(&self, ctx: &mut FnCtx<'_>, i: usize) -> Result<String> {
        self.operands.reread_one(ctx, &self.exprs, i)
    }

    /// Re-read every operand at this point, in order.
    pub(crate) fn reread_all(&self, ctx: &mut FnCtx<'_>) -> Result<Vec<String>> {
        self.operands.reread(ctx, &self.exprs)
    }

    /// How many operands the group holds.
    pub(crate) fn len(&self) -> usize {
        self.exprs.len()
    }

    /// True when this group actually pushed a slot.
    ///
    /// The signal a caller uses to keep an eager unbox — and therefore its
    /// exact register numbering — on the unprotected path, exactly as
    /// `RootedOperands::is_rooted` served `math_simple.rs`'s `MapSet` before
    /// the migration. It reports whether a slot exists, never which one, so it
    /// cannot be turned into a release.
    pub(crate) fn is_rooted(&self) -> bool {
        self.first_slot.is_some()
    }

    /// Root a value that an **emitted step** produced, rather than one lowered
    /// from an expression.
    ///
    /// # Why this exists, and why it took until slice 7
    ///
    /// This file deleted a `root_i64(ctx, reg) -> RootedSlot` combinator unused
    /// and recorded the terms on which a replacement could return: "it should
    /// arrive with its caller and with a written argument for why
    /// [`call_rooted`] cannot serve". Slice 7 found two callers, and they are
    /// the same shape:
    ///
    ///  * `expr/proxy_reflect.rs` — `process.env[k] = v` must coerce the key
    ///    (`js_to_property_key`, which runs a user `Symbol.toPrimitive`) ABOVE
    ///    the value's evaluation, because ES2022 moved `ToPropertyKey` before
    ///    the RHS. The **coerced** key is what has to survive that evaluation,
    ///    and it is a fresh heap string with no other root;
    ///  * `expr/fs_await.rs` — the await loop polls the promise that
    ///    `js_assimilate_thenable` + `js_await_any_promise` produced, which for
    ///    a thenable is a **wrapper** the assimilation allocated, not the
    ///    operand.
    ///
    /// **Why [`call_rooted`] cannot serve.** It fuses the root store to a call
    /// it emits itself and hardcodes [`Repr::Ptr`], so it can only root the
    /// direct `i64` result of one call. Neither value is that: the property key
    /// is a `double` whose raw pointer must be taken BELOW the window, and the
    /// promise is used boxed (`js_value_is_promise`) and unboxed
    /// (`js_promise_state`) in six different basic blocks.
    ///
    /// **Why there is no protection decision to make.** Every other entry point
    /// asks `operand_protection`; this value has no `Expr` to ask about, and
    /// two of the three answers are unavailable on principle rather than by
    /// choice. `Reload` cannot re-derive it — re-emitting the producing call
    /// would call it a *second* time, and both producers are observable. And
    /// `Reuse` is the bug. So the answer is always `Root`, there is no flag,
    /// and a caller cannot pick the wrong one.
    ///
    /// **What it does weaken, stated plainly.** `value` is a register the
    /// caller produced, so "produce it, let something collect, THEN root it" —
    /// #7192 — is writable here, exactly as it is in
    /// [`with_rooted_accumulator`], which has taken a caller-produced `initial`
    /// since slice 3. Call this on the line below the emission that produced
    /// the value.
    ///
    /// # `protect` states the WINDOW, not the strategy (slice 8)
    ///
    /// Slice 7 shipped this without a flag and said so: "there is no flag, and
    /// a caller cannot pick the wrong one". That claim was about the
    /// *strategy* — `Reload` and `Reuse` are unavailable for an emitted value
    /// on principle — and it still holds. `protect` answers the other
    /// question, the one every combinator in this file already takes from its
    /// caller in some form: **does anything between here and the last use
    /// collect?** [`with_rooted_accumulator`] has taken it as `protect` since
    /// slice 3, `RootedGroup::lower`/`adopt` take it as `collects`, and
    /// `with_operands_rooted_across_call` hardcodes it to `true`.
    ///
    /// Slice 8 needed it for `expr/static_field_meta.rs`: a `ClassExprFresh`
    /// with no statics, no captures, no symbol statics and no `static { … }`
    /// block emits nothing at all between the class object's allocation and
    /// the `nanbox_pointer_inline` that returns it, and that shape is
    /// reachable (`lower_decl/body_stmt.rs`'s `fresh_binding` arm builds one
    /// with three empty vectors). `protect == false` emits no push, no
    /// re-reads and no truncate, so it keeps the pre-rooting IR byte for byte
    /// — the same contract `rooted_handle_begin(ctx, h, false)` had.
    pub(crate) fn adopt_emitted(
        &mut self,
        ctx: &mut FnCtx<'_>,
        repr: Repr,
        value: &str,
        protect: bool,
    ) -> EmittedValue {
        let root = if protect {
            let idx = match repr {
                Repr::Ptr => temp_root::temp_root_push_i64(ctx, value),
                Repr::Boxed => temp_root::temp_root_push_double(ctx, value),
            };
            self.note_slot(Some(idx.clone()));
            EmittedRoot::Rooted(RootedSlot { idx, repr })
        } else {
            EmittedRoot::Reused(value.to_string())
        };
        self.emitted.push(root);
        EmittedValue(self.emitted.len() - 1)
    }

    /// Re-read an [`adopt_emitted`](RootedGroup::adopt_emitted) value **here**,
    /// in the representation it was pushed with.
    ///
    /// An unprotected value hands its original register back and emits
    /// nothing, exactly as `RootedOperands::reread_one`'s `Reuse` arm does.
    pub(crate) fn reread_emitted(&self, ctx: &mut FnCtx<'_>, value: EmittedValue) -> String {
        match &self.emitted[value.0] {
            EmittedRoot::Rooted(slot) => read_slot(ctx, slot),
            EmittedRoot::Reused(reg) => reg.clone(),
        }
    }

    /// Allocate an argument-accumulator array of capacity `cap` and root it in
    /// this scope.
    ///
    /// This is the variadic / spread / rest shape: `js_array_alloc(n)`, then one
    /// push per argument. The accumulator holds the ONLY reference to everything
    /// pushed so far while the next argument is lowered, and every push
    /// allocates — so it is an accumulator in exactly [`RootedAcc`]'s sense, and
    /// it lives in the group so that ONE release drops the operands and the
    /// arrays together.
    pub(crate) fn begin_array(&mut self, ctx: &mut FnCtx<'_>, cap: &str) -> AccArray {
        let slot = temp_root::rooted_array_begin(ctx, cap);
        self.note_slot(Some(slot.clone()));
        self.accs.push(slot);
        AccArray(self.accs.len() - 1)
    }

    /// Push one element, re-reading the array from its slot and publishing the
    /// possibly-reallocated pointer back into it.
    pub(crate) fn push_array(&mut self, ctx: &mut FnCtx<'_>, acc: AccArray, value: &str) {
        let slot = self.accs[acc.0].clone();
        temp_root::temp_rooted_array_push(ctx, &slot, value);
    }

    /// Re-read the finished array as a raw `i64` pointer. Does not release: the
    /// consuming call allocates while it reads the array.
    pub(crate) fn read_array(&self, ctx: &mut FnCtx<'_>, acc: AccArray) -> String {
        let slot = self.accs[acc.0].clone();
        temp_root::rooted_array_read(ctx, &slot)
    }

    /// Drop the whole scope. Call it *after* the consuming call: the consumer
    /// allocates while reading these values.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        temp_root::temp_root_release(ctx, self.first_slot);
    }
}

/// Open a [`RootedGroup`] for the duration of `body` and release it on every
/// path out, including `body`'s `?`.
pub(crate) fn with_rooted_group<'f, 'a, R>(
    ctx: &mut FnCtx<'f>,
    capacity: usize,
    body: impl FnOnce(&mut FnCtx<'f>, &mut RootedGroup<'a>) -> Result<R>,
) -> Result<R> {
    let mut group = RootedGroup::new(capacity);
    let out = body(ctx, &mut group);
    group.release(ctx);
    out
}

/// Open a [`RootedGroup`] whose release the CALLER performs, because it must
/// post-dominate blocks this lowering does not lexically contain.
///
/// One shape needs this and it is named rather than left general:
/// `func_ref.rs`'s direct call lowers its arguments, then dispatches through up
/// to four specialized-ABI diamonds that split the block, and the release has
/// to sit in the merge. Releasing inside either side of a diamond would leave
/// the other side's call reading dropped slots.
///
/// `#[must_use]` because dropping the group silently is the one mistake left
/// writable — see the block comment above for why that mistake is
/// over-retention rather than a dangling pointer, and why the dangerous half
/// (an early or mis-ordered truncate) is not writable at all.
#[must_use = "a RootedGroup must be released with `release`, below the call that reads it"]
pub(crate) fn open_rooted_group<'a>(capacity: usize) -> RootedGroup<'a> {
    RootedGroup::new(capacity)
}

/// A saved implicit `this`, held in a rooted slot for the duration of a
/// dispatch (#7211).
///
/// **Moved here in slice 6 rather than re-exported.** It was already a paired
/// combinator with this file's contract — root before the window, re-read
/// after it, never hand out the register — but it lived in the raw API, so six
/// `lower_call/` modules had to name `expr::temp_root` while making no ordering
/// decision at all. Leaving a copy behind would have given the pair two
/// spellings, which is the drift that produced #7114; there is one.
///
/// `js_implicit_this_set` swaps the `IMPLICIT_THIS` cell and returns what was
/// there, read straight out of a cell `scan_implicit_this_roots_mut`
/// (`object/this_binding.rs:176`) registers as a scanned MUTABLE root. The swap
/// has already overwritten the cell, so the returned value is now held ONLY in
/// an SSA register, across the whole call the bind exists to scope.
///
/// Two ways that hurts, and the second is what makes it worse than an ordinary
/// stale read:
///
///  * the enclosing frame still roots the same object, so an evacuating minor
///    inside the callee MOVES it and rewrites that root — leaving this register
///    naming from-space. The restore then publishes a pre-move address back
///    INTO a root the collector scans, so the corruption outlives the call that
///    caused it and surfaces in whatever reads `this` next;
///  * where no other root holds it, the object is simply collected.
///
/// Seven lowerings emit this pair. They had seven copies of the same three
/// lines and therefore seven copies of the same bug, which is why it is a
/// combinator rather than seven edits.
pub(crate) struct ImplicitThisSave {
    slot: RootedSlot,
}

/// Bind `new_this` as the implicit `this` and root the value it displaced.
///
/// Unconditional, unlike an operand group: the window is a user or native call,
/// so `operand_protection`'s "can this window collect?" test has exactly one
/// answer here and there is nothing to gate on.
pub(crate) fn implicit_this_save(ctx: &mut FnCtx<'_>, new_this: &str) -> ImplicitThisSave {
    let prev = ctx
        .block()
        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, new_this)]);
    let idx = temp_root::temp_root_push_double(ctx, &prev);
    ImplicitThisSave {
        slot: RootedSlot {
            idx,
            repr: Repr::Boxed,
        },
    }
}

/// Restore the saved implicit `this`, re-read from its root.
///
/// Reading the slot rather than the register is the fix, not a precaution: the
/// slot is a mutable root, so an evacuating cycle inside the dispatch rewrote
/// it and the register pushed beforehand names from-space.
///
/// The release is emitted BEFORE the restore call so that nested saves — an
/// override arm inside an outer bind — release inner to outer. A release is a
/// stack cut, so a caller holding a LOWER group may release it afterwards and
/// drop this slot a second time harmlessly.
pub(crate) fn implicit_this_restore(ctx: &mut FnCtx<'_>, save: ImplicitThisSave) {
    let prev = read_slot(ctx, &save.slot);
    save.slot.release(ctx);
    ctx.block()
        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev)]);
}

/// The `new.target` cell's saved previous value (#7664).
///
/// Structurally [`ImplicitThisSave`] for a different cell, and it is a separate
/// type rather than a parameter so the two cannot be crossed at a restore.
///
/// The cell is a registered mutable root — `scan_current_new_target_root_mut`,
/// `gc/mod.rs` — so an evacuating cycle inside the constructor rewrites it and
/// a register saved beforehand names from-space. The RUNTIME's own construct
/// paths have always rooted their `prev_new_target`
/// (`object/class_registry/construct.rs`, `scope.root_nanbox_f64`); the
/// generated `new` path saved it into a bare SSA register across the whole
/// constructor body, which is #7226's `prev_this` bug for `new.target`.
///
/// Re-reading the cell instead of rooting it would be the wrong repair, and for
/// the reason `operand_is_reloadable` states: `js_new_target_set` has already
/// overwritten it with THIS class's ref, so a re-read returns the new value,
/// not the saved one. Only a root gives both a rewritten location and the value
/// the save observed.
pub(crate) struct NewTargetSave {
    slot: RootedSlot,
}

/// Set `new.target` to `new_target` and root the value it displaced.
pub(crate) fn new_target_save(ctx: &mut FnCtx<'_>, new_target: &str) -> NewTargetSave {
    let prev = ctx.block().call(DOUBLE, "js_new_target_get", &[]);
    let idx = temp_root::temp_root_push_double(ctx, &prev);
    ctx.block()
        .call(DOUBLE, "js_new_target_set", &[(DOUBLE, new_target)]);
    NewTargetSave {
        slot: RootedSlot {
            idx,
            repr: Repr::Boxed,
        },
    }
}

/// Restore the saved `new.target`, re-read from its root.
///
/// Takes the save by REFERENCE, and does not release — which is the difference
/// from [`implicit_this_restore`] and is forced by the caller. `new.rs` emits
/// this restore on several exits from one save, and its slot is cut by the
/// enclosing expression scope (`temp_root_scope_begin`/`temp_root_scope_end`,
/// which that module already opens precisely because its ~20 return paths make
/// per-path releases the thing that gets missed, #6969). A release here would
/// be a stack cut on one of those paths only.
pub(crate) fn new_target_restore(ctx: &mut FnCtx<'_>, save: &NewTargetSave) {
    let prev = read_slot(ctx, &save.slot);
    ctx.block()
        .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &prev)]);
}

/// A GC-managed value that generated code keeps **updating** while it lowers
/// further expressions: an object literal's half-built handle, `Object.assign`'s
/// threaded target, `Math.min(...)`'s growing argument array.
///
/// It is the operand group's mirror image. An operand is lowered once and read
/// once; an accumulator is written, read, rewritten and read again, with
/// arbitrary user code lowered between the writes. #7154's `ObjectSpread` bug is
/// the canonical failure: the half-built object sat in a raw SSA register while
/// 269 spread values were lowered, an evacuating minor relocated it, and every
/// later field store wrote into abandoned from-space memory — silently, because
/// the fields simply did not appear on the copy the program kept.
///
/// The invariant it enforces is the one a raw handle cannot: **the accumulator
/// never exists as a register the lowering holds across an emission.** Every
/// consuming call re-reads it as part of being emitted ([`RootedAcc::call`],
/// [`RootedAcc::call_void`]), and a helper that returns a fresh address
/// publishes it straight back into the slot ([`RootedAcc::advance`]) rather than
/// handing it out. The single point where a register does escape is the final
/// read, and [`with_rooted_accumulator`]'s `finish` closure owns it: it runs
/// below the last collection point and above the release, so there is no
/// program in which the escaped register outlives its root.
pub(crate) struct RootedAcc {
    slot: Option<RootedSlot>,
    repr: Repr,
    /// The register as first produced. The answer when `protect` was false, in
    /// which case nothing is emitted and the IR matches the un-rooted form byte
    /// for byte.
    value: String,
}

impl RootedAcc {
    /// The accumulator as a call argument.
    fn as_arg(&self) -> Arg<'_> {
        match &self.slot {
            Some(slot) => Arg::Root(slot),
            None => Arg::Plain(self.repr.llvm_ty(), &self.value),
        }
    }

    fn args_with_self<'a>(&'a self, rest: &[Arg<'a>]) -> Vec<Arg<'a>> {
        let mut all = Vec::with_capacity(rest.len() + 1);
        all.push(self.as_arg());
        all.extend_from_slice(rest);
        all
    }

    /// Emit `callee(<accumulator>, ...rest)` and return its result register.
    ///
    /// The accumulator is argument **0**, positionally and deliberately. It is
    /// argument 0 at every site this exists for — `js_object_set_field_by_name`,
    /// `js_object_copy_own_fields`, `js_array_push_f64`, `js_object_assign_one`
    /// — and keeping it positional is what lets the re-read be fused to the
    /// emission instead of handed back to the caller as a register to place.
    pub(crate) fn call(
        &self,
        ctx: &mut FnCtx<'_>,
        ret_ty: LlvmType,
        callee: &str,
        rest: &[Arg<'_>],
    ) -> String {
        call_with_roots(ctx, ret_ty, callee, &self.args_with_self(rest))
    }

    /// [`RootedAcc::call`] for a `void` helper.
    pub(crate) fn call_void(&self, ctx: &mut FnCtx<'_>, callee: &str, rest: &[Arg<'_>]) {
        call_void_with_roots(ctx, callee, &self.args_with_self(rest));
    }

    /// Emit `callee(<accumulator>, ...rest)` and make its result the new
    /// accumulator value.
    ///
    /// For helpers that may **relocate** what they are handed and return the
    /// current address: `js_array_push_f64` reallocs the element storage,
    /// `js_object_assign_one` returns the post-collection target. Keeping the
    /// pre-call register instead is how `Object.assign(t, a, b)` threaded a
    /// stale `t` into `b`'s link before #7200.
    pub(crate) fn advance(&mut self, ctx: &mut FnCtx<'_>, callee: &str, rest: &[Arg<'_>]) {
        let next = self.call(ctx, self.repr.llvm_ty(), callee, rest);
        match (&self.slot, self.repr) {
            (Some(slot), Repr::Ptr) => temp_root::temp_root_set_i64(ctx, &slot.idx, &next),
            (Some(slot), Repr::Boxed) => temp_root::temp_root_set_double(ctx, &slot.idx, &next),
            (None, _) => self.value = next,
        }
    }
}

/// Root a mutable accumulator for the duration of `build`, then hand its final
/// value to `finish` and release it.
///
/// `protect == false` emits nothing at all — no push, no re-reads, no truncate —
/// so a site whose initializers provably cannot collect keeps the IR it had
/// before it was rooted at all.
///
/// The split into two closures is what makes the release unmissable while still
/// letting the final value be *used*. `build` may not hold the accumulator in a
/// register across anything; `finish` receives one, but it runs below the last
/// collection point and above the release, and it is the only place the value
/// escapes. Both paths — `build`'s `?` and `finish`'s — release.
pub(crate) fn with_rooted_accumulator<'f, R>(
    ctx: &mut FnCtx<'f>,
    repr: Repr,
    initial: &str,
    protect: bool,
    build: impl FnOnce(&mut FnCtx<'f>, &mut RootedAcc) -> Result<()>,
    finish: impl FnOnce(&mut FnCtx<'f>, &str) -> Result<R>,
) -> Result<R> {
    let slot = protect.then(|| {
        let idx = match repr {
            Repr::Ptr => temp_root::temp_root_push_i64(ctx, initial),
            Repr::Boxed => temp_root::temp_root_push_double(ctx, initial),
        };
        RootedSlot { idx, repr }
    });
    let mut acc = RootedAcc {
        slot,
        repr,
        value: initial.to_string(),
    };
    let out = (|| {
        build(ctx, &mut acc)?;
        let final_value = match &acc.slot {
            Some(slot) => read_slot(ctx, slot),
            None => acc.value.clone(),
        };
        finish(ctx, &final_value)
    })();
    if let Some(slot) = acc.slot {
        slot.release(ctx);
    }
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
/// was true of both slice-1 modules: they named no `temp_root` symbol before
/// the migration, so `migrated_modules_do_not_reach_past_the_rooting_api` went
/// green the instant the line was added. The listing only means something if the
/// slice ALSO ran the sabotage arm — inject a real, compiling `temp_root_push_*`
/// / `temp_root_truncate` pair into the migrated module, confirm the ledger test
/// goes red and names the lines, then revert. Every slice so far has, and
/// recorded it in its PR; a slice that skips it is adding a line that asserts
/// nothing.
///
/// Slice 2's three modules are the first since the template where the listing is
/// **not** vacuous: all three named `expr::temp_root` before the migration
/// (`lower_exprs_rooted`, `guard_store_operand_across`, `rooted_handle_*`,
/// `temp_root_{push,get,set}_double`), so the ledger line is load-bearing on the
/// committed source and not only under sabotage. The sabotage arm was still run
/// per module — the assert stops at the first offender, so one run cannot speak
/// for three.
///
/// Slice 3 is three of each. `objects_arrays_lit.rs`, `array_literal.rs` and
/// `object_literal.rs` all named `expr::temp_root` before the migration
/// (`temp_root_{push,get,set}_i64`, `lower_exprs_rooted`/`temp_root_release`,
/// `rooted_handle_*`/`temp_root_{push,get}_double`), so their lines are
/// load-bearing. `array_push.rs` named none and its line is vacuous on the
/// committed source — the sabotage arm is the only thing that makes it an
/// assertion, exactly as for both slice-1 modules, and the audit that earned
/// the listing is written into that file's header rather than into this one.
///
/// Slice 4 is three load-bearing and six vacuous. `index_get.rs`,
/// `index_set.rs` and `property_set.rs` all named the `StoreOperandGuard`
/// family (`guard_store_operand`, `guard_store_operand_across`,
/// `reread_store_operand`, `release_store_operand`, `expr_may_trigger_gc`)
/// before the migration, so their lines hold on the committed source. The other
/// six — `index_get/guarded_array.rs`, `index_get/inline_dyn_typed_array.rs`,
/// `index_set_typed_array.rs`, `property_get.rs`, `property_get/globalget.rs`,
/// `property_get/helpers.rs` — named none, and each carries the audit that
/// earned its listing in its own file header.
///
/// ★ **A listed module is not an audited module, and slice 4 is where that
/// distinction became load-bearing.** These two properties are not the same:
///
///   1. every rooting decision the module makes goes through `crate::rooting`
///      — which is what this ledger checks, and what the listing means;
///   2. every window in the module HAS a rooting decision.
///
/// `index_set.rs` and `index_get.rs` satisfy (1) and do not satisfy (2). The
/// migration itself surfaced the gap: translating the six guarded arms of
/// `index_set.rs` made it obvious that its `#5525` typed-array arm, its
/// bounded-index array store and ten arms of `index_get.rs` lower a receiver,
/// then lower more user code, then use the receiver — and root nothing. Three
/// of those were adjacent to arms this slice was already rewriting and are
/// fixed here (#7637, #7638, #7639); the rest are filed as #7640 rather than
/// fixed, because they sit on inline fast paths where a temp root is a measured
/// cost rather than plumbing.
///
/// So do not read a ledger line as "this module has no rooting bugs". It says
/// the module cannot make an ORDERING mistake against the raw API, because it
/// no longer names it. A window with no decision at all is invisible to this
/// check, and the only instrument for it is reading the module.
///
/// Slice 5 is two modules of the `lower_call/` family, both load-bearing:
/// `extern_timers.rs` and `namespace_call.rs` each named
/// `lower_exprs_rooted` / `temp_root_release` before the migration, so their
/// lines hold on the committed source and not only under sabotage.
///
/// It is two rather than the six the family contains, and the four left out are
/// left out for one reason worth recording, because it is a **statement about
/// this API rather than about those files**: three of them need a re-read at
/// more than one point, and every `with_operands_rooted*` form has exactly one.
///
///   * `lower_call/mod.rs` — `lower_call_args_rooted` and
///     `lower_rest_call_args_rooted` return a guard *deliberately*: their
///     consumers in `func_ref.rs` are block-splitting specialized-ABI diamonds,
///     so the release must sit in a merge block that post-dominates four
///     dispatch paths, ~200 lines below the lowering. A closure form can express
///     that only by swallowing the whole dispatch chain, in a file outside the
///     slice.
///   * `lower_call/new.rs` — `refresh_rooted_args` re-reads the SAME operand
///     group at three caller-chosen points (after the instance allocation,
///     before the field initializers, before an inlined constructor body), under
///     a `temp_root_scope_begin`/`_end` marker spanning ~20 return paths.
///   * `lower_call/console_promise.rs` — `lower_dynamic_closure_call` re-reads
///     the receiver and callee below the arguments, then re-reads the arguments
///     again below the allocating rebind unbox. Two stages, one combinator.
///   * `lower_call/early_branches.rs` — its only escape-hatch uses are
///     `implicit_this_save`/`implicit_this_restore`, which is already a paired
///     combinator rather than the raw ordering API. Migrating it means
///     re-exporting that pair through `crate::rooting`, which is a rename that
///     would make the ledger line look substantive while asserting nothing new.
///
/// The honest reading: a module that cannot be migrated because the API cannot
/// say what it means is a gap in the API, and recording it here is what stops
/// the next slice from rediscovering it. The variadic/rest shape (per-element
/// re-reads between allocating pushes) is the concrete missing combinator.
///
/// **Slice 6 built that combinator and it is not the one slice 5 named.**
/// Slice 5's hypothesis was the variadic/rest shape; three modules wanted it and
/// only one of them is variadic. What all three want is
/// [`RootedGroup`] — one temp-root scope, re-readable at ANY number of
/// caller-chosen points — of which the rest shape is the case that also holds
/// an accumulator array. The block comment above [`RootedGroup`] argues the
/// shape and the two entry points; the reason there are two is that
/// `func_ref.rs`'s release must post-dominate four block-splitting dispatch
/// diamonds, which no closure form can own without swallowing the dispatch
/// chain.
///
/// Slice 6 lists three modules, all load-bearing on the committed source:
///
///   * `lower_call/mod.rs` — `lower_call_args_rooted` /
///     `lower_rest_call_args_rooted` / `emit_rooted_call`. Named
///     `lower_exprs_rooted`, `root_operands_begin`, `rooted_array_begin`,
///     `temp_rooted_array_push`, `rooted_array_read` and `temp_root_release`.
///     It now hands its callers a [`RootedGroup`] instead of an
///     `Option<String>` slot index, which is what makes `extern_func.rs`,
///     `namespace_call.rs` and `func_ref.rs` unable to truncate at the wrong
///     slot even though they still hold the scope.
///   * `lower_call/func_ref.rs` — the escaping-release consumer, plus
///     `implicit_this_save` / `implicit_this_restore`.
///   * `lower_call/console_promise.rs` — the two-stage dynamic closure call,
///     the `js_native_call_method_by_id` dispatch (which turned out to be the
///     plain single-re-read shape after all), and eight `console.*` arms.
///
/// **`implicit_this_save` / `implicit_this_restore` MOVED here** rather than
/// being re-exported, so the pair has one spelling. That incidentally clears
/// the escape hatch out of `early_branches.rs`, `method_override.rs` and both
/// `property_get` dispatchers, whose only uses were that pair. They are
/// deliberately NOT listed: a line here would assert that the module makes
/// every rooting decision through this API, and nobody has read those four
/// modules for windows with no decision at all. An unlisted module is honest;
/// a listed unaudited one is the distinction slice 4 had to draw the hard way.
///
/// Slice 7 lists three `expr/` modules, all load-bearing on the committed
/// source (`temp_root_{push,get}_double`, `temp_root_truncate`,
/// `guard_store_operand{,_across}`, `reread_store_operand`,
/// `release_store_operand`, `expr_may_trigger_gc`):
///
///   * `expr/child_proc.rs` — every `child_process` entry point. Three arms
///     rooted unconditionally through the raw API and five rooted nothing at
///     all while holding RAW heap pointers across arbitrary user lowerings.
///   * `expr/proxy_reflect.rs` — the densest unaudited module in the campaign.
///     One arm made a rooting decision (the `PutValueSet` write-IC, #7201) and
///     twenty-eight made none.
///   * `expr/fs_await.rs` — the await loop, whose root was correct and simply
///     never released.
///
/// **Slice 7 added the one combinator this file had refused to add ahead of a
/// caller**, and the refusal was right: the shape it predicted is the shape
/// that turned up. [`RootedGroup::adopt_emitted`] roots a GC-managed value that
/// an emitted step produced rather than one lowered from an `Expr` — the
/// coerced key of `process.env[k] = v`, and the assimilated promise the await
/// loop polls. Its doc carries the argument for why [`call_rooted`] cannot
/// serve and the note on what it weakens. Which is the shape of the answer this
/// campaign keeps arriving at: an API gap recorded in slice N is a combinator
/// in slice N+1, and writing the gap down is what makes the next slice cheap.
///
/// # Slice 8 — the campaign's last slice, and its terminal condition
///
/// The plan's terminal condition was `expr/temp_root.rs` "going
/// `pub(in crate::rooting)` — the raw accessor unreachable, not merely
/// uncounted". As literally spelled that is **not expressible in Rust**:
/// `pub(in path)` requires `path` to be an ANCESTOR module of the item
/// (E0742), and `crate::rooting` is not an ancestor of `crate::expr::temp_root`.
/// So the file MOVED — it is `crate::rooting::temp_root` now, declared with a
/// private `mod temp_root;` and with every accessor additionally carrying an
/// explicit `pub(in crate::rooting)`. Either alone would do it; both are here
/// because the module declaration is one keyword away from re-widening
/// twenty-five items at once.
///
/// Two items keep `pub(crate)` and are re-exported at the top of this file,
/// and neither is an accessor: [`TempRootPool`] is the compile-time slot
/// bookkeeping `FnCtx` owns (no runtime behaviour, no ordering), and
/// `expr_is_inert_primitive` is the shared "can evaluating this run user
/// code?" predicate the loop back-edge poll consults
/// (`crate::loop_purity`). A predicate cannot be called in the wrong order.
///
/// **Fourteen items were DELETED rather than narrowed**, because slice 8 left
/// them with no caller at all: `lower_exprs_rooted`,
/// `lower_operand_pair_rooted`, `any_later_ref_may_trigger_gc`,
/// `RootedOperands::is_rooted`, the whole `StoreOperandGuard` family
/// (`guard_store_operand`, `guard_store_operand_across`,
/// `reread_store_operand`, `release_store_operand`), the whole `RootedHandle`
/// family (`rooted_handle_begin`/`_get`/`_release`) and
/// `temp_root_scope_begin`/`_end`. CLAUDE.md's kill-policy is explicit that
/// "the losing mode should stop compiling", and each of these WAS a losing
/// mode: a caller-managed guard whose combinator replacement owns the release.
///
/// ## Modules migrated, and how they were told apart from the decision-free ones
///
/// The brief for this slice listed 14 files by `expr::temp_root` mention.
/// That count conflates two populations, and the ledger is only meaningful for
/// one of them. Sorting them is the first half of the work:
///
/// **Eight modules made rooting decisions and are listed below.** Seven are
/// load-bearing on the committed source (each named the raw API before the
/// migration):
///
///   * `expr/binary.rs` — five `lower_operand_pair_rooted` +
///     `temp_root_release` pairs, one per dynamic-dispatch arm, each with the
///     release on its own `return` path. They collapse into one
///     `lower_rooted_dynamic_binary` helper over [`with_operands_rooted`]:
///     five chances to misplace a release become none.
///   * `expr/math_simple.rs` — `Expr::MapSet` is a [`RootedGroup`] (two
///     operands with UNEQUAL windows, re-read at eight arm-specific points,
///     released once); `MapGet`/`MapHas` are the plain single-re-read shape.
///     `Expr::ArrayMap` is a live bug, below.
///   * `expr/static_field_meta.rs` — `ClassExprFresh` is a [`RootedGroup`]
///     over the class object with a nested [`with_rooted_accumulator`] for the
///     `__perry_ctor_caps` snapshot array and a nested
///     [`with_operands_rooted`] per symbol static.
///   * `expr/dyn_extern_i18n.rs` — the namespace-object build (#7280's
///     269-member zod case) is exactly [`with_rooted_accumulator`]'s shape.
///   * `lower_string_method.rs` — the receiver root that spans ~60 return
///     paths, via [`open_rooted_group`].
///   * `lower_string_concat.rs` — split out of `lower_string_method.rs` this
///     slice; load-bearing because the code it contains named
///     `lower_exprs_rooted`, `lower_operand_pair_rooted` and four raw
///     push/get/truncate/release spellings on `main`.
///   * `lower_call/new.rs` — `refresh_rooted_args` re-reads one operand group
///     at three caller-chosen points under a scope marker spanning ~20 return
///     paths. Slice 5 named it as the shape the API could not express and
///     slice 6 built [`RootedGroup`] for exactly it; this is the collection.
///
/// The eighth, `lower_call/new_alloc.rs`, is **vacuous on the committed
/// source** — it never named the raw API, because it is the instance
/// allocation carved out of `new.rs` this slice and everything it emits sits
/// above the instance root. It is listed anyway, for the reason slice 3 listed
/// `array_push.rs`: an unlisted sibling of a listed module is the obvious
/// place to put a raw push and escape the check. Its listing means something
/// only because the sabotage arm was run on it.
///
/// **Nine files mention the raw API and make no rooting decision at all.**
/// They are deliberately NOT listed, because a ledger line on a module that
/// never had a decision to make looks substantive and asserts nothing:
///
///   * `expr/mod.rs` — declared `mod temp_root` and typed the `temp_roots`
///     field. Structural; both are gone with the move.
///   * `codegen/entry.rs`, `codegen/method.rs`, `codegen/function.rs`,
///     `codegen/closure.rs` — `TempRootPool::default()` at each `FnCtx`
///     construction. Constructing the pool is not using it.
///   * `stmt/loops.rs` — one call to `expr_is_inert_primitive`, a purity
///     predicate for the back-edge poll.
///   * `loop_purity.rs` — a doc link and nothing else (zero code sites; the
///     brief's count included the comment).
///   * `root_reload.rs`, `gc_call_effects.rs`, `runtime_decls/arrays.rs` —
///     the STRING LITERALS `"js_gc_temp_root_push"` and friends, which name
///     runtime symbols, not this module. So do five test files
///     (`expr/slice7_rooting_tests.rs`, `lower_call/console_rooting_tests.rs`,
///     `lower_call/timer_rooting_tests.rs`,
///     `codegen/testing_feature_gate_tests.rs`) plus
///     `linker_temp_lifecycle_tests.rs`, whose `temp_root_if_clang_available`
///     is about a temporary DIRECTORY.
///
/// ## The three unverified leads slice 7 handed over
///
///   * `static_field_meta.rs`'s `caps_arr` — **a real accumulator shape with a
///     provably empty window.** The array holds the only reference to
///     everything pushed so far while the next element is lowered, which is
///     #6951 exactly; but `captured_args` is built at one site
///     (`lower/lower_expr/arm_class.rs`) as
///     `ids.iter().map(|id| Expr::LocalGet(*id))`, and `expr_may_trigger_gc`
///     answers `false` for every `LocalGet`. So it is rooted through
///     [`with_rooted_accumulator`] with `protect` computed rather than
///     assumed: today that is `false` and the IR is byte for byte unchanged,
///     and the day a non-inert expression reaches the list it is rooted by
///     construction.
///   * `math_simple.rs`'s `ArrayMap` — **CONFIRMED live.** The receiver was
///     lowered, `callback` was lowered, and only then was the receiver
///     unboxed: `unbox_to_i64` below its own window masks a stale box rather
///     than repairing it (#7280 taxonomy (c)). `arr.map(x => …)` allocates a
///     closure at minimum. Fixed via [`with_operands_rooted`].
///   * `dyn_extern_i18n.rs`'s `path_handle` — **DISMISSED, and the premise is
///     wrong about the CFG.** The lead says the raw handle is reused across a
///     compare loop "that runs module `__init` bodies". It does not: each
///     `<prefix>__init()` is emitted into that iteration's MATCH block, which
///     branches straight to the join, so no `__init` dominates any later use
///     of `path_handle`. Along the fallthrough chain the only emissions
///     between the handle's production and its last use are
///     `js_get_string_pointer_unified` and `js_string_equals` — neither
///     re-enters user code nor enumerates an object, which is the standard
///     [`with_operands_rooted_across_call`]'s doc sets for an emitted step
///     (#7198). What that module DID have was the namespace-object
///     accumulator, which is migrated above.
#[cfg(test)]
const MIGRATED_MODULES: &[(&str, &str)] = &[
    (
        "crates/perry-codegen/src/expr/url_main.rs",
        include_str!("../expr/url_main.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_array_method.rs",
        include_str!("../lower_array_method.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/arrays_finds.rs",
        include_str!("../expr/arrays_finds.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_methods.rs",
        include_str!("../expr/array_methods.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/instance_misc1.rs",
        include_str!("../expr/instance_misc1.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/logical_collections.rs",
        include_str!("../expr/logical_collections.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/property_get/map_set.rs",
        include_str!("../lower_call/property_get/map_set.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/objects_arrays_lit.rs",
        include_str!("../expr/objects_arrays_lit.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_literal.rs",
        include_str!("../expr/array_literal.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/object_literal.rs",
        include_str!("../expr/object_literal.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/array_push.rs",
        include_str!("../expr/array_push.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get.rs",
        include_str!("../expr/index_get.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get/guarded_array.rs",
        include_str!("../expr/index_get/guarded_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_get/inline_dyn_typed_array.rs",
        include_str!("../expr/index_get/inline_dyn_typed_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_set.rs",
        include_str!("../expr/index_set.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/index_set_typed_array.rs",
        include_str!("../expr/index_set_typed_array.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get.rs",
        include_str!("../expr/property_get.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get/globalget.rs",
        include_str!("../expr/property_get/globalget.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_get/helpers.rs",
        include_str!("../expr/property_get/helpers.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/property_set.rs",
        include_str!("../expr/property_set.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/extern_timers.rs",
        include_str!("../lower_call/extern_timers.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/namespace_call.rs",
        include_str!("../lower_call/namespace_call.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/mod.rs",
        include_str!("../lower_call/mod.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/func_ref.rs",
        include_str!("../lower_call/func_ref.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/console_promise.rs",
        include_str!("../lower_call/console_promise.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/child_proc.rs",
        include_str!("../expr/child_proc.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/proxy_reflect.rs",
        include_str!("../expr/proxy_reflect.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/fs_await.rs",
        include_str!("../expr/fs_await.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/binary.rs",
        include_str!("../expr/binary.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/math_simple.rs",
        include_str!("../expr/math_simple.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/static_field_meta.rs",
        include_str!("../expr/static_field_meta.rs"),
    ),
    (
        "crates/perry-codegen/src/expr/dyn_extern_i18n.rs",
        include_str!("../expr/dyn_extern_i18n.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_string_method.rs",
        include_str!("../lower_string_method.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_string_concat.rs",
        include_str!("../lower_string_concat.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/new.rs",
        include_str!("../lower_call/new.rs"),
    ),
    (
        "crates/perry-codegen/src/lower_call/new_alloc.rs",
        include_str!("../lower_call/new_alloc.rs"),
    ),
];

/// `rooting/temp_root.rs`, inlined at compile time for the terminal-condition
/// test below.
#[cfg(test)]
const RAW_ROOTING_API_SRC: &str = include_str!("temp_root.rs");

/// The two items in `temp_root.rs` that are allowed to stay `pub(crate)`.
///
/// Neither is an accessor. Adding to this list is how the campaign's terminal
/// condition would be given back, so it is spelled out rather than derived.
#[cfg(test)]
const RAW_API_PUBLIC_EXCEPTIONS: &[&str] = &["struct TempRootPool", "fn expr_is_inert_primitive"];

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
    use super::{
        escape_hatch_uses, MIGRATED_MODULES, RAW_API_PUBLIC_EXCEPTIONS, RAW_ROOTING_API_SRC,
    };

    /// Every `pub`-ish item declared in `temp_root.rs`, as
    /// `("pub(crate)" | "pub(in crate::rooting)" | "pub", "fn name")`.
    fn declared_items(src: &str) -> Vec<(String, String)> {
        src.lines()
            .filter_map(|line| {
                let code = line.split("//").next().unwrap_or(line).trim_start();
                for vis in ["pub(in crate::rooting) ", "pub(crate) ", "pub "] {
                    if let Some(rest) = code.strip_prefix(vis) {
                        let mut it = rest.split_whitespace();
                        let kind = it.next()?;
                        if !matches!(kind, "fn" | "struct" | "enum" | "mod" | "const" | "type") {
                            return None;
                        }
                        let name = it.next()?.split(['(', '<', '{', ':']).next()?.to_string();
                        return Some((vis.trim().to_string(), format!("{kind} {name}")));
                    }
                }
                None
            })
            .collect()
    }

    /// **The campaign's terminal condition** (#7615): the raw rooting API is
    /// unreachable outside `crate::rooting`, not merely unnamed.
    ///
    /// The ledger above can only report what a module NAMES, which is why this
    /// is a separate assertion rather than a stronger phrasing of that one.
    /// Both halves are checked, because either alone can be undone by one
    /// keyword: the module declaration must stay private, and every accessor
    /// must carry `pub(in crate::rooting)` so re-opening the module does not
    /// silently widen twenty-five items at once.
    #[test]
    fn the_raw_rooting_api_is_unreachable_outside_this_module() {
        let items = declared_items(RAW_ROOTING_API_SRC);
        assert!(
            items.len() > 15,
            "expected temp_root.rs to declare the raw API; found {} items — the \
             include_str! target moved and this check is measuring nothing",
            items.len()
        );
        let widened: Vec<&(String, String)> = items
            .iter()
            .filter(|(vis, item)| {
                vis != "pub(in crate::rooting)" && !RAW_API_PUBLIC_EXCEPTIONS.contains(&&**item)
            })
            .collect();
        assert!(
            widened.is_empty(),
            "the Layer 1 campaign's terminal condition is that every accessor in \
             rooting/temp_root.rs is pub(in crate::rooting). These are not, and \
             are not on the two-item exception list:\n{}",
            widened
                .iter()
                .map(|(vis, item)| format!("  {vis} {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let decl = include_str!("mod.rs");
        assert!(
            decl.contains("\nmod temp_root;\n"),
            "rooting/temp_root.rs must be declared with a PRIVATE `mod temp_root;` — \
             a pub(crate) module would make every item in it reachable crate-wide \
             regardless of its own visibility"
        );
    }

    /// Sabotage duty for the terminal-condition check: a widened accessor must
    /// be reported, and the two allowed exceptions must not be.
    #[test]
    fn the_terminal_condition_check_reports_a_widened_accessor() {
        let planted = "\
pub(crate) fn temp_root_push_i64(ctx: &mut FnCtx<'_>, v: &str) -> String {}
pub(in crate::rooting) fn temp_root_truncate(ctx: &mut FnCtx<'_>, idx: &str) {}
pub(crate) struct TempRootPool {}
pub(crate) fn expr_is_inert_primitive(ctx: &FnCtx<'_>, e: &Expr) -> bool {}
";
        let items = declared_items(planted);
        assert_eq!(items.len(), 4, "parsed {items:?}");
        let widened: Vec<&(String, String)> = items
            .iter()
            .filter(|(vis, item)| {
                vis != "pub(in crate::rooting)" && !RAW_API_PUBLIC_EXCEPTIONS.contains(&&**item)
            })
            .collect();
        assert_eq!(
            widened.len(),
            1,
            "exactly the widened accessor must be reported, got {widened:?}"
        );
        assert_eq!(widened[0].1, "fn temp_root_push_i64");
    }

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
