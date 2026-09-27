# Package profile (Phase 3): where Perry's instructions go

**Question.** Phase 1 (`REPORT.md`, `scripts/package_bench.py`) measured *how much* slower real npm packages
compiled by Perry are than on Node. This report measures *where the instructions go*: per workload, which runtime
functions (self and inclusive), which runtime **entry point** the generated code called, and which **JS construct in
which package source function** caused it, rolled up into root-cause buckets and weighted by each workload's
excess over Node.

Generated tables with every workload's chains: [`profile/callgraph.md`](profile/callgraph.md) (JSON:
[`profile/callgraph.json`](profile/callgraph.json)). Call-shape floor probes: [`profile/floor.md`](profile/floor.md).
Re-run recipe at the end.

## Setup and validity

- **Compiler:** `origin/main` 36420d2e5 (perry 0.5.1654; branch head adds only harness files), `--release`
  build of `perry` + static wrappers. Workloads compiled with the harness's `compile` (plain `perry compile`,
  auto-optimize **on**, `PERRY_WORKSPACE_ROOT` set, so each program gets its own rebuilt runtime),
  `PERRY_KEEP_SYMBOLS=1` and `--debug-symbols` (DWARF line tables for generated code). Every binary passed the
  harness's liveness check: the package's own modules were compiled natively (`audit.json` census), no module was
  routed to a JS runtime, and no removed `perry-ext-*` binding symbol was present.
- **Host:** perrymaster (AMD Ryzen 7 7700X, shared and loaded). Only **instruction counts** and
  instruction-sampled profiles are reported; there are no wall-clock numbers from this host. Every `perf` run held
  the harness's measurement mutex (`/tmp/perry-bench-lock.d`).
- **Oracle:** Node 26.5.1 (`/opt/node-v26.5.1-linux-x64`). Every Perry run's stdout was checked byte-for-byte
  against Node's at both N before anything was attributed.
- **Sampling:** `perf record -e instructions:u -F <freq> --call-graph dwarf,16384`, about 12k samples at n2. Each
  workload was recorded at **n1 and n2**, and the n1 profile was subtracted from the n2 profile key by key.
  Every number below is therefore **instructions per iteration**, the same two-N method as the Phase-1 counts, so
  startup, package compilation and warm-up cancel. Sampled totals were scaled to the `perf stat` count; the scale
  factor is 0.97–1.13 for every workload except rate-limiter-flexible/get_penalty (1.34, async and short).
- **Call-graph method:** **DWARF** unwinding. Perry's generated code keeps frame pointers (`"frame-pointer"="non-leaf"`),
  but the auto-optimized runtime is rebuilt with an explicit `RUSTFLAGS` that drops `.cargo/config.toml`'s
  `-C force-frame-pointers=yes`, so FP chains break inside the runtime. `--call-graph lbr` is refused on this
  Zen 4 (`PMU Hardware or event type doesn't support branch stack sampling`). DWARF reached `main` on 56–100% of
  samples (`unwind→main` column; the lowest are axios, where deep async stacks exceed the 16 KB copy). The attribution only needs the chain up to the innermost generated-code frame,
  which is near the leaf, so a truncated root does not affect it.
- **node-forge** fails codegen on current main (#11450, repro added there), so its four workloads were profiled
  from the Phase-1 `PERRY_KEEP_SYMBOLS=1` binaries built at **2febf4214e**, marked † in the generated report.
- **Liveness of the subject:** for every workload the report lists a lower bound on the share of samples with the
  package's own compiled code on the stack (28–100%).

## The per-call floor: there is none on current main (item 3)

**`control/bare_loop` = 26 instr/iter; Node = 56.** Interim runs showed about 1,040–1,250. That figure was not a
call floor: it came from `it.n` re-read in the loop condition (a by-name read of an object that escaped into
`header()`), plus `Math.imul` in the original checksum helper. The phase-1 agent hoisted the bounds in 21c9f5438.
Measured on this build:

| control variant | Perry instr/iter | Node |
|---|---:|---:|
| original (first harness commit): `Math.imul` + `i < it.n` + `header(…, it)` | 1,178 | ~0 |
| `Math.imul` replaced by `mulFnv` | 936 | 55 |
| `i < it.n`, `it` from imported `iters()`, not passed to `header` | 69 | 55 |
| current: bounds hoisted | **26** | 56 |

With the bounds hoisted, LLVM inlines both calls. The 26 instructions are ToInt32 range guards, `cvttsd2si`,
int↔double conversions, the arithmetic, and an induction variable kept both as double and i32. The loop's GC
poll is 2 instructions (`cmpl $0, PERRY_GC_POLL_ARMED` + branch). There is no shadow-frame push/pop, no write
barrier and no boxing on this path.

**A real, non-inlined call** (`control/prop_read`, added by the phase-1 agent: `op()` reads `box.n` and calls
`mulFnv`) = **151 instr/iter vs Node 51**. Its header comment says ~1k; on this build it is 151. The breakdown
from the disassembly and the profile:

| piece | instr/iter | what |
|---|---:|---|
| `op` body | ~76 | 7-register prologue/epilogue; **~45 on parameter representation guards** (NaN-box tag tests + int32-exactness tests for `h` and `i`, which arrive as boxed doubles); the `box.n` read inline (global load + shape-id compare + slot load, ~8); ToInt32 range checks for `^` |
| `mulFnv$spec_b` | ~53 | the same parameter-guard pattern + the arithmetic |
| `js_set_call_location` | ~14 | one call per `box.n` read (records the source location in case the read throws) |
| loop + poll | ~6 | |

So the floor is about 100 instr per non-inlined call pair, mostly parameter-representation guards. It is **real
but irrelevant to the package gap**: the lightest attributed workload has 53k instr/iter of excess, so subtracting
even the `prop_read` floor (100/iter) changes no bucket's share by more than 0.2%. The "floor-adjusted" column in
the generated ranking subtracts `max(0, bare_loop perry − node)` = **0**, so it is identical by construction.

Call shapes, measured the same way (`_callfloor/f*.ts`, `package_bench_attr.py floor`; Node's numbers are noisy at
this N because of JIT tiering, Perry's are stable):

| call shape | Perry instr/call | Node | where Perry's go |
|---|---:|---:|---|
| class method `o.m(x)` reading `this.k` | 51 | 32–47 | inline |
| direct function call | 86 | 47 | inline |
| closure held in a `const` | 299 | 27–51 | `js_implicit_this_set` 43%, `fmod` 20% |
| untyped receiver `o: any` → `o.m(x)` | 160 | 39–46 | inline |
| `arguments[0]` | 509 | 94–231 | `js_arguments_bundle_index_get`, `js_array_length` |
| class getter `o.k` | 484 | 1–30 | `get_field_ic_miss_impl` → `class_accessor_cache::lookup` (#10498) |
| private `this.#m(x)` reading `this.#k` | 934 | 17–36 | `js_private_method_call` 75%, `js_set_call_location` 12% (#10501) |
| read of an object that **escaped** into an imported function | 1,839 | 33–50 | `js_object_get_field_by_name_f64` 77% (#10769) |
| `f.call(o, x)` | 3,702 | 100–117 | dispatcher 94% (#10505) |
| object-literal method + `this.k` | 4,222 | 32 | dispatcher 75%, `this.k` by name 16% (#11420) |
| absent-key read `o.missing` | 8,728 | 47–81 | `get_field_ic_miss_impl` 97% (#10495) |
| `P.prototype.m = function…; o.m(x)` | 9,551 | 35–49 | dispatcher 97% (#10505/#10507) |

The #11420 evidence (the control does not measure #11420's shape) is posted on the issue.

## Ranked root-cause buckets (item 2)

Weights: each workload's **excess** = Perry − Node instructions/iter. A bucket's share of a workload = its Perry
instructions per iteration ÷ excess (normalised if the shares sum above 1). **Equal-weight** averages workloads
within a package, then averages packages, so each of the 23 packages with ratio ≥ 2× counts once. Summing
absolute excess instead would let node-forge/rsa_sign (20.2 G instr/iter of excess) decide everything; that
column is in the generated report. Buckets are assigned from the leaf upwards. Subsystem frames (regex, JSON,
private, prototype-chain read, `arguments`, object allocation) and GC collections pre-empt the walk, and generic
helpers (`memcmp`, `from_utf8`, hashing, `arena_alloc`) are charged to the runtime function that called them.
The rules are in `BUCKET_RULES` / `SUBSYSTEM_RULES` in `scripts/package_bench_attr.py`.

"Fixing bucket B" means driving its cost toward Node's. The removable share is at most the equal-weight %.

| # | bucket | ≈ % of total excess (equal-weight) | packages where ≥5% | existing issues | open PRs touching it |
|---|---|---:|---:|---|---|
| 1 | **property lookup slow path / IC miss** (`shape_descriptor_by_id`, `try_data_get_bytes`, `get_field_by_name_*`, `keys_find_slot_by_bytes`) | **24.5** | 21 | #10761, #10769, #10905, #10871, #10753, #10496, #11420 (fix in progress in another session) | — |
| 2 | **regex (Perex)**: of which ~73% matcher execution, ~19% result building, ~5% spec-protocol property gets, ~3% pattern compilation | **17.1** | 11 | #10166, #10165, #10518, #10519 | — |
| 3 | reads resolved on the **prototype chain** | 5.3 | 9 | #10495, #10497, #10942 | — |
| 4 | **dynamic index get/set**, Array element access | 5.3 | 4 | #10513, #10514, #10718 | — |
| 5 | string ops (incl. UTF-8 validation and key copies on by-name lookups) | 5.1 | 12 | #10753, #10500 | — |
| 6 | closures / boxed captures / `arguments` | 4.6 | 9 | #10520, #10703, #10509 | #11177, #11179 |
| 7 | generated code itself (numeric kernels: big.js, bignumber.js, node-forge, nanoid) | 4.5 | 5 | #10511, #10718, #10741, #10777 | #11321 (codegen tiers) |
| 8 | Buffer / typed-array access + **buffer-registry probes** | 4.1 | 4 | #10515, #10516, #10694 | — |
| 9 | runtime method dispatch (`obj.m()` via the dispatcher) | 4.0 | 10 | #10502, #10505, #10506, #10504 | — |
| 10 | accessor / property-descriptor handling | 3.3 | 6 | #10498, #10871 | #11416 (class accessors as real prototype properties) |
| 11 | GC: minor 3.2 + major 2.1 + other 1.8 + write barrier 0.5 | 7.6 total | 6 (minor alone) | #10362, #10928 | #10204, #9949 |
| 12 | private class members | 2.6 | 2 (lru-cache, redis) | #10501 | — |
| 13 | numeric conversion (`js_dynamic_mod`→`fmod`, `trunc` in stores) | 2.1 | 2 | #10511; dynamic `%` unfiled (see below) | — |

### Construct view: which runtime entry point generated code called

This view counts everything beneath each entry point up to the next JS frame, all buckets combined, as
equal-weight % of excess. It shows which *codegen* path to fix.

| % | runtime entry called from generated code | what it means |
|---:|---|---|
| **17.3** | `js_typed_feedback_native_call_method_by_id` | `obj.m()` that did not take a direct/vtable call: the runtime dispatcher |
| **10.5** | `get_field_ic_miss_impl` | property-read IC miss |
| 6.2 | `<no JS frame>` | GC cycles, event loop |
| 4.8 | `<inline JS>` | generated code's own instructions |
| 4.0 | `js_object_get_field_by_property_id_f64` | by-property-id read (luxon getters, private reads) |
| 4.0 | `js_regexp_test` | |
| 3.9 | `js_object_get_field_by_name_f64` | by-name read (escaped receivers, `for…in` + `obj[key]`) |
| 3.7 | `js_dyn_index_set_strict` | untyped `arr[i] = v` |
| 3.7 | `js_string_replace_js` | |

**The dispatcher's own cost (excluding the JS it calls) is 18.7% of excess across 16 packages**, up to 82% of
node-forge/hmac and 57% of qs/stringify_nested. Inside it, the largest single mechanism is the **own-override
check**: `resolve_own_user_method` (8.3%) → `js_object_has_own` (7.3%) → `is_function_prototype_object_value`
(5.4%, 14 packages). That last function tests "is this value `Function.prototype`?" by calling
`builtin_prototype_value("Function")`, which looks up `globalThis.Function` **by name** and then
`closure_get_dynamic_prop("prototype")` on **every dispatched call** (`js_get_global_this_builtin_value` 6.2%
across 16 packages). #10497 describes this mechanism for `hasOwn`/`getPrototypeOf`. The profile shows the
dominant path in real packages is every runtime-dispatched method call; evidence is posted on #10497 and #10502.
A one-time cached identity for `Function.prototype` would remove ~5% of total excess by itself.

## The 10 worst packages: top call chains (item 1)

Format: share of the workload's Perry instructions · JS site (package file:line, the most recent marked
expression) → runtime entry → bucket [dominant leaf]. Minified files are mapped through their shipped source maps.

- **lru-cache** (churn 252×, ttl_mixed 64×; 215k instr/iter vs 851). `#set` (`index.ts:2180`): 10.3%
  prototype-chain read via `get_field_ic_miss_impl` [`shape_descriptor_by_id`]; 9.9% **private** read via
  `js_object_get_field_by_property_id_f64` [`private_evaluation_brand`]; 7.8% own-property IC miss. `#get` (`:3023`):
  6.8% prototype-chain read. `#evict`: 5.3% private brand check. 4.3% `Map.set` [`compact_map_entries`].
  → #10501, #10495, #10761.
- **cron** (next_dates 160×; 474M/iter). Almost all of it is in **luxon**, which cron uses for dates. 48% enters
  through `js_object_get_field_by_property_id_f64` from class **getters**: `DateTime.isValid`
  (`luxon.js:6287 return this.invalid === null`) 9.3%, `Duration.isValid` 5.1%, `DateTime.zone`
  (`this._zone`) 4.7% [`try_data_get_bytes`]. A package-free class-getter repro costs 484–985 instr/access and
  lands in `class_accessor_cache::lookup` (#10498). The in-body `this.<field>` by-bytes read seen in luxon did not
  reproduce in isolation (details on #10498).
- **mongodb** (batch_query 90×; 230M/iter). **GC is 22%** (`<no JS frame>`: copying minor 12.6%
  [`CopyingPointerSet::classify_arena`], mark-sweep 6.3%, layout bookkeeping 3.0%), driven by `List.push`
  (`src/utils.ts:709-710`) allocating list nodes: `js_object_alloc_class_inline_keys_stamped` +
  `js_gc_declare_typed_shape_layout`. The rest is property lookup (27%). → #10362, #10928.
- **node-forge** (hmac 72×, rsa_sign 57×, aes_cbc 40×, sha256 21×; binaries at 2febf4214). hmac: **81% under the
  dispatcher** (`util.js` ByteStringBuffer methods: `js_typed_feedback_native_call_method_by_id` →
  `shape_descriptor_by_id` / `from_utf8` / `dispatch_handle`). rsa_sign (`jsbn.js`, 20.6G/iter): untyped
  `arr[i] = v` via `js_dyn_index_set_strict` 33%, of which 14.2% is **`is_registered_buffer_slow`** probing a
  plain Array store; `js_array_get_f64` 16%. → #10502, #10513, #10694.
- **qs** (stringify_nested 69×, parse_nested 24×). 53–56% under the dispatcher: `side-channel`'s
  `$channelData.get(key)` / `.set` / `overflowChannel.has(obj)`, object-literal methods named get/set/has
  (`side-channel/index.js:31,41`, `qs/lib/stringify.js:99,108`, `qs/lib/utils.js:20`) [`shape_descriptor_by_id`];
  copying minor GC 8–13% [`per_object_slot_mask`, `rewrite_raw_addr`]. → #10506, #10502.
- **redis** (pipeline 61×, set_get 31×). **Private fields 31–37%**: `BasicCommandParser` constructor
  `this.#keyPrefix = …` via `js_private_field_add`; `SinglyLinkedList.length`/`shift`/`DoublyLinkedList.push`
  private reads [`take_private_field_owner`]; RESP `Decoder.#decodeNestedType` `chunk[this.#cursor]`
  (`private_guard_checked` 14.7% as an entry). → #10501.
- **dayjs** (diff_startof 50×, parse_format 33×). Dispatcher 35–45% (`dayjs.min.js`, `plugin/utc.js`); IC misses
  12–14%; `js_arguments_object_alloc` 3%; in parse_format, `String.replace` with a callback 8.6% [try/catch set-up
  per callback, `exception::try_push_with_kind`]. → #10502, #10509, #10166.
- **moment** (diff_duration 38×, parse_format 31×). 39% IC miss: `isValid` (`moment.js:173-179`
  `m._strict`, `flags.invalidWeekday`) [`shape_descriptor_by_id`, `from_utf8`]; `some.call(flags.parsedDateParts, …)`
  through the dispatcher. → #10761, #10505.
- **rate-limiter-flexible** (get_penalty 33×). IC misses in `RateLimiterAbstract.getKey` /
  `_getKeySecDuration` 25%; `resolve(res)` inside `new Promise` executors 12% (`js_closure_call1_receiverless` →
  prototype-chain read); copying minor 9.7% [`remember_retained_old_to_young_slots`, the production
  remembered-set rebuild, which only lives in `gc/verify.rs`]. → #10761, #10521.
- **validator** (batch 29×). `util/merge.js` (`for (key in defaults) … obj[key]`): by-name reads 20% +
  `js_for_in_keys_stable_value` 11% → #10753. `isByteLength` `encodeURI(str).split(/%..|./)`: **regex split 11.5%**
  → #10165. `assertString` `input.constructor.name` by name 4.2% → #10497.

Also: **dotenv** 80% regex, **node-cron**/validate 74% regex, **uuid** 53% regex (`validate` `REGEX.test`),
**jsonwebtoken**/decode 60% `JWS_REGEX.test` (`jws/lib/verify-stream.js:41`), **nanoid** 48% typed-array
access with buffer-registry probes (`js_uint8array_get/set` → `is_registered_buffer_slow`), **big.js** 35% dyn-index
(`big.mjs:872` `c[j] = 0` on `new Array(n)`: `js_dyn_index_set_strict` → `trunc` + `object_static_prototype`),
**decimal.js** `str.search(/e/i)` 8.3% [`symbol::is_registered_symbol_slow`, the `@@search` lookup] (#10518 family).

## Issues (item 4)

- **No new issue filed.** Every significant hotspot maps to an existing open issue. New evidence (package
  measurement, call chain, per-call cost) was added as comments instead of duplicates: #10502 (dispatcher 18.7%),
  #10497 (Function.prototype by name on every dispatched call, 5.4–6.2%), #10166 (regex 17.1%), #10761 (IC-miss
  path 11.2%, with the `from_utf8`/`copy_of_key` key conversions), #10498 (luxon getters, cron 160×), #10501
  (lru-cache and redis), #10694 (`is_registered_buffer_slow` inside plain-Array stores, 14% of node-forge/rsa_sign).
- **#11420:** the control-floor evidence is posted there. The control does not measure #11420, and after the
  loop-bound hoist it has no floor.
- **#11450** (node-forge codegen regression): a package-free 7-line repro was added (fails only when the array
  receiving `new Worker(x)` is captured by a nested function).
- **Candidate left unfiled: untyped `%`.** `js_dynamic_mod` → libm `fmod` is 11.7% of bignumber.js and 4.2% of
  big.js. Two package-free attempts were compiled to inline code, so there is no reproducer yet and it was not
  filed (the issue rules require one).

## Not attributed / not run

- jsonwebtoken/rs256 (`TypeError … reading 'update'`), node-cron/match (wrong checksum `00000000`), fastify/inject
  (exit 1, #10454), rate-limiter-flexible/consume (exit 1 at n=20000), pg/select and pg/insert_batch (no output),
  mongodb/insert_find (timeout). The same failures as Phase 1 (`report_notes.md`); none were profiled.
- mysql2/*: mysqld did not start under this run's server root (and both workloads segfault per #11366).
- exponential-backoff/retry (1.3×) is profiled but excluded from the ranking (< 2×); fastify is ranked on
  listen_fetch alone (2.2×).
- No wall-clock numbers; no macOS run; no GC-knob variation (default GC only).
- JS source lines come from Perry's `--debug-symbols` line tables. A line is the most recent *marked* expression
  (calls, `new`, member reads), so pure arithmetic inherits the line above it. Minified sources are resolved to
  original lines through the package's source map where one ships (lru-cache); dayjs ships none.

## Re-running (item 5)

```sh
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
(cd benchmarks/packages && npm ci --ignore-scripts)
PERRY_KEEP_SYMBOLS=1 PERRY_WORKSPACE_ROOT=$PWD python3 scripts/package_bench.py compile \
    --perry-bin-dir OUT/bins --perry-flags=--debug-symbols
python3 scripts/package_bench.py profile --callgraph --include-control --lock \
    --perry-bin-dir OUT/bins --perry-commit $(git rev-parse --short HEAD) --out OUT/callgraph.json \
    [--server-root … --pg-bin-dir … --mongod …]          # same server flags as `run`
# after editing BUCKET_RULES: rebuild every table from the saved stacks, no perf runs
python3 scripts/package_bench.py profile --callgraph --reanalyze --include-control --out OUT/callgraph.json
# call-shape floor probes
python3 scripts/package_bench_attr.py floor --out-dir OUT/floor
```

`profile --callgraph` writes `OUT/callgraph.json`, `OUT/callgraph.md` and `OUT/callgraph-stacks/*.json.gz` (per
workload stack histograms, ~100 KB each, enough to re-derive every table). `--filter X --exact` refreshes one
workload in place. Needs Linux `perf` with DWARF unwinding and `llvm-addr2line` or `addr2line`. The tooling is
not wired into any CI job.
