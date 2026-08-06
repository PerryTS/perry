### Performance

**`[...arr]` on an ordinary dense array is now an element copy, not a full
iterator protocol (#7533).** `object_deep_clone` was the worst row in the public
benchmark artifact by a wide margin — 657 ms against bun's 17.5 ms, **37.5×**,
where the next worst row is 11.4×. A symbolicated profile on the pinned quiet
host says the whole gap is one line of the kernel:

```ts
tags: [...o.meta.tags],   // a 3-element string array
```

**90.45% of the entire process is inside `array_from_spread_value`.** The same
copy written as `Array.from(o.meta.tags)` costs 0.09 s where the spread costs
5.93 s — **66×** for an identical result — because `Array.from` reaches
`js_array_clone`'s memcpy tail and the spread does not.

Decomposition at N=500 000 (best-of-3 wall, quiet M1 mini; `dc_*` probes isolate
one construct each):

| probe | best | isolates |
|---|--:|---|
| `dc_full` (the kernel) | 6.20 s | everything |
| **`dc_spread`** | **5.93 s** | **just `[...o.meta.tags]`** |
| `dc_map` | 0.32 s | `.map(x => ({…}))` |
| `dc_lit` | 0.15 s | the two object literals |
| `dc_slice` | 0.12 s | `tags.slice()` |
| `dc_arrfrom` | 0.09 s | `Array.from(tags)` |
| `dc_read` | 0.05 s | property reads only (loop floor) |

Inside the spread (4665 leaf samples, `PERRY_DEBUG_SYMBOLS=1`):

| stage | inclusive |
|---|--:|
| `array_from_spread_value` | **95.80%** |
| ├ `js_object_get_symbol_property` — resolving `@@iterator` | 30.89% |
| ├ `js_iterator_to_array` — the `.next()` drain | 34.34% |
| │  ├ `js_native_call_method` → `dispatch_array_iterator_method` | 16.38% |
| │  ├ `js_object_get_field_by_name` (`.value` / `.done`) | 6.95% |
| │  └ `build_iter_result` | 4.14% |
| └ classification probes + `RuntimeHandleScope` | ~13% + ~5% |
| cross-cutting `_tlv_get_addr` | ~17% |

Two things are structurally wrong there, and neither is observable for an
ordinary array. `@@iterator` resolves through the **by-name** prototype tower —
`js_object_get_field_by_name_f64` → `get_field_by_name_object_tail` →
`array_prototype_property_value` → recursion → `default_object_prototype_
property_value` → `fetch_subclass_handle_id` — and then builds a *named bound
closure* for the method it found. And every `.next()` allocates **five** heap
objects in `build_iter_result`: the `{ value, done }` object, its two key
strings, its keys array, plus the iterator object itself once per spread. A
3-element `[...tags]` therefore costs ~25 allocations where bun does one
allocation and a 24-byte `memcpy`.

`array_from_spread_value` now takes a dense fast path first.
`dense_spread_source` proves the array is ordinary — real `GC_TYPE_ARRAY` via
`addr_class::try_read_gc_header` (which rejects the handle band and the
header-less small-buffer slab *without* dereferencing), not exotic per
`array_iteration_is_exotic`, `Array.prototype[Symbol.iterator]` unmodified, and
no own `[Symbol.iterator]` shadowing it — and `dense_spread_copy` then copies the
elements, normalising `TAG_HOLE` to `undefined` (the one place a raw copy and the
drain disagree: `[...[1, , 3]]` must stay `[1, undefined, 3]`). Anything it
cannot prove falls through to the unchanged protocol.

The own-symbol probe is a new **non-invoking** `symbol::has_own_symbol_property`
rather than `own_symbol_property`: the latter answers by *reading*, which calls a
user getter, and the slow path it falls back to reads the property again — a
getter must observe exactly one call.

**Result on the pinned quiet mini** (same host, same method, best-of-7 wall):

| | before | after | node v26.5.1 |
|---|--:|--:|--:|
| `object_deep_clone` (the kernel, N=50 000) | 610 ms | **40 ms** | 60 ms |
| `dc_spread` alone (N=500 000) | 5.92 s | **0.12 s** | — |
| `dc_full` (N=500 000) | 6.20 s | **0.36 s** | — |

**15.3× on the kernel; 49× on the spread itself.** Against the artifact's bun
figure the row moves from **37.5× to ~2.3×**, and Perry goes from 11.5× node to
**0.67× node — a win**. The 657 ms → 40 ms row should stop being the worst cell
in the artifact by a wide margin; it is now among the better ones.

Verified byte-identical to the node oracle under **both** link modes (auto-optimize
and `PERRY_NO_AUTO_OPTIMIZE=1`), `scripts/auto_opt_app_patterns.sh` 12/12, and a
32-case `[...arr]` semantics matrix byte-identical to pre-change Perry. Under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` on a `PERRY_GC_MOVING_LOOP_POLLS=1` build:
**50 005 retired page sets quarantined, zero faults**, correct checksum — the
instrument proven live by its `[gc-fromspace-protect] mode=… retired_set=#N`
lines rather than assumed.

### Fixed

**`js_array_map_discard` never rooted its callback (#6081's missed sibling).**
`js_array_map` roots it — a callback allocated by a frameless caller (the arrow
in `xs.map(x => …)`) is reachable only through the raw parameter and the native
stack, which an evacuating minor does not scan, so from the second element on the
dispatch reads a moved-or-swept closure. The discard variant kept using the bare
parameter across `js_closure_call3`, which allocates.

It stayed latent because a stale root only bites when a collection lands inside
its window, and nothing put one there. Removing ~25 allocations per loop
iteration moved every subsequent collection in this kernel and dropped one
squarely inside that loop: the kernel started failing `TypeError: value is not a
function`, and `PERRY_GC_PROTECT_FROMSPACE=1` named the site precisely — a
retired `obj_type=4` (GC_TYPE_CLOSURE) at `js_array_map_discard + 788`. **The
kernel faults under the same instrument at the previous commit too**, at a
different site inside `array_from_spread_value`, which is how the defect was
established as pre-existing rather than introduced. Rooted NaN-boxed, so the
read-back is a `get_nanbox_f64` and `scripts/raw_handle_debt.py` stays at 999.

**The dense fast path itself shipped one bug during development, worth recording
because the shape recurs**: it read `length` straight off the address in the
NaN-box, without `clean_arr_ptr`. `js_array_grow` (issue #233) leaves a
`GC_FLAG_FORWARDED` header at the OLD address whose first eight bytes — where
`length`/`capacity` used to live — now hold the forwarding pointer, so the memcpy
was sized from a forwarding pointer reinterpreted as a length. `[...sparse]` after
`sparse.length = 5` and `[...beyond]` after `beyond[9] = 9` both took
EXC_BAD_ACCESS in `_platform_memmove`. Band/slab validation still runs before any
deref; the chain is followed only after that, and the `GC_TYPE_ARRAY` check is
re-read off the post-forwarding header.

`crates/perry-runtime/src/array/spread_dense_tests.rs` pins `dense_spread_source`'s
**verdict** in every case, not only the resulting elements: the slow path is a
correct fallback, so a test comparing elements alone would stay green if the fast
path silently stopped applying — CLAUDE.md's fourth way a gate can be unable to
fail. The forwarding case likewise asserts the probe really forced a
grow-and-forward before testing anything.

### Notes

Two `[...arr]` divergences from node were found while building the semantics
matrix for this change. **Both predate this work and are unchanged by it**
(verified byte-identical at `f06270d06`, before the #7495/#7516/#7527 rooting
stack): `[...MyArr.from([1,2,3])]` on a `class MyArr extends Array` throws
`value is not iterable`, and a replaced `Array.prototype[Symbol.iterator]` is
ignored by spread (`[...[1,2,3]]` yields `[1,2,3]` where node yields the patched
iterator's output). Filed separately rather than folded in here.
