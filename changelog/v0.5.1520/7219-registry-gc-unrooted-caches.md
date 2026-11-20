### Fixed

- **`gc`: the interned `typeof` strings and `JSON.rawJSON`'s interned key are GC
  roots, and are now registered as such** (#7211). `js_value_typeof` caches its
  eight possible results in thread-local `Cell<*mut StringHeader>`s so each is
  built once. Those cells held a **raw pointer into the nursery** with nothing
  referencing the string, so the first minor collection swept or evacuated it
  and the cache named abandoned memory for the rest of the process — every
  later `typeof x === "string"` handed `js_string_equals` a from-space address.
  `json/raw_json.rs`'s cached `"rawJSON"` key had the identical defect and is
  fixed with it. Both now go through `gc_register_mutable_root_scanner`, so they
  are marked (never swept) and rewritten (never stale after a copying minor).

  **This is what kept `sfw-registry --help` red** after #7192, #7206 and #7214
  had closed every stale *register* they could find, and the difference in
  failure signature is the lesson worth keeping:

  | | unrooted register (#7154 class) | unrooted cache (this) |
  |---|---|---|
  | goes bad | only if a collection lands in a few-instruction window | at collection #0, permanently |
  | reproduces | intermittently; needed a zod workload and ten rounds | **10/10** |
  | found by | `scripts/gc_root_dominance_check.py` over emitted IR | nothing static — the tool cannot see a runtime table |

  A perfectly reproducible GC bug is evidence *against* a stale register. The
  detector here was `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
  PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, whose reporter named it outright:

  ```
  [gc-fromspace-protect] FAULT: signal 10 at 0x…558a4
    last-known object: user_ptr=0x…558a0 obj_type=3 size=40
    retired_by_minor=#0
    … perry_closure_node_modules_zod_src_v4_core_schemas_ts__222 + 100
  ```

  `obj_type` 3 is a string and 40 bytes is a 32-byte header plus `"string"`;
  `retired_by_minor=#0` is the tell for a table rather than a register.

- **`codegen`: the saved implicit `this` is rooted across every dispatch that
  binds one** (#7211). `js_implicit_this_set` swaps the `IMPLICIT_THIS` cell and
  returns what was there. That cell is a registered *mutable* root
  (`scan_implicit_this_roots_mut`, `object/this_binding.rs:176`) and the swap has
  already overwritten it, so the returned value is held only in an SSA register
  — across the whole call the bind exists to scope. A minor inside that call
  moves the object and rewrites every root naming it, leaving the register on
  from-space; the restore then writes that pre-move address **back into a root
  the collector scans**, so the corruption outlives the call that caused it.

  #7214 identified this in `js_closure_callN` and left it measured but unfixed.
  It was in fact **seven** lowerings with seven copies of the same three lines:
  `js_closure_callN` (`lower_call/console_promise.rs`), the
  `js_native_call_value` override arms in `lower_call/method_override.rs` and
  both `lower_call/property_get` dispatchers, the static-dispatch arm, the
  #3576 receiverless reset in `lower_call/func_ref.rs`, and both closure-call
  arms in `lower_call/early_branches.rs`. They are now one shared pair,
  `temp_root::implicit_this_save` / `implicit_this_restore`, so the eighth
  lowering that needs it gets the root for free rather than the bug.

- **`codegen`: `Expr::ClassExprFresh` roots the fresh class object across the
  `js_object_set_field_by_name` calls it emits itself** (#7211). The old
  `protect_handle` predicate had four disjuncts and every one asked whether
  something the *author* supplied could collect — a captured argument, a symbol
  static, a `static { … }` body, an initializer expression. None asked whether
  the lowering's own emitted field-store could, and it can: that helper performs
  the keys-array transition and allocates. So `class C { static tag = tag }`,
  one inert `LocalGet`, got no root at all.

  `js_object_mark_class` does not rescue it, and that is the reason this
  survived review once already: it files the pointer in `CLASS_OBJECT_VALUES`,
  which is a registered root and *is* forwarded — which keeps the OBJECT alive
  and the side table's copy correct, and does nothing for the register.
  Reachability is not the invariant; the invariant is that the register you are
  still going to use was rewritten.

### Changed

- **`scripts/gc_root_dominance_check.py`: `js_implicit_this_set` is a root
  READ.** Being non-collecting is exactly what makes a call a root read — the
  same rule `js_closure_get_capture_bits` is listed under — and this one was
  `NONCOLLECTING` without being a source, so the checker saw a safe call, never
  classified the result as a heap value, and reported nothing at either end.
  That blind spot is why `prev_this` survived #7206 and #7214. It reports the
  class now: **214 hits on the parent, 0 after**, the largest single named sink
  in the corpus.

- **`scripts/gc_root_dominance_allowlist.json` is empty.** The four #7211
  entries were deleted because the fix made every one of them match nothing —
  rule 1 working as designed: the gate goes red on a stale entry, and that red
  is the instruction to delete it. An empty list is not a disarmed gate; rule 3
  still fails any violation, the `--min-files`/`--min-binds`/`--min-funcs`
  floors still refuse a corpus that did not exercise the subject, and
  `--self-test` still proves the checker can report a planted violation.

- **`--stale-registers` now honours `--min-binds`**, and `--any-def` with
  `--stale-registers` is a usage error. Both are the "a knob that is silently
  ignored is a disarmed knob" rule the mode already applied to `--max-stale`
  and `--fatal-sinks`, applied in the other direction. `--min-binds` is not a
  bind-anchored-check detail: `run_stale` classifies a shadow-slot load as a
  heap-value source by looking it up in the same `BIND_RE`-derived map, so a
  corpus compiled with `PERRY_INLINE_SHADOW_SLOT=1` loses those sources
  entirely, reports `total 0`, exits 0, and is indistinguishable from a clean
  corpus. That is hazard 4 — the gate runs but its subject never did. Both
  directions are asserted in `--self-test`, over-budget and under.

## Verification

Everything below is measured against the parent commit (`7d1dc9ca2`), built
from its own worktree rather than borrowed from another one.

`sfw-registry --help`, 141 modules, compiled **and** run with
`PERRY_GC_MOVING_LOOP_POLLS=1` (it is compile-time since #7161 *and*
runtime-armed at `gc/policy.rs:1759`; arming only one is the false green that
cost #7214 a round):

| | parent | this PR |
|---|---|---|
| `POLLS=1` | **0/10** — SIGSEGV every run | **10/10 clean** |
| default (no polls) | 10/10 clean | **10/10 clean** |

Gap tests:

| | parent | this PR |
|---|---|---|
| `test_gap_gc_typeof_string_cache_rooting.ts`, `POLLS=1` | `bad 444` **10/10** | `bad 0` **10/10** |
| same, `POLLS=1` + `PERRY_GEN_GC=0` | `bad 0` | `bad 0` |
| `test_gap_gc_closure_call_prev_this_rooting.ts`, `POLLS=1` + `PERRY_GC_ZEAL=1` | `bad 400` **5/5** | `bad 0` **10/10** |
| same, `PERRY_GEN_GC=0` + zeal | `bad 0` | `bad 0` |

The `prev_this` test needs zeal and the reason is worth recording: the window
is a user call, so the collection that exploits it has to be a *moving* one,
and the only moving collections are the loop back-edge poll and the
microtask-pump safepoint. Allocation-triggered collections take
`ManualGcScanGuard::force_full_scan`, which makes the copying minor ineligible.
Zeal is the sanctioned instrument for exactly that, and the `PERRY_GEN_GC=0`
arm proves the test tracks collector mode rather than merely being flaky.

`ClassExprFresh` has **no runtime gap test, deliberately**, and the same
argument is why: its window contains only `js_object_set_field_by_name` — no
user code and no loop, therefore no moving collection can land in it today. The
subject is the invariant, so the gate is the gap test, and it moves:

| checker, `--moving-only`, 130 modules / 2170 functions | parent | this PR |
|---|---|---|
| bind-anchored violations | 5 (all 4 allowlist entries used) | **0**, allowlist empty |
| `--stale-registers`, total | 2912 | **2817** |
| `--stale-registers`, `sink=js_implicit_this_set` | 214 | **0** |
| `--stale-registers --fatal-sinks` | 279 | 279 (untouched by this change) |

`cargo test -p perry-codegen`: **6 failures on the parent, the identical 6
here** — all `loop_safepoint_purity`, which is #7161's default flip, measured on
the parent rather than assumed.

Cost, over the 141-module `sfw-registry` corpus. The implicit-`this` root is on
the dynamic-dispatch path, so it was measured rather than argued:

| | parent | this PR | delta |
|---|---|---|---|
| linked binary | 26,848,048 B | 26,881,088 B | **+0.12 %** |

## Not fixed here, and why

`js_get_string_pointer_unified` hands generated code a **raw** `*StringHeader`,
and its SSO branch allocates. Every `unbox_str_handle` site in
`expr/compare.rs`, `lower_string_method.rs` and `lower_array_method.rs` unboxes
its operands back-to-back into bare registers before one consuming call, so the
first handle is live across the second unbox with no root describing it. It is
**not exploitable today** — that allocation reaches the alloc-point arm of
`gc_check_trigger`, which forces a conservative stack scan, which makes the
copying minor ineligible, so the collection it can cause never moves anything
and the same scan keeps the handle alive. Both halves are closed by accident,
and by accident is the point: it rests on the alloc-point arm staying
non-moving, which is the property the moving-GC work keeps eroding.

Filed as **#7213** rather than fixed here. A `GcSuppressScope` around that
allocation makes the window sound and costs nothing measurable, and it was
written and then reverted: shipping a GC-trigger change with no test that can
fail without it is exactly what CLAUDE.md's knob-kill policy exists to stop.
`js_get_string_pointer_unified` is likewise left out of the checker's
`ROOT_READ_CALLS` — classifying it would report the whole family, roughly forty
sites, and that population deserves its own measured count rather than being
folded in here.
