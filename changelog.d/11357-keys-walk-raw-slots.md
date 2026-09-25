perf(runtime): three runtime keys-array walks stop calling the JS-facing
element accessor per key (#10724).

#10724 profiled `js_array_get_f64` as the top symbol of a natively compiled
`tsc --noEmit` and suspected codegen: that compiled `arr[i]` sites were taking
the full dispatch ladder instead of the short `_unchecked` read. Callgrind
attribution by caller says otherwise. On `typescript@5.9.3`'s `lib/_tsc.js`
type-checking 20 real files, **96% of the 33.7 M `js_array_get_f64` calls came
from three runtime-internal walks over an object's keys array**, and the codegen
miss path (`js_packed_arraylike_index_get`) was 210 K calls, about 0.6%:

| caller | calls | share of `js_array_get_f64` inclusive cost |
|---|---:|---:|
| `native_call_method::class_vtable_fast_guard` | 26.4 M | 75% |
| `object::transition_edge_places_key` | 3.6 M | 10% |
| `class_registry::class_object_own_field_bytes` | 2.8 M | 8% |
| `js_packed_arraylike_index_get` (codegen miss) | 0.2 M | 2% |

`class_vtable_fast_guard` runs on every dynamic method call on a class
instance, and its own-key shadowing scan read each key through
`js_array_get`. That accessor exists for `arr[i]` on a receiver that is *not*
proven to be a plain array. It runs a lazy-array tag strip, a GC-header
read, the Map/Set/typed-array/subclass arms, `clean_arr_ptr`, the descriptor
gate and the hole-to-prototype fallback on every call, about 137 retired
instructions per read here. A keys array is internal, dense and string-only,
so none of that applies.

All three walks now read the raw dense slots:

- `class_vtable_fast_guard` uses `keys_array_dense_slots_resolved`, since its
  `keys` come straight from the live `ShapeDescriptor` with no allocation in
  between. That is the same contract `native_get` and `ic_miss` already rely on.
  The byte comparison (`js_string_key_matches_bytes`) is unchanged.
- `transition_edge_places_key` gets its keys from a pointer-keyed cache, not
  a live descriptor, so it uses `keys_array_dense_slots`, which still resolves
  forwarding and validates, and reads one slot.
- `class_object_own_field_bytes` uses `keys_array_dense_slots` (the keys can
  be a dictionary-mode list) and compares in place with
  `js_string_key_matches_bytes`. The old `js_get_string_pointer_unified`
  heap-materialized every short-string key it passed, which allocated while
  `keys` was held as a bare pointer, and read a symbol slot as if it were a
  `StringHeader`.

Measured with callgrind on the same release-built binary pair (the 20-file
`tsc --noEmit` above, output byte-identical to Node's):

| | before | after |
|---|---:|---:|
| total retired instructions | 246.17 G | 242.47 G (−1.50%) |
| `js_array_get_f64` self | 4.66 G (1.89%) | 0.24 G (0.10%) |
| `js_string_key_matches_bytes` self | 593,333,960 | 593,333,960 |

The unchanged `js_string_key_matches_bytes` count is a check on the guard: its
scan performs exactly the same comparisons as before, just without the
accessor around each read.

This change does not touch compiled `arr[i]` sites. The codegen lead in
#10724 was not where the calls came from on this workload.

Tests: `object/native_call_method/vtable_guard_scan_tests.rs` and
`object/keys_walk_accessor_tests.rs` count `js_array_get_f64` entries rather
than timing them. They assert zero accessor calls across each walk, plus
unchanged answers (shadowing still detected at every slot, below and above the
32-key index threshold, and the right slot/value returned). All four go red
against the old loops, with 6 to 177 extra accessor calls each. The full
`perry-runtime` suite passes (4528 passed, run single-threaded).
