### Gate the guarded array-element store's write barrier on the stored value

`this.vals[i] = v` — the in-bounds arm of the guarded element store — called
`js_write_barrier_slot` unconditionally. On `gc-handoff/apps/pipeline.ts`, whose
`Registry.set` runs that statement 1.44 M times, `PERRY_GC_TRACE=1` counts
**1,265,933 barrier calls of which 1,265,925 (99.999%) return at
`non_pointer_child_skips`**: the stored value is a number, so the call decodes
it, finds no heap pointer, and returns having done nothing.

The call now sits behind the same live test of the stored bits that the
class-field store has carried since #7511, and then behind the parent's
generation test from #7871. The two are **nested, value first**, not fused into
one `and`: `emit_may_carry_heap_pointer_check` is pure register arithmetic on
the value, while the parent test performs a `monotonic` load of
`@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` that LLVM may not hoist out of
the loop — fusing would charge that non-hoistable load to exactly the numeric
stores this exists to make free.

Skipping on the value test alone is sound because `write_barrier_slot_inner`'s
first action is `barrier_child_prologue(child)`, which returns before the SATB
shading, the armed check, the parent decode and the remembered set when
`decode_heap_addr(child) == 0`; the emitted predicate is a documented superset
of that decode (`gc::tests::inline_pointer_bearing_contract` enumerates the
whole 16-bit tag space against it). A value carrying no heap pointer has nothing
to shade, which is why the incremental clause belongs to the parent half and not
this one. The parent half's obligations are unchanged and stay pinned by
`gc::tests::inline_generation_gate_contract`.

`expr/index_set_barrier_tests.rs` walks the emitted def chain rather than
looking for instructions nearby: a hard-wired `br i1 true`/`false` that leaves
the dead predicate in the block fails it, as does dropping any of the three heap
tag comparands, putting the barrier on the false edge, or eliding the call
instead of guarding it.

Measured on the 19-program GC corpus: only `pipeline`, `interp` and `iso_miss`
contain the site at all, and `parent_not_old_skips` is 0 on all three — the
value test is the lever there, not the generation test.
