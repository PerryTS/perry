**A single vocabulary for "receiver R has fact F, valid until boundary B"** (#9254
phase 1). Codegen carries sixteen receiver-keyed fact tables on `FnCtx` —
`cached_lengths`, `bounded_index_pairs`, `packed_f64_loop_facts`,
`masked_window_array_facts`, `buffer_view_slots`, `int_range_facts` and ten more.
Each answers the same two questions (what do we know about this receiver, and how
long may we believe it), and each answers the second one differently: a
`retain(|f| f.scope_id != id)` at scope exit, an insert/remove pair with no
identifier, a mutable field downgraded in place and never removed, or a reload at
the safepoint instead of an invalidation.

A fifth boundary — the **unwind edge** — is expressed by none of them. `lower_try`
clears no fact table. Unwind safety is obtained today by six unrelated means: the
packed matcher rejecting `Stmt::Try` outright, a body shape that cannot contain
one, a post-hoc `contains_gc_unsafe_call` scan, a single `try_depth == 0` gate in
`versioned_indexed_loop`, a dirty bit stored before every call, and a storage kind
that simply never moves. Every one is a local decision by one tier, and a tier
added tomorrow inherits none of them.

This adds the model — `RegionEnder` (what ends a no-relocation region),
`FactBoundary` (how a table expresses extent), `ReceiverClaim` (value vs
representation vs address, the axis that decides whether a boundary is
load-bearing at all) and `boundary_admits`, the algebra in one place. **It emits
no IR and no lowering path consults it**; the `#![allow(dead_code)]` at the top of
the module is the marker for that, matching the #854 subgraphs in `hir_facts`, and
the whole thing is revertible by deleting the file.

The load-bearing artifact is the equivalence lint, which holds the model against
`loop_purity::loop_may_allocate` — shipping and audited — on a shared battery,
asserting the direction with teeth: *if the model finds no relocation point, then
`loop_may_allocate` must also have proven the body alloc-free.* The converse is
deliberately not asserted, since `loop_may_allocate` answers `true` for any
statement it does not model (`Return`, `Switch`), which is imprecision rather than
a collection point.

That lint paid for itself before this landed. Written the obvious way — enumerate
the enders, default to safe — the model passed every hand-written test and failed
the battery on three entries: a generic `IndexGet`/`PropertyGet` can reach an
accessor or a proxy trap, `Expr::Closure` allocates, and `is_inert` belongs on the
whole coercing node rather than per operand (the #6975 hole one abstraction up).
Inverting the match to an allowlist with an `Unmodelled` catch-all closes that
class: adding an HIR variant can no longer silently widen a region.

Note one deliberate divergence from `collectors::safepoint_sites`, which does not
count property reads — over-counting reads would over-spill a read-heavy loop, and
its consumer only needs a spill estimate. A region model has the opposite
obligation: missing one licenses a stale cached address.

All sixteen tables are also transcribed as test data with their declaration sites,
pinning the exact set whose unwind safety is *external to its stated boundary*:
`stable_packed_loop_facts` (emergent — `stmt_flags` has a `_ => {}` arm, so
`Stmt::Try` is invisible to its admission scan) plus the three immutable-fact
tables that lean on non-movable storage. A flag there is not a bug report; it says
the safety comes from somewhere the boundary vocabulary cannot express, which is
what phase 2 has to fix, and pinning the set is how phase 2 proves it closed one.
