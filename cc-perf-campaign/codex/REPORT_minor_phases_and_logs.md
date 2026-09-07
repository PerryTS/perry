# Copying-minor phases and remaining scanner young logs

Phase-instrument commit: `b444f0251221d8c40dd645c741367dcf5276dff9`

Scanner-log implementation commit: `364ed3f07c54e365e65bbb5e23bd703a9fc54a0e`

Branch: `perf/minor-phases-and-logs`, based on
`e2eee113d486b5208c56ae1e3f0f4d0dffcbf2b2`.

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

This scanner actually owns three tables: call-site to cooked/raw template
arrays, cooked to raw template arrays, and array named properties. Template
array keys/values are strong roots, so they deliberately keep
`addr_is_minor_relevant`: a Longlived template can contain transitive GC
edges. Array named-property owners are metadata-only and collectible-only;
their NaN-box values remain broad strong roots. No partial log existed.
`array/header/young_roots.rs:29`, `:35`, and `:45` define the three logs;
every publication and array-growth transfer arms before publish at
`array/header.rs:191`, `:262`, `:346`, `:410`, `:446`, and test seeding at
`:604`. The minor scanner is `array/header/young_roots.rs:181`; the full walk
still visits every entry and rebuilds all three logs.

### `scan_symbol_side_table_roots_mut`

This walks six slot shapes: `SYMBOL_PROPERTIES` owner metadata and strong
symbol/value pairs, `SYMBOL_PROPERTY_ATTRS` owner metadata and strong symbol
keys, symbol accessors plus get/set roots, class-static symbol/value pairs,
and metadata-only `SYMBOL_POINTERS`. There was no partial log. A typed slot log
at `symbol/gc_roots.rs:146-209` records exactly the slot shape that can matter
to a minor. Production funnels arm before publication in `symbol.rs:582`,
`:1030`, `:1065`, `symbol/properties.rs:93`, and
`symbol/accessors.rs:94-95`; direct test seeders follow the same contract. The
direct minor path at `symbol/gc_roots.rs:443` and the budgeted step path at
`:229` take only logged slots. Property-owner slots sort before their entries,
so owner rekeying precedes entry lookup; entry scans heal a snapshot owner
through forwarding. Full direct and step walks still take authoritative
whole-table snapshots and rebuild the log.

All five scanners emit their existing `[gc-young-log]` accounting with
logged/visited/kept/table size. Release minors do not enumerate a whole table
to obtain the symbol table size: that exact diagnostic count is itself gated
on `PERRY_GC_DIAG`.

## Sabotage tests

- `descriptor_log_rederivation_rejects_a_suppressed_setter`: suppresses the
  real property-attrs funnel; re-derivation must report the missing owner.
- `closure_log_rederivation_rejects_a_suppressed_setter`: suppresses the real
  closure dynamic-property funnel; re-derivation must report the missing
  owner.
- `builtin_closure_log_rederivation_rejects_a_suppressed_writer`: suppresses
  the arity setter; re-derivation must report the missing closure.
- `template_raw_log_rederivation_rejects_a_suppressed_writer`: suppresses the
  cooked/raw publication funnel; re-derivation must report the missing pair.
  `array_named_log_rederivation_rejects_a_suppressed_setter` independently
  covers the third table owned by that scanner.
- `symbol_log_rederivation_rejects_a_suppressed_property_writer`: suppresses
  the production symbol-property store; re-derivation must report its missing
  typed slots.

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
- `scripts/gc_runtime_root_holders.py`: PASS.
- `scripts/gc_rekeyed_key_tables.py`: PASS.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1` via
  `measure_lock.sh --build`: NOT GREEN solely because it was run detached with
  a PTY. Compilation completed and 3,273 tests passed (including every new
  sabotage), 4 were ignored, and the sole failure was
  `tty::tests::columns_undefined_when_not_tty`, whose assertion correctly saw
  the allocated PTY. Two earlier attempts stopped at compile diagnostics in
  the newly extracted module; those visibility/TLS/null-pointer/type issues
  were fixed before this complete run.
- The required non-PTY rerun was NOT RUN: immediately afterward `df -g /`
  reported 7 GB free, below the binding 12 GB floor. Per the task rule, no
  further Cargo command was started and no disk wait was attempted.
- `cargo build --release -p perry-runtime --features wasm-host -j4`: NOT RUN,
  same 7 GB disk stop.
- `cargo build --release -p perry -j4`: NOT RUN, same 7 GB disk stop.

## Predictions and exact perrymaster request

Predictions: on a zero-live steady minor, each of
`scan_descriptor_roots_mut`, `scan_closure_dynamic_props_roots_mut`,
`scan_builtin_closure_metadata_roots_mut`, `scan_template_raw_roots_mut`, and
`scan_symbol_side_table_roots_mut` is at most **0.3 ms**. Steady scanner total
is at most **5 ms**. This removes roughly 10 ms from a representative steady
minor when the five logs are empty; the phase table, not that estimate, must
name the next non-scanner lever. RSS changes should be small retained log
buffers and remain inside Ralph's allowed +1-10% band.

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
deliverable that names the next lever. Confirm all five named scanners are at
most 0.3 ms on zero-live steady minors and steady scanner total is at most 5
ms. Finally run paired **5x3300 + 3x400** against both main and #9950's runtime,
reporting cc turn CPU and peak RSS; target node/bun CPU parity, allowing only
+1-10% RSS.
