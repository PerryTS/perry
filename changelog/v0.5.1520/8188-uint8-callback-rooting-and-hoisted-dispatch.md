### Fixed — GC rooting in the uint8 Buffer callback dispatcher (#8179)

`dispatch_uint8_buffer_method` — the shared uint8 `%TypedArray%.prototype`
dispatcher that every Buffer-backed `Uint8Array` callback method funnels
through, on all three of its entries — kept the callback closure, the receiver,
`map`'s freshly allocated result buffer, `sort`/`toSorted`'s permuted output and
`reduce`/`reduceRight`'s accumulator in bare Rust locals across
`js_closure_call{2,3,4}`.

The closure is the live half. It is an ordinary nursery allocation
(`GC_TYPE_CLOSURE`, with a `GcMoveHookKind::ClosureDynamicProps` move hook — it
both moves and dies), and a callback handed in by a frameless caller is
reachable only through that raw parameter plus the native stack, which an
evacuating minor does not scan. `array::buffer_receiver_dispatch` rooted it at
the boundary; the `%TypedArray%.prototype` thunk and `dispatch_buffer_method`'s
catch-all did not, so both `Uint8Array.prototype.map.call(u, fn)` and a
statically typed `u.map(fn)` were exposed.

`test-files/test_gap_gc_uint8_buffer_callback_rooting.ts` fails on the **shipped
default** before the fix (`TypeError: value is not a function`, exit 1), and
under `PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=<n>
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1` it dies on the FIRST
scheduled collection, for every seed tried (1, 7, 42):

```
[gc-fromspace-protect] FAULT: signal 10 at 0x2454161058c
  last-known object: user_ptr=0x24541610580 obj_type=4 size=24
[gc-schedule] FAILURE (signal 10) under seed=7
[gc-schedule]   safepoints=1 scheduled_collections=1
```

`obj_type=4` is `GC_TYPE_CLOSURE` and the faulting address is `user_ptr + 12` —
`CLOSURE_TYPE_TAG_OFFSET`, i.e. `get_valid_func_ptr`'s `CLOSURE_MAGIC` probe
reading a retired from-space closure header. After the fix the same seeds run to
completion with the instrument's own liveness verdict showing the subject was
live: `safepoints=5306 scheduled_collections=5306 copying_minors=5306
moved_objects=25760`, zero faults, exit 0. The witness is registered in
`test-parity/gc_repsel_corpus.txt`.

The receiver is the other half, and is treated differently on purpose. A Buffer
is `arena_alloc_gc_old` + `GC_FLAG_TENURED` (`buffer/header.rs`) — the same
old-arena space `typed_array_alloc` calls "non-movable space: raw data pointers
are handed out" — and every `%TypedArray%` sibling already holds its receiver in
a plain local across callbacks on that invariant. It is now rooted for
*liveness* (the raw parameter is otherwise its only reference on two of three
entries) with its address read from the root once per arm rather than once per
element. The one collector arm that relocates an old-arena page is old-page
defrag, which is opt-in and default-off (`PERRY_GC_OLD_DEFRAG=1`); making that
safe is a tree-wide property of every holder of an old-arena raw address, and
re-reading per element cost +28 % on the `Uint8Array` benchmark for a knob that
is off.

Two sibling families get the same treatment: `js_typed_array_reduce` /
`js_typed_array_reduce_right` now root their accumulator (as `js_array_reduce`
has since the 2026-07-02 audit), and the two non-BigInt arms of
`js_typed_array_sort_with_comparator` /
`js_typed_array_to_sorted_with_comparator` now root the comparator closure —
`sorted_bigint_lanes` beside them already did.

### Performance — hoisted per-element closure dispatch (#8180)

`js_closure_callN` re-derived, on every element of every fused array-callback
loop, three answers that cannot change while one closure is being called:
`get_valid_func_ptr` (two address-band checks, a volatile `CLOSURE_MAGIC` probe
through `*(closure + 12)`, a volatile `func_ptr` load), `resolve_strategy` (a
`perry_thread_local!` single-slot cache — on Darwin a `tlv_get_addr` CALL plus a
load and a compare even on a hit), and the `DispatchStrategy` match before the
indirect jump.

New `closure/dispatch/direct.rs` generalises `array/sort.rs`'s `ComparatorCall`
trick — introduced to "skip ~50M HashMap lookups over a 1.25M-element sort", and
until now its only consumer — into `DirectCall{1,2,3,4}`: resolve once, call
directly, fall back to `js_closure_callN` for a bound method/function, a rest
parameter, a declared arity above the call arity, or an invalid closure pointer.
`resolve_call2_direct` is deleted rather than left standing beside it.

Hoisted at 31 call sites: `array/iter_methods.rs` (14), `array/reduce_right.rs`
(1), `typedarray/iterate.rs` (9), `typedarray/transform.rs` (5 — including the
BigInt lane comparator, which resolved once per *comparison*) and the uint8
`%TypedArray%.prototype` dispatcher.

Instructions retired, quiet M1 mini, best of 5, arms interleaved:

| benchmark | main | +#8179 | +#8179+#8180 | vs main |
|---|---|---|---|---|
| 21M plain-`Array` callback invocations | 5,027,207,909 | 5,027,015,176 | 3,970,592,563 | **−21.0 %** |
| 7.9M Buffer-`Uint8Array` callback invocations | 1,979,043,224 | 2,535,814,615 | 1,978,697,176 | **−0.02 %** |

Peak RSS is flat: 46,628,864 B on both arms of the first benchmark;
14,794,752 → 14,876,672 B (+0.55 %, 20 pages) on the second.

`array/generic.rs`'s `js_arraylike_*` engine is deliberately not converted: its
per-element cost is dominated by generic array-like property access (`al_has` +
`al_get`), not dispatch, and the spec order it implements reads
`LengthOfArrayLike` before `IsCallable`, so a hoisted resolve would have to be
sequenced after the existing `callable()` call rather than inserted at the top.
