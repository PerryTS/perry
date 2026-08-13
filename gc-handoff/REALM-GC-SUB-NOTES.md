# Realm-local GC roots: #8002 / #8003

Worktree: `/Users/amlug/projects/perry/wt-realm-gc-sub`

Target: `/Users/amlug/cargo-targets/realm-gc-sub`

Base inspected: `origin/main` at `a9a99d8b7e8d3e2d3bd35d8725a34a4cab403f97`

## Current-main audit

- Both #8002 and #8003 are still open and have no issue comments.
- #8002 remains live in shipped builds. The iterator prototype slots are bare
  `AtomicI64` statics. The typed-array and generator tower slots are declared
  through `per_test_global!`, which is per-thread only under `cfg(test)` and
  expands to the original process-global statics in production.
- #8003 is partly stale. PR #8024 / commit `a2ee0012b` converted
  `FUNCTION_CLASS_IDS` and its companion class heap registries to
  `perry_thread_local!`, so that subsection is already fixed on `main`.
- #8003's native-module caches remain production-global because they too use
  `per_test_global!`. `HTTP_METHODS_CACHE` and `FS_CONSTANTS_CACHE` hold values
  allocated in the calling thread's long-lived arena; the five OS namespace
  caches hold realm-local namespace objects.
- #8003's `LOCAL_STORAGE_PTR` / `SESSION_STORAGE_PTR` remain bare process-global
  atomics and are overwritten on every realm bootstrap. Brand checks still
  compare against those shared cells.

## Planned mechanism fix

Use one small `RealmAtomicI64` / `RealmAtomicU64` adapter over
`perry_thread_local!` so existing load/store call sites remain explicit while
every shipped backing slot is per-agent. Mutable root scanning must borrow the
calling thread's backing atomic; behavioural and runtime tests must prove both
nonzero liveness and distinct addresses while two realm threads remain alive.

## Implementation and focused validation

- `RealmAtomicI64` / `RealmAtomicU64` are process-global *handles* containing
  no heap pointer; their backing atomics are `perry_thread_local!` values.
  Loads, barriered stores and mutable-root scanner visits all resolve through
  the same current-agent slot.
- Converted 23 roots: six iterator prototypes, two `%TypedArray%` roots, six
  generator/async-generator roots, seven native-module caches, and two Web
  Storage brands.
- `build_iterator_prototypes` also gained a `GcSuppressScope`: it carried raw
  shared/family pointers across allocating method and tag installation just as
  the already-protected generator and TypedArray builders do.
- Added the two-live-agent runtime gate
  `realm_owned_intrinsic_module_and_storage_roots_are_distinct`. It materializes
  all 23 roots on both threads, proves every root word is nonzero, proves every
  backing atomic differs, keeps both arenas alive at a barrier, and proves all
  heap addresses differ.
- Added the positive-control no-move test
  `iterator_prototype_tower_runs_in_a_no_move_window` and a behavioural
  `perry/thread` probe covering iterator/generator/typed-array prototype
  mutation isolation, native constants, Web Storage brands, and main-realm
  preservation.

Validated so far (all against this worktree/target):

- `cargo check -p perry-runtime`: pass (pre-existing warnings only).
- all four `gc::tests::lazy_intrinsic_towers` tests: 4/4 pass.
- `object_cache_roots_survive_a_guard_clear_on_another_thread`: pass.
- `test_gc_init_mutable_scanner_families_rewrite_runtime_slots`: pass.
- `scripts/check_test_registration.py`, `scripts/check_gc_doc_claims.py`,
  `scripts/gc_store_site_inventory.py`, and
  `scripts/gc_runtime_root_holders.py`: pass.

The required combined static-archive/compiler build passed for
`-p perry -p perry-runtime-static -p perry-stdlib-static`. The pinned artifacts
were rebuilt from this source and checked directly:

- `perry-dev/perry`: 2026-08-13 23:44:05
- `perry-dev/libperry_runtime.a`: 2026-08-13 23:43:44
- `perry-dev/libperry_stdlib.a`: 2026-08-13 23:44:07

The registered behavioural test passes with those pinned artifacts. A direct
binary run was byte-identical to the expected output under both live arms:

- seed 1, rate 1, moving loop polls, from-space protection depth 800:
  1,385 copying minors, 32,047 moved objects, 60,040 loop polls, and 1,385
  quarantined page-set retirements.
- seed 7, rate 1, moving loop polls, evacuation verification: 1,399 copying
  minors, 32,047 moved objects, 60,040 loop polls, and 22,239 copied-object
  events in the diagnostics.

Sabotage/restore and the current-main landing-equivalent check are still
pending; do not treat this note as a merge claim yet.
