# Object metadata and enumeration attribution (#12015)

Two changes reduce instructions on all seven real programs: reuse seeded ConstFn
shape facts at method-record publication, and borrow shape key metadata during
enumeration. Qs stringify improves 4.02%; the method-factory micro improves
39.9% and for-in improves 20.8%. No retained shape storage is added.

Initial base: `2ebeca0e8d9fe6e47b9d30a478bbb7a122c9bcbd`. Measured main:
`3028a94186379cafb4c4101c9bb747cc9b6f85a4`; measured implementation:
`07a6670882f24a9eea3df8c740714f73f6f925da`, merged with that main. The final
fetch found `9d9c3f206e`, which adds only a changelog fragment; it is merged too.
The final counter-isolation edit is entirely `cfg(test)` and has no non-test
expansion. Documentation and harness edits do not change measured product code.

Issue [#12015](https://github.com/PerryTS/perry/issues/12015) was read without
changes. The shape owns receiver facts. A method site may remember `(ShapeId,
slot)` and validate the shape on every use before loading the receiver's current
value. RegExp, `object/private*`, statepoint lowering and call/apply/bind
implementations were not edited. No versions were changed.

The CPU acceptance checks pass. The literal RSS requirements are **not fully
met**: four medians rise by at most 1 MiB, and qs stringify falls by 1.34 MiB,
outside a symmetric ±1 MB window in the beneficial direction. The report does
not claim every RSS median is negative. Descriptor reflection remains an
optimization opportunity; its micro increases 0.19%.

## Retained attribution (historical evidence)

The supplied attribution is from `aef2f031910b7df14b80e893e4471c5439c9f909`, not this base. Three cycle-weighted recordings per workload were pooled. Percentages below are the self-time **charged to this bucket**, extracted from every function’s `all_mapping_parts_json`; inclusive totals and a function’s other bucket charges are excluded. Only the six functions TSV and six analysis JSON files were extracted. The equal-workload bucket mean was 7.61%. These are opportunity shares, not predicted savings.

| Program | Bucket share | Leading functions (bucket self-time) |
|---|---:|---|
| commander | 4.679% | `class_registry::class_meta::is_anon_shape_class_id` 0.393%; `exotic_expando::expando_clear_on_alloc` 0.270%; `prop_plan::read_plan_lookup` 0.225%; `shapes::shape_descriptor_intern_with_special_mode` 0.224%; `dictionary::is_dictionary` 0.203% |
| fastify | 8.566% | `class_registry::class_meta::is_anon_shape_class_id` 0.569%; `key_attrs::object_key_entry_filtered` 0.451%; `descriptor_state::class_instance_set_may_intercept` 0.371%; `iterator_prototypes::note_iterator_prototype_exposed` 0.333%; `shapes::shape_descriptor_intern_with_special_mode` 0.318% |
| qsparse | 7.596% | `exotic_expando::expando_lookup` 0.659%; `class_registry::class_meta::is_anon_shape_class_id` 0.544%; `native_module::callable_exports::bound_native_callable_module_and_method` 0.369%; `key_attrs::object_key_entry_filtered` 0.288%; `shapes::stamp_object_shape_id_with_carrier_note` 0.267% |
| qsstr | 8.979% | `static_shapes::finalized_constfn_facts` 1.211%; `shapes::shape_descriptor_intern_with_special_mode` 0.852%; `class_registry::parent_static::is_class_object_ptr` 0.841%; `native_module::callable_exports::bound_native_callable_module_and_method` 0.547%; `shapes::birth_stamp_object_shape` 0.474% |
| tsc | 6.654% | `shapes::shape_descriptor_intern_with_special_mode` 0.597%; `class_registry::class_meta::is_anon_shape_class_id` 0.403%; `key_attrs::object_key_entry_filtered` 0.356%; `field_rep_store::checked_slot_bits` 0.326%; `canonical_keys::probe_node` 0.224% |
| zod5k | 9.197% | `shapes::shape_descriptor_intern_with_special_mode` 0.906%; `class_registry::class_meta::is_anon_shape_class_id` 0.751%; `field_rep_store::checked_slot_bits` 0.579%; `live_slots::object_live_slot_count` 0.446%; `native_module::bound_native_method_length` 0.403% |


The bucket includes shape creation and receiver metadata used by reads and stores, not just Object.keys. In qs stringify, `finalized_constfn_facts` and its callers dominate; optimizing only a keys loop would miss that mechanism.

## Fresh profiles on the measured main

Linux qb6, Node 26.5.1, symbol-bearing release compiler and both static wrapper
archives rebuilt together. Program compilation used `PERRY_KEEP_SYMBOLS=1`,
`PERRY_NO_AUTO_OPTIMIZE=1`, `PERRY_NO_CACHE=1`. Each program below has a fresh
`perf record -e cycles:u -F 1999 -g` recording. These single recordings guide
mechanism selection; their percentages are not interleaved instruction savings.

For fresh source attribution, 270 historical functions whose mapping was
unambiguously #12015 were selected. Eleven shared/mixed helpers were excluded,
rather than charging all their new time to this bucket. The table reports whole
function self-cycle percentages for that subset, not a reconstructed complete
fresh bucket share. Nearest JS callers sum disjoint selected leaf samples, never
inclusive stack totals. Retained ELF `JsFunctionInfo` source records identify
closures; package source identifies class methods lacking those records.

| Program | Fresh leading metadata functions: whole-program self cycles |
|---|---|
| tsc | `key_attrs::object_key_entry_filtered` 0.66%; `class_registry::class_meta::is_anon_shape_class_id` 0.43%; `shapes::shape_descriptor_intern_with_special_mode` 0.30%; `object_keys` 0.28%; `alloc_basic::object_alloc_unpublished` 0.28% |
| zod5k | `shapes::shape_descriptor_intern_with_special_mode` 1.00%; `shapes::try_birth_stamp_preinstalled_shape` 0.80%; `class_registry::registration::is_class_id_registered` 0.50%; `field_rep_store::checked_slot_bits` 0.50%; `live_slots::object_live_slot_count` 0.42% |
| qsparse | `exotic_expando::expando_lookup` 0.62%; `key_attrs::object_key_entry_filtered` 0.43%; `class_registry::class_meta::is_anon_shape_class_id` 0.39%; `live_slots::object_live_slot_count` 0.37%; `shapes::shape_descriptor_intern_with_special_mode` 0.36% |
| qsstr | `shapes::shape_descriptor_intern_with_special_mode` 1.66%; `shapes::try_birth_stamp_preinstalled_shape` 0.81%; `static_shapes::finalized_constfn_facts` 0.71%; `static_shapes::finalize_constfn_static` 0.58%; `shapes::birth_stamp_object_shape` 0.54% |
| commander | `class_registry::class_meta::is_anon_shape_class_id` 0.51%; `prop_plan::read_plan_lookup` 0.44%; `shapes::try_birth_stamp_preinstalled_shape` 0.38%; `live_slots::object_live_slot_count` 0.19%; `native_module::callable_exports::bound_native_callable_module_and_method` 0.13% |
| fastify | `class_registry::class_meta::is_anon_shape_class_id` 0.83%; `object_keys` 0.65%; `key_attrs::object_key_entry_filtered` 0.65%; `canonical_keys::probe_node` 0.50%; `key_attrs::keys_attrs` 0.47% |

| Program | Leading nearest compiled JS callers (selected leaf cycles) | Driving JS operations |
|---|---|---|
| tsc | export helper `__16` 0.649%; `createNodeArray` (`__4740`) 0.565%; `forEachChildRecursively` (`__5860`) 0.379% | Export helper walks `for (var name in all)` and defines `{get: all[name], enumerable: true}`. Node arrays acquire `pos`, `end`, `hasTrailingComma`, `transformFlags`; traversal repeatedly reads queue and node metadata. |
| zod5k | `ZodObject._parse` 1.548%; `ZodString._parse` 1.130%; `ZodNumber._parse` 0.752% | `ZodObject._getCached` calls `this._def.shape()` and `util.objectKeys(shape)` (index.mjs:2344–2349). `_parse` walks `ctx.data` with for-in (2367), then `shape[key]`, `ctx.data[key]`, `key in ctx.data`, and allocates status/value pairs (2374–2381). String/number parsers read definition/check metadata and create issue/status records. |
| qsparse | `parseQueryStringValues` (`parse.js::__15`) 1.616%; decoder (`utils.js::__30`) 1.101%; weak-channel `set` (`__16`) 0.923% | Parser builds an own-key record and writes dynamic decoded keys; decoder calls replace and the selected decoder. Weak-channel `set` creates a WeakMap or fallback map and returns/uses captured method records. |
| qsstr | weak-channel `set` (`__16`) 4.613%; `getSideChannel` (`__11`) 1.353%; `getSideChannelWeakMap` (`__11`) 1.211% | `side-channel*/index.js` factories create `assert/delete/get/has/set` records capturing channel data. `set(key,value)` lazily creates its WeakMap/fallback. Every fresh method record repeats completed ConstFn publication. |
| commander | `Command` constructor 1.646%; `parseOptions` 0.630%; `setOptionValueWithSource$generic` 0.572% | `lib/command.js:21` initializes many receiver fields, including `_optionValues={}` and `_optionValueSources={}` (39–40). `setOptionValueWithSource` (944) writes both by dynamic key (948,950); `parseOptions` repeatedly reads receiver methods and option metadata. |
| fastify | light-my-request `Request` (`request.js::__17`) 2.774%; `getNullSocket` (`response.js::__43`) 0.826%; `Response` (`__11`) 0.707% | Request constructs Readable options, URL/header/payload and `_lightMyRequest` metadata; Response creates its own response state. `getNullSocket` allocates a Writable options record containing `write`. `serializeHeaders` (`__46`, 0.281%) explicitly calls `Object.keys(headers)` and reads/writes each value. |

The qs stringify factory path is the largest supported first fix. Its fresh
`finalized_constfn_facts` self time is 0.71%, and the repeated publication also
pays parsing/allocation and registry helpers. Enumeration's key-array probes
and descriptor checks form the next supported mechanism across workloads.

## What belongs on the shape, and what belongs at the site

Own key order, logical prefix count, live inline count, attributes, hole state,
receiver kind, prototype identity, field representation and immutable ConstFn
body identity belong on the shape. `ShapeRecord` and its existing extension
already carry the facts used here. Shared backing length is not the receiver's
key count. Dictionary shapes explicitly delegate their owned list to the receiver.

The existing non-enumerable attribute summary proves the ordinary all-enumerable
case. Classless per-slot attributes resolve slow keys; class declarations retain
effective descriptor lookup because ancestor and derived declarations can share
a public name. Accessor/proxy paths require snapshot and per-key rechecks: a
getter can delete or hide a later key. Neither fix removes those checks.

An ordered own-enumerable slot list or descriptor-free/data-only bit would belong
on the shape, rather than an owner side table. They were not added: reuse of
existing storage already improves the measured programs without retaining extra
lists. Descriptor-specific specialization remains unimplemented.

A repeated method/value read may memoize `(ShapeId, slot)` at the call site,
validate the receiver shape, then load its current closure/value. Captures belong
to the receiver, not to a cached body or another receiver. This lane adds no
method-site memo and does not change bind. ConstFn publication instead reuses
existing shape-owned body facts while proving the current receiver slots.

## Fixes and isolated measurements

1. **Seeded ConstFn publication** (`95c763bd83`). A seeded target already owns
   its complete key prefix and body list. Publication borrows that body list and
   proves the receiver key identity, counts, kind, prototype, representations,
   generation, hole state, summaries, mask and current closure slots. The
   immutable `FN_COMPILED_BODY` ABI bit skips redundant native/bound/class
   registry admission. Native permanent-image fixtures retain general admission;
   unseeded/worker reconstruction retains packed-name parsing and rooted mint
   revalidation. Rebindable-this capture checks remain. No new production
   counter, latch, registry, cache or name check is introduced.

2. **Shape key views** (`3055f08978`). The key/live-count helper and serializer
   twin borrow `ShapeRecord` rather than lifting a complete descriptor.
   `ObjectKeys::get` reads internal dense storage using the logical prefix,
   forwarded backing/front and hole normalization. Object.keys uses the existing
   shape non-enumerable summary, and classless slow keys use existing slot
   attributes. Values/entries use the internal key read while retaining their
   snapshot and descriptor rechecks. Class effective-descriptor resolution and
   private/WASI filters remain.

A third arm, refreshed main plus only the ConstFn implementation/test files, was
built in its own `fix1-target`. Five interleaved main/fix1/combined trials isolate
the fixes; all QS outputs match Node. This cohort is separate from the acceptance
cohort below, so its main median differs slightly.

| Isolated qs stringify arm | Instructions:u median | Max RSS median (KiB) | Instruction change from preceding arm |
|---|---:|---:|---:|
| main | 75,548,423,618 | 65,464 | — |
| fix1 | 72,518,728,690 | 64,268 | -4.010% |
| head | 72,500,498,276 | 63,712 | -0.025% |

Fix 1 alone cuts the factory micro from 5471.60 to 3287.24 instructions/op
(39.92%) and QS instructions by 4.010%. Fix 2 cuts the same-cohort keys micro by
3.38%, values/entries by 0.95%, for-in by 20.76%, spread/assign by 1.53%; its
additional QS reduction is 0.025%. The descriptor micro rises 0.19%; factory
changes only 0.001% after fix 2. Both isolated RSS medians decrease (1196 then
556 KiB), though process-level RSS is noisier than instructions.

## Micro table: measured main, combined head and Node

Every operation uses a parameter receiver (`any`); literals appear only at the
caller or the factory being measured. One operation is one outer loop body.
Keys includes a non-enumerable property; values/entries invokes both APIs;
descriptors invokes getOwnPropertyDescriptor plus getOwnPropertyNames; for-in
includes computed value reads; spread/assign invokes both; the factory creates
a captured five-method record and reads two methods through a parameter receiver.

Five serial interleaved trials per arm measure
`(instructions(110000) - instructions(10000)) / 100000`. This removes startup
consistently. Every stdout and stderr at both counts matches Node 26.5.1. Node's
JIT/GC makes its rows less stable; these are the observed medians, not a universal
per-operation cost.

| Mechanism | Main instr/op | Head instr/op | Node 26.5.1 instr/op | Head Δ |
|---|---:|---:|---:|---:|
| keys | 6,057.48 | 5,852.48 | 260.21 | -3.384% |
| values_entries | 23,046.39 | 22,827.50 | 4,544.95 | -0.950% |
| descriptors | 7,944.43 | 7,959.41 | 506.44 | +0.189% |
| for_in | 22,010.77 | 17,441.82 | 219.63 | -20.758% |
| spread_assign | 26,039.06 | 25,641.14 | 528.55 | -1.528% |
| method_factory | 5,471.58 | 3,287.24 | 321.29 | -39.922% |

## Real-program acceptance cohort

Five interleaved trials alternate main/head order. Instructions use
`perf stat -e instructions:u`; max RSS uses `/usr/bin/time -f %M` (KiB).
All seven match Node stdout and stderr on every trial. Node's
`MODULE_TYPELESS_PACKAGE_JSON` launcher warning is disabled with the specific
`--disable-warning=MODULE_TYPELESS_PACKAGE_JSON` flag; program diagnostics are
still compared. No receiver code, driver or expected output was normalized.

| Program (arguments) | Main instructions:u | Head instructions:u | Δ | Main RSS KiB | Head RSS KiB | RSS Δ KiB |
|---|---:|---:|---:|---:|---:|---:|
| tsc (1) | 10,005,276,788 | 9,999,023,692 | -0.0625% | 214,124 | 214,584 | +460 |
| zod5k (5000) | 14,858,959,001 | 14,780,685,138 | -0.5268% | 55,256 | 55,036 | -220 |
| qsparse (20000, 1000) | 28,059,493,456 | 27,924,388,397 | -0.4815% | 57,436 | 57,620 | +184 |
| qsstr (20000, 1000) | 75,538,111,230 | 72,498,513,505 | -4.0239% | 65,100 | 63,728 | -1,372 |
| commander (5000, 200) | 7,677,490,478 | 7,655,955,012 | -0.2805% | 54,048 | 54,096 | +48 |
| hello (none) | 1,385,483 | 1,385,329 | -0.0111% | 15,664 | 15,644 | -20 |
| fastify (500, 30) | 6,750,051,501 | 6,741,272,650 | -0.1301% | 138,220 | 139,244 | +1,024 |

Every instruction median is negative and below main +0.1%; QS clearly improves.
The tiny hello/tsc deltas should not be presented as large practical wins.
Fastify is exactly +1 MiB in this cohort; tsc, qs parse and commander also have
small positive RSS deltas. QS stringify is −1.34 MiB, outside a literal symmetric
window. A separate isolated cohort also shows a QS RSS reduction larger than
1 MiB. No padding allocation or selective trial replacement was used to force
the RSS window. The original all-program-negative RSS goal is unproven.

## Correctness, sabotage and baseline comparison

Builds use `cargo build --release -j 16 -p perry -p perry-runtime
-p perry-runtime-static -p perry-stdlib-static`, with each source and target
separate. Runtime, stdlib and codegen use release tests, `RUST_TEST_THREADS=1`
and `--test-threads=1`.

| Suite | Main | Head | New failures |
|---|---|---|---:|
| runtime | 5113 passed, 5 ignored; isolated/integration tests pass | 5118 passed, 5 ignored; isolated/integration tests pass | 0 |
| stdlib | 250 passed, 2 failed | 250 passed, same 2 failed | 0 |
| codegen | 2046 unit passed, 1 ignored; integrations/doc tests pass | same | 0 |

The failure list for **both** stdlib arms is:

- `runtime_thread_exit_tests::symbols_tests::thread_exit_releases_the_threads_symbol_side_table_entries`
- `runtime_thread_exit_tests::thread_exit_releases_the_threads_closure_side_table_entries`

Five new runtime tests establish non-vacuous premises: successful seeded promotion
with distinct live captures and zero parsing/native probes; refusal of a distinct
shape prefix; shared backing versus logical prefix and distinct live count;
consumed front and a hole; dictionary-owned keys. Test-only counters use
`per_test_global!` inside `cfg(test)` and retain no production storage.

Four temporary production-path sabotage variants each went red and were restored:
bootstrap parsing and native admission each fail the zero-probe test; removing
both key-identity guards fails the prefix-refusal test; corrupted prefix bound,
front offset and dictionary fallback fail all three key-view tests. All five new
tests were observed failing under their designated sabotage and passing restored.

`test-files/test_gap_12015_metadata_factory.ts` covers captured factories, method
and extracted value reads, keys/values/entries, method replacement, getter
replacement, deletion and assign/spread. `test_gap_12015_metadata_keys.ts` covers
shared prefixes, returned-array mutation isolation, numeric key order,
non-enumerable/read-only attributes, delete/readd, visibility changes, a getter
that deletes/hides/adds later keys through a parameter receiver, array attributes
and inherited/derived class field names. Both match Node stdout and stderr on
both arms. The literal-binding getter-deletion variant exposed a pre-existing
main lowering defect; parameter mutation is used to isolate enumeration behavior.

## Lint, formatting and GC evidence

The unmodified `scripts/run_lint_gates.sh` exits 4 on **both** arms before running
its commands: checkout-step extraction yields zero commands for `lint`,
`warnings` and `check` (`Checkout repository (inline git)`). These are both exact
runner failure lists. `cargo fmt --all -- --check` exits 1 on both arms, with the
same sole file `crates/perry-runtime/src/buffer/view_tests.rs:262`; lane files
have no formatting failures.

To get meaningful gate coverage despite that baseline runner defect, a temporary
copy outside each source preserves ROOT and removes only the three checkout
setup steps from the in-memory workflow before extraction. It runs the actual
117 script and 11 compile commands, with two CI-only skips. No real workflow,
runner or gate is changed. The complete supplemental failure lists follow.

| Supplemental command | Main failure list | Head failure list |
|---|---|---|
| Type-check Windows runtime and stdlib — `cargo xwin check -p perry-runtime -p perry-stdlib --target x86_64-pc-windows-msvc` | fail | fail |
| Check formatting — `cargo fmt --all -- --check` | fail | fail |
| File size limit — `./scripts/check_file_size.sh` | fail | fail |
| Local binding type-proof audit — `python3 scripts/local_binding_type_audit.py` | fail | fail |
| GC store-site inventory — `python3 scripts/gc_store_site_inventory.py` | fail | fail |
| Address-classification audit — `python3 scripts/addr_class_inventory.py` | fail | fail |
| String payload-access inventory — `python3 scripts/string_payload_access_inventory.py` | fail | fail |
| SSO string-unboxing inventory — `python3 scripts/sso_unbox_inventory.py` | fail | fail |
| Registry lifetime audit (#11511) — `python3 scripts/registry_lifetime_check.py` | fail | fail |
| Native runtime ABI consistency — `python3 scripts/runtime_abi_check.py --check-wasm-abi` | fail | fail |
| Raw-handle debt ratchet — `python3 scripts/raw_handle_debt.py` | fail | fail |
| Native-handle ledger ratchet — `python3 scripts/native_handle_ledger.py --self-test` | fail | fail |
| Unrooted-local shape ratchet — `python3 scripts/unrooted_local_shape.py --check` | fail | fail |
| Test registration (dark tests) — `python3 scripts/check_test_registration.py --self-test` | fail | fail |
| Test registration (dark tests) — `python3 scripts/check_test_registration.py` | fail | fail |
| Per-test global sinks — `python3 scripts/global_sink_isolation.py` | fail | fail |
| warnings: rustc warnings (product) — `RUSTFLAGS="-D warnings" cargo check -p perry --bins` | fail | pass |
| warnings: rustc warnings (host-compatible, all targets) — `RUSTFLAGS="-D warnings" cargo check --workspace --all-targets [host UI exclusions]` | fail | pass |
| check: Clippy (product) — `cargo clippy -p perry --bins` | fail | pass |
| check: Clippy (host-compatible) — `cargo clippy --workspace [host UI exclusions]` | fail | pass |
| check: Lean dependency boundaries and helper compatibility — `cargo check --locked -p perry --no-default-features --features dev-cli` | fail | pass |
| check: Check for API docs drift — `git diff --quiet -- docs/src/api/reference.md docs/api/perry.d.ts` | fail | fail |

Main has 22 failing commands; head has 17, all also present on main. Failed
inventory diagnostics are identical, including file-size, store sites, address
classes, string payload/SSO, registration and local/handle debt. After isolating
the two test counters, global-sink diagnostics and the asserted-static count are
also identical (65, baseline 60). The only fmt diff is the baseline buffer test.
The five baseline-only development compile gates report `E0425` for
`monomorphize_modules`; both release builds independently compiled perry-hir and
perry successfully. No improvement in those compile gates is attributed to this
runtime lane. Full command arguments and logs, including host UI exclusions,
are retained in the evidence. No baseline inventory or allowlist was raised.

The final runtime suite was rerun after counter isolation and passed with the
same 5118/5 totals. The exact runner's checkout extraction and fmt lists remain
unchanged. All five unit sabotage checks were repeated successfully on the isolated
counters, followed by a green restored focused run.


The checker self-test, allocation/poll/immovable audits, archive-symbol proof,
curated native generation and native dominance check all pass. Native covers
7722 functions / 299 modules, 71009 safepoints, 52300 nonempty live bundles,
145388 relocates and 134101 (safepoint,root) pairs; unrooted and stale counts are
zero, with no immovable exemptions, and 40/40 seeded violations are caught.
Curated shadow dominance and unrooted-alloca checks also pass: 7723 functions,
299 modules, 36044 root stores, zero dominance/alloca violations, and 40/40 seeds
caught. All CI breadth floors and budgets are preserved.

Curated shadow stale-register checks also report zero. The private launcher
forks the unchanged per-function engine across 16 processes, retaining the full
parsed call graph, original CLI floors, budgets and aggregation. It checks all
7723 functions and agrees with serial results on 50 sampled functions. The
original serial corpus scan also completed with zero stale uses. Its owned
orchestrator was stopped by PID to avoid repeating dependency work already
running independently; no checker or lowering source was edited.

Dependency native covers 6410 functions / 81 modules, 37379 safepoints, 30435
nonempty live bundles, 206634 relocates and 196868 (safepoint,root) pairs. It has
one unrooted alloc result at `js_get_string_pointer_unified` (within the existing
CI budget of three), zero stale uses and 40/40 caught planted violations.
Dependency shadow has 6410 functions, 81 modules, 20237 root stores, zero
dominance and unrooted-alloca violations, and 40/40 seeds caught. Its stale scan
reports one alloc-source use at the same sink, within the existing budget of
ten; serial agreement holds on 50 functions. No budget was raised. Remaining
codegen debt is outside this runtime lane and statepoint lowering is excluded.

All 18 required generation/audit/check operations have recorded successful
statuses in `gc-final-statuses.json`, including original stages 0–10, the curated
parallel stale check and independent dependency stages 12–17. Native, shadow,
dependency, checker self-tests and seeded-violation checks are green at the CI
floors and budgets. The launcher, both curated stale logs and complete commands
are retained for review.

## Reproduction and retained evidence

`scripts/perf_12015_lane.py --hostdir /root/codex-lanes/cx-meta` provides
`compile-main`, `compile-head`, `compile-fix1`, `profiles` and `compare` actions.
Main, combined and fix1 sources use `main-src`, `src`, `fix1-src`; targets use
`main-target`, `target`, `fix1-target`. No arm shares archives. Coherent compiler
and runtime stamps were verified; no fake stamp override was used. `PERRY_SKIP_BUILD=1`
prevents the benchmark harness from rebuilding an arm. HTTP's required no-auto
archives remain source-local and separate. All builds happened on Linux; none
happened on the Mac. Real-program dependencies are TypeScript 5.8.2, Zod
3.23.8, qs 6.16.0, commander 15.0.0, Fastify 5.12.5, light-my-request 6.6.0,
side-channel 1.1.1 and side-channel-weakmap 1.0.2. Driver, dependency manifest
and executable SHA-256 fingerprints are retained with the evidence.

Each SSH command exports PATH with `/opt/node-v26.5.1-linux-x64/bin` and cargo,
CARGO_TARGET_DIR, PERRY_RUNTIME_DIR **after** `bash -lc`, PERRY_WORKSPACE_ROOT and
RUST_TEST_THREADS. Disk space was checked before builds (≥25 GB). Drivers were
copied into the owned hostdir; authoritative drivers were not changed. Final
fetch is changelog-only relative to the measured main, so the performance/test
product inputs remain equivalent on the final merge.

Trial counters, outputs, fresh perf reports/source attribution, sabotage logs,
baseline/head test and gate logs are retained under
`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/meta-evidence/`.
The refreshed Git bundle is
`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/meta.bundle`.
Owned build targets are deleted after evidence collection.

Unfinished: satisfying the literal RSS window/all-negative RSS requirement;
descriptor reflection specialization; new method-site `(ShapeId,slot)` memos;
retained own-enumerable slot lists. The latter changes were not justified by
this lane's measured fixes, and would require their own correctness/construction
cost evidence. No work requires editing the excluded owner files.
