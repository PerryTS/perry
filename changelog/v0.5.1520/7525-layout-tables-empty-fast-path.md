**The per-object GC layout side tables are now skipped while they are provably
empty — and a shape's keys array no longer keeps them permanently non-empty
(#7510, construction/death half of #5094).**

`LAYOUT_SLOT_MASKS` and `TYPED_LAYOUTS` are address-keyed thread-local maps.
Since #6893 moved the canonical typed layout of a well-behaved object into the
shape-keyed `SHAPE_LAYOUTS`, what is left in them is the residue: objects that
*diverged* from their shape, objects with no `keys_array`, ambiguous shapes. On
a monomorphic workload that residue is empty for the whole run.

Empty was not free. Every allocation (`layout_init_pointer_free`), every
typed-shape install, every object death (`layout_clear_for_ptr`) and every
relocation (`layout_transfer`) probed both maps to clear whatever a previous
tenant of a recycled address might have left — two `RefCell` round-trips plus
two hashes, per object, to remove nothing.

`PER_OBJECT_LAYOUTS_NONEMPTY` now answers "is there anything in either map at
all" in one load. Its `false` state is a *proof* of emptiness: only an insert
can break it, and every insert routes through the guarded accessors in the new
`gc/layout_tables` module, which arm it; the removal paths re-test and clear it
again once the maps drain. A stale `true` costs exactly the pre-#7510 probe, so
the flag is an accelerator and never an authority — no caller may treat it as
one, and `assert_flag_sound` in the new tests asserts the implication after
every transition that can populate or drain the maps.

**The flag was worth nothing until the second half of this change.** Measured
first: it fired **once in 40 million calls** on `churn_alloc`. One entry was
holding both maps hostage — the canonical `keys_array` of the program's single
object shape. `js_build_class_keys_array` fills it with interned key strings and
notes each element, which grows a per-array pointer mask; the shape cache then
anchors that array for the program's lifetime (#179), so the mask never drains.
Since ~every program builds at least one shape, the emptiness fast path was
dead on arrival for essentially all of them.

The mask is now replaced, once the last key is stored, by the
`GC_LAYOUT_ALL_POINTERS` header declaration — which is exactly true of a keys
array (every slot in `0..length` holds an interned string) and immutable for the
rest of the program (growing a shape builds a *new* array, `shape_keys_grown`).
The per-element notes during the fill stay: they are what keeps the
already-stored prefix traceable if allocating the *next* key string triggers a
GC, and the declaration can only be made once the last slot is filled. With it,
`churn_alloc` runs with both maps at zero entries and the fast path on ~100% of
calls.

`layout_note_slot` also stops cloning the descriptor out of the map on every
store. The clone existed only so `layout_set_typed_unknown` could not re-enter a
live `RefCell` borrow; it now computes a two-state `SlotVerdict` inside the
borrow and acts after it, so a `Heap` mask no longer allocates a `Vec` per
write.

Measured on `gc-handoff/bench/churn_alloc.ts` (20M `{v, w}` literals): the
`gc::layout` family falls from **26.0% to 20.8%** of the symbolicated leaf
profile, with `layout_forget_object` 3.0% → 1.6% and
`js_gc_init_typed_shape_layout` 13.6% → 9.5%.

End to end, interleaved A/B (arms alternating per round, best-of-9 user CPU,
corroborated on the pinned bench host):

| bench | speedup |
|---|--:|
| `push_num` | 1.10× |
| `churn_alloc` | 1.03× |
| `churn`, `push_cls`, `deeplist`, `churn_read` | 1.00× |
| `tree` | 0.98–1.00× |

`tree` is the one arm that can only lose: it genuinely holds per-object records,
so its flag stays armed and it pays the fast-path test without getting the fast
path. The removal paths were rewritten to key their re-test on
`remove(…).is_some()` so an armed workload runs the pre-#7510 instruction
sequence plus a load, which took `tree` from −2.2% back to within noise on the
pinned host; the residual is at the edge of what either host can resolve.

Collector behaviour is field-identical across the two arms — `churn` 105 cycles
/ 0.0036 GB copied, `tree` 43 cycles / 0.0159 GB, promoted bytes byte-for-byte
equal, peak RSS within ±2 MB.

**This does not meet #7510's acceptance bar** (≥1.5× on `churn_alloc`,
`gc::layout` below 8%) and the ticket stays open. The reason is worth recording:
the profile the ticket was filed from is out of date. `layout_forget_object` is
no longer 14.5% of `churn_alloc` — after #7469/#7474/#7486/#7487/#7501 it is
3.0%, so removing it *entirely* could never have delivered 1.5×. What the
profile now shows as the remaining layout cost is not the side tables at all: it
is `js_gc_init_typed_shape_layout` + `shape_install_shared` (~13% combined),
which still rebuild both masks, re-probe `SHAPE_LAYOUTS` and re-compare the
descriptor on *every* construction of an already-installed shape. That is
#7510's item 1 ("construction should become a header bit-set") and is the next
lever; the emptiness fast path this PR establishes is a prerequisite for it, not
a substitute.

New tests in `perry-runtime/src/gc/tests/layout_trace/per_object_tables.rs`:
the flag invariant across install / partial drain / typed downgrade / in-place
mask growth, and a witness that a shape's keys array declares all-pointer slots
instead of a mask *and* still enumerates and traces all three key strings — so a
green test means the declaration is as precise as the mask it replaced, not that
the entry merely disappeared.
