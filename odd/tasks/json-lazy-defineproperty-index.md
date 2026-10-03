# JSON lazy array index descriptor parity

## Objective and problem

Make indexed reads of arrays produced by lazy `JSON.parse` honor an own numeric accessor installed with `Object.defineProperty`, matching pinned Node 26.5.1. Issue #10097 and `test_gap_json_lazy_defineproperty_index.ts` describe stale parsed-element reads in both sparse and scanned/materialized states; current `origin/main` is `0ed699b9f26267d5ede9e9aabd12160c842cec81`. Fresh RED confirms the sparse case, while a scanned-only control already matches Node.

## Root hypothesis

Fresh unit RED refined the mechanism: `Object.defineProperty` materializes an initially sparse `GC_TYPE_LAZY_ARRAY`, but `define_array_property` rejects that original receiver and the generic path stores the accessor under the lazy header's address. Later indexed reads resolve the materialized `ArrayHeader` and bypass that side-table owner. A previously scanned/materialized array already takes the array descriptor path. Route a numeric definition on a lazy header through the array-specific path so definition and read use the same canonical owner; leave lazy parsing and unrelated keys unchanged.

## Scope and constraints

- Root-scoped runtime correction and focused Rust/TypeScript coverage only, within the coordinator's allowed edit surfaces.
- Preserve ordinary arrays, direct JSON parsing (`PERRY_JSON_TAPE=0`), existing accessor and inherited-index semantics, and lazy JSON for unaffected reads.
- No version bump, full workspace suite, unrelated baseline refresh, push, PR, or issue mutation. Code/comments/artifacts in English.
- Run runtime tests with `RUST_TEST_THREADS=1`; rebuild the compiler plus static runtime and stdlib archives before relying on parity output.

## Tasks

- [x] **JLDI-1 — Reproduce and locate.** Pinned Node 26.5.1 printed `lazy-defineproperty-index 79280`; freshly built Perry's exact filtered harness reported `PARITY_FAIL` (exit 1), Node exit 0/Perry exit 1, `descriptor read bypassed (scan=false)`. `PERRY_JSON_TAPE=0` printed 79280 (exit 0). An isolated scanned-only variant printed 79440 on both Node and Perry (exit 0). Existing focused Rust seam is `json_tape/cached_read.rs`; edit-surface extension requested from coordinator.
- [x] **JLDI-2 — Test and correct.** New `unscanned_lazy_descriptor_invokes_getter_without_losing_neighbour` failed on the pre-fix cached value/probe, then passed after numeric lazy definitions entered the array-specific path. Existing materialized descriptor test and the fixture's scanned arm remain green; neighbour access stays intact.
- [x] **JLDI-3 — Verify and commit.** After final normalization, exact Rust test (1 pass), cached-read module (7 pass), and `perry-runtime` crate (4,855 pass, five ignored) passed with `RUST_TEST_THREADS=1`. Compiler/runtime/stdlib release archives were rebuilt after the last Rust edit (all timestamps advanced); targeted fixture and `PERRY_JSON_TAPE=0` control each passed 1/1 on pinned Node. Snapshot check, known-failure audit, rustfmt check, and `git diff --check` passed. Narrow snapshot entry was removed via `scripts/gap_snapshot.py update` from the one-test report; known-failure entry was removed because the audit is bidirectional. The wrapper's update route was unavailable locally because root npm dependencies are absent. Commit evidence: this document is included in the local work-unit commit (`git log -1`).

## Acceptance and verification evidence

- Pinned Node and Perry outputs match for sparse and materialized accessor reads; getter call counts and neighbour reads remain correct.
- A targeted Rust test, when feasible, is observed failing before and passing after the runtime fix.
- Exact focused and crate-level checks pass with single-threaded runtime tests; archive timestamps prove the linked runtime was rebuilt.
- The snapshot and known-failure entries for this one passing test are removed; all unrelated entries remain untouched.

## Route, delivery, and next step

Delegated direct work in this dedicated Orca worker: the parent delegated because the implementation requires multi-file source/test changes, fresh compiler execution, and sequential runtime exploration. The coordinator approved `crates/perry-runtime/src/json_tape/cached_read.rs` as an additional exact edit surface. Actual authored delta is below approximately 400 lines, advisory only. Delivery strategy: ask-on-risk; one local work-unit commit, no PR slice or remote action in this dispatch. Next: coordinator owns remote delivery and issue #10097 follow-up; Linux CI still needs to confirm the macOS-targeted snapshot result.
