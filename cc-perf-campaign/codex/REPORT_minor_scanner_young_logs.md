# Minor scanner young logs

Implementation SHA: `d399c39ddb638a92b2735a6bacc2aef13def944a`

## Map and mechanism

- `object/shapes.rs:1972` scans two address-keyed structures. `families` maps a
  keys-array address to every descriptor id whose slab record carries that
  address; the descriptor record is the authoritative rewritable `keys` edge.
  A family is a strong minor root only when an old receiver or an optimization
  cache carries one of its descriptors. `indices` is a weak key-to-slot
  accelerator keyed by the same keys-array address and needs only relocation
  repair. Shape property-key payloads are strings/symbol headers, both GC
  leaves. Nursery keys arrays can move; old arrays cannot; Longlived arrays do
  not move or die but can temporarily contain a collectible key leaf.
- Shapes already had #9755's `young_keys` address log and the four
  `shapes.indices` arm sites. Its keep predicate was
  `addr_is_minor_relevant`, so every Longlived keys array stayed in the log
  forever. `object/shapes.rs:2154` now re-derives actual minor work: nursery
  addresses remain for relocation, malloc roots remain while carrier-owned,
  and a Longlived carrier remains only while its property-key payload contains
  a collectible leaf. `object/shapes.rs:271` receives old/cache carrier notes
  without recursively borrowing the shape table; `object/shapes.rs:801` is the
  enforced structural-publication funnel that re-arms a same-address mutation.
  Scanner-internal rekeys do not enqueue a duplicate visit.
- `box.rs:954` previously walked every address in `BOX_REGISTRY`. These are
  malloc-allocated mutable-capture/async state cells; the registry address is
  not a GC pointer. Only the `Box::value` NaN-box can point into the nursery.
  `I32Box` and `BoolBox` registries contain no GC edge and were never part of
  this scanner. There was no partial box log.
- `box.rs:123` adds the box remembered set. Both allocation arms and both
  mutation ABIs arm it before publishing a minor-relevant payload
  (`box.rs:133`, `box.rs:705`, `box.rs:725`, `box.rs:1318`, `box.rs:1357`).
  The trusted setter is included because generated boxed-local stores use it;
  omitting that silent path would violate the enforced-funnel rule. Release
  paths only clear/de-register cells, and scanner rewrites compact their own
  entries. `box.rs:1040` owns the priced `visited` counter.
- Both minor walks sort/deduplicate their logged addresses, drop stale keys,
  and keep only post-visit non-old entries. Full/major scans still enumerate
  the authoritative whole tables and rebuild the logs. Under
  `debug_assertions` and in lib tests, each minor scan re-derives the relevant
  set from the whole table and asserts that the log is complete.
- `gc/copying.rs:1889` now emits `pause_us=` and `scan_us=` together on every
  completed `[gc-copy-minor] ran` line. `pause_us` is sampled as the final
  action before the copied-minor returns to the mutator; `scan_us` is the
  already-profiled scanner total returned by `gc/scanner_profile.rs:131`.
  Timing remains behind the existing cached `PERRY_GC_DIAG` gate.

## Tests and sabotages

- `shape_table_minor_walk_visits_exactly_k_young_entries`: N old families and
  k young families produce `visited == k`. Sabotage: remove
  `note_young_keys`; the completeness re-derivation panics.
- `shape_table_rederivation_rejects_a_suppressed_logging_site`: a test-only
  suppression skips the production family arm and the scan must panic.
- `shape_mutation_to_new_young_key_rearms_minor_log`: a Longlived carrier that
  gains a new nursery key at the same address must move that key. Sabotage:
  remove the re-arm in `stamp_object_shape_id_with_carrier_note`.
- `box_roots_minor_walk_visits_exactly_k_young_entries`: N old payloads and k
  young payloads produce `visited == k`. Sabotage: remove either allocator arm.
- `box_root_rederivation_rejects_a_suppressed_mutation_hook`: a test-only
  suppression skips `js_box_set_bits` logging and the authoritative registry
  walk must panic.
- `box_mutation_to_new_young_object_is_visited`: an old box changed to a new
  nursery object is visited. Sabotage: remove the setter hook.
- `promoted_shape_entry_leaves_young_log_and_remains_in_major_walk` and
  `promoted_box_root_leaves_log_and_is_found_by_full_walk`: promotion makes
  `kept == 0`, while the next authoritative full walk still visits the entry.
  Sabotage: retain the pre-visit/from-space classification or scope the full
  walk to the log.
- Existing scanner-completeness and moving-witness suites are unchanged and
  remain part of the requested runtime-lib gate.

## Validation

- `scripts/check_file_size.sh`: PASS.
- `git diff --check`: PASS.
- Cargo gates: NOT RUN. `df -g /` immediately before the first possible Cargo
  invocation reported `0` GB available, below the binding 12 GB floor. Per the
  task rule, no Cargo command was started and no wait for disk was attempted.
- Not run for the same reason:
  `cargo test -p perry-runtime --release --lib -- --test-threads=1`;
  `cargo build --release -p perry-runtime --features wasm-host`;
  `cargo build --release -p perry`.

## Predictions and exact perrymaster request

Predictions: on a zero-live steady minor,
`object::shapes::scan_shape_table_rekey_mut` and
`r#box::scan_box_roots_mut` each fall from about 2 ms to at most 0.2 ms;
steady-minor scanner total falls from 7–8 ms to at most 3 ms; every completed
minor reports `pause_us` and `scan_us`. CPU bound is about -3% at 3300 chars
and larger at 400 chars, where minors are a larger share. RSS should be
unchanged (small retained log capacities only, within the allowed 1–10%).

Perrymaster request, from pushed SHA: relink on the I7-view tree
(runtime-only), then run the three gates through
`/Users/amlug/projects/perry/secret-tests/cc-perf-campaign/measure_lock.sh --build`
at `-j4` using detached `nohup`: (1)
`cargo test -p perry-runtime --release --lib -- --test-threads=1`, (2)
`cargo build --release -p perry-runtime --features wasm-host`, and (3)
`cargo build --release -p perry`. Because this is GC-adjacent, the coordinator
must apply `run-extended-tests`. After green gates, do one graceful four-turn
3300-char run and one 400-char run with `PERRY_GC_DIAG=1`, preserving complete
`[gc-copy-minor] ran pause_us=... scan_us=...` and
`[gc-scanner-profile] copying_minor` lines. Then run paired 5x3300 + 3x400
against I7-view for CPU and RSS.
