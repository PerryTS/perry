Module-init and program-entry bodies now take part in canonical
representation selection. Before this, `codegen/entry.rs` hard-coded
`repsel_context_allows_canonical_{i32,str}: false` for both entry contexts, so
**every** top-level local stayed boxed no matter what the per-value rules said
— and 9 of the 17 `benchmarks/suite` workloads put their entire hot loop at
module top level.

## Why the exclusion existed

Nothing recorded a hazard. Phase 1 (#6903) introduced it with the comment
*"top-level locals interleave with import/init machinery; the win lives in
function bodies"*, and Phase 3a (#6909) copied it for `Str`. It is a scoping
decision, and the premise is false for the corpus Perry is measured on:
`08_string_concat`'s `result` is a fully eligible canonical-`Str` local doing
`+=` self-append in a loop — Phase 3a's own motivating pattern — blocked by
nothing else.

An entry body is lowered by the same `stmt::lower_stmts_inner` as a function
body, into an ordinary straight-line LLVM function. Every entry-only property
is already covered by a value-level rule or is not a difference at all: module
globals (`@perry_global_*`) and boxed closure captures each have their own
pre-existing exclusion; entry allocas, the module-init shadow frame (which
binds pointer-typed locals only), in-frame `await` polling, id-refreshing init
unroll, and the `@perry_global_*`-only entry emission are all unchanged. The
audit is on `expr::MODULE_INIT_CONTEXT`.

`Ptr<Shape>` stays excluded in entry bodies, now on its own flag
(`repsel_context_allows_ptr_shape`). Phase 5a reused the canonical-i32 gate, so
lifting it would silently have enabled guard-free receiver access there as a
side effect of an unrelated phase — and #6991 is a live rooting bug in exactly
that position, where a compiled receiver goes stale across the
`globalThis`-population collection that runs around module init. Behaviour for
`Ptr<Shape>` is bit-identical everywhere, and `module_init_context` remains its
named unconsumed mechanism in `--opt-report`.

## What it converts

Wider sweep, 452 files (`test-files/test_gap_*.ts` + the app-pattern kernels),
canonical-slot verdicts:

| verdict | `e7bc73bd6` | `4d3ddc9a3` (#7122) | this PR |
|---|---|---|---|
| selected `I32` | 131 | 168 | **305** |
| selected `Str` | 67 | 67 | **247** |
| selected `U32` | 2 | 2 | **6** |
| denied `module_init_context` | 289 | 321 | **0** |
| denied `not_index_used_or_bounded` | 201 | 121 | 121 |
| denied `closure_referenced` / `declared_bigint` | 14 / 5 | 14 / 5 | 14 / 5 |

**All 321 `module_init_context` denials convert to selections, and the other
three denial populations are unchanged** — selected total 237 → 558, exactly
+321. No residue, no fourth mechanism. #7122 predicted that its loop-induction
rule proves 22 more locals than it can promote, 18 of them blocked only by this
issue; on this wider scope that subset is the 289 → 321 column, and it lands.

Promotion census, 25 → 26 workloads: `canonical-i32` 17 → 48, `canonical-str`
1 → 3, `canonical-u32` 1 → 2; `canonical-i32` goes from promoting in 2 of 18
real workloads to 17 of 18. `ptr-shape` / `ptr-shape-consumed` unchanged at
7 / 3. No floor was lowered; 20 were raised. The new liveness fixture
`fixture_module_init_canonical.ts` declares no function, method or closure at
all, so its counts can only come from the module-init `FnCtx`.

## Selected is not emitted, and this is a good illustration

Of 39 benchmark/fixture workloads compiled with both compilers, **10 produce a
different object**. The rest shrink the pre-optimization IR by one
`alloca double`, one `sitofp` and one `fptosi` per promoted local and then
optimize to a byte-identical object — under the parallel-shadow model every
`LocalGet` already read the i32 slot, so the double slot was dead and `-O3` was
already deleting it.

The `Str` case is a real lowering change that survives `-O3`.
`08_string_concat`'s top-level `result = result + "x"` went from two
`js_get_string_pointer_unified` calls per iteration to Phase 3a's four-arm
dispatch whose hot arm derives both handles with a bare
`and i64 …, 0xFFFF_FFFF_FFFF`. Its shadow-slot bind/store count and
write-barrier count are unchanged (9 and 3 in both arms) — canonical `Str` is
tagged-at-rest and does not move storage.

Measured after the fact (#7123): that loop scaled to 3 000 000 iterations, timed
by the program's own `Date.now()` delta over 20 interleaved pairs on an M1 under
load, goes **74 ms → 69 ms (−6.8 %)**, distributions 72–79 ms vs 68–72 ms. It is
the only timing claim attached to this change, and it is confined to the idiom
the change targets — the other 29 of 39 workloads compile to a byte-identical
object.

## Also

`note_canonical_local` reported a hard-coded `RegionKind::Function` to
`--opt-report`. That was invisible while module-init bodies could not select at
all; it now takes the region from the ambient scope, the same source `consume()`
uses, so a module-init selection renders as `module-init`.
