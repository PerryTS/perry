# Copying-minor phases and remaining scanner young logs

Phase-instrument commit: `09846784c`

Scanner-log implementation commit: `dae519296`

Branch: `perf/minor-phases-and-logs`, based on
`8b7dc3342`.

## Copying-minor phase instrument

- `crates/perry-runtime/src/gc/copying_phase.rs:26` is the diagnostic-only
  accumulator. It uses the same `Instant` clock as `pause_us` and records
  non-overlapping spans for `root_scan`, `copy_evacuation`,
  `remembered_set_young_logs`, `promotion`,
  `dead_owner_side_table_pruning`, `from_space_finalization`,
  `forwarding_fixups`, and `block_reset_flip`. `other` is the exact residual
  between those named spans and the whole pause, and `phase_sum_us` is formed
  from the nanosecond partition before conversion, so it equals `pause_us`
  apart from the shared sub-microsecond truncation (well inside 2%).
- `crates/perry-runtime/src/gc/copying.rs:1220-1749` starts and records the
  counters in the functions whose work they price. The two registered-root
  passes accumulate in `root_scan`; the transitive worklist drain is
  `copy_evacuation`; remembered snapshot/dirty scan and post-cycle restore are
  accumulated together; promotion covers retag plus finish; forwarding covers
  promoted-edge rebuild and verification/fixup work; reset covers to-space
  preparation plus the final reset/flip.
- `crates/perry-runtime/src/gc/copying_phase.rs:84` renders counts where the
  collector already owns them: copied/promoted objects and bytes, remembered
  entries and dirty slots, and finalized map/set/error/regexp owners. The
  dead-owner fan-out at `crates/perry-runtime/src/gc/dead_owner.rs:261` clocks
  every registry table separately and appends those table names and
  microseconds to the same field. Those prune callbacks expose no removed-row
  count, so no invented count is printed.
- `crates/perry-runtime/src/gc/copying.rs:1962` appends `phases:` to every
  completed `[gc-copy-minor] ran` line. `PERRY_GC_DIAG` off creates no phase
  accumulator, takes no phase clocks, and builds no detail strings.
- The sabotage unit
  `copied_minor_phase_residual_makes_the_partition_exact` removes a named
  bucket from the expected arithmetic if the partition is widened or omitted.

## Scanner map and young-entry logs

### `scan_descriptor_roots_mut`

This walks string-keyed property-attribute and accessor tables plus their two
owner indexes. Owner addresses are metadata-only and need a minor visit only
while movable/reclaimable; accessor get/set NaN-boxes are strong roots and may
require tracing through Longlived values. A #9754 owner log already existed.
The write funnel at `object/descriptor_state.rs:148` is present at all five
publication/transfer sites (`:985`, `:1208`, `:1277`, `:1394`, `:1428`). This
change narrows the metadata-key half from `addr_is_minor_relevant` to
`addr_is_minor_collectible`; the re-derivation and post-visit keep predicate
use the same rule at `object/descriptor_state/young.rs:42` and
`object/descriptor_state/gc_scan.rs:17`. The full walk is unchanged and
rebuilds the log.

### `scan_closure_dynamic_props_roots_mut`

This walks `CLOSURE_PROPS` values, `CLOSURE_STATIC_PROTOTYPES` values, and the
metadata-only owners of those tables and `CLOSURE_DELETED_KEYS`. A #9754 owner
log already existed. The enforced write funnels are
`closure/dynamic_props.rs:117`, `:186`, `:241`, `:388`, and `:1068`. Owner
retention is now collectible-only; property/prototype values keep the broader
transitive predicate. The minor path is `:533`; the full path remains whole
table and rebuilds the log.

### `scan_builtin_closure_metadata_roots_mut`

This walks two owner-keyed, pointer-free metadata tables: closure arity and the
non-constructable set. Only the closure address can move or die. There was no
partial log. The tables and their complete setters were extracted to
`object/native_module/callable_exports/builtin_closure_metadata.rs`; `:18`
arms the owner log before either setter publishes, `:95` drains only logged
collectible owners on a minor, and the unchanged full walk visits all owners
and rebuilds the log.

### `scan_template_raw_roots_mut`

This scanner owns three small tables: call-site to cooked/raw template arrays,
cooked to raw template arrays, and array named properties. The attempted young
logs were reverted after MP measurement showed 2.76 ms for the keyed path
against 1.83 ms for the original full walk. The scanner again walks the three
authoritative tables directly, with no insert-side log upkeep.

### `scan_symbol_side_table_roots_mut`

This walks six slot shapes: `SYMBOL_PROPERTIES` owner metadata and strong
symbol/value pairs, `SYMBOL_PROPERTY_ATTRS` owner metadata and strong symbol
keys, symbol accessors plus get/set roots, class-static symbol/value pairs,
and metadata-only `SYMBOL_POINTERS`. The attempted typed-slot young log was
reverted after MP measurement showed 2.47 ms for the keyed path against
1.74 ms for the original full walk. Direct scans again iterate the
authoritative tables, and budgeted scans again use the pre-existing full slot
snapshot; none of the symbol writers pays young-log upkeep.

The retained descriptor, closure-dynamic-property, built-in-closure-metadata,
and shape-cache young logs continue to emit `[gc-young-log]` accounting with
logged/visited/kept/table size.

## MP measurement and the two reverted/fixed logs

Perrymaster measured MP-stage medians over 16 steady 3300-character minor
collections, comparing app-m6mp (the five scanner changes) with app-m6ms
(without them):

| scanner | m6ms full walk | m6mp young log | delta |
|---|---:|---:|---:|
| descriptor_roots | 4.12 ms | 4.02 ms | -0.1 ms |
| closure_dynamic_props | 3.75 ms | 3.06 ms | **-0.7 ms** |
| builtin_closure_metadata | 1.42 ms | 0.93 ms | **-0.5 ms** |
| shape_cache | 0.68 ms | 0.27 ms | **-0.4 ms** |
| **template_raw_roots** | 1.83 ms | **2.76 ms** | **+0.9 ms** |
| **symbol_side_table** | 1.74 ms | **2.47 ms** | **+0.7 ms** |
| transition_cache / intern / class_side / singleton_closure / box | flat | flat | flat |
| **total** | **15.5 ms** | **15.3 ms** | **-0.2 ms** |

Both regressions are case (b): the logged path made each visited entry more
expensive than the dense full walk. They are not duplicate-log failures:
`YoungLog::take_sorted` sorts and globally deduplicates every batch, and each
writer tests the young/relevant predicate before noting a key.

- `template_raw_roots`: the full scanner streams each map directly and only
  removes/reinserts keys that actually move. The young path sorted its keys,
  performed a hash lookup for every cache entry, and unconditionally removed
  and reinserted every logged raw-map and named-property owner even when the
  owner did not move. A pointer or index cannot safely retain the full walk's
  per-entry cost because insertion and rekeying can relocate these `HashMap`
  entries. The three logs, their publication hooks, and their two rederivation
  tests were therefore removed.
- `symbol_side_table`: the full scanner streams the maps and their property
  vectors. The typed-slot path sorted its keys, then recovered every property
  entry through an owner hash lookup plus a linear search of that owner's
  vector; the other slot shapes also paid keyed table lookups. Hash-map
  rekeying and vector growth make raw entry pointers or indices unstable, so a
  safe O(young) path with the full walk's per-entry cost would require a
  structural table redesign. The typed log, all writer hooks, and its
  rederivation test were therefore removed.

Re-measurement falsifier: on perrymaster, `template_raw_roots` must be at most
**1.83 ms** and `symbol_side_table` at most **1.74 ms** at the median, the
three improved scanners must remain unchanged, and total scanner time must be
at most **14 ms**.

## Sabotage tests

- `descriptor_log_rederivation_rejects_a_suppressed_setter`: suppresses the
  real property-attrs funnel; re-derivation must report the missing owner.
- `closure_log_rederivation_rejects_a_suppressed_setter`: suppresses the real
  closure dynamic-property funnel; re-derivation must report the missing
  owner.
- `builtin_closure_log_rederivation_rejects_a_suppressed_writer`: suppresses
  the arity setter; re-derivation must report the missing closure.
- `template_raw_log_rederivation_rejects_a_suppressed_writer`,
  `array_named_log_rederivation_rejects_a_suppressed_setter`, and
  `symbol_log_rederivation_rejects_a_suppressed_property_writer` were removed
  with the two reverted logs; their enforced-writer invariant no longer
  exists.

Each completeness check is compiled under `debug_assertions` and `test`. In
the release lib run below, every named sabotage test passed.

## Shape residual

The residual is real young work, not another whole-table leak. The exact keep
predicate is `object/shapes.rs:2154`:

- Nursery Eden, either survivor half, and `PromotedYoung` keys arrays stay
  logged because their table keys must be rewritten if they move.
- Malloc-GC keys arrays stay only when an old/cache carrier makes the family a
  root and the allocation remains minor-collectible.
- Longlived keys arrays stay only when an old/cache carrier roots the family
  **and** at least one property-key leaf in the array is collectible. Longlived
  non-carriers and carriers whose leaves are all old/Longlived drop out.
- Old keys arrays always drop out.

There is one intentional transient duplicate at `object/shapes.rs:2244`: the
mark pass may move a family before the metadata-only slot index is repaired in
the rewrite pass, so both the post-copy address and stale index address must
survive between the passes. Tightening any of these remaining cases would
skip relocation, collection of malloc keys, a strong carrier edge, or the
between-pass index repair. This explains why shape time appears only on the
steady minors that create/grow a burst of genuinely young shape-key arrays;
there is no sound additional predicate tightening in this change.

## Validation

- `git diff --check`: PASS.
- `scripts/check_file_size.sh`: PASS (all Rust files at most 2,000 lines).
- `cargo fmt --all -- --check`: PASS.
- `scripts/check_thread_locals.py --self-test`: PASS in all seven directions.
- `scripts/check_thread_locals.py`: PASS, 411 hot declarations and 273 cold
  declarations in 84 recorded files, below the 768-slot hot capacity.
- `scripts/gc_rekeyed_key_tables.py`: PASS, 42 sites and 25 registered prunes
  classified with zero gaps. The split child now owns the `visit_owner`
  inventory entry.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1` via
  `measure_lock.sh --build`: PASS, 3,271 passed, 0 failed, 4 ignored. The three
  rederivation tests tied to the reverted logs were explicitly removed; the
  retained sabotage tests passed.
- `cargo build --release -p perry-runtime --features wasm-host -j4` via
  `measure_lock.sh --build`: PASS.

## Predictions and exact perrymaster request

Prediction after the MP follow-up: the two reverted scanners return to their
measured full-walk medians or better, the retained closure, built-in closure,
and shape-cache improvements remain, and steady scanner total is at most
**14 ms**. The phase table, not an estimate, must name the next non-scanner
lever. RSS should fall slightly because the two reverted logs and their
retained buffers are gone.

Exact perrymaster request: fetch pushed branch `perf/minor-phases-and-logs` and
relink this runtime-only change on main's cache. Run the three required gates
through
`/Users/amlug/projects/perry/secret-tests/cc-perf-campaign/measure_lock.sh --build`
detached, using exactly:

1. `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1`
2. `cargo build --release -p perry-runtime --features wasm-host -j4`
3. `cargo build --release -p perry -j4`

Then run one
graceful four-turn 3300-character cc workload and one 400-character workload
with `PERRY_GC_DIAG=1`, printing and preserving **every complete**
`[gc-copy-minor] ran` line. The phase table for a steady minor is the
deliverable that names the next lever. Confirm `template_raw_roots` is at most
1.83 ms, `symbol_side_table` is at most 1.74 ms, the three improved scanner
medians are unchanged, and total scanner time is at most 14 ms. Finally run
paired **5x3300 + 3x400** against both main and #9950's runtime,
reporting cc turn CPU and peak RSS; target node/bun CPU parity, allowing only
+1-10% RSS.
