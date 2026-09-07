# GC pointer-validation / buffer-registry report

## Branch and implementation SHA

- Branch: `perf/gc-pointer-validation`
- Base: `origin/main` at `504e180d085f37aaf0b9189e0c1bdb328d508f70`
- Runtime implementation commit: `8d49faa556004b87e57e29a22ca822d3369a8ef3`

The checkout's shared `.git` metadata was read-only in this environment. The
branch was therefore fetched and created in an isolated Git directory while
the work tree remained `/Users/amlug/projects/perry/agent-trees/gc-overhead`.

## What changed

- The synchronous exact `ValidPointerSet` census clears each arena block's
  existing object-start bitmap and stamps every walkable header as it visits
  it. This includes headers written by codegen's inline allocator. The normal
  allocation path remains Map-only and gains no per-allocation bitmap write.
- `ValidPointerSet::contains` asks the direct page-class metadata and exact-
  start bitmap before its two-level sorted-run floor lookup. Non-arena and
  page-table-control cases retain the sorted-run fallback. Malloc membership
  remains an exact `BTreeSet` union and is always considered after an arena
  miss.
- `ValidPointerSet::enclosing_object` still uses the sorted census runs; the
  bitmap is exact-start membership only.
- `buffer::is_registered_buffer` now uses
  `try_read_tracked_gc_header` to reject a tracked non-`GC_TYPE_BUFFER` object
  before the address filter and exact registries. A `GC_TYPE_BUFFER` match is
  not accepted from the header: managed buffers, headerless external buffers,
  and SharedArrayBuffer backings retain the authoritative registry path.
- `PERRY_GC_DIAG` gained one whole-run `[gc-pointer-validation]` line and one
  `[gc-runtime-handles]` line. `PERRY_BUFFER_DIAG` gained
  `header_rejects=` and renamed its downstream count `filter_probes=`.
- Added named sabotage tests for the page-class exact-start arm and Buffer
  header-class rejection.

## Handle-scope source audit

No handle-scope behavior changed.

- A successful `js_segments_view_open` creates one `RuntimeHandleScope` and
  pushes one input root across the cursor allocation. `js_segments_view_next`
  and `js_segments_view_code_point_at` create no scope and push no roots; both
  are allocation-free by their implementation contract. The other view entry
  points do not open a scope per call.
- A successful `js_regexp_new` creates one scope and pushes exactly two roots:
  the pattern plus either the caller's already-canonical flags string or the
  newly allocated canonical flags string.
- Both public entry points receive raw values/pointers. A caller-owned scope
  cannot let either callee refresh a relocated argument after an allocation
  without changing the ABI to pass a mutable root/slot. This is not a one-line
  scope removal.
- Actual cc-turn scope and push counts were not measured locally. The added
  diagnostic reports total scopes, pushes, pushes-per-scope, and maximum live
  slots for the perrymaster run.

## Gates and checks

Run locally:

- `rustfmt --edition 2021 --check` over all 12 changed Rust files: pass.
- `./scripts/check_file_size.sh`: pass; zero Rust files over 2,000 lines.
- `python3 scripts/addr_class_inventory.py`: address-class audit pass over
  1,357 files, 315 allowlisted and 521 known sites. It separately reported two
  pre-existing stale baseline reductions in `set.rs` and `crypto/kdf.rs`; no
  baseline was rewritten.
- `git diff --check`: pass.

Not run:

- Cargo invocations: 0. Immediately before the intended gates, `df -g /`
  reported 24 GB available, but the binding wrapper
  `secret-tests/cc-perf-campaign/measure_lock.sh` is absent. The task requires
  every Cargo invocation to go through that wrapper, so no Cargo command was
  issued.
- `cargo build --release -p perry` with default features: not run.
- `cargo test -p perry-runtime --release --lib -- --test-threads=1`: not run.
- Sabotage tests executed: 0 of 2; both were authored but require the blocked
  Cargo test gate.
- `nm -g libperry_runtime.a`: not run. No exported symbol or exported function
  signature was changed; no local archive exists because `target/` was
  deleted and no build was permitted.

## Counters and measurements

- Pre-fix cc-rig counter run: not measured. The private cc harness and campaign
  files named in the task are absent from this checkout, and the mandatory
  build wrapper is absent.
- Post-fix cc CPU/RSS: not measured.
- Locally observed performance numbers: none.
- The source map supplied with the task is not restated as a branch result.

New counter line schemas to collect:

```text
[gc-pointer-validation] contains=... page_hits=... page_rejects=... run_fallbacks=... malloc_probes=... malloc_hits=... enclosing=...
[gc-runtime-handles] scopes=... pushes=... pushes_per_scope=... max_slots=...
[buffer-diag] header_rejects=... filter_probes=... admits=... rejected=... true_positives=...
```

## Exact perrymaster measurement request

Fetch `fork/perf/gc-pointer-validation` and relink the runtime-only implementation
SHA `8d49faa556004b87e57e29a22ca822d3369a8ef3` on main6's cache. Run the I7-view
cc rig for the pushed SHA and its `origin/main` base: **5×3300 + 2×400 + idle
rows**. Record turn CPU, main-thread samples, collector share, RSS, and the
named pointer-validation/buffer leaves. Add the existing page-table control
arm with `PERRY_GC_PAGE_CLASS_TABLE=0`. In counter runs enable
`PERRY_GC_DIAG=1` and `PERRY_BUFFER_DIAG=<path>` and return the counter lines
whole, including every `[gc-pointer-validation]`, `[gc-runtime-handles]`, and
`[buffer-diag]` line; do not summarize or truncate them.

## Could not verify

- Compilation, linking, exported archive contents, unit/sabotage test execution,
  cc-turn behavior, CPU parity, and RSS impact.
- The task-referenced `cc-perf-campaign/BRIEF_COMMON.md`, `STATUS.md`,
  `ARCHITECTURE.md`, `BRIEF_gc-overhead.md`, and
  `RESULT_page_class_table.md` were not present under this worktree or the
  stated `/Users/amlug/projects/perry/perry` repository.
