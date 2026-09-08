# Per-object layout-mask residue histogram

Runtime implementation SHA: `97b550085869f108c0789ec07ac620ab9de7f295`

Young-log replay SHA: `dd279a8ba2de6b86a044d1074955e87fccf76d53`

Branch: `perf/layout-residue-histogram`, based on
`fork/perf/minor-phases-and-logs` at
`151680fa0f6ffbf6ebb7d8166e2424fc0d031b49`.

## Part 0: #9895 replay

`git cherry 151680fa0 0a427a39f` prints `- 390796a11`:
`390796a11`'s one-pass layout prune and saturating filter are already in the
base. It prints `+ 19a6cd201`, so that commit alone was replayed. The only
conflict was `gc/tests/young_log_tests.rs`; the #9957 fixed-cost scanner tests
and #9895's layout-prune tests were kept as adjacent blocks. The runtime hunks
were not edited.

`git range-diff 19a6cd201^! dd279a8ba^!` shows only commit metadata/message and
the expected `young_log_tests.rs` insertion context. No runtime/code hunk
differs. The replayed tests are present:

- `dead_young_masked_owner_is_pruned_through_the_layout_log`
- `surviving_young_masked_owner_is_rekeyed_and_stays_logged`
- `old_layout_records_are_skipped_by_a_minor`

The focused `layout` filter ran the first and third green. The middle name
does not contain `layout`; its required full-suite execution is one of the
gates the 12 GB disk floor prevented, so it is present but not claimed green
on this machine.

The superseded `41a8af7da`, `bdd1fc003`, and `0a427a39f` were not replayed.
Consequently their full-walk test names/guard are intentionally absent:
`young_closure_prop_value_is_traced_and_moved_by_a_minor`,
`young_value_under_an_old_closure_owner_is_traced_by_a_minor`,
`old_closure_entries_survive_a_minor_full_walk`, and
`dropping_a_logged_closure_owner_trips_the_prune_rule2_check`. The original
young-walk tests remain because #9957 measured `closure.dynamic_props` winning
3.75 -> 3.06 ms and the base retains that log.

## Diagnostic fields and derivation

The instrument calls `layout_on()` exactly once in each non-empty full or
young death prune. When it is false, neither an `Instant` nor the surviving
mask pass is created. When true, `prune_walk_us` times only the existing prune
entry loop: the two `retain` walks for a full prune, or the drained young-key
loop for a minor. The diagnostic histogram pass runs afterwards and is not
included in that price.

The first new line is:

```text
[layout-diag] residue keys=<n> closure=<n> object=<n> array=<n> other=<n> slots{4-7=<n> 8-15=<n> 16-31=<n> 32-63=<n> 64-255=<n> 256+=<n>} ptr_share{q1=<n> q2=<n> q3=<n> q4=<n>} space{nursery=<n> old=<n> malloc=<n>} inserts_since{birth=<n> rebuild=<n> store=<n>} prune_walk_us=<us>
```

- `keys` is the surviving `LAYOUT_SLOT_MASKS.len()` after the death prune.
- `closure`, `object`, `array`, and `other` come from the tracked owner's
  `GcHeader.obj_type`. `other` is defensive: legitimate mask owners are the
  three layout-bearing kinds.
- Logical slots use the same owner facts as tracing: masked closure
  `real_capture_count`, shape-derived `object_live_slot_count`, and array
  `min(length, capacity)`. Other types fall back to GC payload words. The six
  indices are `<=7`, 8-15, 16-31, 32-63, 64-255, and 256+. Under the default
  floor a legitimate first-bucket mask is 4-7; the `<=7` implementation keeps
  totals exhaustive if an override or corrupt residue produces a smaller one.
- Pointer slots are `LayoutSlotMask::count_slots(logical_slots)`. Quartiles
  are non-overlapping: q1 `<=25%`, q2 `>25% and <=50%`, q3 `>50% and <=75%`,
  q4 `>75%`.
- `nursery` is an arena owner in Eden or either survivor half. `malloc` is an
  owner without `GC_FLAG_ARENA`. Remaining arena spaces (`Old`, `Longlived`,
  and transient `PromotedYoung`) are `old` for this three-way price split.
- `birth`, `rebuild`, and `store` count fresh HashMap keys inserted by
  `layout_init_from_slots`, `layout_rebuild_from_slots` (including the exact
  rebuild wrapper), and `layout_note_slot`. Updates to an existing mask and
  GC rekeys do not count. The three counters reset after every emitted prune.
  Provenance is deliberately not stored on each `LayoutSlotMask`: doing so
  would change a representation and trace/store path that exists when the
  diagnostic is off. The separate counters answer insertion traffic, not the
  historical origin of each survivor.
- `prune_walk_us` is `Instant::elapsed().as_micros()` around the existing loop,
  saturated to `u64`.

The second new line is:

```text
[layout-diag] price per_key_prune_ns=<n> est_tag_checks_saved_per_trace=<n>
```

`per_key_prune_ns` is integer `prune_walk_us * 1000 / keys` (zero for no mask
keys). `est_tag_checks_saved_per_trace` is the saturated sum of
`logical_slots - pointer_slots` over every surviving mask: the maximum number
of exact tag checks the whole residue can avoid in one full trace. It is a
benefit ceiling, not a claim that every object is traced every cycle.

This histogram can decide which owner kind and logical-slot bucket dominates,
how sparse the masks are, where their owners live, and whether the 64+ buckets
where a mask can plausibly earn program-global upkeep are a rounding error.
It does not change a threshold or selection rule.

## Tests and sabotage evidence

- `layout_residue_histogram_counts_by_kind_and_bucket` arms a per-thread test
  sink, creates 5- and 20-capture closures with pointer captures, rebuilds one
  70-slot object, prunes with all owners surviving, and asserts kind, slot,
  quartile, space, insert-site, saved-check, and output fields. It passed in
  the focused release `layout` run (137 passed, 0 failed). Sabotage swapped
  the 8-15 and 16-31 destinations; the
  test failed with slots `[1, 1, 0, 0, 1, 0]` against expected
  `[1, 0, 1, 0, 1, 0]`.
- `layout_residue_histogram_is_silent_when_unarmed` forces the per-thread sink
  off, performs a prune, asserts no residue output, and reads a test-only
  histogram-entry counter. It passed in the same focused release run.
  Sabotage removed the full-prune `if layout_diag` guard; it failed with 1
  entry visited against expected 0.

## Follow-up rule candidates

### (a) Young-entry log for the prune

This is orthogonal to deciding which masks deserve to exist, and Part 0 has
already stacked it here. `PerObjectLayoutHint.young_keys` is armed before every
new/moved young record becomes findable. A minor drains only those candidates,
drops stale/dead/promoted keys, and a full prune rebuilds the log from its
authoritative table walk. It changes repeated minor pruning from O(standing
keys) toward O(young churn), while the histogram describes the residue and is
armed-only. It cannot remove the full-trace walk or the mask's insert/store/
death costs, so a high floor or shared representation can still win on top.

### (b) Measured break-even floor

A follow-up can replace the corpus default of four with a floor derived from
`per_key_prune_ns`, expected prunes during an owner's lifetime, and a separately
measured tag-check nanosecond cost. The mask's maximum per-trace return is
already printed as `slots - pointer_slots`; its standing prune cost is printed
per key. The relevant funnels are centralized: bulk birth and rebuild compare
against `layout_mask_min_slots`, while store-time creation goes through
`layout_prefers_scan_over_mask` (with the existing object-specific default of
eight). The missing input is tag-check cost and trace frequency by lifetime;
without those, converting the current price line directly into a slot number
would mix one prune with one full trace and repeat the campaign's wrong-ratio
failure mode.

### (c) Closure masks keyed by function

This is structurally possible when every instance agrees. `ClosureHeader`
provides a stable native `func_ptr` and `real_capture_count`, and
`layout_init_from_slots` observes the complete birth mask. A function-keyed
descriptor can store `(slot_count, mask)`, reuse it for agreeing instances,
and poison the function on the first differing birth or later capture store,
matching `SHAPE_LAYOUTS`' `Some/None` ambiguity pattern. Poison must make every
instance fall back to conservative tag scanning (safe even for earlier
instances that omitted per-object masks), while a diverging stored instance
can retain its exact per-object mask. The present header has only
`GC_LAYOUT_SIDE_MASK`, not a function-shared state, so mask resolution and
`layout_note_slot` would need an explicit shared lookup/fallback protocol.
Function entries then need no death prune because code pointers are stable and
the table is O(functions), but agreement/poison tests must cover post-birth
stores and differing capture counts before this becomes a rule.

## Validation

- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 layout`:
  PASS before the two sabotage checks: 137 passed, 0 failed, 3,143 filtered.
  Both sabotages were then reverted. A final rerun against the restored source
  could not start because the mandatory free-space check fell below 12 GB.
- Full `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1`:
  NOT RUN: blocked by the same free-space floor.
- `cargo build --release -p perry-runtime --features wasm-host -j4`: NOT RUN:
  blocked by the same free-space floor.
- `rustfmt --check`: PASS on every changed Rust file.
- `git diff --check`: PASS.
- `scripts/check_file_size.sh`: PASS; no Rust file exceeds 2,000 lines
  (`gc/layout.rs` is 1,999).
- No local cc run, as requested.

Every Cargo attempt checked `df -g /` immediately beforehand. The final gate
rerun is currently blocked because the latest check reports 11 GB available,
below the binding 12 GB floor; no below-floor Cargo command was started.
Per the campaign's parked-lane rule, this worktree's disposable `target/` was
then removed. The focused run did compile and execute its release test binary,
but that cleanup means there is no retained artifact mtime to present as final
build proof; the focused result is evidence for the named tests, not a
substitute for the blocked final gates.

## Stage LP: exact perrymaster request and falsifiers

Relink the #9957 tree plus `97b550085869f108c0789ec07ac620ab9de7f295`
(young-log prune plus residue histogram) on m6mp's object cache. Run two
graceful four-turn 3300-character candidate repetitions against `app-m6mp`
with `PERRY_GC_DIAG=1`; do not enable layout diagnostics for the CPU A/B,
because its deliberate whole-residue histogram is measurement overhead. Then
run one additional candidate capture with
`PERRY_GC_DIAG=1 PERRY_LAYOUT_DIAG=stderr`.

The falsifiers are:

- `dead_owner_side_table_pruning` for `LAYOUT_SLOT_MASKS + TYPED_LAYOUTS`
  falls from 8.7-9.3 ms to at most 1 ms per steady minor, following the
  +3...+10 young-key churn rather than 155-161k standing keys.
- Total steady copied-minor pause falls from about 46 ms to about 38 ms.
- Four-turn CPU improves by 1-2%; report both repetitions, not only a minimum.
- Peak and settled RSS remain within +3%.

For the +29 MB settled result #9895 observed on main, one 160k-key log retains
two `Vec<usize>` buffers. At a 262,144-entry capacity that is about 4 MiB, so a
single table-owning thread cannot explain +29 MB; that delta would require
about seven similarly grown thread-local logs or allocator-retained secondary
effects. Stage LP must either report enough per-thread log capacity to account
for it or show the settled delta gone. Do not label +29 MB “the log” without
that reconciliation.

From the layout-diagnostic run, preserve the residue lines for minors 5, 10,
13 (turn-2 maximum), 17, and 25. Report medians of both price fields over the
steady minors, alongside the phase values. Those rows decide whether closure/
object/array and the 4-7/8-15/16-31/32-63/64+ populations justify a threshold
or function-keyed follow-up.
