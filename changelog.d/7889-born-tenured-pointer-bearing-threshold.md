### Performance

- **`shapes` 0.139 → 0.043 s (3.2×, now 1.9× FASTER than node) and −55.6% peak RSS: a
  2000-element array was being born immortal.** `arena_alloc_gc` births anything over
  `LARGE_OBJECT_THRESHOLD_BYTES` (16 KB) in the old generation *and* stamps
  `GC_FLAG_TENURED` — and a minor collection never sweeps old-gen. A `Node2D[]` of 2000
  elements has a 16 400-byte backing store: **sixteen bytes over the line**. So every
  round's array was permanently live, the write barrier had recorded an old→young edge for
  each of its 2 000 stores, and every subsequent minor's remembered-set scan marked all of
  them live again. `shapes` re-marked **94 000 then 118 006** slots that way; its
  young-survival ratio read 739‰ and 925‰ while its actual live set was ~3 200 objects, and
  its two minor collections cost 94 ms of a 139 ms program.

  The threshold is now **type-dependent**. Crossing it trades *copy cost* — one `memcpy`,
  bounded by the object's own size — against *retention cost*. For a `pointer_free` object
  those are the same quantity, so 16 KB stays. For a pointer-bearing one the retention is
  transitive and unbounded, so arrays, objects and closures get
  `LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES` = **128 KB** (V8's
  `kMaxRegularHeapObjectSize`, which draws this line for this reason). The selection reads
  the existing `GcTypeInfo::pointer_free` flag rather than a hardcoded type list, and an
  unknown type keeps the conservative value.

  Measured on the quiet mini, best-of-5, exit-checked; `shapes` goes from 2 minor
  collections handling 351 501 objects to **1 handling 7 416**, and peak RSS from 72.6 MB
  to 32.3 MB. Every other program in the 22-program corpus moves within noise and its peak
  RSS within ±0.3%, `phase_flip` (the untraced-promotion RSS bound probe) included.

- **`pipeline` 0.244 → 0.166 s: `this.vals[i] = v` had no inline arm at all.**
  `lower_index_set_fast` gives `a[i] = v` a guarded diamond only when `a` is a stack local,
  because it needs a slot to write a realloc'd head back to. Every other receiver shape —
  `this.vals[i]`, `obj.arr[i]`, a closure-captured array — fell straight through to a
  five-argument `js_typed_feedback_array_set_f64_extend` call, while the matching *read*
  has had a complete inline diamond all along. A **strictly** in-bounds store changes no
  head and no length, so it needs no writeback and can be inlined for exactly those
  receivers; growth, sparse extension and every exotic array still take the helper.

### Fixed

- A pointer-bearing object between 16 KB and 128 KB is now reclaimed by a minor collection
  instead of surviving until a full mark-sweep. Programs that build and drop
  medium-sized arrays of records in a loop — the `for (…) { const rows = build(n); … }`
  shape — were retaining every record they ever allocated.
