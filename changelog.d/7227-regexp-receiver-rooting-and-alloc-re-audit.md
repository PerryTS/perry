### Fixed

- **`codegen`: the receiver of `re.test(s)` / `re.exec(s)` is rooted across the
  `ToString(s)` coercion the lowering emits below it** (#7154). Both
  `Expr::RegExpTest` and `Expr::RegExpExec` lowered the receiver first, unboxed
  it to a raw `RegExpHeader*` in a bare SSA register, and only then emitted
  `js_jsvalue_to_string_coerce`. That coerce is not a bystander: it allocates,
  and on an object argument it dispatches a user `[Symbol.toPrimitive]` /
  `toString` / `valueOf`, which is arbitrary JS with its own loop back-edge
  polls. Under `PERRY_GC_MOVING_LOOP_POLLS=1` one of those polls runs an
  evacuating minor while the regexp is live only in that register.

  This is the residual #7226 measured and named rather than fixed. In the
  `sfw-registry` reproducer it is `src/lib/api/shared.ts:67`,
  `/\[[a-zA-Z]+\]/.test(url)`, faulting at
  `perry_fn_src_lib_api_shared_ts__defineApiCall + 404`:

  ```asm
  bl   js_regexp_new                 ; ALLOCATES
  and  x20, x0, #0xffffffffffff      ; raw regexp pointer -> bare register
  ldr  d0, [sp, #0x28]
  bl   js_jsvalue_to_string_coerce   ; ALLOCATES, runs user toString
  mov  x0, x20                       ; STALE
  bl   js_regexp_test                ; faults here
  ```

  The receiver now takes the established `guard_store_operand_across` /
  `reread_store_operand` pair, and the unbox moves BELOW the coerce — unboxing
  above it is what parked the pre-move address in a register in the first
  place. `RegExpExec` had the identical defect and is fixed with it.

### Changed

- **`scripts/gc_root_dominance_check.py`: `ALLOC_RE` audited against the
  runtime's real symbol table instead of an assumed naming convention.** This
  is the more valuable half of the change, because the miss above was the
  *second* allocator to escape this pattern (`js_implicit_this_set` was the
  first, #7226) and each one has cost a full investigation round.

  The old pattern carried an alternative spelled `regexp_alloc\w*`. **No such
  symbol has ever existed.** It was not a typo — it was an extrapolation:
  whoever wrote it knew a RegExp allocates and inferred the `_alloc` suffix
  from its neighbours. Reconciling every alternative against
  `extern "C" fn js_\w+` over perry-runtime + perry-stdlib, intersected with
  the names perry-codegen actually declares, found **four alternatives matching
  nothing at all** — `regexp_alloc\w*`, `promise_alloc\w*`, `bigint_alloc\w*`
  and `typed_array_alloc\w*`. A quarter of the pattern was decorative.

  The root cause is that the runtime materializes fresh GC objects under
  **three** naming conventions and the pattern modelled one:

  | convention | examples | matched before |
  |---|---|---|
  | `*_alloc*` | `js_object_alloc`, `js_array_alloc`, `js_closure_alloc`, `js_uint8array_alloc`, `js_inline_arena_slow_alloc` | yes |
  | `*_new*` | `js_regexp_new`, `js_promise_new`, `js_symbol_new`, `js_date_new`, `js_error_new`, `js_typed_array_new`, `js_weakmap_new`, `js_url_new`, `js_boxed_string_new`, ~140 more | **no** |
  | `*_create*` | `js_object_create`, `js_array_create`, `js_vm_create_context`, `js_crypto_create_hash`, ~40 more | only `object_create*` |

  All three are now matched as conventions, and the constructors that use none
  of them are enumerated explicitly: the `_construct*` ctor forms, the fresh
  string producers (`string_coerce`, `jsvalue_to_string*`, `string_slice`,
  `string_to_*_case`, `string_pad_*`, `string_trim*`, …), the copy-on-read and
  ES2023 change-by-copy array family (`array_to_sorted*`, `array_to_spliced`,
  `array_with`, `array_flat*`, `array_like_to_array`, `iterator_to_array`, …),
  the whole-object producers (`object_keys*`, `object_entries*`,
  `object_from_entries`, `object_get_own_property_descriptor*`,
  `structured_clone*`, the Set-methods family), the namespace/class-shape
  helpers, and BigInt's `bigint_from*` (which has neither `_alloc` nor `_new`).

  Widening is safe in the checker's one-sided direction: a name that turns out
  not to allocate costs a false positive to triage, while a missing one costs a
  shipped use-after-free plus the round it takes to find by hand. The file now
  says so, so the next person extends it rather than guessing.

- **`scripts/gc_root_dominance_check.py`: the ToPrimitive / ToString / ToNumber
  coercion family is `POLL_CAPABLE_RUNTIME`.** This is the second half of the
  same blind spot and it is the half that matters for CI, because
  `--moving-only` is the mode `gc-root-dominance.yml` gates on. With `ALLOC_RE`
  widened but the coercions unmodelled, the `/re/.test(s)` site was reported by
  the raw `--stale-registers` count and **still invisible to `--moving-only`**:
  nothing in its window was classified as reaching a moving minor. A coercion
  does not look like a call into user code, but ToPrimitive is exactly that —
  and `js_string_coerce`'s own doc comment already said so ("a `POINTER_TAG`
  object routes through `js_jsvalue_to_string`, which can invoke a user
  `toString` / `valueOf`"). The checker just never read it.

- **`scripts/gc_root_dominance_check.py`: `js_regexp_test` / `js_regexp_exec`
  are fatal sinks.** A stale `RegExpHeader*` is dereferenced immediately by
  both, and this one faulted rather than merely answering wrong, so it belongs
  in the `--fatal-sinks` ranking and not only in the raw count.

## Verification

Measured against the parent (`4e99c1bad`, #7226's head), built from this
worktree rather than borrowed from another one.

The checker change is what makes the codegen change checkable, so it is
reported first. Over the gap-test IR for the new reproducer:

| `--stale-registers` over `test_gap_gc_regexp_receiver_rooting.ts` | parent | this PR |
|---|---|---|
| base checker (`regexp_alloc\w*`) | **0 reported** | — |
| widened `ALLOC_RE`, raw count | 3 | **0** |
| widened + `--moving-only` (the gate's mode) | **3**, `MOVING: YES via js_jsvalue_to_string_coerce` | **0** |

All three are named exactly: `source (alloc): call i64 @js_regexp_new`,
`stale use: call i32 @js_regexp_test` / `@js_regexp_exec`, with
`js_jsvalue_to_string_coerce` in the window.

Over the 130-module / 2170-function gap corpus, both checker widenings
together:

| `--moving-only` | parent | this PR |
|---|---|---|
| bind-anchored violations (**the gate**) | 0 | **0**, allowlist still empty |
| `--stale-registers`, total | 2730 | 2738 |
| `--stale-registers --moving-only` | 2 | **62** |
| `--stale-registers --fatal-sinks` | 279 | 282 |
| `--stale-registers --moving-only --fatal-sinks` | 0 | **0** |
| `--unrooted-allocas`, moving-reachable | 57 | 85 |

The gate does not move. The 60 newly-*moving* stale-register leads were all
already in the 2730-entry diagnostic list; modelling the coercions is what
reclassified their windows. Triaged mechanically by the shape of the stale
use:

| stale use | count | verdict |
|---|---|---|
| `lshr … , 48` | 37 | NaN-box **tag** read. Relocation rewrites the low 48 bits; the tag is unchanged. Not a bug. |
| `fadd double` | 3 | float arithmetic on a value the mode could not prove non-pointer. Not a bug. |
| `getelementptr i8, …, 32` | 15 | direct field access in the `*__pshape` pointer-shape specializations. A real dereference shape and a real population — **left for its own PR**, since it is the `PERRY_PTR_SHAPE_LOCALS` family and wants its own measured count. |
| call argument | 7 | typed-feedback array receivers held across `js_number_coerce`. Same call: real shape, own population, own PR. |

Nothing in the newly-visible set is a fatal sink, which is why the fatal count
moves only by the three regexp entries this PR then fixes.

Gap test — `test_gap_gc_regexp_receiver_rooting.ts`, compiled **and** run with
`PERRY_GC_MOVING_LOOP_POLLS=1`:

| | parent | this PR |
|---|---|---|
| `POLLS=1` + `PERRY_GC_ZEAL=1` | **0/10 — SIGSEGV/SIGBUS every run** | `bad 0` **10/10** |
| `POLLS=1` + zeal + `PERRY_GEN_GC=0` | `bad 0` | `bad 0` |

The parent arm is a hard fault rather than a nonzero `bad`, and that is the
honest signature: with a regex **literal** receiver — the registry's shape —
`js_regexp_new`'s result is held only in the register, so the evacuating minor
retires the block under it and the deref lands in from-space. The
`PERRY_GEN_GC=0` arm proves the test tracks collector mode rather than being
flaky. Zeal is required for the same structural reason #7226 recorded for
`prev_this`: the window is a user call, so only a *moving* collection exploits
it, and allocation-triggered collections take
`ManualGcScanGuard::force_full_scan`, which makes the copying minor ineligible.

## `sfw-registry` moves, and does NOT reach 30/30

`sfw-registry --help`, 141 modules, `PERRY_FORCE_WELL_KNOWN=iovalkey`, compiled
**and** run with `PERRY_GC_MOVING_LOOP_POLLS=1`:

| | parent (`4e99c1bad`) | this PR |
|---|---|---|
| `POLLS=1`, **30 runs** | **0/30** | **28/30** |

Both arms use the same runtime archives (the codegen fix is the only
difference) and the same firewall tree, so the comparison is like-for-like.
The parent's 30 failures are **not** crashes — every one is a deterministic
`TypeError: Cannot convert undefined or null to object`, which is what a stale
regexp receiver produces here: `defineApiCall` computes
`urlRequiresInterpolation = /\[[a-zA-Z]+\]/.test(url)` at definition time, the
stale read returns the wrong boolean, and the wrong branch hands `undefined`
to a downstream `Object` operation. The fix removes that failure mode
completely.

Note that #7226 reported 26/30 for this same parent commit. That measurement
was taken against a different firewall checkout; on the tree measured here the
parent is 0/30. The delta this PR is responsible for is the one measured above,
on one tree, with one runtime.

**This does not close #7154 and #7161's stopgap stays.** Two runs in thirty
still SIGSEGV, and the residual is a *different* object from the one this PR
fixes. Under `PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` it is deterministic — **40/40** — and the
reporter names it:

```
[gc-fromspace-protect] FAULT: signal 10 at 0x…5d5c
  block=0x…20000 +220508 retired_bytes=253416 retired_by_minor=#155
  last-known object: user_ptr=0x…5d58 obj_type=3 size=80
```

`obj_type=3` is a **string**, not a `RegExpHeader`, so it is not the receiver
this PR rooted. Disassembling the faulting frame confirms it: the return
address is `defineApiCall + 428`, and `+424` is `bl js_regexp_test` — the same
call site, twenty bytes further along, which is exactly the size of the
`js_gc_temp_root_push` / `js_gc_temp_root_get` pair this PR inserts. The
receiver operand is now correct; the surviving stale value is a string reaching
the same call.

That the protected arm is 40/40 while the unprotected arm is 2/30 is the useful
part: the next round has a deterministic reproducer instead of a 7 % one. It is
**not** fixed speculatively here — no edit ships without a test that can fail
without it.
