# Regex literal-site `.test` report

## Branch and SHA

- Branch: `perf/regex-literal-site-test`
- Base: `107d40adb9881ff5f91f94124e24907fcfea5796`
- Implementation commit: `2f799c7b0318b683bd0f320705ea6855f73fed85`
- Target remote: `fork/perf/regex-literal-site-test`

## Map and mechanism

- Ordinary regex literals still derive an identity from the address of a compiler-emitted private `i64` global, never from a hand-written constant (`crates/perry-codegen/src/expr/logical_collections.rs:61-79`). Ordinary lowering loads the interned pattern/flags handles and calls `js_regexp_new_site(pattern, flags, site)` (`logical_collections.rs:1382-1413`; runtime entry at `crates/perry-runtime/src/regex.rs:988-994`).
- A direct HIR `RegExpTest` whose receiver is exactly `Expr::RegExp` is the non-escaping shape: the literal node is consumed solely as this call's receiver, and only the boolean result is published. It lowers through `js_regexp_site_test_new`, captures `.test` before evaluating the argument, roots receiver and method across argument evaluation, then dispatches (`crates/perry-codegen/src/expr/instance_misc1.rs:1192-1226`). An escaping receiver such as `const r = /x/g; r.test(a); r.test(b)` remains on `js_regexp_new_site` plus the ordinary `js_regexp_test` route.
- `js_regexp_site_test_new` allocates and records one header on the cold evaluation, then reuses it on canonical hits (`crates/perry-runtime/src/regex/site_test.rs:157-200`). The site table owns a strong mutable raw root; its registered visitor marks and rewrites the header during evacuation (`site_test.rs:493-500`; registration at `crates/perry-runtime/src/gc/mod.rs:1003-1005`). The header therefore survives minors and cannot enter regex-death finalization while the site lives.
- Validation is performed on every evaluation. The realm's `RegExp.prototype`, canonical `test` closure, and own-slot index are recorded at intrinsic installation; the two heap values are representation-correct GC roots. The probe rejects a replaced/deleted/accessor `test` slot and any explicit receiver prototype (`crates/perry-runtime/src/object/regex_proto_thunks.rs:319-408`). A decline resolves the actual property and calls it generically (`site_test.rs:397-490`). Property Get remains before argument evaluation, so an argument that patches the prototype still invokes the method captured before that patch.
- Ordinary `js_regexp_test` implements stateful global/sticky `lastIndex` behavior (`crates/perry-runtime/src/regex.rs:1701 onward`). A fresh literal starts with zero on every evaluation, so the allocation-free dispatch resets the private cached header to zero immediately before its test (`site_test.rs:460-484`). The resulting write is unobservable: this exact receiver has no escaping reference, no `this` capture, and only the already-validated builtin sees it.
- The segment-view tier validates the same canonical prototype and calls `regexp_test_str_bounded` on a borrowed segment (`crates/perry-runtime/src/intl/segments_view.rs:350-394`; bounded matcher at `crates/perry-runtime/src/regex.rs:1661-1699`). It deliberately refuses global/sticky regexes and operates only after its receiver exists. Consequently it removed segment materialization but could not remove `g54.default()`'s per-grapheme RegExp construction; generic function-result dispatch is documented at `crates/perry-runtime/src/object/native_call_method/primitive_methods.rs:541-545`.
- For the real bundle shape, codegen recognizes `<zero-arg-call>().test(arg)` and preserves the actual call and member lookup (`crates/perry-codegen/src/expr/calls.rs:77-101,879-950`). Functions/closures are eligible to claim an active caller site only when HIR proves zero parameters, non-async, non-generator, and exactly one `return <regex literal>` statement (`crates/perry-codegen/src/codegen/function.rs:523-531`; `closure.rs:561-571`). The runtime resolves the actual callee's native entry on every call and pairs it with the site; only the exact factory identity may claim/reuse the header (`site_test.rs:202-252,321-395`). Reassignment, a rebound namespace member, a non-literal body, or a nested helper declines to the generic result path. Active identity frames have exception savepoints so caught throws cannot leave stale authorization.
- `[regex-diag]` now prints `site_test_no_alloc=` and `site_test_declined=(patched_prototype=...,callee_mismatch=...,non_literal=...)` in the whole line (`crates/perry-runtime/src/hot_diag.rs:196-203,374-424`). The no-allocation counter is incremented inside the construction entries that avoid the priced allocation. `new=` falls by the served count because a hit never enters `js_regexp_new_impl`.
- `PERRY_GC_CENSUS` now contains `regex.content_cache`, `regex.literal_sites`, and `regex.site_test_headers` (`crates/perry-runtime/src/gc/census.rs:565-604`; aggregation at `crates/perry-runtime/src/regex/site_test.rs:503-520`).

## Named correctness and sabotage coverage

- Codegen: `direct_literal_test_uses_the_site_header_and_post_get_dispatch`, `escaping_literal_is_not_transformed_and_keeps_one_stateful_receiver`, `direct_factory_call_records_function_identity_and_uses_the_caller_site`, and `namespace_member_factory_call_uses_the_member_wrapper` (`crates/perry-codegen/src/expr/regex_site_test_tests.rs:73-207`).
- Runtime: `direct_global_site_allocates_one_header_and_resets_last_index` compares a table with fresh generic `/x/g`; `direct_sticky_site_starts_each_evaluation_at_zero` checks `/x/y` anchoring and reset; `escaping_generic_global_header_carries_last_index_between_tests` proves the untransformed stateful case (`crates/perry-runtime/src/regex/site_test.rs:586-640`).
- Cross-function sabotage: `direct_factory_site_reuses_only_the_recorded_callee`, `nested_exact_factory_cannot_claim_a_different_callees_site`, and `namespace_member_factory_site_is_covered` cover direct `f()`, nested/non-literal decline, and namespace-member rebinding on the next call (`site_test.rs:699-813`).
- Prototype/rooting sabotage: `patched_regexp_prototype_test_declines_on_the_next_call`, `caught_throw_restores_an_orphaned_factory_site_frame`, and `site_header_root_is_rewritten_by_a_copying_minor` cover the next-call patch guard, exception cleanup, and copied-minor root rewriting (`site_test.rs:753-880`). The decline tests assert the individual reason buckets, not only the total.

## Gates

Completed after the final edits:

- Direct `rustfmt` over touched Rust files.
- `git diff --check`.
- `scripts/check_file_size.sh`: passed; `regex.rs` is below the 2,000-line ceiling.
- `python3 -m json.tool scripts/gc_runtime_root_holders.json`: passed.
- `python3 scripts/gc_runtime_root_holders.py --self-test`: passed, 90 planted declarations and 347 inventory entries.
- `python3 scripts/gc_runtime_root_holders.py`: passed, 1,371 holders scanned, 594 reached by registered scanners, 358 classified, 414 frontier-pinned, 152 scanners.
- `python3 scripts/gc_rekeyed_key_tables.py`: passed, 42 rekey sites, 25 registered prunes, 0 gaps.

Cargo history and disk stop:

- `cargo test -j4 -p perry-codegen` through `measure_lock.sh --build` passed before the last guard-only codegen edit: 1,452 unit tests passed, 1 ignored, followed by all integration and doc tests passing. The final tree was not rerun.
- `cargo test -j4 -p perry-runtime --release --lib -- --test-threads=1` initially passed (3,265 passed, 4 ignored) before the exception/counter additions. The final-tree attempt compiled successfully and ran 3,266 passing tests plus one new synthetic nested-wrapper test failure; that fixture's trivial Rust wrapper had been release-folded with its callee. The fixture was made observably distinct with an atomic side effect and `inline(never)`, but was not rerun.
- Immediately after that invocation, `df -g /` reported 9 GB available. The binding rule prohibits every further Cargo invocation below 12 GB, so no wait or additional build was attempted.
- Not run on the final tree: the runtime lib gate rerun, the codegen gate rerun, `cargo build --release -p perry-runtime --features wasm-host`, and `cargo build --release -p perry`.
- A fresh archive and `nm` check were not produced because the archive build was prohibited. No local cc CPU/RSS measurement was run.

## Predictions

For the supplied I6d 3,300-character reply:

- `new=`: 1,074,006 -> at most 10,000.
- `site_test_no_alloc=`: approximately 1,068,858 (one cold header means the exact value may be one lower for that site).
- `header_bytes`: 60,144,336 bytes -> approximately 0.3 MB.
- `ptr_ins` / `ptr_rm`: approximately zero at reply scale, apart from cold and unrelated regex objects.
- `test=`: unchanged at approximately 2,139,156.
- Turn CPU at 3,300 characters: -4% to -6%, from removing `js_regexp_new`, regex-death/finalization, and pointer-side-table work. Peak RSS should be lower; +1% to +10% remains acceptable under the campaign goal.

## Exact perrymaster request

This commit touches CODEGEN. Build the compiler from `2f799c7b0318b683bd0f320705ea6855f73fed85` on the I6d tree, or on the I7-view tree if all prerequisite picks apply, and perform a full cc bundle recompile; do not reuse the base bundle. From the resulting artefact, use `nm` to prove the new site-test runtime entry symbols are present and report the number of emitted call sites for `js_regexp_site_test_new`, `js_regexp_site_factory_call_value`, and `js_regexp_site_factory_call_method` (including the `g54.default().test(O)` site).

Run one identical 3,300-character reply and provide the entire `[regex-diag]` line plus the per-pattern table. Confirm the 12,807-byte emoji `/.../g` row is constructed once, built once, and tested approximately 1,068,858 times; report `new`, `site_test_no_alloc`, the three decline buckets, `header_bytes`, `ptr_ins`, `ptr_rm`, `test`, `test_global`, and compile/cache counters. Expected: `new <= 10,000`, `site_test_no_alloc ~= 1,068,858`, `header_bytes ~= 0.3 MB`, `ptr_ins/ptr_rm ~= 0`, and unchanged `test`.

Then run paired 5x3,300-character and 3x400-character comparisons against the base bundle with identical warmup, environment, inputs, and node-parity stop conditions. Report every turn's CPU and peak RSS. Expected 3,300-character turn CPU improvement is 4% to 6% and peak RSS is lower. Finally capture a perf draw and verify `js_regexp_new`, `regex_header_clear_dead_for_gc`, and the dead-owner regex path have disappeared from the top 25.
