# Plan: Small String Optimization (SSO) rollout

**Status:** infrastructure landed in v0.5.213. No creation sites migrated yet —
see "Why infrastructure-only first" below.

**Goal:** strings of length 0..=5 bytes encode inline in the NaN-boxed f64
payload (48 bits: 8-bit length + 5 bytes of data + the SHORT_STRING_TAG
band). Zero heap allocation for short strings. Tier 1 #2 per
`docs/memory-perf-roadmap.md`.

## Current state (v0.5.213)

Landed:

- `SHORT_STRING_TAG = 0x7FF9_0000_0000_0000`, constants at
  `crates/perry-runtime/src/value.rs:44-56`.
- `JSValue::try_short_string(&[u8])` — returns `Some(Self)` for
  `bytes.len() <= 5`, `None` otherwise. `value.rs`.
- `JSValue::short_string_to_buf(&mut [u8; 5]) -> usize` — decoder.
- `JSValue::is_short_string()`, `JSValue::is_any_string()` (matches
  both heap STRING_TAG and inline SSO), `JSValue::is_string()`
  (legacy strict heap predicate — unchanged).
- `js_string_new_sso(ptr, len) -> f64` in `string.rs` — SSO-aware
  construction that returns an SSO value for short inputs and falls
  through to `js_string_from_bytes` + `JSValue::string_ptr` for
  long inputs.
- `str_bytes_from_jsvalue(value, &mut scratch) -> Option<(*const u8, u32)>`
  in `string.rs` — central decoder that produces a `(ptr, len)` view
  for both heap and SSO forms (SSO decodes into caller-owned
  scratch).
- `js_string_materialize_to_heap(value) -> *mut StringHeader` in
  `string.rs` — compatibility shim that allocates a heap
  StringHeader from an SSO value. For call sites that can't easily
  be migrated.
- `typeof` (builtins.rs) accepts both tags — returns `"string"` for
  SSO values.
- `js_jsvalue_equals` + `js_jsvalue_compare` (value.rs) handle SSO
  on both sides, with a bitwise fast path when both operands are
  SSO (canonical encoding: same bytes ⇒ same bits).
- `js_value_length_f64` (value.rs) returns length directly from the
  SSO tag byte for SHORT_STRING_TAG values, no heap access.
- Three stringify arms in `json.rs` (top-level value, object field
  inline dispatch, array element inline dispatch) decode SSO into
  escaped output.
- Six unit tests in `value::tests` cover roundtrip / rejection /
  embedded-NUL / tag-distinctness / empty / byte-order.

## Why infrastructure-only first

Attempting to emit SSO from `DirectParser::parse_string_value` in
the same commit produced immediate regressions: three
`test_json_lazy_*.ts` tests diffed from Node because stringify
walkers for objects + arrays in json.rs have **many** inline
dispatch sites (verified: `grep "== STRING_TAG" crates/perry-runtime/src/json.rs`
returns 20+ hits) and each needs a parallel SSO arm before the
creation site can ship safely. Beyond json.rs, consumer sites include:

- `crates/perry-runtime/src/object.rs` — property-get helpers,
  field-key lookups, `Object.keys/values/entries`, proxy handlers.
- `crates/perry-runtime/src/string.rs` — every string method
  (`split`, `replace`, `slice`, `indexOf`, `includes`, `startsWith`,
  etc.).
- `crates/perry-runtime/src/regex.rs` — match result string
  extraction, `replace` substitution.
- `crates/perry-runtime/src/set.rs` + `map.rs` — key comparison
  when keys are strings.
- `crates/perry-runtime/src/builtins.rs` — `console.log` argument
  stringification, `String(x)` coercion, the handful of `is_string()`
  type guards.
- `crates/perry-stdlib/src/fastify/`, `mysql2/`, `pg/`, `redis/` —
  request/response body paths use `js_string_from_bytes` + assume
  heap-pointer semantics.
- `crates/perry-codegen/src/expr.rs` — string-literal emission,
  template-literal concat, tag function calls.

Flipping `DirectParser::parse_string_value` to SSO immediately
without the consumer audit breaks each of those paths in a
different way. Landing the infrastructure without producers is
safe (the new tag value is allocated but unused) and unblocks
incremental per-site migration without coordinating a single giant
commit.

## Migration roll-out

Each step is self-contained: it picks one consumer cluster, adds
SSO-aware dispatch to every site in that cluster, tests against the
full regression suite + targeted new tests, then ships.

### Step 1 — stringify consumers (json.rs) ✅ mostly landed

**Landed in v0.5.214** (on top of v0.5.213 infrastructure):
- Every `== STRING_TAG` dispatch in `json.rs` now has a parallel
  `== SHORT_STRING_TAG` arm. Coverage:
  - `stringify_value` (v0.5.213)
  - `stringify_value_depth` (v0.5.213)
  - `stringify_object_inner` inline field dispatch + replacer block
  - `stringify_array_depth` inline element dispatch
  - `extract_string_array`
  - Replacer spacer paths (3 sites)
  - `js_json_stringify_full` top-level replacer arm
  - Spacer tag check (when user passes short string like `"  "` as indent)
- New `PERRY_SSO_FORCE=1` test gate — forces
  `DirectParser::parse_string_value` to emit SSO for strings ≤ 5
  bytes. Cached via OnceLock like `PERRY_JSON_TAPE`.
- `js_jsvalue_to_string` materializes SSO → heap for callers that
  need `*mut StringHeader`.
- `js_object_get_field_by_name` handles `.length` on SSO
  receivers (returns the length byte directly).

**Test result (v0.5.214):** 8 out of 10 `test_json_*.ts` tests match
Node byte-for-byte under `PERRY_SSO_FORCE=1`. Default mode (SSO off)
stays 10/10 — no user-visible regressions from the infrastructure
landing.

**Remaining 2 failures** are both caused by Step 1.5 (below), not
by missed stringify arms. They fail under SSO_FORCE because the
codegen's PropertyGet receiver guard filters out SSO values before
they reach any runtime path — so property access on SSO strings
other than `.length` (e.g. `.indexOf`, `.slice`) returns
`undefined` instead of dispatching correctly.

### Step 1.5 — codegen PropertyGet SSO dispatch ✅ landed v0.5.215

Added an explicit three-way branch in `expr.rs::Expr::PropertyGet`
receiver guard: `is_sso = tag == 0x7FF9` → new `sso_idx` block →
`js_object_get_field_by_name_f64` → final merge. PIC / invalid /
SSO all merge via a three-way phi. Also fixed `js_array_join` to
decode SSO elements inline (was coercing to NaN via the generic
number path).

**Final result:** 10/10 `test_json_*.ts` tests match Node under
default AND `PERRY_SSO_FORCE=1`. Runtime tests 136/136. Gap tests
25/28 (+1 from baseline). Fastify 5/5, thread 4/4.

Historic notes on the failed attempt:
**Why needed:** `crates/perry-codegen/src/expr.rs` PropertyGet has a
receiver-validity guard at ~line 2647 that masks `tag & 0xFFFD` and
checks `== 0x7FFD`. This accepts POINTER_TAG (0x7FFD) + STRING_TAG
(0x7FFF) but rejects SHORT_STRING_TAG (0x7FF9). SSO receivers fall
to the "invalid" branch → return `undefined`.

Widening the mask to `0xFFF9` accepts SSO but the PIC fast path's
subsequent `*(obj_handle + 16)` read lands in arbitrary userspace
memory for SSO values (the low 48 bits are SSO data, not a heap
pointer). On some systems this reads garbage, on others it
crashes. Verified: widening without further guarding crashed 2
tests under SSO_FORCE.

**Safe fix:** emit a three-way branch in the receiver guard:
1. `tag_masked == 0x7FFD` → PIC fast path (existing).
2. `tag == 0x7FF9` → call `js_object_get_field_by_name_f64` directly
   (skip PIC, no memory read).
3. Otherwise → invalid, return `undefined`.

The `js_object_get_field_by_name` runtime entry already has an SSO
arm (v0.5.214) that returns `.length` and dispatches other keys to
`undefined`. Extending its dispatch to string methods is a Step 4
concern — this step just wires the codegen to route SSO receivers
into that entry.

Estimated effort: ~2 hours, one codegen site, ~20 lines of new IR
emission. Ship criterion: 10/10 `test_json_*.ts` tests match Node
under `PERRY_SSO_FORCE=1`.

### Step 2 — DirectParser + lazy tape emit SSO ✅ landed v0.5.216

Both `DirectParser::parse_string_value` (json.rs) and tape-path
`materialize_string_value` (json_tape.rs) now emit SSO
unconditionally for values ≤ 5 bytes. Also fixed `js_number_coerce`
to accept SSO via `is_any_string` + `str_bytes_from_jsvalue` — the
one producer-flip regression (reviver → `Number(sso)` returning
NaN).

**Measurements:**

| Bench | v0.5.215 direct-only | v0.5.216 default (lazy+SSO) | Delta |
|---|---|---|---|
| bench_json_roundtrip | 410 ms / 138 MB | 80 ms / 108 MB | — (lazy dominates) |
| bench_json_readonly | 290 ms / 120 MB | 80 ms / 90 MB | — (lazy dominates) |
| bench_json_readonly_indexed | 300 ms / 120 MB | 90 ms / 90 MB | — (lazy dominates) |
| bench_sso_strings (direct) | 290 ms / 123 MB | 150 ms / 76 MB | **-48% time, -38% RSS** |

SSO is most visible on workloads that:
- Fall through to the direct parser (<1 KB blobs, >16 MB blobs in auto-mode, non-array roots, `PERRY_JSON_TAPE=0`).
- Hit indexed access on a lazy result (force-materialize invokes `materialize_string_value`).
- Contain many short string values (enum strings, status codes, short IDs).

The 3 main JSON benches are dominated by the lazy fast paths
(`.length` direct from `cached_length`, stringify via blob memcpy),
where SSO has smaller impact because string materialization isn't
on their hot paths.

### Step 3 — object key storage (object.rs)

Field keys are currently stored as a JSValue array. If keys are
often short (typically 2-6 chars) and typing SSO-aware key lookup
to accept both forms, we can skip key-header allocation entirely.
Requires audit of:

- `js_object_set_field_by_name` + `js_object_get_field_by_name` —
  convert key pointer to normalized form.
- Shape cache + transition cache — keys are currently interned via
  PARSE_KEY_CACHE into `*const StringHeader`. If we switch to SSO
  for short keys, the cache becomes `Vec<u8> -> JSValue`.
- FNV-1a hashing — must hash equivalent bytes regardless of
  representation.

Biggest win target: the `"id"`, `"v"`, `"k"` keys in the JSON
benches would all become zero-allocation. Reduces PARSE_KEY_CACHE
hot-path cost further.

### Step 4 — string methods (string.rs)

For each of `js_string_length`, `js_string_concat`,
`js_string_equals`, `js_string_substring`, `js_string_indexof`,
`js_string_split`, etc., accept JSValue in place of
`*const StringHeader` and dispatch on tag. Alternative: keep
signatures, add `js_string_*_f64` variants that take a NaN-boxed
value, and migrate callers incrementally.

### Step 5 — codegen string literals

TypeScript source like `const s = "ok";` currently lowers to an
`Expr::String("ok")` which codegen emits as a runtime
`js_string_from_bytes` call at every evaluation. Short string
literals can instead compile to a constant `f64` (the NaN-boxed
SSO encoding), loaded via `double_literal` — zero runtime cost.

### Step 6 — stdlib HTTP / DB paths

Audit perry-stdlib crates for `js_string_from_bytes` call sites
that construct short strings (status codes, short header values,
short DB cell values). Migrate to `js_string_new_sso`.

## Measurement gates

After each step lands, rerun:

- `cargo test --release -p perry-runtime --lib` — must stay green.
- `run_gap_tests.sh` + `run_parity_tests.sh` — no regressions.
- `bench_json_{roundtrip,readonly,readonly_indexed}` — should improve
  or hold steady. Any regression blocks the step.
- A new `bench_string_heavy.ts` that allocates many short strings
  — should show the targeted RSS + time win.

## Expected aggregate win (all steps)

Projected on `bench_json_roundtrip`: ~5-10 MB RSS reduction + small
time improvement from skipping StringHeader alloc overhead on short
strings.

Projected on string-heavy synthetics (many short-string allocations):
~20-40 MB RSS reduction + 10-20% time improvement.

Not as large as tier 2/3 wins, but fully additive with them and a
much smaller scope per step.

## Decision gate

After Step 2 ships, re-evaluate whether Steps 3-6 are worth the
effort. If the measured win on the JSON benches is <3 MB RSS and
<5% time, the remaining steps should be deferred and effort shifted
to tier 2/3 (escape analysis + precise root tracking +
generational GC) which give 10-50× larger wins.

If Step 2 gives the projected win, Steps 3-6 proceed in priority
order — codegen string literals (Step 5) is the cheapest next and
compounds with Step 2 without additional migration risk.

## Reference

- Tag encoding: `docs/audit-lazy-json.md` §4 has a parallel
  discussion of NaN-boxing tag layout that applies here.
- Tier classification: `docs/memory-perf-roadmap.md` §#2.
