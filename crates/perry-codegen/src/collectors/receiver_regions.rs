//! Receiver regions — one vocabulary for "receiver R has fact F, valid until
//! boundary B" (#9254).
//!
//! # Why this exists
//!
//! Phase 1 found sixteen separate receiver-keyed fact mechanisms on `FnCtx`.
//! The original issue singled out six (`cached_lengths`,
//! `bounded_index_pairs`, `packed_f64_loop_facts`,
//! `masked_window_array_facts`, `buffer_view_slots`, and the
//! `packed_receiver_*` trio); Phase 4 has now moved all six into this table.
//! The expanded audit also records `int_range_facts`,
//! `bounded_buffer_index_pairs`, `guarded_buffer_index_pairs`,
//! `element_shape_loop_facts`, `class_field_loop_facts`,
//! `versioned_indexed_loop_facts`, `stable_packed_loop_facts`,
//! `string_window_array_facts`, `buffer_data_slots`, and `class_keys_slots`.
//! Historically, each answered the same two questions
//! — *what do we know about this receiver* and *how long may we believe it* —
//! and each answers the second question in a different, hand-rolled way:
//!
//! | mechanism | tables |
//! |---|---|
//! | `retain(|f| f.scope_id != id)` at scope exit | `receiver_descriptors[bounded_index]` / `[packed_f64_loop]` / `[masked_window_array]` (formerly three independent tables) |
//! | insert/remove pair with no id | `receiver_descriptors[cached_length]` and the base address payload (formerly `cached_lengths` and `packed_receiver_*`) |
//! | mutable field downgraded in place, never removed | `receiver_descriptors[buffer_view]` (formerly `buffer_view_slots`) |
//! | reloaded at the safepoint instead of invalidated | base address payload (formerly `packed_receiver_*`) |
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
//! before carrying that address across a back-edge poll. Phase 3 lets ordinary
//! counted loops attach a conditional plain/numeric-array validation to the
//! same entry, but only after this module proves the loop region contains no
//! ender other than the poll covered by that refresh recipe. Phase 4 folds the
//! five remaining mechanisms named by the proposal into this table: cached
//! lengths, bounded indices, packed and masked representations, and
//! non-moving buffer views.
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

// `ReceiverDescriptorTable`, the poll boundary algebra and region formation
// are production consumers. Inventory/lint helpers remain test-only, so keep
// their allowance central rather than scattering per-item attributes.
#![allow(dead_code)]

use crate::expr::{MaskedWindowArrayFact, PackedF64LoopFact};
use crate::loop_purity;
use crate::native_value::BufferViewSlot;
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
    /// A raw address into storage whose allocation is explicitly non-moving.
    /// Collection and unwind do not stale it; receiver reassignment, backing
    /// replacement, disposal and alias escape are handled by descriptor-table
    /// degradation APIs instead.
    NonMovingAddress,
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
    /// from the region it forms (the packed matcher does; a buffer-view
    /// descriptor has no region at all).
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
        ReceiverClaim::ScalarRelation | ReceiverClaim::NonMovingAddress => Ok(()),

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

/// Strength of the one-time array validation attached by an ordinary counted
/// loop. Numeric validation includes every plain-array invariant and also
/// proves raw-f64 element representation, so it may serve a plain read too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverArrayValidationKind {
    Plain,
    Numeric,
}

/// Descriptor data consumed at an ordinary bounded array read.
///
/// `valid_i1` is loop-invariant. When false the read takes its established
/// guarded fallback and never consumes `base_handle_slot`; when true the
/// region contract guarantees the cached handle remains usable until the next
/// poll refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiverArrayAccess {
    pub(crate) valid_i1: String,
    pub(crate) base_handle_slot: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveArrayValidation {
    contract: ReceiverDescriptor,
    kind: ReceiverArrayValidationKind,
    valid_i1: String,
}

#[derive(Debug, Clone)]
struct ActiveReceiverDescriptor {
    contract: ReceiverDescriptor,
    data: ActiveReceiverData,
}

#[derive(Debug, Clone)]
enum ActiveReceiverData {
    Address {
        refresh: ReceiverPollRefresh,
        array_validation: Option<ActiveArrayValidation>,
    },
    CachedLength {
        slot: String,
    },
    BoundedIndex {
        index_local_id: u32,
        scope_id: u32,
    },
    PackedF64Loop(PackedF64LoopFact),
    MaskedWindowArray(MaskedWindowArrayFact),
    BufferView(BufferViewSlot),
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
        self.entries.iter().any(|entry| {
            entry.contract.receiver == receiver
                && matches!(&entry.data, ActiveReceiverData::Address { .. })
        })
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
            data: ActiveReceiverData::Address {
                refresh: ReceiverPollRefresh {
                    rooted_box_slot,
                    base_handle_slot,
                    source_root,
                },
                array_validation: None,
            },
        });
        true
    }

    /// Install phase 3's ordinary-counted-loop descriptor.
    ///
    /// The caller supplies the exact region enders after replacing only the
    /// indexed read that will consume this validation with its trusted form.
    /// Both the cached-address and representation contracts must admit every
    /// ender before the entry becomes visible to lowering. A duplicate reuses
    /// an outer descriptor; callers can query [`Self::array_access`] to learn
    /// whether that outer entry is strong enough for their read.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn materialize_region_validated_array(
        &mut self,
        receiver: u32,
        rooted_box_slot: String,
        base_handle_slot: String,
        source_root: String,
        kind: ReceiverArrayValidationKind,
        valid_i1: String,
        enders: &[RegionEnder],
    ) -> Result<bool, BoundaryViolation> {
        if self.contains(receiver) {
            return Ok(false);
        }
        let address_contract = ReceiverDescriptor {
            table: "receiver_descriptors",
            receiver,
            claim: ReceiverClaim::Address,
            boundary: FactBoundary::PollRefresh,
            // The supplied ender list is the proof: an unwind edge below is
            // rejected rather than excused by claiming it was excluded.
            excludes_try: false,
        };
        for &ender in enders {
            boundary_admits(&address_contract, ender)?;
        }
        let representation_contract = ReceiverDescriptor {
            table: "receiver_descriptors[array_validation]",
            receiver,
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::DynamicExtent,
            excludes_try: false,
        };
        for &ender in enders {
            boundary_admits(&representation_contract, ender)?;
        }
        self.entries.push(ActiveReceiverDescriptor {
            contract: address_contract,
            data: ActiveReceiverData::Address {
                refresh: ReceiverPollRefresh {
                    rooted_box_slot,
                    base_handle_slot,
                    source_root,
                },
                array_validation: Some(ActiveArrayValidation {
                    contract: representation_contract,
                    kind,
                    valid_i1,
                }),
            },
        });
        Ok(true)
    }

    /// Install a loop-invariant `receiver.length` value for one dynamic
    /// extent. Unlike an address, this scalar survives every relocation
    /// boundary; ownership still belongs in the descriptor table so the
    /// extent cannot drift from the receiver fact it serves.
    pub(crate) fn materialize_cached_length(&mut self, receiver: u32, slot: String) -> bool {
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors[cached_length]",
            receiver,
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::DynamicExtent,
            excludes_try: false,
        };
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            data: ActiveReceiverData::CachedLength { slot },
        });
        true
    }

    /// End the dynamic extent of a cached length without disturbing another
    /// active descriptor payload for the same receiver.
    pub(crate) fn dematerialize_cached_length(&mut self, receiver: u32) -> bool {
        let Some(index) = self.entries.iter().rposition(|entry| {
            entry.contract.receiver == receiver
                && matches!(&entry.data, ActiveReceiverData::CachedLength { .. })
        }) else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    /// The loop-invariant boxed-double length slot for `receiver`.
    pub(crate) fn cached_length_slot(&self, receiver: u32) -> Option<&str> {
        self.entries.iter().rev().find_map(|entry| {
            if entry.contract.receiver != receiver {
                return None;
            }
            match &entry.data {
                ActiveReceiverData::CachedLength { slot } => Some(slot.as_str()),
                ActiveReceiverData::Address { .. }
                | ActiveReceiverData::BoundedIndex { .. }
                | ActiveReceiverData::PackedF64Loop(_)
                | ActiveReceiverData::MaskedWindowArray(_)
                | ActiveReceiverData::BufferView(_) => None,
            }
        })
    }

    /// Record that `index_local_id` is in bounds for `receiver` throughout a
    /// lexical loop-proof scope. Scalar relations survive safepoints, while
    /// reassignment and scope exit invalidate them through the table APIs
    /// below.
    pub(crate) fn materialize_bounded_index(
        &mut self,
        receiver: u32,
        index_local_id: u32,
        scope_id: u32,
    ) {
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors[bounded_index]",
            receiver,
            claim: ReceiverClaim::ScalarRelation,
            boundary: FactBoundary::ScopeId,
            excludes_try: false,
        };
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            data: ActiveReceiverData::BoundedIndex {
                index_local_id,
                scope_id,
            },
        });
    }

    /// Whether the current descriptor scope proves this exact receiver/index
    /// pair in bounds.
    pub(crate) fn has_bounded_index(&self, receiver: u32, index_local_id: u32) -> bool {
        self.entries.iter().any(|entry| {
            entry.contract.receiver == receiver
                && matches!(
                    &entry.data,
                    ActiveReceiverData::BoundedIndex {
                        index_local_id: active_index,
                        ..
                    } if *active_index == index_local_id
                )
        })
    }

    /// Invalidate bounded-index relations whose receiver or index binding was
    /// reassigned.
    pub(crate) fn invalidate_bounded_indices_for_local(&mut self, local_id: u32) {
        self.entries.retain(|entry| {
            !(entry.contract.receiver == local_id
                && matches!(&entry.data, ActiveReceiverData::BoundedIndex { .. }))
                && !matches!(
                    &entry.data,
                    ActiveReceiverData::BoundedIndex { index_local_id, .. }
                        if *index_local_id == local_id
                )
        });
    }

    /// Install a packed numeric-array representation fact for one guarded
    /// clone. The producing matcher excludes calls and `try`; the remaining
    /// back-edge poll is admitted here by the shared boundary algebra.
    pub(crate) fn materialize_packed_f64_loop(&mut self, fact: PackedF64LoopFact) {
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors[packed_f64_loop]",
            receiver: fact.array_local_id,
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
        };
        boundary_admits(&contract, RegionEnder::BackEdgePoll)
            .expect("a packed representation descriptor must survive its loop poll");
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            data: ActiveReceiverData::PackedF64Loop(fact),
        });
    }

    /// Packed representation facts in installation order. Nested consumers
    /// may reverse this iterator to prefer their innermost scope.
    pub(crate) fn packed_f64_loop_facts(
        &self,
    ) -> impl DoubleEndedIterator<Item = &PackedF64LoopFact> {
        self.entries.iter().filter_map(|entry| match &entry.data {
            ActiveReceiverData::PackedF64Loop(fact) => Some(fact),
            _ => None,
        })
    }

    pub(crate) fn has_packed_f64_loop_facts(&self) -> bool {
        self.packed_f64_loop_facts().next().is_some()
    }

    /// Install a statically bounded masked-window representation fact for one
    /// guarded clone. Its producer admits only a call-free scalar region and
    /// excludes `try`, leaving the loop poll as the sole region boundary.
    pub(crate) fn materialize_masked_window_array(&mut self, fact: MaskedWindowArrayFact) {
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors[masked_window_array]",
            receiver: fact.array_local_id,
            claim: ReceiverClaim::Representation,
            boundary: FactBoundary::ScopeId,
            excludes_try: true,
        };
        boundary_admits(&contract, RegionEnder::BackEdgePoll)
            .expect("a masked-window representation descriptor must survive its loop poll");
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            data: ActiveReceiverData::MaskedWindowArray(fact),
        });
    }

    pub(crate) fn masked_window_array_facts(
        &self,
    ) -> impl DoubleEndedIterator<Item = &MaskedWindowArrayFact> {
        self.entries.iter().filter_map(|entry| match &entry.data {
            ActiveReceiverData::MaskedWindowArray(fact) => Some(fact),
            _ => None,
        })
    }

    /// Install or replace the function-lifetime native storage descriptor for
    /// a Buffer/TypedArray receiver. The pointed-to allocation is non-moving;
    /// all semantic invalidation is expressed by mutating or removing this
    /// payload through the APIs below.
    pub(crate) fn materialize_buffer_view(
        &mut self,
        receiver: u32,
        view: BufferViewSlot,
    ) -> Option<BufferViewSlot> {
        let previous = self.dematerialize_buffer_view(receiver);
        let contract = ReceiverDescriptor {
            table: "receiver_descriptors[buffer_view]",
            receiver,
            claim: ReceiverClaim::NonMovingAddress,
            boundary: FactBoundary::InPlaceDegradation,
            excludes_try: false,
        };
        for ender in [RegionEnder::BackEdgePoll, RegionEnder::UnwindEdge] {
            boundary_admits(&contract, ender)
                .expect("a non-moving buffer-view address survives relocation boundaries");
        }
        self.entries.push(ActiveReceiverDescriptor {
            contract,
            data: ActiveReceiverData::BufferView(view),
        });
        previous
    }

    pub(crate) fn dematerialize_buffer_view(
        &mut self,
        receiver: impl std::borrow::Borrow<u32>,
    ) -> Option<BufferViewSlot> {
        let receiver = *receiver.borrow();
        let index = self.entries.iter().position(|entry| {
            entry.contract.receiver == receiver
                && matches!(&entry.data, ActiveReceiverData::BufferView(_))
        })?;
        let entry = self.entries.remove(index);
        let ActiveReceiverData::BufferView(view) = entry.data else {
            unreachable!("buffer-view lookup selected another descriptor payload")
        };
        Some(view)
    }

    pub(crate) fn contains_buffer_view(&self, receiver: impl std::borrow::Borrow<u32>) -> bool {
        self.buffer_view(receiver).is_some()
    }

    pub(crate) fn buffer_view(
        &self,
        receiver: impl std::borrow::Borrow<u32>,
    ) -> Option<&BufferViewSlot> {
        let receiver = *receiver.borrow();
        self.entries.iter().find_map(|entry| {
            if entry.contract.receiver != receiver {
                return None;
            }
            match &entry.data {
                ActiveReceiverData::BufferView(view) => Some(view),
                _ => None,
            }
        })
    }

    pub(crate) fn buffer_view_mut(
        &mut self,
        receiver: impl std::borrow::Borrow<u32>,
    ) -> Option<&mut BufferViewSlot> {
        let receiver = *receiver.borrow();
        self.entries.iter_mut().find_map(|entry| {
            if entry.contract.receiver != receiver {
                return None;
            }
            match &mut entry.data {
                ActiveReceiverData::BufferView(view) => Some(view),
                _ => None,
            }
        })
    }

    pub(crate) fn buffer_views(&self) -> impl Iterator<Item = (u32, &BufferViewSlot)> {
        self.entries.iter().filter_map(|entry| match &entry.data {
            ActiveReceiverData::BufferView(view) => Some((entry.contract.receiver, view)),
            _ => None,
        })
    }

    pub(crate) fn buffer_views_mut(&mut self) -> impl Iterator<Item = (u32, &mut BufferViewSlot)> {
        self.entries
            .iter_mut()
            .filter_map(|entry| match &mut entry.data {
                ActiveReceiverData::BufferView(view) => Some((entry.contract.receiver, view)),
                _ => None,
            })
    }

    /// End every descriptor fact owned by a lexical proof scope. Each Phase 4
    /// migration adds its scoped payload here, replacing a separate
    /// `retain(scope_id)` discipline at the lowering site.
    pub(crate) fn dematerialize_scope(&mut self, scope_id: u32) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            let active_scope = match &entry.data {
                ActiveReceiverData::BoundedIndex { scope_id, .. } => Some(*scope_id),
                ActiveReceiverData::PackedF64Loop(fact) => Some(fact.scope_id),
                ActiveReceiverData::MaskedWindowArray(fact) => Some(fact.scope_id),
                _ => None,
            };
            active_scope != Some(scope_id)
        });
        before - self.entries.len()
    }

    /// End the dynamic extent of one materialised receiver.
    pub(crate) fn dematerialize(&mut self, receiver: u32) -> bool {
        let Some(index) = self.entries.iter().position(|entry| {
            entry.contract.receiver == receiver
                && matches!(&entry.data, ActiveReceiverData::Address { .. })
        }) else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    /// The promotable, precise-root box slot consumed by `LocalGet`.
    pub(crate) fn rooted_box_slot(&self, receiver: u32) -> Option<&str> {
        self.entries.iter().find_map(|entry| match &entry.data {
            ActiveReceiverData::Address { refresh, .. } if entry.contract.receiver == receiver => {
                Some(refresh.rooted_box_slot.as_str())
            }
            _ => None,
        })
    }

    /// The pre-masked base-handle slot consumed by packed address math.
    pub(crate) fn base_handle_slot(&self, receiver: u32) -> Option<&str> {
        self.entries.iter().find_map(|entry| match &entry.data {
            ActiveReceiverData::Address { refresh, .. } if entry.contract.receiver == receiver => {
                Some(refresh.base_handle_slot.as_str())
            }
            _ => None,
        })
    }

    /// Conditional validation and refreshed base handle for an ordinary array
    /// read. A numeric consumer requires the stronger numeric validation; a
    /// plain consumer may reuse either kind.
    pub(crate) fn array_access(
        &self,
        receiver: u32,
        require_numeric: bool,
    ) -> Option<ReceiverArrayAccess> {
        let entry = self.entries.iter().find(|entry| {
            entry.contract.receiver == receiver
                && matches!(&entry.data, ActiveReceiverData::Address { .. })
        })?;
        let ActiveReceiverData::Address {
            refresh,
            array_validation,
        } = &entry.data
        else {
            unreachable!("address lookup selected a non-address descriptor")
        };
        let validation = array_validation.as_ref()?;
        if require_numeric && validation.kind != ReceiverArrayValidationKind::Numeric {
            return None;
        }
        Some(ReceiverArrayAccess {
            valid_i1: validation.valid_i1.clone(),
            base_handle_slot: refresh.base_handle_slot.clone(),
        })
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
            let ActiveReceiverData::Address {
                refresh,
                array_validation,
            } = &entry.data
            else {
                continue;
            };
            boundary_admits(&entry.contract, RegionEnder::BackEdgePoll)?;
            if let Some(validation) = array_validation {
                boundary_admits(&validation.contract, RegionEnder::BackEdgePoll)?;
            }
            refreshes.push(refresh.clone());
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
    expr_region_ender_with_trusted_operation(e, is_inert, &|_| false)
}

fn expr_region_ender_with_trusted_operation(
    e: &Expr,
    is_inert: &dyn Fn(&Expr) -> bool,
    is_trusted_operation: &dyn Fn(&Expr) -> bool,
) -> Option<RegionEnder> {
    // A production consumer may replace one exact operation with a form whose
    // guard establishes that it cannot dispatch or allocate. Children still
    // run through the ordinary walker before this classification, so trusting
    // `arr[i]` never accidentally trusts an effectful `i`.
    if is_trusted_operation(e) {
        return None;
    }
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
    region_enders_in_stmts_with_trusted_operations(stmts, controls, is_inert, &|_| false)
}

/// Region walk used by a guarded consumer that replaces a precisely matched
/// operation with a non-dispatching form. Only the operation node is trusted;
/// its children retain normal ender classification and execution order.
pub(crate) fn region_enders_in_stmts_with_trusted_operations(
    stmts: &[Stmt],
    controls: &[&Expr],
    is_inert: &dyn Fn(&Expr) -> bool,
    is_trusted_operation: &dyn Fn(&Expr) -> bool,
) -> Vec<RegionEnder> {
    let mut out = Vec::new();
    for s in stmts {
        enders_in_stmt(s, is_inert, is_trusted_operation, &mut out);
    }
    for c in controls {
        enders_in_expr(c, is_inert, is_trusted_operation, &mut out);
    }
    out
}

fn enders_in_expr(
    e: &Expr,
    is_inert: &dyn Fn(&Expr) -> bool,
    is_trusted_operation: &dyn Fn(&Expr) -> bool,
    out: &mut Vec<RegionEnder>,
) {
    // Child expressions execute before the operation represented by their
    // parent (`f(makeClosure())` allocates the closure before it calls `f`).
    // Region boundaries are ordered data once a lowering path consumes them,
    // so a pre-order walk would put the call before the allocation.
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        enders_in_expr(child, is_inert, is_trusted_operation, out)
    });
    if let Some(r) = expr_region_ender_with_trusted_operation(e, is_inert, is_trusted_operation) {
        out.push(r);
    }
}

fn enders_in_stmt(
    s: &Stmt,
    is_inert: &dyn Fn(&Expr) -> bool,
    is_trusted_operation: &dyn Fn(&Expr) -> bool,
    out: &mut Vec<RegionEnder>,
) {
    match s {
        // A throw is an unwind edge *and* the helper allocates the Error.
        Stmt::Throw(e) => {
            enders_in_expr(e, is_inert, is_trusted_operation, out);
            out.push(RegionEnder::UnwindEdge);
        }
        Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            enders_in_expr(e, is_inert, is_trusted_operation, out)
        }
        Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            enders_in_expr(condition, is_inert, is_trusted_operation, out);
            for st in then_branch {
                enders_in_stmt(st, is_inert, is_trusted_operation, out);
            }
            if let Some(else_branch) = else_branch {
                for st in else_branch {
                    enders_in_stmt(st, is_inert, is_trusted_operation, out);
                }
            }
        }
        // A nested loop's back-edge poll is a relocation point for the
        // *enclosing* region too — this is why the armed-poll refresh reloads
        // every active receiver cache, not just the innermost scope's.
        Stmt::While { condition, body } => {
            enders_in_expr(condition, is_inert, is_trusted_operation, out);
            for st in body {
                enders_in_stmt(st, is_inert, is_trusted_operation, out);
            }
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::DoWhile { body, condition } => {
            for st in body {
                enders_in_stmt(st, is_inert, is_trusted_operation, out);
            }
            enders_in_expr(condition, is_inert, is_trusted_operation, out);
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                enders_in_stmt(init, is_inert, is_trusted_operation, out);
            }
            if let Some(condition) = condition {
                enders_in_expr(condition, is_inert, is_trusted_operation, out);
            }
            for st in body {
                enders_in_stmt(st, is_inert, is_trusted_operation, out);
            }
            if let Some(update) = update {
                enders_in_expr(update, is_inert, is_trusted_operation, out);
            }
            out.push(RegionEnder::BackEdgePoll);
        }
        Stmt::Labeled { body, .. } => enders_in_stmt(body, is_inert, is_trusted_operation, out),
        // Every statement in a `try` body may divert to the handler.
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body {
                enders_in_stmt(st, is_inert, is_trusted_operation, out);
            }
            out.push(RegionEnder::UnwindEdge);
            if let Some(catch) = catch {
                for st in &catch.body {
                    enders_in_stmt(st, is_inert, is_trusted_operation, out);
                }
            }
            if let Some(finally) = finally {
                for st in finally {
                    enders_in_stmt(st, is_inert, is_trusted_operation, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            enders_in_expr(discriminant, is_inert, is_trusted_operation, out);
            for c in cases {
                if let Some(t) = &c.test {
                    enders_in_expr(t, is_inert, is_trusted_operation, out);
                }
                for st in &c.body {
                    enders_in_stmt(st, is_inert, is_trusted_operation, out);
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
