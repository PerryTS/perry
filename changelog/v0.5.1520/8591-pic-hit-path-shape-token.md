### Performance — generic property reads

Cut six AArch64 instructions from the hit path of **every** generic
`obj.property` read, worth **−7.05% instructions on `interp`** (8.167 G →
7.591 G) and **−4.88% on `iso_miss`** (12.006 G → 11.420 G) — the two
tree-walking-interpreter benchmarks tracked in #8591. Against the pinned Node
26.5.1 oracle that moves `interp` from 1.34x to 1.25x and `iso_miss` from 1.87x
to 1.78x. Measured as instructions retired (`/usr/bin/time -l`, best of three,
same host, both arms built from the same target directory). The rest of the
CPU-bound corpus — `shapes`, `tree`, `churn`, `fib40`, `push_cls` — moved by at
most 0.05%, peak RSS is unchanged (32.8 MB / 33.0 MB), and every program still
prints byte-identical output to `node --experimental-strip-types`.

Two independent removals, both in `perry-codegen`'s PIC dispatch
(`expr/property_get/generic_dispatch.rs`); no runtime change, no layout change.

**The ShapeId range test was redundant against the cache compare.** The emitted
token used to be `is_stamp ? (pcid | 1<<62) : 0` followed by a separate
"token is non-zero" compare — a `select`, two constant materialisations and two
extra compares that the backend spent six instructions on, per read. The range
test is *implied*: `pic_prime_get` is the only writer of the cached token
(`js_put_value_set_ic_miss` writes a different, set-side cache), and it is only
ever handed `object_shape_stamp(obj) | PIC_ID_TOKEN_BIT`, which is zero outside
`SHAPE_ID_BASE..SHAPE_ID_END`. So an equal token already proves the receiver
carries that ShapeId. The one case the range test caught that equality does not
is a cached `0 | 1<<62`, primed by an unstamped receiver, aliasing a receiver
whose `parent_class_id` is 0 — a single `pcid != 0` compare replaces it, and
keeps the #809 behaviour (a keyless `Object.create(proto)` receiver still falls
through to the prototype-chain walk) exactly as it was.

**The property key was materialised on the fast path but only used on cold
ones.** Every consumer of the pooled `StringHeader*` — the IC-miss handler,
both `js_object_get_field_by_name_f64` arms, the class-ref helper — sits on a
cold edge of the dispatch diamond, yet the load was emitted once up front, so
every hit paid a dependent load of a global it never read. It is now emitted
per consumer. Because the pool entry is a mutable global that GC evacuation
rewrites, re-reading it at each use is also the more correct reading.

Measured and rejected along the way: folding each `(token, slot)` cache pair
into one `i128` load, to make the backend emit `ldp` and drop the duplicated
address materialisation. It does shrink `evalNode` by 1.3% of its instructions,
but the backend simply trades `adrp`+`ldr` for `adrp`+`add`+`ldr` — the
benchmarks moved by less than run-to-run noise, so the complexity was not kept.
