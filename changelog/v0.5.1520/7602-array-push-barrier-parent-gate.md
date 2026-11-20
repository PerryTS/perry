### Codegen: the array-push write barrier is gated on a live parent-generation test (#7511)

Write barriers were **29.6% of `push_cls`'s symbolicated leaf profile** at
v0.5.1339 — re-measured after #7536, #7594 and #7596, because the ticket's
headline was taken at v0.5.1325 and three tickets this campaign were worked from
stale numbers. It had not collapsed; it had grown.

| symbol | leaf samples | share |
|---|--:|--:|
| `js_write_barrier_slot` | 346 | 15.1% |
| `write_barrier_decoded_parent` | 161 | 7.0% |
| `barrier_child_prologue` | 139 | 6.1% |
| `incremental_mark_barrier_value` | 30 | 1.3% |
| **total** | **676 / 2286** | **29.6%** |

**All of it is one call site**, and the call graph names it: `chunk + 568`, the
`keep.push(new Node(...))`. `push_cls_ts__Node_constructor` appears as a leaf
with no barrier beneath it at all — the constructor's two `number` field stores
already pay nothing, because a declared-`number` field selects the raw-f64
representation and its store is guarded by an inline plain-finite check with a
downgrading cold fallback (`property_set.rs`, the `ptr_shape_set.raw_store`
arm). That is why #7536 measured `push_cls` unchanged.

**What the surviving barrier actually does, by `PERRY_GC_TRACE` counters:**

| bench | calls | `non_pointer_child_skips` | `parent_not_old_skips` | `old_to_young_slow_hits` | `new_inserts` |
|---|--:|--:|--:|--:|--:|
| `push_cls` | 19,945,222 | 0 | 19,743,573 (99.0%) | 0 | **0** |
| `churn_alloc` | 19,945,222 | 0 | 19,743,573 | 0 | **0** |
| `churn` | 19,945,222 | 0 | 19,743,573 | 0 | **0** |
| `tree` | 62,612,898 | 20,867,845 | 41,271,511 | 0 | **0** |
| `cycles` | 15,674,953 | 7,832,430 | 7,368,973 | 0 | **0** |
| `retain` | 6,304,687 | 0 | **0** | 3,204,922 | 6,268 |

The remembered set is **never inserted into — not once — in 20 million calls** on
the three headline benches. And #7536's value-side test can reach none of it:
`non_pointer_child_skips == 0`, because the pushed value genuinely *is* a heap
pointer. `expr_produces_non_pointer_bits_by_construction` is not merely
unhelpful here, it is *correct* to say "pointer". **The waste is entirely
parent-side**, which is a question no by-construction proof can answer: `keep`
crosses hundreds of collections between its allocation and its last push, so
"the parent is young" is exactly the kind of claim #7501 showed gets revoked at
runtime.

So it is decided by a **live test at the store**, which is what #7501 concluded
collector-facing metadata requires:

```text
parent_may_need_remembering(parent) :=
      (header(parent).gc_flags & GC_FLAG_TENURED) != 0
   || PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT != 0
```

Both clauses are load-bearing, for unrelated reasons.

**The TENURED clause.** The remembered set only ever needs an entry when
`barrier_parent_needs_remembering` classifies the parent `Old`. Soundness is the
superset property again — `Old ⟹ TENURED`, so `!TENURED ⟹ !Old`, and this gate
can only skip a subset of what the runtime already skips. Every path that places
an object into an old-gen block sets the bit in the same breath
(`gc/copying.rs:612–637` selects `arena_alloc_gc_old` and `GC_FLAG_TENURED` from
one `promote` expression; `gc/oldgen.rs:1740` and `:1838`; `buffer/header.rs:486`;
`typedarray/mod.rs:722`; `json_tape.rs` via `arena_alloc_gc_old_born_tenured`),
and nothing ever clears it on a live object. The stronger justification is that
a parent which is neither physically old nor logically tenured is **fully traced
by every minor GC** (`gc/trace.rs:747`), so its edges are rediscovered.

That invariant is true but **unenforced**: `arena_alloc_gc_old` writes
`GC_FLAG_ARENA | gc_birth_extra_flags()` and leaves the bit to each of its eight
callers, so a ninth would compile, pass every test, and strand a child in
generated code only. Per this repo's gate doctrine an invariant a fast path
depends on must be able to fail, so it is pinned over the production birth paths
by `every_old_gen_birth_path_sets_tenured`.

A `debug_assert!` in `barrier_parent_needs_remembering` — the better enforcement,
since it rechecks at every old-parent store in every debug run — was written and
then **reverted**: dozens of existing tests build old-gen fixtures straight from
`arena_alloc_gc_old` without the bit (`alloc_old_test_object`,
`alloc_old_test_array`, `alloc_old_test_promise`, most of `gc/tests/oldgen.rs`),
some deliberately, so it fired on fixtures rather than defects. Why it is absent
is recorded where someone would next try to add it; making it shippable means
fixing those fixtures first.

**The incremental clause.** Skipping the call also skips
`barrier_child_prologue`'s `incremental_mark_barrier_value` — the
insertion/SATB shading, which is not a generational question and must never be
dropped while a cycle is live. A zero count *proves* this thread's
`INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null, because
`incremental_mark_barrier_enable` installs the thread-local **before**
incrementing the count — an ordering `gc/barrier.rs` already documents as
load-bearing. This is the same gate, on the same already-exported global, that
`expr/shadow_inline.rs` and `expr/shadow_slot.rs` emit for the root shading
barrier; no new runtime symbol was needed.

The gate reads the array's header byte at `arr_handle - 7`, which the `nofwd`
block **already loads** for the forwarding test in the dominating block — so the
whole test is one `and`, one `icmp`, one global load and an `or`. It is emitted
only inside `apush.inbounds`, reached only after that header test, so the
dereference rests on a validation the existing code already performed. The slot
store stays unconditional and outside the branch; only the barrier moves. Under
`PERRY_WRITE_BARRIERS=0` nothing is emitted at all, so that knob's A/B does not
acquire a predicate wrapped around an empty arm.

**Measured.** Barrier calls on the same counters: `push_cls` / `churn_alloc` /
`churn` 19,945,222 → **119,674** (−99.4%), `push_num` 19,584,064 → 117,506.
`retain`'s real work is bit-identical across arms — `old_to_young_slow_hits`
3,204,922 and `new_inserts` 6,268 in both — so every genuine remembered-set
insert survives, and GC cycle counts are unchanged on every bench (105 / 42 / 16
/ 7 / 4 / 18). On the pinned quiet mini (load ~1.5, best-of-5 `user+sys`, both
arms against the same runtime archive, baseline measured twice): `push_cls`
0.650 → 0.490 s (**1.33x**), `churn_alloc` 0.660 → 0.510 (1.29x), `push_num`
0.310 → 0.250 (1.24x), `churn` 0.950 → 0.800 (1.19x); `tree`, `retain`,
`cycles`, `deeplist` and `churn_read` flat, because their barrier traffic is
class-field stores rather than array pushes — the honest scope limit, and the
remaining #7511 lever. All ten bench binaries produce byte-identical stdout,
including `cls_mistyped.ts`, and both arms re-run clean under
`PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_VERIFY_MARK=1` and under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` with the instrument proven live
(110 `[gc-fromspace-protect] mode=ProtectPages retired_set=#N` sets on
`push_cls`).

**Tests.** `perry-runtime`'s new `gc::tests::inline_generation_gate_contract`
pins the codegen comparand against `GC_FLAG_TENURED`; asserts the invariant
directly over the three birth paths where the bit is a caller's obligation
rather than a consequence of surviving (`arena_alloc_gc_old_born_tenured`, the
large-object arm, `buffer_alloc`); and carries a **stranding witness** — an
old+tenured parent, a young child, and the exact gated store sequence, where a
sabotaged always-false gate leaves the old→young edge unrecorded and
`verify_old_to_young_edges_covered` rejects it, while the shipped gate keeps it
and the remembered-set scan marks the child. A third arm stores through a
*nursery* parent and asserts nothing is stranded, so the sabotage arm is shown
to be about the parent's generation and not about skipping a barrier per se, and
a fourth pins that an active incremental cycle forces the call for that same
nursery parent. Codegen-side structure — barrier inside `apush.barrier`, slot
store outside it, both clauses present — is pinned in `array_push.rs`'s
`parent_gate_tests`, which run under `cargo test -p perry-codegen --lib` rather
than in the tag-only integration suite.

That last test initially could not fail. Replacing the gate's `or` with a
constant-true leaves both `and i8 …, 32` and the incremental global in the IR —
the clauses are still computed, just no longer consulted — so substring matching
stayed green while the gate had stopped gating, which is CLAUDE.md hazard 4
applied to a test rather than a job. It now follows the `cond_br`'s condition
back to its definition and requires an `or i1` of an i8 header test and an i32
count test.
