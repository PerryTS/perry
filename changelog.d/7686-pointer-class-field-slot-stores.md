### class fields: one pointer field no longer demotes an object's whole store set (#5094)

A single pointer-typed class field (`peer: Cell | null`, `next: LNode | null`,
`left: Tree | null`) put **every** field store on that object — its `number`
fields included — on `js_object_set_field_by_name`: by-name dispatch, a
`RuntimeHandleScope`, `layout_note_slot`, and a per-object side-table entry, for
stores whose slot index is a compile-time constant.

Quiet M1 mini, best-of-3 wall, both arms run back-to-back, stdout byte-identical
to the pre-change binary in every row:

| bench | before | after | scriptc 0.0.22 | node 26.5.1 |
|---|--:|--:|--:|--:|
| `cycles` | 0.79 | **0.24** | 0.31 | 0.07 |
| `deeplist` | 1.14 | **0.33** | 0.22 | 0.09 |
| `tree_wide` | 12.18 | **3.02** | 7.01 | 0.89 |
| `tree` | 5.92 | **4.43** | 4.80 | 0.45 |

`cycles`, `tree` and `tree_wide` now beat scriptc. Unchanged, as required:
`push_cls` 0.35, `churn` 0.66, `churn_alloc` 0.36, `push_num` 0.13, `retain`
1.32, `retain1` 0.42, `churn_read` 0.35.

**Three changes, and they are not separable.**

1. *The sloppy-mode class-field route reaches boxed slots*
   (`expr/property_set.rs`). #7288 opened it for raw-f64 slots only — "boxed
   slots need the layout note and write barrier that the guard-call path emits"
   — but those come from `emit_jsvalue_slot_store_pointer_tested`, not from the
   guard, and this arm calls it with the identical value-side predicates the
   strict arm uses. What the guard actually contributes is descriptor-aware
   dispatch and the setter-in-chain walk, and the #5093 inline precheck refuses
   every receiver that needs either. The miss stays
   `js_put_value_set(..., strict = 0)`, so a rejected sloppy write is still a
   silent no-op.

2. *A pointer-bearing class declares its layout at allocation*
   (`typed_shape.rs`). #7510 required an EMPTY pointer mask, which excluded
   exactly these classes — so their descriptor arrived *after* every store in
   their constructor and none could pass its intact-bit guard. That is #7512's
   defect, still open for this shape: `tree_wide`'s eight `number` fields were on
   the by-name fallback because two `Tree | null` siblings existed. Obligation 2
   is now discharged rather than avoided: both `new` allocation paths pre-fill
   every slot with `TAG_UNDEFINED` (`object/alloc.rs` #4717 and codegen's inline
   bump path), which the tracer rejects at its tag check, so a pointer-masked
   slot visited before its first write cannot strand anything. Obligation 1 (no
   read may observe a raw-f64 slot before its first write) is unchanged and still
   demanded of every `number` field.

3. *The constructor prologue admits literals and pure operator trees*
   (`lower_call/field_init.rs`). `constructor(v) { this.next = null; this.v = v }`
   is the canonical linked-structure constructor and its prologue truncated at
   statement 0, coming back EMPTY — costing both the dead-`undefined`-store
   elision and, through the same set, the declaration in (2). `tree_wide`'s
   `this.b = s + 1` needs the operator-tree half. Admitted forms are parameter
   reads, literals, and `Binary`/`Unary`/`Compare`/`Logical` trees over those:
   no member access, no call, no closure, and no `This` anywhere in the tree.

**Why they must land together, measured rather than argued.** (1) alone
*regresses* `tree_wide`. Compiling the benchmarks as ESM — which already takes
the class-field route — against the pre-change compiler gives `tree_wide`
12.21 → **14.88** s: routing a constructor's stores to a guard the construction
path has made unsatisfiable is slower than the inline cache it displaces. (2) is
what makes that guard passable.

**GC.** A pointer store must still reach the remembered set.
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` at depth 800, over binaries
compiled with `PERRY_GC_MOVING_LOOP_POLLS=1`, on pointer-cycle / linked-list /
wide-tree probes: clean, and **not vacuous** — 20005 / 40005 / 5
`[gc-fromspace-protect]` retirements prove the copying minor ran, and all three
match the pinned `node 26.5.1` oracle including a walk that sums every numeric
slot as well as every pointer edge. `PERRY_GC_VERIFY_EVACUATION`,
`PERRY_GC_VERIFY_MARK` and `PERRY_GC_FROMSPACE_SCAN_ABORT` are clean under zeal.
`PERRY_GC_TRACE` drift: `churn` bit-identical (105 cycles, 0.0039 GB copied,
positive reclamation every cycle); `deeplist`/`tree`/`tree_wide` keep their cycle
counts and kinds exactly; `cycles` copies 10,758 → 14 objects and promotes
4,746 → 2, the side-table bookkeeping this change removes no longer holding dead
objects reachable.

**A latent gate bug this exposed.** `scripts/gc_root_dominance_check.py`'s
`NONCOLLECTING` set is a second copy of a fact
`perry-codegen/src/gc_call_effects.rs` already states, and the two had drifted:
#7510 added `js_gc_declare_typed_shape_layout` beside
`js_gc_init_typed_shape_layout` in the Rust match and not in the Python set. It
stayed invisible only because the corpus then held no class the #7510 gate
admitted. Widening that gate printed **358 spurious violations**, every one
`js_object_alloc_class_inline_keys->js_gc_declare_typed_shape_layout`. The two
entry points share a body (`typed_shape_layout_entry` → `init_typed_shape_layout`)
and differ only in a `TypedShapeProof` that makes `declare` do strictly *less*
work, so one classification covers both. With the drift fixed the gate reports 0
violations on both arms, with its 40-seeded-violation control catching 40/40.
