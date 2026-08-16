### perf(object): remove the derivable `object_type` and `field_count` header words — 56 B → 48 B

`ObjectHeader` is now `{class_id @0, parent_class_id @4, keys_array @8, meta @16}`
— **24 bytes on LP64, 16 on ILP32**, down from 32/24. A two-field object literal
costs **48 bytes instead of 56** (`GcHeader 8 + header 24 + 2 slots`), and the
eight-slot case 96 instead of 104. Measured with `rustc -O` on the exact
`#[repr(C)]` shapes: removing **either** word alone saves **zero** — the struct
re-pads — so the two had to go together. This is half of #8047's prize and needs
none of its GC descriptor-rooting work (#8112).

Both words were derivable from facts the object already carries:

* the receiver **kind** — ordinary / class / native error — from
  `GcHeader.obj_type` plus the immutable ShapeId descriptor's `object_kind`;
* the **live inline-slot bound** from that descriptor's `live_inline_slot_count`.

#### The offset-0 type confusion this had to disarm

`ObjectHeader::object_type` was prefix-punned against `error::ErrorHeader`'s
first word, and **nine** sites read raw offset 0 to decide Error-vs-ordinary —
two more than previously catalogued (`promise/rejection.rs:181` and `:464`).
Deleting the word makes offset 0 `class_id`, and `OBJECT_TYPE_ERROR` is **2**
while class ids are handed out from 1, densely, in source-declaration order
(`run_pipeline.rs`: `let mut next_class_id = 1`). Left alone, those reads would
have reclassified **every instance of the second class a program declares** as an
`ErrorHeader` and served `message`/`name`/`stack`/`errors` out of its field
slots — a silent wrong answer of exactly the #8100 shape. Plain object literals
are `class_id == 0`, so the `OBJECT_TYPE_REGULAR` arms would have inverted in
both directions at once.

All nine now go through `error::ptr_is_native_error()` (`GcHeader.obj_type ==
GC_TYPE_ERROR`, the only kind `alloc_error` uses). Five sabotage-shaped
acceptance tests in `object/tests.rs` pin it: each first asserts the confusable
value really is sitting at offset 0, then asserts the answer. Reverting the
discriminator to the raw read turns three of them red.

`proxy.rs`'s #6595 store-plan gate moves to `object_is_regular()`. #8047's census
warned that this substitution re-opens #6595 — that warning was **stale**: #8086
rewrote `object_is_regular` to mean exactly `descriptor.object_kind == Ordinary`,
so it is still false for a heap class object. A test pins that too.

#### Mint-then-stamp

With `field_count` gone, the ShapeId descriptor is the **only** record of a live
object's slot bound, so a stamp-cleared window is a window in which the collector
traces zero payload slots — a fresh #7154/#7164. Every clear-then-remint sequence
is restructured: `shapes::publish_object_shape_from` mints the successor while
the predecessor stamp is still installed, and the single `parent_class_id` store
(which cannot allocate, hence cannot collect) is the publication point.
`set_object_keys_array`, `set_object_live_slot_count` and
`js_object_delete_field` no longer clear; `shapes::clear_object_shape_stamp` is
now `#[cfg(test)]`, surviving only so tests can manufacture the unstamped state.

`typed_feedback::object_shape`'s defensive self-heal is **deleted**. It called
`synchronize_object_shape_descriptor`, which derived the bound from the header
word; without that word it would publish `live = 0` for an unstamped receiver —
a read-only observation path silently truncating the object's traced and writable
payload. It misses closed instead. #6804's "no pre/post-stamp token split"
property survives by the stronger route: every allocator birth-publishes, so the
population needing a heal is empty.

#### Codegen

`object_header_size_bytes` 32 → 24 (LP64) and 24 → 16 (ILP32). The inline-`new`
path's two packed header stores collapse to one (`class_id ‖ ShapeId`). Eleven
hard-coded IR offsets renumber `class_id @+4` → `@+0` and ShapeId `@+8` → `@+4`;
GcHeader-relative offsets (`-8`/`-7`/`-6`) are untouched. Emitted IR never read
`field_count` — #8067 moved the PIC hit path onto an exact ShapeId match — so
codegen only ever wrote it.

The two `object_header_size_bytes(..) / 8` word-index sites (`expr/proxy_reflect.rs`,
`stmt/loops.rs`) become byte geps. Both quotients are exact today (24/8, 16/8),
but #8047's ILP32 header is 12 bytes and `12 / 8 == 1` truncates silently; a new
`object_header_size_is_a_whole_number_of_heap_words` test pins the divisibility
rather than the quotient.

Four codegen doc comments claiming "24 on 64-bit, 20 on ILP32" — wrong since
`meta` landed in #6759 — are corrected, along with `docs/src/platforms/watchos.md`
(the only user-facing statement of the pair), `docs/object-write-matrix.md`, and
`TYPE_LOWERING.md`.

#### Two gates that could not have caught this

**`perry-ffi`'s ABI mirror had never executed.** `object_header_matches_runtime`
is `#[cfg(all(test, feature = "runtime-link"))]`, `runtime-link` was enabled
nowhere in `.github/`, and `cargo-test` is a per-package loop with default
features — so the module never compiled. Field *deletion* still went red (an
`offset_of!` on a missing field stops compiling), but a **size or padding
divergence was invisible**, which is precisely this change's failure mode.
`cargo-test` now runs `cargo test -p perry-ffi --features runtime-link --lib`
unconditionally. It earned its keep on the first run: it caught a real bug in
this change — the parity `debug_assert` inside the new mint-then-stamp
publication compared the freshly stamped descriptor against a header keys word
the new ordering has not written yet.

**`perry-ffi` is published to crates.io.** This is a **breaking ABI change** for
out-of-tree wrappers: one compiled against the old mirror and linked against the
new runtime reads `class_id` out of the deleted `object_type` slot with no
compile error. That cannot be guarded retroactively — the old mirror references
no version symbol, so there is nothing the runtime can withhold. Recorded as a
deliberate break, with a tripwire introduced for the *next* one:
`perry_ffi::OBJECT_HEADER_ABI_REVISION` (= 2) paired with the runtime's
`extern "C" perry_object_header_abi_revision()`, asserted equal by the
now-running mirror test.

#### Gate updates

`scripts/shape_descriptor_census.py` narrows to `keys_array` (the last mirror)
and gains three rules the deletion needs: the exact `ObjectHeader` field list, so
re-adding a word is red rather than merely un-baselined; a ban on any publication
path clearing the stamp, plus a check that `clear_object_shape_stamp` stays
`#[cfg(test)]`; and a fixed emitted-guard offset rule. That last one was
**vacuous** — it matched only `add(..., "N")` while all four functions it names
emit `gep(I8, &p, &[(I64, "N")])` — so it now matches both spellings and requires
each guard to be shown reading the ShapeId at all. Three new sabotage self-tests
cover the new rules.

#### Measured

19-program corpus, quiet M1 mini, **best-of-5**, `instructions retired` and
`peak memory footprint` reported together. Both arms built from one worktree
with `-p perry -p perry-runtime-static -p perry-stdlib-static`, per-arm
`PERRY_RUNTIME_DIR` **and** `PERRY_CACHE_DIR`, `PERRY_NO_AUTO_OPTIMIZE=1`; all
three archives `cmp`-verified to differ, all 19 compiled corpus binaries
`cmp`-verified to differ (no row measures nothing), all 19 stdout byte-equal
between arms with `rc=0` on every run.

Base `3be2016c1` — i.e. **after** #8157 (PtrHashMap shape probes) and #8110
(census gate).

| prog | Δ instructions | Δ peak RSS |
|---|---:|---:|
| `deeplist` | **+9.03%** | −4.06% |
| `retain1` | +8.24% | −3.89% |
| `churn_alloc` | +5.34% | −0.08% |
| `push_cls` | +5.29% | +0.23% |
| `pipeline` | +4.33% | +0.00% |
| `interp` | +3.35% | +0.40% |
| `retain` | +3.34% | **−9.30%** |
| `retain_wide` | +3.30% | −5.46% |
| `shapes` | +3.29% | −0.17% |
| `churn` | +3.08% | +0.08% |
| `retain_wide1` | +2.97% | −6.06% |
| `iso_miss` | +2.86% | −0.06% |
| `cycles` | +0.81% | −0.08% |
| `tree` | +0.52% | **−12.81%** |
| `tree_wide` | +0.46% | −6.35% |
| `asyncpipe` | +0.45% | +2.90% |
| `push_num` | +0.02% | −0.15% |
| `churn_read` | +0.01% | −0.57% |
| `fib40` | +0.00% | +0.00% |

**0 of 19 rows are faster; 12 of 19 pay more than 1%.** The RSS win is intact
and lands where predicted (`tree` −12.81%, `retain` −9.30%, the object-literal
retain family −3.9…−6.1%), and the rows with no object population (`fib40`,
`push_num`, `churn_read`) move by ~0 — that is the control.

**`asyncpipe`'s +2.90% peak RSS is not a footprint regression.** It is exactly
1024 KB — one arena block — and it is *arena block quantization*: sweeping
`PERRY_GC_SCAVENGE_NURSERY_MB` moves it and **flips its sign** (−288 KB at cap 4,
+80 KB at 8, +944 KB at 12, +992 KB at the default 16, +624 KB at 24/32). Under
`PERRY_GC_DIAG=1` the two arms run the same single copying minor with the same
6767 copied objects, and the shrunk arm holds strictly *less* live data
(`copied_bytes` 449,480 vs 452,896; `post_in_use` 450,160 vs 453,576). With 48 B
objects the allocation stream lands differently against the 1 MB block
granularity, so the peak straddles one extra block at some caps and one fewer at
others.

#### Where the residual is — the earlier attribution is RETRACTED

An earlier revision of this fragment named `proxy.rs`'s #6595 store-plan gate as
the site. **That is wrong and is withdrawn.** Direct instrumentation at the gate
counts `total=1` on `interp`, `2` on `shapes`, `1` on `iso_miss`, and the counter
never arms at all on `retain`/`retain_wide`/`retain1`/`deeplist`/`churn`/
`pipeline`/`push_cls`/`tree`/`churn_read`. The counter had been reading a
`#[inline]`, non-`#[track_caller]` frame and swallowing its callers. With
`#[track_caller]` on `object_is_regular` itself the true caller is
`array/element_shape.rs:258` — the element-shape check on array push — which is
**pre-existing on main**, byte-identical between arms. `object_live_slot_count`,
the derivation this rung actually introduces, is called **zero** times on every
hot row.

Two components, separated by an arm-C probe (deletion *without* the shrink: the
same code with 8 B of inert padding, which validates as a control at ~0.00% RSS
on every row):

* **Footprint-coupled** — `deeplist` +8.28 SIZE / −0.07 CODE, `retain1` +6.75
  SIZE. Smaller objects genuinely cost instructions here, opposite in sign to
  #8047's pad probe on a neighbouring benchmark. The cache-line count is not the
  mechanism; it is unexplained.
* **Code** — +1.0…+2.5% on every allocation-heavy row.

#### What #8157 did and did not recover

#8122 was held on #8125 in the expectation that #8157 — which made every ShapeId
probe 15–25% cheaper and is worth `deeplist` −17.2% / `churn` −25.2% **on main
alone** — would absorb this rung's cost, since that cost is extra descriptor
probing. **Re-measured on post-#8157 main, it does not.** The regression is
slightly *worse* than the pre-#8157 table on most rows (`deeplist` +8.20 → +9.03,
`retain1` +7.99 → +8.24, `churn` +1.76 → +3.08); only `shapes` improves (+4.96 →
+3.29). This is consistent with the arm-C partition: the dominant rows are
footprint-coupled, and a cheaper probe cannot recover a cost that is not probing.

**A new consumer arrived in the meantime.** #8094 (guarded ordinary-parameter
specialization) landed 2026-08-15 18:00, *after* this branch's original base
(`83b6b8c69`, 02:35), and `param_type_guard::GuardState::plain_object` read both
deleted words directly. The rebase necessarily converts those two free `u32`
loads into `object_is_regular` + `object_live_slot_count`. `js_param_type_guard`
is the **#2 self-time symbol on `interp` in both arms**, and the three rows that
newly regressed are exactly the app-shaped ones: `interp` +0.29 → **+3.35**,
`iso_miss` +0.30 → **+2.86**, `pipeline` +0.34 → **+4.33**. A differential symbol
profile on `interp` (`PERRY_DEBUG_SYMBOLS=1`, three repeats per arm) shows the
shift is in the guard's *callees*, not its own body:

| self-time samples, `interp` | arm A (base) | arm B (shrunk) |
|---|---:|---:|
| `shapes::shape_descriptor_by_id` | 19 / 23 / 17 | **46 / 38 / 28** |
| `gc::layout::init_typed_shape_layout` | absent | **27 / 18 / 20** |
| `js_param_type_guard` (own body) | 110 / 96 / 101 | 98 / 86 / 96 |

Direction stable across all three pairs. Caveat: this row family has a documented
sensitivity to codegen/inlining perturbation (finding 3 below), so "probe cost"
versus "inlining perturbation" is supported but not fully separated.

Three findings worth carrying forward, all from measuring rather than assuming:

1. The first cut regressed instructions by up to **+30%** (`deeplist` +30.5%,
   `cycles` +28.4%, `tree` +25.4%). Five GC-side sites already read the bound
   descriptor-first with the header word as an `unwrap_or` fallback — and
   **`unwrap_or` is eager**, so the substitution made each do *two* shape-table
   probes, one of them (`gc/layout.rs`'s `layout_note_slot`) on every object
   field store. Fixed, along with `weakref::is_weak_target_trace_slot` (three
   probes per traced slot → one) and six write paths that read the bound twice.
2. A 64-way direct-mapped `ShapeId → count` memo — sound without invalidation,
   and the obvious recovery — **measured null and was deleted**: `retain` +4.26%
   with it vs +3.26% without, `retain_wide` +4.46% vs +2.89%, better only on
   `shapes`. Its first sabotage test was *vacuous* (two arbitrary shapes get
   consecutive ids and so never share a memo way) and the sabotage run caught
   that. The numbers survive as a doc comment so it is not rebuilt.

3. The counter-guided repair for the site above — pass the `GcHeader` the caller
   already holds, hoist a free compare ahead of the probe — is semantically
   identical and strictly less work, and **measured as a reproducible
   regression**: `interp` +0.29% -> **+9.59%**, `pipeline` +0.34% -> +4.43%,
   while doing nothing for `retain` (+3.26% -> +3.04%, noise). Implemented,
   measured, reverted. The mechanism is codegen/inlining rather than semantics
   and is not established. "Semantically identical and strictly less work" is an
   argument about the source; only the corpus can make it a claim about the
   binary.

GC canaries (`retain`/`tree`/`churn`/`shapes` × plain / `FORCE_EVACUATE` +
`VERIFY_EVACUATION` / `FORCE_EVACUATE` + `PROTECT_FROMSPACE DEPTH=32`, all under
`PERRY_GC_DIAG=1`): all exit 0 and byte-exact, with `copied_objects > 0` or
`promoted_objects > 0` on every row (`retain` copies 368,635 and promotes 2.1 M),
and the protect arm printing 8 `[gc-fromspace-protect]` lines against
`copying_minors=8` — so no arm is vacuous.

#### Also

* `perry-ui-android/src/json.rs` deleted — 606 lines, every function private with
  no callers, its own trailing comment saying `js_json_*` now lives in
  `perry-runtime/json.rs`. It read `field_count` in three places and is invisible
  to CI three ways (`#![cfg(target_os = "android")]`, outside the host-compatible
  workspace scope, and the only Android job is `continue-on-error`).
* `NullObjectBytes` gains the `meta` word it has been missing since #6759 — a
  `(*obj).meta` read on the unresolved-namespace stub was running 8 bytes past
  the end of the static.
* `object/mod.rs` reached the 2000-line cap, so `live_slots.rs` (the bound plus
  the ABI revision) and `null_stub.rs` split out.
