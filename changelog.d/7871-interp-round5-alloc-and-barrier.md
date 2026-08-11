### `interp` 1.095 → 0.844 s, `iso_miss` 1.464 → 1.234 s: two independent codegen gates

Quiet M1 mini, best-of-5, exit-checked, outputs byte-compared to
`node --experimental-strip-types`. The four binaries that come out
**byte-identical** across the two arms (`churn`, `push_cls`, `retain`, `fib40`)
set the run's noise floor at −0.1%/+0.3%, which is what makes the rest credible.

| bench | before | after | delta |
|---|--:|--:|--:|
| `interp` | 1.0945 | **0.8441** | **−22.9%** |
| `cycles` | 0.1115 | **0.0866** | **−22.3%** |
| `iso_miss` | 1.4635 | **1.2338** | **−15.7%** |
| `tree` | 1.1665 | 1.0222 | −12.4% |
| `tree_wide` | 1.6464 | 1.5216 | −7.6% |
| `deeplist` | 0.1071 | 0.1018 | −4.9% |
| `pipeline` | 0.2738 | 0.2643 | −3.5% |
| the other 12 | — | — | within ±0.3% |

#### 1. The class-field write barrier now tests the parent's generation

`expr/write_barrier.rs::emit_jsvalue_slot_store_pointer_tested` (#7511) put the
store's three bookkeeping calls behind one live test of the stored **value** —
"does this publish a heap pointer at all". It never asked the barrier's other
question, "is the parent old enough for anyone to care", even though
`emit_parent_may_need_remembering_check` sits 400 lines up in the same file.
That predicate had exactly one caller: `expr/array_push.rs`.

HIR rewrites every closed-shape object literal into a `new` of a synthesized
anon-shape class, so `{ kind: "num", num: n }` reaches a shared
`<class>_constructor` that writes its fields into an instance allocated a few
instructions earlier **in the nursery** — the `!TENURED` case, where the minor
GC retraces the parent anyway and the remembered-set record is pure cost. The
same predicate now gates the class-field store, on the identical argument
(`Old ⟹ TENURED`, plus the incremental-cycle count so SATB shading is never
skipped). It stays a **live header test**: a parent promoted between its
allocation and the store reads `TENURED` and takes the call.

#### 2. A hot recursive function may inline its bump allocator

`lower_call/new_alloc.rs::new_site_is_in_loop` admitted a `new` site to the
inline bump allocator only if it was lexically in a loop or its function was in
`collect_hot_loop_callees` — a set capped at **4 direct call sites module-wide**.
That cap is `inlinehint`'s anti-bloat backstop, where cost scales with call
sites because LLVM duplicates the body at each one. The inline bump allocator
costs ~268 bytes **per `new` site in the function**, once, whatever the caller
count, so the cap priced a cost that does not exist — and excluded exactly the
functions that earn it. `interp.ts`'s `evalNode` is the shape: the hottest
function in the program, one allocation per invocation, and 11 direct call
sites because ten of them are its own recursion.

New collector `collect_alloc_hot_functions` answers the allocator's question
with the allocator's cost model: ≥1 in-loop direct call site (**uncapped**), or
direct self-recursion — a function that calls itself is a loop the lexical test
cannot see. It is a second set, not a widening of `hot_loop_callees`: raising
the shared cap to 32 instead buys `interp` −26.5% but **regresses `iso_miss`
+4.0%**, because it also moves `inlinehint`.

`interp`'s compiled binary grows 16 KB (+0.13%).

#### Validation

19-program corpus, both arms: outputs byte-identical to node and exit 0,
including the `iso_miss` `misses 0` counter and `shapes`' `1176000`. 10 of 19
binaries are byte-identical across the arms, so only 9 needed timing at all.
The nine that differ also pass under `PERRY_GC_VERIFY_EVACUATION=1
PERRY_GC_FORCE_EVACUATE=1`, and five of them additionally under
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800
PERRY_GC_SCHEDULE_RATE=1`.

`expr/class_field_barrier_tests.rs` is the gate, and it is **sabotage-verified**
rather than merely green. Its first draft asserted the TENURED mask and the
incremental-count load were *present in the branching block* — and passed a
sabotage that hard-wired the branch to `br i1 false` while leaving the dead
predicate instructions behind it. It now walks the def chain from the branch
condition (`or i1` → `icmp ne i8 … , 0` → `and i8 …, 32` → `load i8`; and
`icmp ne i32` → the atomic count load), and both sabotages — constant condition,
swapped successors — go red with the diagnostic that names the failure.

#### Refuted

`PERRY_WRITE_BARRIERS=0` is **not** a ceiling probe for barrier cost. It makes
`interp` 4.3× and `iso_miss` 5.4× *slower*: the knob is compile-time and the
GC's evacuation policy requires generated barriers to be active, so turning
them off makes the copying minor ineligible and the program falls back to full
mark-sweeps. It measures "no generational GC", not "no barrier".
