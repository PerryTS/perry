# `PERRY_GC_VERIFY_EVACUATION` malloc-borrow fix

## Commit

- Fix commit: `499b71628d53b76d87ba68c6f4a59ed597e6d82e` (`fix(gc): release malloc borrow before verification`)
- Base: `8b7dc3342b22fe6270739c8d51585c3d2cdfa618` (`origin/main` when the task started)

## Re-entrancy path

The observed copied-minor path is diagnostic-only:

1. `crates/perry-runtime/src/gc/copying.rs:1516-1519` gates
   `verify_old_to_young_edges_covered()` on
   `gc_verify_evacuation_enabled()`.
2. Before this fix, `verify_old_to_young_edges_collect()` held a shared
   `MALLOC_STATE` borrow while iterating `s.objects` at
   `crates/perry-runtime/src/gc/verify.rs:798-805` (base commit lines).
3. Each candidate reached
   `verify_old_young_parent_slots_covered()` → `visit_gc_rewrite_slots()` →
   `verify_old_young_slot_covered()` at current
   `crates/perry-runtime/src/gc/verify.rs:731-753` and `:690-700`.
4. A non-arena child reaches the exact membership check in
   `remembered_child_needs_tracking()` at
   `crates/perry-runtime/src/gc/barrier/mod.rs:1568-1585`.
5. That calls `gc_malloc_header_is_tracked()`, whose inner mutable borrow is
   `crates/perry-runtime/src/gc/malloc.rs:526-529` (`borrow_mut()` is line 527)
   and whose `ensure_set_built()` may rebuild from `objects` at `:508-515`.

Because `MALLOC_STATE` is thread-local, the shared outer borrow and mutable
inner borrow are on the same collection thread. `RefCell` therefore panics
before the verifier can inspect the heap. The path is entered only when
`PERRY_GC_VERIFY_EVACUATION` is enabled; the later stale-forwarded-reference
walk is independently gated at `copying.rs:1596-1600`.

I audited the production exact-membership callers (`barrier/mod.rs`,
`young_log.rs`, `native_handle.rs`, `timer.rs`, `path.rs`, `symbol/get.rs`,
`value/dyn_index.rs`, and `json/stringify.rs`). None invokes the helper while
holding a `MALLOC_STATE` borrow. The exact nested path above is verifier-only;
there is no non-diagnostic production re-entrancy to prioritize. A second
diagnostic (`PERRY_GC_VERIFY_CLASSIFIER`) can cause live classification from
some GC walks, but that is also diagnostic, not a production path.

## Change

`crates/perry-runtime/src/gc/verify.rs:10-12` now snapshots the malloc header
vector and releases the `MALLOC_STATE` borrow before any verifier callback.
Every verifier-owned malloc-object walk uses that helper, including the
old-to-young check, marked-child checks, array-slot enumeration, and the final
evacuation heap walk. Exact validation semantics remain unchanged: there is no
`try_borrow` fallback and no weakened pointer check.

The named regression test is
`gc::tests::copying::verify_malloc_borrow::test_copied_minor_verify_evacuation_releases_malloc_registry_before_validation`.
It runs on a spawned worker thread, creates a malloc-backed closure parent and
malloc-backed child, makes the non-empty registry inactive, proves the exact
lookup rebuild count advances, then completes a copying minor with evacuation
verification enabled and asserts that an actual nursery object copied and both
verification phases ran. Sabotage is explicit: restore the malloc verifier loop
under `MALLOC_STATE.with(...borrow())`; the child lookup's `borrow_mut()` panics
the worker and makes `join().expect(...)` fail.

## Validation

Not run because the mandatory pre-Cargo check, `df -g /`, reported only **11
GB available**, below the 12 GB floor. Per task instructions I did not invoke
Cargo and did not wait for disk capacity. Consequently these gates were not
run:

- the named regression test;
- every test matching `verify_evacuation`;
- every test matching `malloc`;
- `cargo test -p perry-runtime --release --lib -- --test-threads=1`;
- `cargo build --release -p perry-runtime --features wasm-host`.

Non-Cargo static checks completed: `rustfmt --check` on all edited Rust files
and `git diff --check`. The largest touched Rust file is 1,994 lines, below the
2,000-line repository cap.

## Perrymaster request

Relink on main's cache, then run `cc` with
`PERRY_GC_VERIFY_EVACUATION=1` for **4 turns**. The run must complete all four
turns, emit `[gc-verify]`-style verifier output proving the diagnostic was live,
and contain no `RefCell already borrowed` or other panic.

## VF2: the parent names itself

### Commit

- Implementation commit: `1ec9e0e8ac72cbce4098d86940b205f90effafd8`
  (`fix(gc): attribute stale evacuation pointers`)
- Extended branch base: `4295d390d` (the first verifier report commit)

### Failure line and field derivation

Every stale-forwarding panic remains one physical line and now carries the
same field vocabulary for heap slots, roots, and runtime side-table scanners:

- `parent` is the user address (`header + GC_HEADER_SIZE`). `parent_type` is
  `gc_type_info(header.obj_type).name`. Root-owned slots use `n/a(root)` because
  their scanner has no GC parent header.
- `parent_space` first reads `GC_FLAG_PINNED` (`pinned`), then
  `GC_FLAG_ARENA` (clear means `malloc`). Arena parents use the current arena
  block class: `PromotedYoung` means `promoted_in_place_this_cycle`; Eden and
  the active survivor half mean `nursery_from`; the inactive survivor half
  means `nursery_to`; Old and Longlived mean `old_page`. The transient
  `PromotedYoung` block class is the collector's this-cycle promotion set, so
  no historical/guessed tenuring classification is used.
- `slot_index` is the cumulative zero-based pointer-slot index within the
  parent. It is derived only after failure by replaying the layout descriptors.
  `visitor` is the matching `GcLayoutSlotKind` or, for side fields absent from
  the layout walk, the matching `GcRewriteDescriptorKind` (for example
  `GcMutableSlotDescriptor`, `ArrayElements`, `ObjectFields`, `RegExpFields`,
  `ClosureCaptures`, `ObjectMeta`, or the corresponding side-field family).
- `child_type` reads `obj_type` from the still-valid forwarding source header;
  if that source is not in the exact pointer census, it falls back to the
  forwarded-to header. `child_space` classifies `forwarded_to` with the same
  `GC_FLAG_PINNED` / `GC_FLAG_ARENA` checks and arena block classes. Thus
  `nursery_to` is a survivor copy and `old_page` is a promoted copy.
- `remembered` re-snapshots the live remembered set on the cold failure path
  and applies `old_young_slot_covered`, including an external `(page, owner)`
  entry when the slot is outside its parent. `dirty_snapshot` tests the exact
  pre-collection `RememberedDirtySnapshot` consumed by this copied minor.
  Nursery parents report both as `n/a(nursery_parent)`. `young_logged` is
  `n/a(heap_parent_uses_remembered_set)` for heap parents: young-entry logs own
  side-table keys, not heap parents. Root/side-table failures report
  `n/a(root_scanner_does_not_expose_owner_key)` because the scanner API exposes
  the scanner class and value but not the table's log key; this is an explicit
  non-answer rather than a guessed `no`.
- `minor` is a per-collection-thread evacuation-verifier ordinal. `trigger` is
  the cycle's `GcTriggerKind`. `after_budgeted_step` compares a new per-thread
  budgeted-cycle completion counter with the value observed by the preceding
  verified minor, so an unrelated agent heap cannot set it.

The original `slot`, `old`, and `forwarded_to` values remain present. The
failure line also has `surface`, a whitespace-free scanner/surface token, so a
root-side miss can be grouped without parsing prose.

### Panic sites covered

All callers of `panic_stale_forwarded_reference` pass the cycle context and
use the common formatter:

- heap object rewrite descriptors (`heap fields`), remembered dirty ranges,
  and remembered fallback headers;
- shadow-stack, native stack-map, and global mutable roots;
- each named Rust mutable-root scanner and the FFI mutable-root scanner;
- Rust and FFI copy-only root scanners;
- the `RuntimeRootVisitor` NaN-box, heap-word, tagged-raw-address, and
  metadata-raw-address paths used by runtime side tables.

Heap dirty-range verification now retains the owner header while visiting a
dirty slot, so a failure in that earlier verifier surface names the parent too,
instead of waiting for the later whole-heap walk.

### Passing-path cost and liveness line

The expensive work is behind the cold stale-reference branch: type lookup,
space classification, remembered-set re-snapshot, coverage probes, string
construction, and descriptor replay occur only immediately before panic. The
passing per-slot closure is unchanged: it records the layout read and invokes
`verify_slot`, with no new type, arena, set, or snapshot read. Counts are
accumulated once per accepted parent and once per descriptor from the
descriptor's existing slot count; there is no added per-slot diagnostic probe.

When a copied minor passes and `PERRY_GC_DIAG=1`, it emits exactly one:

```text
[gc-verify] minor=<n> evacuation_ok parents=<count> slots=<count> old_young_edges=<count>
```

`parents` and `slots` come from the live whole-heap verification walk;
`old_young_edges` is the already-computed count from
`verify_old_to_young_edges_covered`, so the line proves both verifier phases
ran without adding a second edge walk.

### Tests and gates

The disk-compliant focused gate ran with 13 GB available:

```text
cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 verify
```

All 16 matching tests passed: the 14 existing verify/malloc-borrow tests plus
the two named VF2 tests. That run preceded the final failure-only dirty-owner
retention and cumulative slot-index cleanup; rerunning the resulting commit is
**not run: disk**.

- `stale_forwarded_reference_panic_names_parent_slot_and_coverage` publishes
  an old-page parent's nursery field without the write barrier, catches the
  copied-minor verifier panic on a worker thread, and asserts
  `parent_type=`, `parent_space=old_page`, `slot_index=0`, `remembered=no`,
  `child_type=`, `minor=`, and `trigger=`.
- `evacuation_verifier_pass_line_counts_parents_and_slots` uses an isolated
  child-test process with diagnostics enabled, captures stderr, requires
  exactly one copied-minor success line, and parses `parents` as a positive
  integer.

Sabotage reruns: **not run: disk**. Removing one asserted panic field was
restored without retaining the mutation; the mandatory pre-Cargo check then
reported 11 GB, below the 12 GB floor. Skipping the pass line was likewise not
run for the same disk reason. The assertions are direct (missing the field or
line reaches `assert!` / `assert_eq!`), but no mutation-test pass is claimed.

The full release `--lib` suite and the release `wasm-host` build are **not run:
disk**: subsequent checks reported 10--11 GB available. The attempted field
sabotage command was interrupted immediately after its pre-check exposed the
sub-floor value; no further Cargo gate completed. Static gates passed:
`rustfmt --check` and
`git diff --check`. Touched Rust files are below 2,000 lines; the largest is
`copying.rs` at 1,928 lines (`roots.rs` 1,886; `cycle.rs` 1,797; `verify.rs`
1,558; `verify_diag.rs` 293).

### Perrymaster request

Relink `app-vf` (main + this branch) and `app-vfat` (the AT2 tree + this branch)
on their existing caches. Run **N = 6** four-turn 3300 sessions for each app
with:

```text
PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_DIAG=1 RUST_BACKTRACE=1
```

Report every verifier failure line verbatim together with that minor's
preceding `[gc-step]`, `[gc-trigger]`, and `[gc-survival]` lines. For one clean
run of each app, report the `[gc-verify]` pass-line counts so verifier liveness
is independently visible.
