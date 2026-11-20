Closed two GC issues in the "a real hazard exists and nothing exercises it" class.

**#7251 — no-move window for the two lazy intrinsic-tower builders.**
`build_generator_tower` (`object/global_this/generator.rs`) and
`ensure_typed_array_intrinsic` (`object/global_this/typed_array.rs`) build
the same shape of immortal object graph #7217 fixed for
`populate_global_this_builtins`: raw `*mut ObjectHeader`/`*mut ClosureHeader`
locals threaded across a dozen-plus allocating installs, with no root the
collector knows about until the final `AtomicI64::store`. Both now open a
`crate::gc::GcSuppressScope` for the whole build.

The blocker had been the gate, not the fix — three prior attempts all passed
with the window deleted, plus a fourth found while building this one: arming
the ArenaBytes byte-threshold trigger under `force_legacy_gc_pacing` also
passed vacuously, because under that pacing an ArenaBytes trigger only starts
a *budgeted* cycle that needs an explicit host-safepoint pump to advance, so
`gc_collection_count()` never moved whether or not the builder was
suppressed. Fixed by arming `GC_OLD_RECLAIM_PENDING` instead — the branch
`gc_check_trigger` services synchronously regardless of pacing — combined
with forcing the current arena block (very nearly) full before calling the
builder, so its first allocation reaches `gc_check_trigger()` at all despite
being three orders of magnitude smaller than a block.

Also fixed the ordering hazard behind two of the three original failed
attempts: the six intrinsic-tower `AtomicI64` statics
(`TYPED_ARRAY_INTRINSIC_PTR`, `TYPED_ARRAY_INTRINSIC_PROTO_PTR`,
`GENERATOR_FUNCTION_INTRINSIC_PTR`, `GENERATOR_INTRINSIC_PROTO_PTR`,
`GENERATOR_PROTOTYPE_PTR`, `ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR`,
`ASYNC_GENERATOR_INTRINSIC_PROTO_PTR`, `ASYNC_GENERATOR_PROTOTYPE_PTR`) were
plain process-global statics, built once per *process* rather than per test,
so whichever libtest thread happened to touch a tower first poisoned every
other test's "never built" precondition. Converted to `per_test_global!`
(#7672's mechanism for exactly this hazard): each test thread now gets its
own zeroed instance in test builds, with zero change to non-test builds.

New gate: `crates/perry-runtime/src/gc/tests/lazy_intrinsic_towers.rs`, two
tests each asserting both halves of "the arming was live" — no collection
during the builder call, and the same armed request serviced once the window
closes. Sabotage-verified: commenting out either `GcSuppressScope::new()`
line turns its test red.

**#7254 — `PERRY_GC_ZEAL` + `PERRY_GC_VERIFY_EVACUATION`, exercised in CI for
the first time.** Reproduced 3/3 on current `main`; liveness confirmed via
`PERRY_GC_DIAG=1` (14,072 copying-minor runs, 10,775 objects copied). The
panic surface has moved since the issue was filed — now `"stale forwarded
pointer in native stack-map roots"`, not `"shadow stack roots"` — because
the statepoint/stack-map native root walker
(`gc/roots/stack_maps.rs::visit_stack_map_root_slots`, layer 2 in
`docs/src/internals/rfc-rooting-by-construction.md`) is live on this
aarch64 host; this is now a layer-2 defect, not layer-3 as previously
classified.

Sized the population across all 59 `test-parity/gc_repsel_corpus.txt`
files under the exact pairing: 20 PASS, 3 correctly INERT (zeal's own
exit-70 self-check), 3 confirmed panics with the identical signature
(`test_gap_repsel_p4a_inline_tiers`, `test_gap_repsel_p4a3_ptr_numarray` —
the filed reproducer, `test_gap_repsel_element_shape_loop_clone`), and 33
timeouts under a 40s budget. The timeouts are not trusted as a defect
finding here: the sweep ran under sustained host load of 30-55 (20
concurrent users), which makes host contention vs. ZEAL's legitimate
per-poll collection cost vs. a genuine second defect undecidable from this
data alone — flagged as follow-up work needing a quiet host, not asserted
as a finding.

The defect itself is NOT fixed in this PR — it is a statepoint/native-root
liveness bug, the class this project's own GC campaigns
(`#7154`/`#7341`) treat as needing a disassembly-driven investigation
before a confident fix, not something to rush (see #7211's cautionary
precedent: an author actively thinking about rooting still shipped a wrong
predicate). Gated instead: extended `scripts/gc_instrument_smoke.sh`
(already wired into the required-ish `gc-stress` per-PR step) with a new
arm 5 — the pairing on the script's existing small fixture (proves
non-vacuity and no false positive on known-good code, `copied_objects>0`
under the verifier) plus a pinned-regression witness against
`test_gap_repsel_p4a3_ptr_numarray` that fails loudly if the panic ever
stops reproducing, whether from a fix (promote the file, delete the pin)
or a silent change in failure shape (needs a fresh look).

Deliberately not routed through `gc_repsel_matrix.sh`'s arm registry: a
registered arm joins every corpus file via `--arms all` on push/schedule,
and the unsized 33-file timeout population makes that irresponsible within
the existing 90-minute job budget today.
