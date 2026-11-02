### GC: stop minting per-object pointer masks for single-slot payloads

`interp.ts` — the tree-walking interpreter that best resembles real software in
the benchmark corpus — spent **~19% of its runtime in `layout_forget_object`**,
plus another ~6% in the hashbrown probe underneath it. That is side-table
bookkeeping, not user work, and by design it should have been ~zero: #7510's
`PER_OBJECT_LAYOUTS_NONEMPTY` flag exists so that the allocation, store, death
and relocation paths can skip both per-object layout maps whenever they are
empty, which "on a monomorphic workload they are".

They were not. Instrumented on `iso_FIB.ts` (the isolated FIB half):

```
forget_total=15,000,000  fast=52  slow=14,999,948
residency: masks=313,875 -> 381,505 -> 400,430 (still climbing)
```

The disarmed fast path fired **52 times in 15 million calls**. Every other call
took two `RefCell` round-trips and two hashes against a 400k-entry, cache-cold
map — once per allocation, program-wide.

**Cause.** `layout_note_slot`'s "first pointer stored into a `POINTER_FREE`
object" arm minted a per-object entry in `LAYOUT_SLOT_MASKS`. The interpreter
allocates `{ names: [p], vals: [a], parent }` per interpreted call, so it minted
two masks per call — **1.8M of them**, each a mask over a payload of exactly
**one slot**. A mask over one slot cannot skip anything: the tracer consults
`layout_pointer_bearing_bits` on that slot either way, so the entry was the
mask's entire contribution. The entries also outlive their arrays — they are
only reclaimed when the recycled address is allocated over — so residency grew
without bound, and a single live entry anywhere keeps the flag armed for every
allocation in the program. This is #7510's "one immortal entry nullifies
`is_empty()`" a second time, from the other direction.

**Fix.** Both mint sites (`layout_note_slot` and
`layout_rebuild_from_slots_with_policy`) now decline the mask when the payload
is below `DEFAULT_MASK_MIN_SLOTS` (2, i.e. single-slot payloads only) and use
`GC_LAYOUT_UNKNOWN` — the tag-checked scan-all-slots state — instead. That state
is already the established fallback on this exact path, and the tag check is
exact here: neither site is reachable for an object with an intact typed
descriptor, so there are no raw-f64 slots whose bits could be misread as a
pointer. `PERRY_LAYOUT_MASK_MIN_SLOTS` overrides the threshold for bisection.

Two details worth keeping:

- An array reports its `length`, but **only for a store into an already-formed
  array**. Every append protocol notes the slot *before* bumping `length`, so
  mid-construction `length` is the pre-append value; judging on it stranded
  every incrementally built array — a `push` loop, a JSON parse — in the scan
  state regardless of final size. Capacity is not a substitute either:
  `MIN_ARRAY_CAPACITY` is 16, so a one-element literal reports 16 and the
  distinction disappears entirely.
- An object reports a bound derived from `GcHeader::size`, not `field_count`,
  because `size` is maintained for every GC allocation whatever its
  type-specific header holds.

Both directions of error are *correct*, only differently priced: over-estimating
mints a mask that was not needed (the old behaviour), and under-estimating
routes the object to a scan that visits a superset of what the mask would have
selected. Neither can hide a live child.

**Measured** (quiet M1 mini, best-of-5, interleaved against the same binaries
with the policy disabled, outputs byte-identical to node and exit codes checked):

| bench | before | after |
|---|--:|--:|
| `interp` | 1.894 | **1.697** |
| `iso_miss` | 2.371 | **2.157** |
| `bench/mask_tax` (new probe) | 0.1218 | **0.1049** |
| `bench/mask_tax_nopointer` (control) | 0.0929 | 0.0929 |

No regression anywhere on the 19-benchmark corpus, including the GC-heavy
`tree`, `tree_wide`, `retain*`, `cycles` and `deeplist`. The correctness canary
(`iso_miss` printing `checksum 437840 misses 0`) holds plain and under
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`,
`PERRY_GC_VERIFY_EVACUATION=1` and `PERRY_GC_FORCE_EVACUATE=1`.

**New probe.** `gc-handoff/bench/mask_tax.ts` reduces the interpreter's
environment chain to the shape that mints the masks, with
`mask_tax_nopointer.ts` as a numeric-element control that holds flat at 1.000.
The arrays have to genuinely escape: a first version kept them in a local,
codegen scalar-replaced the array away, and the probe measured a 1.000 ratio
while the bug was fully intact.

**Left on the table, deliberately.** Raising the threshold to 9 or above pays
roughly twice as much (`interp` 1.619, `iso_miss` 2.046) with still no
regression on the corpus, but 21 tests in this crate encode "a small mixed
payload uses a mask" as a precondition (5 do at 2, 11 at 3, saturating at 21
from 9). That is a contract change worth making deliberately rather than as a
side effect of a perf patch.
