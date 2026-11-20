**gc-matrix: the `collect` arms now require a cycle that reclaimed something, the numarray-growth probe reaches the collector, and four known-inert entries are retired (#7016, #7017; #7018 closed as not reproducing).**

Three issues, one disease: a check that could not fail.

### #7016 — the probe never collected, so 19 cells measured nothing

`test_gap_repsel_p4a3_numarray_growth` was the one corpus row still `UNVER` on
every GC arm. Reproduced: **0 cycles**, and `PERRY_GC_DIAG=1` printed *nothing at
all* — the file allocates inside one 1 MB arena block and makes no `gc_malloc`
calls, so the arena trigger never arms. `PERRY_GC_HEAP_LIMIT` cannot reach it
(`gc_trigger_absolute_ceiling_bytes` is budget/4 with a floor). **The probe was
at fault, not the predicate.**

Sections 1–5 are untouched, so their "fully contained, therefore promoted" shape
is unchanged; a section 6 adds `test_gap_repsel_gc_stress`'s escaping
module-level churn sink, with the numeric-array local initialized *before* the
churn, grown by `push` past several capacity doublings *while* it runs, and read
*after* it in the same iteration.

Measured across all 13 arms: **PASS on every one**, output byte-identical to the
pinned Node 26.5.1 oracle in each. `default` 4 cycles / **74,404 objects copied**;
`gen_gc_off` and the other `collect` arms 7 cycles; `shipped_default` unchanged
at 0, as its control role requires. Per #7666, a probe that merely allocates a
lot can still run zero copying minors — this one was checked for the copying
minor specifically, not just for a cycle.

### #7017 — `cycles > 0` counted a teardown cycle

A `collect` cell was `PASS` on any cycle. On a small corpus file that cycle lands
at the event-loop boundary *after* the program's last output and reclaims
nothing, because everything allocated after it armed was born black. That scored
identically to a run that collected mid-program while the test's
representation-selected locals were live — the property the matrix exists to
assert. **The predicate was at fault.**

`collect` now requires a **productive** cycle. Following #7657's widening of the
gc-ratchet rule to `copied + promoted > 0`, the counter names a *destination*
rather than a single number: `reclaimed` sums `sweep_freed`, `block_reclaim`,
`eden_dead_bytes`, `freed_bytes` and `dead_bytes`, in **both** the `k=N` and JSON
`"k": N` spellings. Reading only `k=N` scored
`test_gap_gc_symbol_local_rooting` — 86 malloc-count-triggered cycles that free
31.9 MB of symbols — as reclaiming zero, because the malloc sweep's bytes appear
only in the JSON trace.

It is a conservative proxy and the script says so: a mid-program cycle over a
heap that is entirely live reclaims nothing and reads `UNVER`. Under-claiming is
the safe direction for a liveness gate; `cycles` over-claimed.

Both halves move together. `reclaimed` is threaded into the per-cell JSON, and
`gc_matrix_liveness_check.py`'s `REQUIREMENTS["collect"]` reads it. Three new
self-test cases pin the change — a cycle that reclaimed nothing must *not*
satisfy `collect`, a productive one must, and `scavenged`/`evacuated` must not
stand in for reclamation on an arm that is non-moving by construction. Reverting
the counter to `cycles` fails exactly two of them, so the fix is shown able to
fail. The per-run liveness table now prints `reclaimed` beside `collected` so the
gap between them stays on screen: measured at **collected 3/3, reclaimed 1/3**
for `gen_gc_off` on the Phase 4a.3 slice.

Corpus-wide under `gen_gc_off`, of 58 files: 12 never collect (already `UNVER`),
**14 collect but reclaim nothing** — these were `PASS` and are now honestly
`UNVER` — and 32 are productive, which keeps every `collect` arm live.

### The four known-inert arms were live, and CI had been saying so

`test-parity/gc_matrix_inert_arms.txt` registered `default`, `verify_evac`,
`cons_scan_off` and `cons_scan_off_force` as inert because
`PERRY_GC_MOVING_LOOP_POLLS` is default-off, so the copying minor is "ineligible
by construction". **The poll flag has not changed and all four scavenge anyway.**

`gc-stress` on `main`, run 31240304595 (2026-08-08): each satisfied
`requires=scavenge` on **41 of 58 cells**, `default` at `counter=1384046`, and
the job was red with four `STALE-REGISTRY` lines. Reproduced locally on
`test_gap_repsel_gc_stress` — a file this PR does not touch, under a predicate
this PR does not change — at 1/1 live per arm, `default` copying 228,181 objects;
`shipped_default`, with no pressure knob and no GC env at all, copies 340,956.
The shipped configuration relocates today.

All four entries are deleted, with the measurement recorded in their place. What
changed is the collector around the flag (#7370's statepoint default, #7432,
#7657, #7666), not the flag — the entries' stated cause outlived its truth by
exactly the mechanism the registry was created to catch, which is why they are
deleted rather than re-worded. The registry is now empty and `gc-stress` passes
its liveness gate again.

### #7018 — not reproducing, and the hypothesis is structurally refuted

`PERRY_GC_TRACE=1` was reported to SIGSEGV `test_gap_repsel_scalar_replaced_locals`
under the evacuating arms. 20 runs across both link modes: **0 crashes**, stdout
byte-identical with and without the flag.

The first link mode was vacuous and is reported as such: auto-optimize relinks
the runtime `--no-default-features`, so `diagnostics` is off and `GcCycleTrace::emit`
falls to its stub — all 113 `[gc] cycle` lines were "diagnostics feature
disabled", and the real tracer never ran. Re-run with the diagnostics archive:
**110 real `"event":"gc_cycle"` objects, 466,424 objects copied** — the arm is
demonstrably live and still does not crash. Four other corpus files behave the
same.

Structurally, the hypothesis cannot hold. `PERRY_GC_TRACE` is read in exactly one
place (`gc_trace_enabled()`, `gc/policy.rs`) with two call sites: a scalar
counter snapshot, and a thread-local `u64` counter bump. Emission serialises
already-accumulated scalars. **Nothing on that path dereferences a heap object,
walks the object graph, or reads a `GcHeader`**, so it cannot dereference a
forwarded pointer and shares nothing with #6998/#6995. The issue's description —
"the tracer runs inside a collection, walking structures the collector is mid-way
through mutating" — describes `gc/trace.rs`, the *marking* tracer, which runs on
every collection regardless of the flag. A name collision, not a defect.
