//! Layer 1 prototype: rooting by construction (`docs/src/internals/rfc-rooting-by-construction.md`).
//!
//! **Status: prototype, not yet on any lowering path.** It exists to settle the
//! one question the RFC could not answer on paper — *does the borrow checker
//! actually reject the bug shape?* — before anyone pays the migration cost of
//! threading these types through `perry-codegen`. The `compile_fail` doctests
//! below are the answer, and they are executed by `cargo test`, so this claim
//! cannot rot into prose the way the RFC's example could.
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
//! authority; this prototype does not widen it.

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
// across_*`): never hand out an unrooted handle at all. `call_rooted` emits the
// collecting call and roots its result in one step, so there is no window in
// which an unrooted register exists to be misused, and `read` re-reads through
// the slot every time.
//
// This is weaker than the borrow formulation -- it prevents the bug rather than
// detecting attempts to write it -- but it needs no emitter rewrite, which is
// what makes it migratable one call site at a time.
// ---------------------------------------------------------------------------

use crate::expr::FnCtx;

/// A slot holding a GC-managed pointer for the duration of a lowering.
#[derive(Debug, Clone)]
pub struct RootedSlot {
    idx: String,
}

impl RootedSlot {
    /// Re-read the slot. Called afresh at every use: the returned register is
    /// only valid until the next emission that can collect, and re-reading is
    /// cheaper than reasoning about whether one has happened.
    pub fn read(&self, ctx: &mut FnCtx<'_>) -> String {
        crate::expr::temp_root::temp_root_get_i64(ctx, &self.idx)
    }

    /// Release the slot. Call after the last [`RootedSlot::read`].
    pub fn release(self, ctx: &mut FnCtx<'_>) {
        crate::expr::temp_root::temp_root_truncate(ctx, &self.idx);
    }
}

/// Emit a call that can collect and root its result in one step.
///
/// The point is what this function does NOT return: an unrooted register. A
/// caller cannot hold the result across a later collection point because it
/// never has the result -- only a slot -- which is what makes the #7453 shape
/// unwritable here rather than merely reviewable.
pub fn call_rooted(
    ctx: &mut FnCtx<'_>,
    ret_ty: crate::types::LlvmType,
    callee: &str,
    args: &[(crate::types::LlvmType, &str)],
) -> RootedSlot {
    let reg = ctx.block().call(ret_ty, callee, args);
    let idx = crate::expr::temp_root::temp_root_push_i64(ctx, &reg);
    RootedSlot { idx }
}
