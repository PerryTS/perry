### perf(codegen): route the class-id dispatch tower to the proven-`this` clone behind an inline keys check (#7142)

`benchmarks/app-patterns/kernels/batch.ts` — the workload `Ptr<Shape>` exists
for — consumed nothing from representation-selection Phase 5a. Its hot call is
`r.rescore(1.5)` inside `rows.map((r) => …)`, whose closure parameter has no
static class, so `receiver_class_name()` returns `None`, neither of the two
routing sites #7141 fixed is ever reached, and the call lowers to the class-id
switch tower in `lower_call/property_get/dynamic_dispatch.rs`.

#7141 refused to route that tower because a `class_id` match is not a layout
proof: `delete inst.f` compacts the packed inline slots while **preserving**
`class_id` (`object/delete_rest.rs`), so on `class Row { a; b; c }`,
`delete row.b` moves `c` from slot 2 into slot 1 and the clone's bare
fixed-offset loads would read the wrong slot.

The compaction installs a freshly **cloned** keys array, so an inline pointer
compare against the class's `@perry_class_keys_*` token catches it exactly.
That check is now emitted at `idispatch.caseN` — one basic block, 21 IR
instructions, no calls: four loads off the receiver (three from header words the
tower's own `js_object_get_class_id` already touched), one entry-hoisted token
reload, nine ALU ops, one branch. The dereference needs no gate/deref split
because a non-zero class-id match already rejects the handle band, the
Set/Map/RegExp registries, out-of-heap addresses and non-`GC_TYPE_OBJECT`
allocations.

The check is **dynamic on purpose**. The `delete` shape barrier that stands the
whole analysis down is collected per module while receivers alias across modules
(#7143), so at this site a static proof would be the wrong instrument — which is
also why the two pre-existing routing sites were safe and this one was not.

Beyond the keys token the check carries the sticky
`@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` latch (prototype-level descriptors,
tracing mode), the per-object `OBJ_FLAG_HAS_DESCRIPTORS` bit (instance-level
installs deliberately do not flip the process-global latch, #5654),
`OBJ_FLAG_FROZEN` (a proven-`this` clone may contain field writes, and Phase 5a
rules a frozen receiver out only through a module-scoped kill — this makes the
tower route strictly stronger than `js_method_direct_shape_guard`), plus
not-forwarded and `object_type == OBJECT_TYPE_REGULAR`. Routing is restricted to
cases whose receiver class **declares** the method, so the clone's `this` is
exactly the class it was compiled for.

Unlike the two guard-dominated sites, this one pays for its own proof, so it
consults a profitability gate (`collectors/repsel_benefit.rs`, the module #7132
added) instead of routing unconditionally: the re-check is one instance of the
same header check the public body runs at every `this.field`, so the trade is
*N in-body checks for 1 at the call site* and the break-even is exactly `N == 1`.
It routes at `N >= 2`. The rule is a count with no target-specific term.

Measured on `batch.ts` (perry-dev, darwin-arm64), the two bodies the tower now
chooses between:

| body | IR lines | class-field guards | by-name field ops | total `js_*` call sites |
|---|---|---|---|---|
| `…__Row__rescore` (public) | 352 | 3 | 3 | 15 |
| `…__Row__rescore__pshape` | 189 | 0 | 0 | 6 |

`…__rescore__pshape` goes from **0 call sites to 1** — the module-wide static
totals are deliberately unchanged, because the public body survives as the miss
arm and as the registered vtable symbol, which is why the A/B is reported at the
call site rather than as a module count.

Timed on a quiet Raspberry Pi 5 with `perf stat`, two compilers from the same
tree (arm `before` has the tower route forced off, nothing else), ASLR disabled,
pinned to one core, interleaved, 12 reps each. The workload is bimodal at ~1%
independently of the arm (3/12 runs per arm in the low mode; zero GC collections
in both, so it is not a GC-schedule effect), so the delta is reported per mode:
**−0.156%** in the low mode and **−0.168%** in the high mode, overall median
−0.17%. That is ≈6.0M instructions over 40,000 `rescore` calls, ≈150
instructions per call; the whole-program figure is small because `rescore` is a
small share of `batch.ts`, which is dominated by allocation, `sort` and
`reduce`. The fixture is there because its *receiver shape* is the hard case,
not because its method is the hot spot.

`test-files/test_gap_repsel_pshape_tower_delete.ts` is the soundness test, built
red-first: construct → `delete` a field through a cross-module alias → call the
method. Against a class-id-only route it prints `after: 103,NaN,309,412` where
Node 26.5.1 prints `103,206,309,412`; with the keys check the whole file is
byte-identical to the oracle. Registered in `test-parity/gc_repsel_corpus.txt`
and GC-live by construction (`copied_objects=5971` at `--pressure 8`), so the
evacuating arms have a receiver to move rather than reporting the file inert.

Three call-site ratchets in `collectors/proven_this_routing_tests.rs` with
disjoint red sets, verified by building each sabotage arm: reverting the routing
reddens only the routing test, removing the keys compare reddens only the
keys-guard test (which traces the token global → entry slot → guard-block reload
→ `icmp eq i64` → the branch into the clone's block), and removing the
profitability gate reddens only the refusal test.

Unchanged `(double this, args…)` ABI; the clone still stores its receiver to a
slot and `js_shadow_slot_bind`s it with no safepoint between (#6925/#6990 —
`GC_TYPE_OBJECT` is movable in the shipped configuration, so the `TaPtr` no-bind
shortcut does not transfer).
