### GC rooting residue: #7210's two remaining unrooted-alloca sites + #7640 sections A/C

Closed the two named flagship sites from #7210's remaining unrooted-alloca
enumeration:

- `codegen/helpers.rs`'s `emit_namespace_populator` staged every re-exported
  binding's value into a plain stack alloca while the per-entry loop called
  allocating helpers (closure singleton alloc, cross-module getters); an
  already-staged entry had no root and could go stale before
  `js_create_namespace` read the whole buffer. Fixed by rooting each value in
  a `RootedGroup` as it is produced and deferring every store to a second,
  call-free pass immediately before the consuming call.
- `lower_call/early_branches.rs`'s `obj[strKey](args)` computed-key dispatch
  lowered receiver/key/args into bare registers in sequence, and (in the
  static-key arm) `unbox_str_handle` — an allocating SSO materialisation —
  ran between the args buffer's stores and the call. Fixed the same way:
  root `[object, index, ...args]` in one `RootedGroup`, build the args
  buffer last in each branch.

From #7640 section A, three more `index_set.rs` arms with no rooting
decision at all are now fixed: the bounded-index-pair array store, `globalThis[k]
= v`, and the width-tracked typed-array non-numeric-index store. Section A's
`#5525 recv_unknown` inline dyn-TA store and the TA runtime-key / TA
final-fallback / Uint8Array runtime-key arms are not reached.

From #7640 section C, resolved the open question rather than mechanically
fixing it: two `property_set.rs` comments claimed a class-field store's
receiver survives an allocating RHS via "the same statepoint re-read" a
sibling arm relies on. That mechanism does not exist. For a bare
`Expr::LocalGet`/`Expr::This` receiver the claim is true, via `root_reload.rs`
(#7280) — a front-end pass, independent of RS4GC, that re-materialises a
value derived from a shadow-slot or handle-global load below any collection
point it doesn't dominate. For a compound receiver — a class-field READ used
as the assignment target, `this.target.x = allocPoint(n).x` — the claim is
false, and the gap is invisible to both `root_reload` and the
`--stale-registers` checker (confirmed by hand in the emitted IR). Left
unfixed deliberately: rooting the receiver unconditionally would tax the
dominant plain-local case on what the issue itself calls the hottest store
path in the compiler — a measured-cost tradeoff for a follow-up, not a
mechanical gap this change's tools can close for free.

Section E's seven named callees were triaged (not fixed): each looks like a
hazard that is not one — a single already-rooted caller, an address derived
after the allocating step rather than before, or typed-array immovability
(the same category #7210 section 5 already flagged). Static triage only,
not checker-verified per callee.

New gap-suite fixtures pin all of the above:
`test_gap_gc_namespace_and_computed_dispatch_rooting.ts` (+
`fixtures/gc_namespace_rooting_pkg/`), `test_gap_gc_index_set_bounded_globalthis_ta_rooting.ts`,
and an expanded `test_gap_gc_class_field_receiver_rooting.ts` covering both
halves of the section C finding.
