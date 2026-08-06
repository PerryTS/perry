### Fixed

**`[...obj.arr]`'s inlined lowering dereferenced retired from-space in the
`[Symbol.iterator]` prototype walk (#7498).** The spread has two lowerings. Out
of line it calls `js_iterator_to_array` — the drain rooted by #7495. Inlined, it
routes through `array_from_spread_value`, which resolves `[Symbol.iterator]`
through the whole property-lookup tower first, and three frames on that walk held
a GC-managed value somewhere the collector cannot see. Latent for every user on
both link modes: evacuation copies rather than zeroes, so the stale read returned
the correct old bytes and the program printed the right answer — until allocation
timing recycled those bytes, at which point it becomes the family's usual
`TypeError: value is not a function` somewhere else entirely.

* **`symbol::get::req_handle_symbol_fallback`** read the receiver into a bare
  `usize`, interned a `"_req"` key (an allocation), then read a field off the
  pre-move address. `js_object_get_field_by_name` followed that copy's stale
  `keys_array` into a retired 40-byte `GC_TYPE_ARRAY`. This helper runs on every
  heap-object symbol read whose own-symbol lookup missed, so the window is
  unconditional — the reproducer faults 5/5, not intermittently.
* **A `&str` / `&[u8]` borrowed out of the key's `StringHeader`.**
  `get_field_by_name_object_tail` slices the property name straight out of the
  key payload and hands it to `array_prototype_property_value`, which allocates
  three times before reading it — including inside `js_string_from_bytes`, which
  reads its *source* bytes after its own `string_storage_alloc`. **No
  `RuntimeHandleScope` can fix that shape**: rooting the key rewrites the slot,
  not a `&str` that already names the pre-move address, and a borrow is not a
  slot. The new `HeapKeyBytes` copies the bytes off the heap once, before the
  arm's first allocation — a 64-byte stack buffer in the common case, spilling
  only for a longer key so the guarantee is total rather than typical.
* **`array_from_spread_value`'s receiver**, carried through a dozen
  classification probes and the entire symbol walk and then used to rebind
  `this` for the `[Symbol.iterator]()` factory. Rooted before the function's
  first allocation, with the argument shadowed by a reader so the
  pre-collection address is not nameable afterwards.

Also rooted, same shape and same measured path:
`default_object_prototype_property_value`'s key and receiver (plus the displaced
`this` and accessor-receiver override its own doc comment flagged as a residual),
and the `fetch_subclass_handle_id` / `temporal_subclass_cell` subclass-marker
probes, each of which allocates a key string between reading its receiver and
using it. Every handle is NaN-boxed, so `scripts/raw_handle_debt.py` stays at
999.

**Witness:** `test-files/test_gap_gc_spread_symbol_iterator_rooting.ts`,
registered in `test-parity/gc_repsel_corpus.txt`. It is
`test_gap_gc_iterator_drain_rooting`'s shrunk twin — small enough that `clone`
inlines, so the spread takes the other lowering, and its size is load-bearing.
Measured on macOS/arm64: `PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=200` FAULTs 5/5 before and is clean 5/5 after,
on **both** the default (auto-optimize) and `PERRY_NO_AUTO_OPTIMIZE=1` links,
with the auto-optimize rows A/B'd against a runtime archive rebuilt from source
on each side. The clean verdict is non-vacuous: the same run reports four
`[gc-fromspace-protect] retired_set=#N` page-sets and `[gc-copy-minor] ran
copied_objects=11794` on the first minor.

**The protected run is not clean everywhere, and the remainder is filed rather
than absorbed.** `test_gap_gc_iterator_drain_rooting` — the out-of-line sibling
— still faults, in `js_native_call_method`, which reads its *rooted* receiver
into a local reused across a dozen allocating probes before `is_closure_ptr`
dereferences it. Patching the one faulting line was tried and reverted: the
fault moved 800 bytes further into the same function, which is the signal that
the whole receiver needs re-reading rather than one arm of it. That file's
corpus note is updated to say so — its own text asked for exactly this check.
