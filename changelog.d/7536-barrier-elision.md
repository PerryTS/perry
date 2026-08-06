### Codegen: class-field store GC bookkeeping decided by one inline test instead of three calls (#7511)

Write barriers were 16.1% of `churn_alloc`'s symbolicated profile on a program
whose stores are all doubles, and #5334 lever D — which elides the barrier for a
value that is a non-pointer *by construction* — never fired there.

**Why lever D cannot fire, which is not what the ticket assumed.** The ticket's
premise ("the types say `v: number`") is wrong in that form: Perry does not
validate declared types at runtime, so a field declared `number` legitimately
receives a string through an `any`, and the annotation is never a layout fact.
The real reason is structural. Since the `[#bloat]` `force_ctor_call` default
(`lower_call/new.rs`), a class with its own constructor is **not inlined** at the
`new` site — its body is compiled once as the shared
`<class>_constructor(this, p0, …)` symbol and called. HIR rewrites every
closed-shape object literal into a `New` of a synthesized anon-shape class with
exactly that constructor shape (`lower/context.rs::mint_anon_shape_class`), so
`{v: base + j, w: j}` lands there too. The expression reaching the field store is
therefore `Expr::LocalGet(<ctor param>)` — an LLVM function argument of a
function shared by every `new` site in the module, including ones that pass
pointers. No by-construction proof about that value can exist at that site.

What can be decided there is the same question the three bookkeeping callees each
ask first, one at a time, across three cross-crate calls: `js_write_barrier_slot`
(`barrier_child_prologue` returns immediately when `decode_heap_addr(child) == 0`),
`js_string_addref_if_heap_string` (tag-checked no-op off `STRING_TAG`), and
`js_gc_note_slot_layout` (for a non-pointer value the note can only ever *clear*
mask state, never set it — the identical argument
`class_field_store_needs_layout_note` already ships, including its
`requires_raw_f64 == false` precondition). Ask it once, inline, and branch over
all three:

```text
may_carry_heap_pointer(bits) :=
      (bits >> 48) ∈ { 0x7FFA, 0x7FFD, 0x7FFF }   // BIGINT / POINTER / STRING
   || ((bits >> 48) == 0 && bits >= 0x1000)       // bare heap address
```

The slot store stays unconditional and outside the branch. Callers that already
proved the value statically pass all three flags `false`, so lever D's existing
elision emits no test and no blocks at all — this composes with it rather than
replacing it. The bare-address floor is the *lower* of the two runtime floors
(`layout_pointer_bearing_bits`' `0x1000` rather than `decode_heap_addr`'s
`0x10000`), which is also what keeps `0.0` — whose bit pattern is all zeros —
off the slow path.

Soundness is the superset property, pinned from both sides because codegen cannot
call the runtime predicates. `perry-runtime`'s new
`gc::tests::inline_pointer_bearing_contract` enumerates all 65,536 tags against
`decode_heap_addr` *and* `layout_pointer_bearing_bits`, and carries a stranding
witness: an old parent, a young child, and the exact guarded store sequence, where
a sabotaged always-false guard leaves the old→young edge unrecorded and
`verify_old_to_young_edges_covered` rejects it, while the shipped guard keeps it
and the remembered-set scan marks the child. Deleting `0x7FFD` from the comparand
set turns three of the five tests red. Codegen-side structure (store outside the
guard, all three calls inside it, every call still emitted, no guard on the
raw-f64 path) is pinned in `tests/class_field_store_pointer_test.rs`.

Measured on `gc-handoff/bench/churn_alloc.ts` via `PERRY_GC_TRACE` counters:
barrier calls 79,780,888 → 39,890,444 with `non_pointer_child_skips`
39,890,444 → 0 — exactly the wasted calls, with every remaining call doing real
work and the GC cycle count unchanged at 105. On the pinned quiet host (M1, load
1.46, best-of-7 `user+sys`, both arms linked against the same runtime archive):
`churn_alloc` 1.930 s → 1.630 s (1.18×) and `churn` 2.210 s → 1.960 s (1.13×),
with `push_cls`, `tree`, `churn_read`, `deeplist` and `push_num` unchanged — the
prediction, since none of those uses the `class_field_set` store path. The whole
bench set produces byte-identical stdout with `rc=0` and re-runs clean under
`PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_VERIFY_MARK=1` with copying minors
confirmed live (105 copying-minor verifications on `churn_alloc`).

The `put.pic.hit` store path (`expr/proxy_reflect.rs`), which is what a
user-written `this.f = v` on a union-typed field lowers to, emits the same triple
and is untouched — its layout note is the scalar-aware variant and its existing
static skip rests on a separate precondition, so it is left for a follow-up.
