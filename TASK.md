Work through this checklist by dependency readiness. Fully implement one ready task before moving to the next. Respect After: dependencies. Preserve current behavior unless a task requires a cutover. Validate the cited issue before replying. Keep updates concise and implementation-focused.
-----
- [x] node-sqlite-custom-functions-authorizers - Add custom functions, aggregates, defensive mode, and authorizers
  Problem: `DatabaseSync` lacks Node-compatible SQL callback/control hooks for `function`, `aggregate`, `enableDefensive`, and `setAuthorizer`.
  Context: Implement callback argument/value conversion, authorizer constants, allow/deny/ignore handling, and validation without changing unrelated database APIs.
  Reference: https://github.com/PerryTS/perry/issues/3185; DeepWiki: repo=`nodejs/node`, question=`How does node:sqlite implement DatabaseSync function, aggregate, enableDefensive, setAuthorizer, callback argument conversion, authorizer constants, and validation errors?`
  Acceptance: Tests register scalar and aggregate SQL callbacks, verify authorizer allow/deny/ignore behavior, defensive mode, and invalid argument errors.

- [x] node-sqlite-sessions-constants - Implement sessions, changesets, applyChangeset, and constants
  Problem: Perry has no Node builtin `Session` object, changeset/patchset behavior, `applyChangeset`, or `sqlite.constants` namespace.
  Context: `db.createSession()`, `Session#changeset`, `Session#patchset`, `Session#close`, disposal, `db.applyChangeset`, and changeset/action/authorizer constants.
  Reference: https://github.com/PerryTS/perry/issues/3186; DeepWiki: repo=`nodejs/node`, question=`How does node:sqlite implement createSession, Session changeset/patchset/close/disposal, applyChangeset, and sqlite.constants for changeset and authorizer behavior?`
  Acceptance: Parity tests create sessions, mutate tracked tables, return `Uint8Array` changesets/patchsets, apply changesets to a matching database, and verify close/disposal errors and constants.

- [x] node-sqlite-finalize-dirty-scope - Finalize the dirty custom-function and Session SQLite slice
  Problem: The remaining SQLite feature work is implemented but unstaged, with two untracked parity tests and unchecked task state.
  Context: `crates/perry-stdlib/src/sqlite.rs`, runtime/native-module dispatch, HIR/codegen routing, `Cargo.lock`, and `test-files/test_parity_sqlite_custom_functions_authorizers.ts` / `test_parity_sqlite_sessions_constants.ts`.
  Reference: Existing tasks `node-sqlite-custom-functions-authorizers` and `node-sqlite-sessions-constants`.
  Acceptance: The two existing tasks are verified, checked off, and committed with no untracked SQLite feature files left.

- [ ] node-sqlite-regen-api-docs - Regenerate API docs for node:sqlite manifest changes
  After: node-sqlite-finalize-dirty-scope
  Problem: `crates/perry-api-manifest/src/entries.rs` changed, but generated API docs are not updated.
  Context: `scripts/regen_api_docs.sh`, `docs/api/perry.d.ts`, and `docs/src/api/reference.md`; CI has an API docs drift guard.
  Reference: `.github/workflows/test.yml` API docs drift job.
  Acceptance: Regenerated docs are committed and `git diff` is clean after rerunning the docs generator.

- [ ] node-sqlite-clear-stale-parity-gaps - Remove stale SQLite known-failure and gap inventory
  After: node-sqlite-finalize-dirty-scope
  Problem: `test-parity/known_failures.json` and `docs/runtime-parity-gaps.md` still describe implemented `node:sqlite` surface as missing.
  Context: `test_parity_sqlite` output currently matches Node locally, and the new focused SQLite outputs should match Node on the current fixtures.
  Reference: Existing SQLite parity reports and output files under `test-parity/output/{node,perry}/`.
  Acceptance: Stale SQLite known-failure/gap entries are removed or narrowed, and the full SQLite parity filter passes without hiding implemented APIs.

- [ ] node-sqlite-session-build-portability - Prove the SQLite session build is portable
  After: node-sqlite-finalize-dirty-scope
  Problem: Enabling `rusqlite`'s `session` feature pulls in `libsqlite3-sys` bindgen/libclang and new SQLite session FFI symbols.
  Context: `crates/perry-stdlib/Cargo.toml`, `Cargo.lock`, `ffi::sqlite3session_*`, `ffi::sqlite3changeset_apply`, and clean Linux/macOS CI images.
  Reference: Session feature dependency changes in the current dirty worktree.
  Acceptance: Either CI/base images are proven to build the session feature, or the implementation removes the extra bindgen/libclang requirement while `cargo check -p perry-stdlib --features database-sqlite` passes.

- [ ] node-sqlite-main-rebase-proof - Rebase and verify SQLite against current origin/main
  After: node-sqlite-regen-api-docs, node-sqlite-clear-stale-parity-gaps, node-sqlite-session-build-portability
  Problem: The branch is dirty and far behind `origin/main`; fresh build and parity evidence must be captured after conflict resolution.
  Context: Rebase the five committed SQLite commits plus finalized dirty follow-up onto current `origin/main`.
  Reference: `git rev-list --left-right --count origin/main...HEAD` from the SQLite worktree.
  Acceptance: Worktree is clean after rebase, SQLite parity fixtures pass, and focused cargo checks for `perry-stdlib --features database-sqlite`, runtime/codegen/api-manifest, and manifest consistency pass.

- [x] typedarray-byte-helper-live-validation - Reuse live typed-array validation in byte helper callers
  Problem: Byte helpers such as `typed_array_bytes` and `typed_array_bytes_mut` can still trust stale `TYPED_ARRAY_REGISTRY` entries outside the new `NativeMemory` path.
  Context: `crates/perry-runtime/src/typedarray.rs`, `strict_typed_array_from_raw`, `typed_array_bytes(_mut)`, and callers such as `crypto.randomFillSync` in `object/native_module_dispatch.rs`; keep GC side-table cleanup in the separate GC pass.
  Reference: Review finding on stale/wrong registry entries outside `NativeMemory`.
  Acceptance: Forged or finalized old-arena typed-array registry entries are rejected through a non-`NativeMemory` byte-helper caller, and existing native-memory safety tests still pass.

- [x] stable-hash-native-memory-tags - Give native-memory HIR nodes unique stable hash tags
  Problem: New native-memory hash tags collide with existing `GetIterator` and `ForOfToArray` discriminants.
  Context: `crates/perry-hir/src/stable_hash/expr.rs` tags for `NativeArenaView`, `NativeMemoryFillU32`, `GetIterator`, and `ForOfToArray`.
  Reference: Review finding on duplicate tags `11238` and `11243`.
  Acceptance: Stable hash tags are unique, with a regression guard that detects duplicate `tag(h, N)` discriminants.

- [x] generic-podview-typevar-monomorph - Preserve bare type parameters in `NativeArena.podView<T>()`
  Problem: `podView<T>()` can resolve bare constrained type params through `extract_ts_type_with_ctx`, embedding the constraint instead of `TypeVar("T")`.
  Context: `try_native_arena_public_api`, `bare_type_param_type_arg`, `Expr::NativePodView.view_type`, and monomorph substitution.
  Reference: Review finding on `crates/perry-hir/src/lower/expr_call/intrinsics.rs:655`.
  Acceptance: A generic `T extends PerryPod<any>` function using `arena.podView<T>()` specializes to concrete POD layouts in HIR/codegen tests.

- [x] native-memory-generic-operand-regression - Prove generic calls inside NativeMemory operands before broad traversal changes
  Problem: Review suspected generic calls nested inside `NativeMemory.fillU32` and `NativeMemory.copy` operands may not be discovered or rewritten.
  Context: `crates/perry-hir/src/monomorph/driver.rs`, `update_call_sites.rs`, `NativeMemoryFillU32`, and `NativeMemoryCopy`.
  Reference: Review finding on wildcard fallthrough in monomorph traversal.
  Acceptance: Add focused tests for `NativeMemory.copy(makeView<T>(), other<T>())` and `NativeMemory.fillU32(makeView<T>(), value<T>())`; only change traversal code if the regression fails.

- [x] final-native-memory-evidence - Run final native-memory proof on the current branch
  After: typedarray-byte-helper-live-validation, stable-hash-native-memory-tags, generic-podview-typevar-monomorph, native-memory-generic-operand-regression
  Problem: The MVP needs one end-to-end signal that the public native-memory surface is safe and usable after the focused fixes land.
  Context: Current `origin/main`, the separate GC pass if it has landed, runtime/HIR/codegen tests, and `native-abi-proof` workloads; use Python 3.11+ for the proof runner if local `python3` is too old.
  Reference: Latest xhigh review findings across native-memory safety, compiler stability, and integration.
  Acceptance: After a fresh fetch/rebase if needed, `git rev-list --left-right --count origin/main...HEAD` reports `0 N`, `git status --short` has no unintended files, and runtime/HIR/codegen tests plus `native-abi-proof --gate` pass.

- [x] stream-compose-data-flow - Wire stream.compose stages into a real data pipeline
  Problem: `stream.compose(...stages)` returns a fresh Duplex shape stub and ignores the supplied stream or transform stages.
  Context: `crates/perry-runtime/src/node_stream_constructors.rs`, `node_stream_pipeline.rs`, `node_stream_iter_helpers.rs`, and compose known failures in `test-parity/known_failures.json`.
  Reference: https://github.com/PerryTS/perry/issues/3232; DeepWiki repo/question: `nodejs/node` - "How does Node.js implement stream.compose data forwarding through multiple stream and Transform stages, including backpressure and chunk ordering?"
  Acceptance: `stream.compose(upper, exclaim)` transforms `Readable.from(["a", "b"])` into `A!B!` and related compose data-flow fixtures pass or move to precise follow-ups.

- [x] stream-compose-error-propagation - Propagate compose stage errors to the composite Duplex
  After: stream-compose-data-flow
  Problem: Errors thrown or emitted by middle compose stages never reach the returned composite because stages are not executed or subscribed.
  Context: Compose error handling in `crates/perry-runtime/src/node_stream_constructors.rs`, `node_stream_pipeline.rs`, and event/error state in `node_stream_readwrite.rs`.
  Reference: https://github.com/PerryTS/perry/issues/3233; DeepWiki repo/question: `nodejs/node` - "How does Node.js stream.compose propagate thrown or emitted errors from middle stages to the returned composite Duplex, including destroy and close behavior?"
  Acceptance: A middle Transform failure emits `error` on the composite and close/destroy behavior matches Node for the tested error paths.

- [x] stream-compose-callable-stages - Execute async function and iterable compose stages
  After: stream-compose-data-flow
  Problem: Callable stages such as async generator functions and async iterable sources are not run through `stream.compose()`.
  Context: `js_node_stream_compose_args()`, `compose_readable_snapshot()`, async iterator helpers, and fixtures like `with-async-fn`, `from-async-iterable`, and `async-handler-promise`.
  Reference: https://github.com/PerryTS/perry/issues/3234; DeepWiki repo/question: `nodejs/node` - "How does Node.js stream.compose execute async generator function stages, async iterable sources, and handler rejection paths?"
  Acceptance: `compose(Readable.from(["a", "b", "c"]), async function* ...)` emits transformed chunks in order, ends, and propagates async handler rejections according to Node behavior.

- [x] stream-compose-lifecycle-events - Emit completion lifecycle from composed Duplexes
  After: stream-compose-data-flow, stream-compose-error-propagation, stream-compose-callable-stages
  Problem: A Duplex returned by `stream.compose()` does not reflect source/stage `end`, `close`, or completion state after the composed chain drains.
  Context: Composite Duplex lifecycle state in `node_stream_constructors.rs`, `node_stream_pipeline.rs`, and `node_stream_readwrite.rs`.
  Reference: https://github.com/PerryTS/perry/issues/3235; DeepWiki repo/question: `nodejs/node` - "How does Node.js decide when a Duplex returned by stream.compose emits end, finish, close, and updates completion flags after the composed chain drains?"
  Acceptance: `compose(Readable.from([...]), new Transform(...))` emits `end` after data drains, with close/destroy/finished flags matching Node for the normal-completion fixture.

- [x] stream-promises-input-validation - Validate stream/promises finished and pipeline inputs
  Problem: `stream/promises.finished()` and `pipeline()` can accept invalid values and resolve or remain pending instead of rejecting with Node-compatible argument errors.
  Context: `crates/perry-runtime/src/node_submodules/stream_promises.rs`, direct pipeline paths, and Node error helpers for `ERR_INVALID_ARG_TYPE` and `ERR_MISSING_ARGS`.
  Reference: https://github.com/PerryTS/perry/issues/3070; DeepWiki repo/question: `nodejs/node` - "Where does Node.js validate stream/promises.finished and stream/promises.pipeline inputs, and how are ERR_INVALID_ARG_TYPE and ERR_MISSING_ARGS constructed for invalid calls?"
  Acceptance: `finished(123)`, `finished("x")`, `pipeline()`, `pipeline(123)`, and `pipeline(123, 456)` reject with the expected Node-shaped errors while existing valid stream paths still pass.

- [x] stream-promises-duplex-finished - Wait for both Duplex sides in finished()
  Problem: `finished(duplex)` resolves when either `end` or `finish` fires, but Node waits for both sides unless options disable one side.
  Context: `pending_finished_promise()` and option parsing in `crates/perry-runtime/src/node_submodules/stream_promises.rs`.
  Reference: https://github.com/PerryTS/perry/issues/3229; DeepWiki repo/question: `nodejs/node` - "How does Node.js stream/promises.finished track readable and writable completion for Duplex streams, and how do readable:false and writable:false options alter resolution?"
  Acceptance: Default `finished(duplex)` stays pending after only `finish` or only `end`, while `{ readable: false }` and `{ writable: false }` resolve on the selected side.

- [x] stream-promises-pipeline-return - Resolve pipeline with terminal async function return value
  Problem: `stream/promises.pipeline()` always fulfills with `undefined`, even when the terminal async function returns a value.
  Context: `stream_promises_pipeline_callback()` in `crates/perry-runtime/src/node_submodules/stream_promises.rs` and collected pipeline completion in `node_stream_pipeline.rs`.
  Reference: https://github.com/PerryTS/perry/issues/3230; DeepWiki repo/question: `nodejs/node` - "How does Node.js stream/promises.pipeline capture and resolve with the return value of a terminal async function stage while stream-to-stream pipelines resolve undefined?"
  Acceptance: `await pipeline(Readable.from(["a", "b"]), async source => "AB")` fulfills with `AB`, while stream-to-stream pipeline success still fulfills with `undefined`.

- [x] stream-readable-wrap-bridge - Implement Readable.wrap old-style stream bridging
  Problem: `Readable.prototype.wrap(oldStream)` is a chainable no-op, so old-style `data` and `end` events never reach the modern Readable wrapper.
  Context: `crates/perry-runtime/src/node_stream_readwrite.rs::readable_methods()`, `duplex_methods()`, `node_stream.rs::ns_chain1()`, and EventEmitter listener wiring.
  Reference: https://github.com/PerryTS/perry/issues/3341; DeepWiki repo/question: `nodejs/node` - "How does Node.js Readable.prototype.wrap bridge old-style streams into modern Readable streams, including data, end, error, pause, resume, and pipe behavior?"
  Acceptance: `wrap-old-stream` prints `joined: wrapped`, `wrap-pipe-chain` forwards into pipe output, and the bridge wires at least `data`, `end`, and `error` without regressing existing Readable and pipe fixtures.

- [x] stream-readable-reduce-empty - Reject empty Readable.reduce without an initial value
  Problem: `Readable.from([]).reduce(fn)` without an initial value resolves `undefined`, but Node rejects with a TypeError.
  Context: `crates/perry-runtime/src/node_stream_iter_helpers.rs::ns_iter_reduce()` and iterator-helper parity fixtures under `test-parity/node-suite/stream/iter-helpers/`.
  Reference: https://github.com/PerryTS/perry/issues/3415; DeepWiki repo/question: `nodejs/node` - "How does Node.js implement Readable.prototype.reduce for empty streams, especially the no-initial-value TypeError and seeded empty-stream behavior?"
  Acceptance: Empty unseeded reduce rejects with Node-shaped `TypeError`, while seeded empty reduce and non-empty unseeded reduce continue to resolve like Node.

- [x] stream-compose-array-form-data-flow - Fix stream.compose array-form data flow
  After: stream-compose-data-flow
  Problem: The remaining `compose-array-form` parity fixture duplicates array-source output: Node prints `composed via spread: AB`, while Perry prints `composed via spread: A,B,A,B`.
  Context: `crates/perry-runtime/src/node_stream_constructors.rs::js_node_stream_compose_args()`, collected/classic compose routing in `node_stream_pipeline.rs`, `test-parity/node-suite/stream/compose/compose-array-form.ts`, and `test-parity/known_failures.json`.
  Reference: Final Clayloop verification on 2026-05-31; `./run_parity_tests.sh --suite node-suite --module stream --filter compose/` still reports `node-suite/stream/compose/compose-array-form` as an output mismatch.
  Acceptance: `compose/compose-array-form` parity passes and is removed from `known_failures.json`, while `compose/multiple-transforms`, `compose/single-transform`, and callable compose fixtures still pass.

- [x] stream-compose-source-replay-dedup - Avoid duplicate output in source-headed multi-stage compose chains
  After: stream-compose-data-flow, stream-compose-lifecycle-events
  Problem: Source-headed compose chains with three stages or a passthrough middle stage replay the final output twice; Node emits one `got: <A>`, while Perry emits two identical `got: <A>` lines.
  Context: `compose_readable_snapshot()`, tail data/end listener wiring in `node_stream_pipeline.rs`, `test-parity/node-suite/stream/compose/three-stages-order.ts`, `test-parity/node-suite/stream/compose/with-passthrough-mid.ts`, and `test-parity/known_failures.json`.
  Reference: Final Clayloop verification on 2026-05-31; `./run_parity_tests.sh --suite node-suite --module stream --filter compose/` still reports `three-stages-order` and `with-passthrough-mid` output mismatches.
  Acceptance: `compose/three-stages-order` and `compose/with-passthrough-mid` parity pass and are removed from `known_failures.json`, while `compose/lifecycle-normal` and `compose/multiple-transforms` still pass.
