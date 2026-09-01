//! Receiver regions — one vocabulary for "receiver R has fact F, valid until
//! boundary B" (#9254).
//!
//! # Why this exists
//!
//! Phase 1 found sixteen separate receiver-keyed fact mechanisms on `FnCtx`
//! (`cached_lengths`, `bounded_index_pairs`, `packed_f64_loop_facts`,
//! `masked_window_array_facts`, `buffer_view_slots`, `int_range_facts`,
//! `element_shape_loop_facts`, `class_field_loop_facts`,
//! `versioned_indexed_loop_facts`, `stable_packed_loop_facts`,
//! `string_window_array_facts`, `buffer_data_slots`, the two class-shape slot
//! maps and the `packed_receiver_*` trio). Each answers the same two questions
//! — *what do we know about this receiver* and *how long may we believe it* —
//! and each answers the second question in a different, hand-rolled way:
//!
//! | mechanism | tables |
//! |---|---|
//! | `retain(|f| f.scope_id != id)` at scope exit | `bounded_index_pairs`, `packed_f64_loop_facts`, `masked_window_array_facts` |
//! | insert/remove pair with no id | `cached_lengths`, `packed_receiver_*` |
//! | mutable field downgraded in place, never removed | `buffer_view_slots` |
//! | reloaded at the safepoint instead of invalidated | `packed_receiver_*` |
//!
//! and a fifth boundary — the **unwind edge** — is expressed by none of them.
//! It is honoured today only indirectly: the packed matcher rejects
//! `Stmt::Try` outright (`stmt/loops.rs`), masked-window regions consult
//! `ctx.try_depth` before privatising, and `flush_packed_accumulator_locals`
//! writes loop-carried accumulators back at the throw site (#9185/#9210).
//! Every one of those is a local decision by one tier. A tier added tomorrow
//! inherits none of them.
//!
//! # What this module is
//!
//! The model plus the active descriptor table. Phase 1 added the boundary
//! vocabulary and equivalence lint without changing lowering. Phase 2 routes
//! the packed/versioned clone's receiver hoist through
//! [`ReceiverDescriptorTable`]: the table owns the rooted box, pre-masked base
//! handle and poll refresh recipe as one entry, and asks [`boundary_admits`]
//! before carrying that address across a back-edge poll. Other fact tables are
//! migrated one consumer at a time.
//!
//! The precedent is `TypeFacts::purity` / `TypeFacts::shape_stability`
//! (`collectors/hir_facts.rs`, #854): a subgraph the collector populates and
//! no pass yet consumes.
//!
//! # The conservative direction
//!
//! A region is a run of code across which no object can be *relocated*. The
//! safe error is to report **too many** enders: more enders means shorter
//! regions means fewer facts believed for less time. That is the same bias
//! `collectors::safepoint_sites` takes for a different consumer ("an
//! over-approximation biased toward spilling"), and the same one-sided
//! contract `loop_purity::loop_may_allocate` states for itself — `false` must
//! mean *provably* cannot allocate. `region_enders_in_stmts` is asserted equal
//! to `loop_may_allocate` on a shared battery in the phase-1 tests, which is
//! what keeps this file honest: the model is checked against a shipping,
//! audited predicate rather than against its own restatement.

// #9254 phase 2: `ReceiverDescriptorTable` and the poll boundary algebra are
// production consumers. Region formation remains lint-only until ordinary
// counted loops migrate in phase 3, so the rest of this module is still dead
// in a non-test build. Keep that incomplete state explicit rather than
// scattering per-item allows; this attribute can go when region formation is
// itself consumed.
#![allow(dead_code)]

use crate::loop_purity;
use perry_hir::{CompareOp, Expr, Stmt, UnaryOp};

/// Why a no-relocation region ends.
///
/// Derived from the collection points codegen actually has. Ordering is by
/// how hard the ender is to see in source, not by severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionEnder {
    /// A call whose direct callee is not on the audited non-collecting
    /// allowlist (`gc_call_effects::classify_direct_callee` → `Unknown`), a
    /// call to a module function not proven leaf by the transitive closure, or
    /// any indirect call. The overwhelming majority of enders.
    CollectingCall,
    /// An allocating literal, property store or index store: these lower to a
    /// runtime helper that allocates and can collect. Separate from
    /// `CollectingCall` because they carry no callee name in source and are
    /// the ones #8583 found invisible.
    AllocatingOperation,
    /// A coercing operator over an operand not proven a non-pointer primitive.
    /// `ToPrimitive` dispatches to a user `valueOf` / `Symbol.toPrimitive` /
    /// `toString`, which is arbitrary JS: it allocates, and it collects.
    Coercion,
    /// `await`, `yield`, or an async-first call: control reaches the microtask
    /// pump, whose outermost boundary runs the moving minor collection.
    Suspension,
    /// A `throw`, or the unwind successor of an `invoke`. The throw helpers
    /// allocate the Error they raise, so the handler's roots are relocated —
    /// and, unlike every other ender, this one leaves the *fall-through* path
    /// untouched, which is why a tier can be accidentally correct on it for
    /// years (#9185).
    UnwindEdge,
    /// A loop back-edge GC poll. Deliberately placed, and the only collection
    /// point inside an otherwise call-free fast clone.
    BackEdgePoll,
    /// An expression this model does not classify. Reported as an ender
    /// because the default must be "assume it collects": the allowlist below
    /// is what has been argued sound, and everything else — a closure
    /// allocation, a template literal, a spread, a regex, a `PropertyGet` that
    /// may reach a getter — has not been.
    Unmodelled,
}

impl RegionEnder {
    /// Whether the ender can relocate objects *without* transferring control
    /// out of the region's fall-through path.
    ///
    /// `UnwindEdge` is the one that cannot: it diverts. A fact consulted only
    /// on the fall-through path is unaffected by it, which is exactly why
    /// unwind safety is so easy to get accidentally right — and why a fact
    /// that is *written back* at region exit (a loop-carried accumulator) is
    /// not, because the unwind edge skips that exit block.
    pub(crate) fn is_fallthrough(self) -> bool {
        !matches!(self, RegionEnder::UnwindEdge)
    }
}

/// How a fact table expresses the extent of its claim.
///
/// One variant per mechanism found in the fifteen tables. These are
/// descriptions of what is implemented today, not a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FactBoundary {
    /// `retain(|f| f.scope_id != id)` at the exit of the scope that pushed it.
    ScopeId,
    /// An insert/remove (or push/pop) pair around one lowering call, with no
    /// identifier — validity is the dynamic extent of that call.
    DynamicExtent,
    /// Never removed. A mutable field on the entry is downgraded at each
    /// hazard instead (`AliasState::MayAlias`,
    /// `BufferViewPointerState::Invalidated`).
    InPlaceDegradation,
    /// Reloaded from the authoritative root at the safepoint, rather than
    /// invalidated. `receiver_descriptors` is the first consumer.
    PollRefresh,
    /// Nothing removes or downgrades it.
    Never,
}

/// What a table claims about a receiver. The axis that matters for boundary
/// checking is whether the claim names an *address*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverClaim {
    /// A `length` value, an index range, or any other arithmetic relation. A
    /// moving collection changes an object's address, never its length, so
    /// these survive relocation by content.
    ScalarRelation,
    /// The receiver's element representation (packed raw f64, dense i32, a
    /// proven element shape). Survives relocation — it is a property of the
    /// object, not of where it lives — but dies at anything that can *mutate*
    /// the receiver, which is a strictly larger set than the enders here.
    Representation,
    /// A cached raw pointer, box, or masked handle. Relocation invalidates it
    /// outright; this is the only claim for which a region boundary is
    /// load-bearing rather than incidental.
    Address,
}

/// One table's claim about one receiver, in the shared vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverDescriptor {
    /// The fact table this descriptor was normalised from, for diagnostics.
    pub(crate) table: &'static str,
    /// The receiver's `LocalId`.
    pub(crate) receiver: u32,
    pub(crate) claim: ReceiverClaim,
    pub(crate) boundary: FactBoundary,
    /// Whether the tier that owns this table structurally excludes `Stmt::Try`
    /// from the region it forms (the packed matcher does; `buffer_view_slots`
    /// has no region at all).
    pub(crate) excludes_try: bool,
}

/// Why a descriptor's declared boundary does not cover an ender present in its
/// region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundaryViolation {
    pub(crate) table: &'static str,
    pub(crate) receiver: u32,
    pub(crate) ender: RegionEnder,
    pub(crate) why: &'static str,
}

/// The core rule: may `desc` still be believed across `ender`?
///
/// Read this as the one place the boundary algebra lives. Every tier
/// implements some projection of it by hand today.
pub(crate) fn boundary_admits(
    desc: &ReceiverDescriptor,
    ender: RegionEnder,
) -> Result<(), BoundaryViolation> {
    let deny = |why| {
        Err(BoundaryViolation {
            table: desc.table,
            receiver: desc.receiver,
            ender,
            why,
        })
    };

    match desc.claim {
        // A length or an index range is a value. Relocation does not touch it.
        // It dies at mutation, which no ender in this enum implies on its own.
        ReceiverClaim::ScalarRelation => Ok(()),

        // A representation claim survives relocation but not arbitrary user
        // code, which can convert the receiver's storage out from under it.
        ReceiverClaim::Representation => match ender {
            RegionEnder::BackEdgePoll => Ok(()),
            RegionEnder::CollectingCall
            | RegionEnder::AllocatingOperation
            | RegionEnder::Coercion
            | RegionEnder::Suspension
            | RegionEnder::Unmodelled => {
                deny("user code reachable from this ender can change the receiver's storage kind")
            }
            RegionEnder::UnwindEdge => {
                if desc.excludes_try {
                    Ok(())
                } else {
                    deny("representation claim may be consulted in a handler the tier never gated")
                }
            }
        },

        // An address dies at every relocation, with exactly one exception.
        ReceiverClaim::Address => match (ender, desc.boundary) {
            // The poll is the one ender a refresh recipe is written for.
            (RegionEnder::BackEdgePoll, FactBoundary::PollRefresh) => Ok(()),
            (RegionEnder::BackEdgePoll, _) => {
                deny("cached address is not reloaded at the back-edge poll that may have moved it")
            }
            // The unwind edge diverts, so a read-only cache consulted only on
            // the fall-through path is unharmed — but only if the tier
            // actually excluded `Try` from its region.
            (RegionEnder::UnwindEdge, _) if desc.excludes_try => Ok(()),
            (RegionEnder::UnwindEdge, _) => deny(
                "cached address may be consulted in a handler reached after a relocating throw",
            ),
            _ => deny("cached address does not survive this relocation point"),
        },
    }
}

/// The concrete refresh recipe carried by a materialised address descriptor.
///
/// The box slot is a precise frame root, so evacuation rewrites it directly.
/// `source_root` remains the authoritative binding used by #9111's poll-arm
/// reload, and `base_handle_slot` caches `box & POINTER_MASK` for packed lanes.
/// Keeping the three names in one value prevents the parallel-map drift this
/// module was introduced to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverPollRefresh {
    pub(crate) rooted_box_slot: String,
    pub(crate) base_handle_slot: String,
    pub(crate) source_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveReceiverDescriptor {
    contract: ReceiverDescriptor,
    refresh: ReceiverPollRefresh,
}

/// Active materialised receiver descriptors for one function lowering.
///
/// Entries are kept in installation order so refresh IR is deterministic.
/// Nested packed clones reuse an outer entry for the same receiver; a scope
/// removes only the entries it installed itself.
#[derive(Debug, Default)]
pub(crate) struct ReceiverDescriptorTable {
    entries: Vec<ActiveReceiverDescriptor>,
}

impl ReceiverDescriptorTable {
    /// Whether an active scope has already materialised `receiver`.
    pub(crate) fn contains(&self, receiver: u32) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.contract.receiver == receiver)
    }

    /// Install the first production descriptor consumer (#9254 phase 2): a
    /// rooted receiver address refreshed at every fired back-edge poll.
    ///
    /// Returns `false` for a duplicate receiver. Callers use that to reuse an
    /// outer clone's descriptor instead of shadowing and prematurely removing
    /// it when the inner clone ends.
    pub(crate) fn materialize_poll_refreshed_address(
        &mut self,
        receiver: u32,
        rooted_box_slot: String,
        base_handle_slot: String,
        source_root: String,
        excludes_try: bool,
    ) -> bool {
        if self.contains(receiver) {
            return false;
        }
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors",
            receiver,
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::PollRefresh,
            excludes_try,
        };
        // This is not a descriptive side table any more: installing an active
        // address requires the shared boundary algebra to license the exact
        // relocation point its refresh recipe covers.
        boundary_admits(&contract, RegionEnder::BackEdgePoll)
            .expect("a poll-refreshed receiver descriptor must survive its poll boundary");
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            refresh: ReceiverPollRefresh {
                rooted_box_slot,
                base_handle_slot,
                source_root,
            },
        });
        true
    }

    /// End the dynamic extent of one materialised receiver.
    pub(crate) fn dematerialize(&mut self, receiver: u32) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.contract.receiver == receiver)
        else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    /// The promotable, precise-root box slot consumed by `LocalGet`.
    pub(crate) fn rooted_box_slot(&self, receiver: u32) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.contract.receiver == receiver)
            .map(|entry| entry.refresh.rooted_box_slot.as_str())
    }

    /// The pre-masked base-handle slot consumed by packed address math.
    pub(crate) fn base_handle_slot(&self, receiver: u32) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.contract.receiver == receiver)
            .map(|entry| entry.refresh.base_handle_slot.as_str())
    }

    /// Refresh recipes admitted at a back-edge poll.
    ///
    /// Every active entry is checked at the boundary before its recipe is
    /// returned. A future consumer that installs an address under the wrong
    /// boundary therefore fails compilation here instead of silently carrying
    /// a stale pointer past a collection.
    pub(crate) fn poll_refreshes(&self) -> Result<Vec<ReceiverPollRefresh>, BoundaryViolation> {
        let mut refreshes = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            boundary_admits(&entry.contract, RegionEnder::BackEdgePoll)?;
            refreshes.push(entry.refresh.clone());
        }
        Ok(refreshes)
    }
}

/// Check one descriptor against every ender in its region.
pub(crate) fn violations_for(
    desc: &ReceiverDescriptor,
    enders: &[RegionEnder],
) -> Vec<BoundaryViolation> {
    enders
        .iter()
        .filter_map(|&e| boundary_admits(desc, e).err())
        .collect()
}

/// Classify a single expression as a region ender.
///
/// `is_inert` proves an operand is a non-pointer primitive — the same
/// injected predicate `loop_purity::loop_may_allocate` takes, so that the two
/// can be held to the same answer (see the phase-1 equivalence test).
///
/// Returns the *first* reason found; an expression can qualify several ways
/// and the caller only needs to know the region ends.
pub(crate) fn expr_region_ender(e: &Expr, is_inert: &dyn Fn(&Expr) -> bool) -> Option<RegionEnder> {
    match e {
        // ---- Provably not a relocation point -------------------------------
        // Constants, reads of a local/global, and references. No dispatch.
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::This
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::EnumMember { .. } => None,

        // Typed-array and buffer element access: a fixed-layout numeric load,
        // or a store into a backing buffer that never grows. Mirrors
        // `loop_purity::expr_alloc_free`.
        Expr::BufferIndexGet { .. }
        | Expr::Uint8ArrayGet { .. }
        | Expr::BufferIndexSet { .. }
        | Expr::Uint8ArraySet { .. } => None,

        // `===` / `!==` never coerce; `&&` / `||` / `??` run only ToBoolean,
        // which on an object is a tag test. `!x`, `typeof x`, `void x` reach
        // no user-defined conversion. All stay open to operands of any type.
        Expr::Compare {
            op: CompareOp::Eq | CompareOp::Ne,
            ..
        }
        | Expr::Logical { .. }
        | Expr::Unary {
            op: UnaryOp::Not, ..
        }
        | Expr::TypeOf(_)
        | Expr::Void(_) => None,

        // Pure plumbing; the interesting part is in the children, which the
        // caller walks.
        Expr::LocalSet(..) | Expr::Conditional { .. } => None,

        // ---- Coercing operators --------------------------------------------
        // Relational/loose comparison, arithmetic and bitwise `Binary`, the
        // remaining `Unary` forms and `x++` / `x--` all run ToPrimitive /
        // ToNumeric, and a user-defined `valueOf` / `Symbol.toPrimitive` /
        // `toString` is arbitrary JS: it allocates, and it collects.
        //
        // `is_inert` is applied to the WHOLE node, not to the operands, which
        // is what `loop_purity` does. Recursing into operands does not see the
        // dispatch — `a < b` over two plain locals recurses clean while the
        // comparison itself can call user code, the hole #6975 closed one
        // abstraction over.
        Expr::Compare { .. } | Expr::Binary { .. } | Expr::Unary { .. } | Expr::Update { .. } => {
            if is_inert(e) {
                None
            } else {
                Some(RegionEnder::Coercion)
            }
        }

        // ---- Suspension ----------------------------------------------------
        // Control reaches the microtask pump, whose outermost boundary runs
        // the moving minor collection.
        Expr::Await(_) | Expr::Yield { .. } | Expr::AsyncFirstCall { .. } => {
            Some(RegionEnder::Suspension)
        }

        // ---- Calls ---------------------------------------------------------
        // Phase 1 does not consult `gc_call_effects::classify_direct_callee`:
        // that keys off an emitted IR symbol name and this is an HIR walk, so
        // every call is an ender. Conservative, and the direction that keeps
        // the soundness implication below exact. Phase 2 is where a direct
        // call to an audited non-collecting helper stops ending a region.
        Expr::Call { .. }
        | Expr::CallSpread { .. }
        | Expr::NativeMethodCall { .. }
        | Expr::StaticMethodCall { .. }
        | Expr::SuperCall(_)
        | Expr::SuperCallSpread(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::ObjectSuperMethodCall { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::NewDynamicSpread { .. } => Some(RegionEnder::CollectingCall),

        // ---- Allocating operations with no callee in source ----------------
        // #8583's blind spot: these lower to an allocating runtime helper that
        // RS4GC gives a statepoint, but carry no callee name.
        Expr::Object(_)
        | Expr::ObjectSpread { .. }
        | Expr::ObjectAssign { .. }
        | Expr::Array(_)
        | Expr::ArraySpread(_)
        | Expr::Closure { .. }
        | Expr::PropertySet { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::IndexSet { .. }
        | Expr::IndexUpdate { .. } => Some(RegionEnder::AllocatingOperation),

        // ---- Reads that can reach user code --------------------------------
        // A generic property or index READ can hit an accessor or a proxy
        // trap, both arbitrary JS. `collectors::safepoint_sites` deliberately
        // does NOT count these, because over-counting reads would over-spill a
        // read-heavy hot loop and its consumer only needs a spill estimate. A
        // region model has the opposite obligation: missing one licenses a
        // stale cached address. `loop_purity` excludes them for the same
        // reason, via its `_ => false` arm.
        Expr::PropertyGet { .. } | Expr::IndexGet { .. } => Some(RegionEnder::CollectingCall),

        // ---- Everything else -----------------------------------------------
        // Assume it collects. Adding a variant to the allowlist above requires
        // an argument; landing a new HIR variant does not silently widen a
        // region.
        _ => Some(RegionEnder::Unmodelled),
    }
}

/// Every region ender reachable from `stmts` and `controls`, without
/// descending into nested closures (a closure is its own frame, and its
/// regions are its own).
///
/// Duplicates are preserved: the count is meaningful to a caller forming
/// maximal regions, and de-duplicating would hide a second ender of the same
/// kind at a different point.
pub(crate) fn region_enders_in_stmts(
    stmts: &[Stmt],
    controls: &[&Expr],
    is_inert: &dyn Fn(&Expr) -> bool,
) -> Vec<RegionEnder> {
    let mut out = Vec::new();
    for s in stmts {
        enders_in_stmt(s, is_inert, &mut out);
    }
    for c in controls {
        enders_in_expr(c, is_inert, &mut out);
    }
    out
}

fn enders_in_expr(e: &Expr, is_inert: &dyn Fn(&Expr) -> bool, out: &mut Vec<RegionEnder>) {
    // Child expressions execute before the operation represented by their
    // parent (`f(makeClosure())` allocates the closure before it calls `f`).
    // Region boundaries are ordered data once a lowering path consumes them,
    // so a pre-order walk would put the call before the allocation.
    perry_hir::walker::walk_expr_children(e, &mut |child| enders_in_expr(child, is_inert, out));
    if let Some(r) = expr_region_ender(e, is_inert) {
        out.push(r);
    }
}

fn enders_in_stmt(s: &Stmt, is_inert: &dyn Fn(&Expr) -> bool, out: &mut Vec<RegionEnder>) {
    match s {
        // A throw is an unwind edge *and* the helper allocates the Error.
        Stmt::Throw(e) => {
            enders_in_expr(e, is_inert, out);
            out.push(RegionEnder::UnwindEdge);
        }
        Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            enders_in_expr(e, is_inert, out)
        }
        Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            enders_in_expr(condition, is_inert, out);
            for st in then_branch {
                enders_in_stmt(st, is_inert, out);
            }
            if let Some(else_branch) = else_branch {
                for st in else_branch {
                    enders_in_stmt(st, is_inert, out);
                }
            }
        }
        // A nested loop's back-edge poll is a relocation point for the
        // *enclosing* region too — this is why the armed-poll refresh reloads
        // every active receiver cache, not just the innermost scope's.
        Stmt::While { condition, body } => {
            enders_in_expr(condition, is_inert, out);
            for st in body {
                enders_in_stmt(st, is_inert, out);
            }
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::DoWhile { body, condition } => {
            for st in body {
                enders_in_stmt(st, is_inert, out);
            }
            enders_in_expr(condition, is_inert, out);
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                enders_in_stmt(init, is_inert, out);
            }
            if let Some(condition) = condition {
                enders_in_expr(condition, is_inert, out);
            }
            for st in body {
                enders_in_stmt(st, is_inert, out);
            }
            if let Some(update) = update {
                enders_in_expr(update, is_inert, out);
            }
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::Labeled { body, .. } => enders_in_stmt(body, is_inert, out),
        // Every statement in a `try` body may divert to the handler.
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body {
                enders_in_stmt(st, is_inert, out);
            }
            out.push(RegionEnder::UnwindEdge);
            if let Some(catch) = catch {
                for st in &catch.body {
                    enders_in_stmt(st, is_inert, out);
                }
            }
            if let Some(finally) = finally {
                for st in finally {
                    enders_in_stmt(st, is_inert, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            enders_in_expr(discriminant, is_inert, out);
            for c in cases {
                if let Some(t) = &c.test {
                    enders_in_expr(t, is_inert, out);
                }
                for st in &c.body {
                    enders_in_stmt(st, is_inert, out);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

/// The phase-1 equivalence check.
///
/// `loop_purity::loop_may_allocate` is the shipping, audited answer to "can
/// this loop body reach a collection point". The model must be **no weaker**
/// than it:
///
/// > if the model finds no relocation point other than a back-edge poll, then
/// > `loop_may_allocate` must also have proven the body alloc-free.
///
/// That is the direction with teeth. The model's whole purpose is to license
/// believing a fact across a span of code; a span the model calls clean and
/// `loop_may_allocate` does not is either a real hole in the model or a real
/// imprecision in `loop_may_allocate`, and phase 1 exists to find out which
/// before a lowering path depends on the answer. Writing this file the
/// obvious way — enumerate the enders, default to "safe" — put three such
/// holes in it (generic `IndexGet`/`PropertyGet` reaching an accessor,
/// `Expr::Closure` allocating, and `is_inert` applied per-operand instead of
/// to the whole coercing node). Inverting the match to an allowlist with an
/// `Unmodelled` catch-all is what closes that class.
///
/// **The converse is deliberately not asserted.** `loop_may_allocate` answers
/// `true` for any statement it does not model — `Return`, `Switch`, `Throw`,
/// `Try` all fall to its `_ => false` arm — which is imprecision, not a
/// collection point. Requiring equality would force this model to inherit
/// that imprecision and would make a `return` inside a region end it, which is
/// wrong.
pub(crate) fn model_is_no_weaker_than_loop_purity(
    body: &[Stmt],
    controls: &[&Expr],
    is_inert: &dyn Fn(&Expr) -> bool,
) -> bool {
    let enders = region_enders_in_stmts(body, controls, is_inert);
    let model_finds_relocation = enders
        .iter()
        .any(|e| !matches!(e, RegionEnder::BackEdgePoll));
    // no relocation found  =>  loop_purity proved it alloc-free
    model_finds_relocation || !loop_purity::loop_may_allocate(body, controls, is_inert)
}
