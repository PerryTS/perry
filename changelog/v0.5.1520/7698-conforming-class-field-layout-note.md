### class fields: a conforming pointer store stops calling into the layout machinery (#5094)

`this.left = left`, `a.peer = b`, `node.next = n` — a pointer written into a
slot the class's **own compile-time pointer mask already declares a pointer** —
called `js_gc_note_slot_layout` on every store, to re-derive a fact codegen knew
when it emitted the mask. Counted with an instrumented runtime, `tree.ts` makes
**20,447,156** such calls and **20,447,154** of them return `Conforms`: a
cross-crate call, a header decode, a TLS touch and a `SHAPE_LAYOUTS` hashmap
lookup, per store, to change nothing.

`class_field_store_needs_layout_note` already elided the note for a value that is
a non-pointer *by construction*. This is the complementary case, and its doc
comment named both the obstacle and the unblocking condition:

> **Deliberately NOT elided: a pointer-valued store into a pointer-masked slot.**
> […] Closing that exit (#6921) is the prerequisite for the stronger elision.

#6921 is closed — `lower_call/new.rs` emits the layout init on the last `new`
exit that used to return an undeclared instance — so the elision is available.
It is taken as a **live header test with the real note kept on the cold arm**,
not as an outright removal:

```llvm
%res  = load i16, ptr (obj - 6)          ; GcHeader::_reserved
%m    = and i16 %res, -12288             ; STATE_MASK | TYPED_LAYOUT_INTACT
%ok   = icmp eq i16 %m, -28672           ; SIDE_MASK  | TYPED_LAYOUT_INTACT
br i1 %ok, label %done, label %layout_note
```

Two facts make the taken arm a proof rather than a guess. The #5093 inline
precheck has already established, on this path, that the receiver's `keys_array`
is **this class's** keys global and its `field_count` exceeds the slot index — so
whatever descriptor is reachable for it was installed from these very mask
globals, shared by shape or per-object. And the slot is in that mask's
`pointer_mask` and (checked, not assumed) absent from its `raw_f64_mask`, so
neither of `layout_note_slot`'s two downgrade arms can fire. Anything else — a
cleared intact bit, `POINTER_FREE`, `UNKNOWN`, a receiver from a path this
reasoning did not enumerate — falls to the unchanged call.

**Measured** — quiet M1 mini, best-of-3 wall, arms interleaved back-to-back,
stdout byte-identical in every row, `PERRY_NO_AUTO_OPTIMIZE=1`:

| bench | before | after |
|---|--:|--:|
| `cycles` | 0.33 | **0.29** |
| `tree_wide` | 7.90 | **7.77** |
| `tree` | 5.17 | **5.13** |

Unchanged, as required: `churn` 1.21, `push_cls` 0.89, `churn_alloc` 0.89,
`retain` 2.41, `retain1` 0.04, `retain_wide` 3.38, `retain_wide1` 0.06,
`deeplist` 0.03, `cls_mistyped` 0.02. Peak RSS unchanged.

The `sample` profile is the clearer statement of what happened: on `cycles_big`,
`layout_note_slot` is the **second-heaviest leaf frame in the base arm (155
samples)** and is **absent from the fix arm's profile entirely**. The wall-clock
share is smaller than that because these workloads are GC-dominated on the
current default pacing (arena walk, barriers, sweep).

**Demotion is not weakened, and that is tested rather than asserted.** The intact
bit is not made sticky and no downgrade path is touched: `cls_mistyped.ts` — a
`number`-declared field constructed with a heap string per instance, which must
demote or the collector never traces those strings — still prints
`20000 string payload-19999`. `PERRY_GC_VERIFY_MARK=1 PERRY_GC_VERIFY_EVACUATION=1`
is clean over the bench set, and `PERRY_GC_TRACE` cycle counts and copied bytes
are identical between arms (`churn` 13, `tree` 20).

The three new tests are unit tests (`cargo-test`-visible, per #5960), and each
was checked to be able to fail. The positive one asserts the elision is
**reached** — a predicate that silently answered `false` everywhere would
otherwise be invisible. The negative asserts a slot in *neither* mask
(`flag: boolean`) keeps its unconditional note: there the note is not a no-op to
skip but the only thing that ever sets the pointer bit the collector reads, and
sabotaging the predicate to return `true` makes that test fail.
