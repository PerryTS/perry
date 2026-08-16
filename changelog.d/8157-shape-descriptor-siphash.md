### Performance

**The ShapeId descriptor table no longer pays SipHash on every probe, and the
id a shape resolves to no longer depends on hash iteration order.**

`ShapeTableInner::descriptors` — the map `shape_descriptor_by_id` reads — was a
`std::collections::HashMap<u32, _>` with the default `RandomState`, i.e.
SipHash-1-3 on a bare `u32`. It is the hottest lookup in the object model:
`object_is_regular` runs it once per array element-shape test (3,000,000 times
on the `retain` bench, 20,000,002 on `churn`) and `object_live_slot_count` runs
it on every indexed field get/set. The sibling field on the same struct,
`indices`, already used `crate::fast_hash::PtrHashMap`; `descriptors` and
`ids_by_keys` were simply never converted, and `fast_hash`'s own module doc
already records the identical finding ("`hash_one` was 14% leaf samples") for
the pointer-keyed registries.

`PtrHasherImpl` gains a `write_u32` fast path — without it a `u32` key falls
into the generic byte-stream fallback, because `Hasher`'s default `write_u32`
forwards to `write(&n.to_ne_bytes())`: four rotate/xor rounds for a key that
needs one multiply. `ids_by_facts` is deliberately NOT converted, since
`PtrHasher`'s `write_*` methods overwrite the accumulator rather than folding
it — right for a one-word key, wrong for that five-field one.

`rebuild_descriptor_reverse_indices` now sorts each id vector. The rebuild
walks `descriptors` in HASH order and `shape_descriptor_ensure_with_generation`
reuses `ids.first()`, so which ShapeId a facts key resolved to after a GC
rewrite depended on the hasher: two objects with identical facts, one born
before a collection and one after, carried different ids, and every id-keyed
consumer (the typed shape-layout install, the emitted PICs) split its
population. Latent before, exposed by the hasher swap — which alone measured
`interp` +3.4% with `gc::layout::init_typed_shape_layout` newly doubled in the
profile, and `interp` −5.9% once the order was pinned.

Measured on the 19-program corpus, `/usr/bin/time -l`, best-of-3, per-arm
`PERRY_RUNTIME_DIR` and object cache, archives `cmp`-verified to differ, all 19
stdouts byte-compared and exit-checked:

```
churn -25.2%   deeplist -17.2%   retain_wide -15.2%   retain1 -15.1%
cycles -14.9%  retain -14.7%     retain_wide1 -14.3%  tree -13.4%
shapes  -9.4%  tree_wide  -6.5%  interp        -5.9%  iso_miss -4.7%
pipeline -3.6% asyncpipe  -1.1%  fib40 / push_num / churn_read ~0
```

Peak memory footprint is unchanged on every row.

Two discriminating tests: `u32_keys_take_the_multiplicative_fast_path` goes red
if `write_u32` is deleted (the default forwards to the byte fold and produces a
different digest), and `sequential_shape_ids_spread_across_low_bit_buckets`
goes red if `mix`'s avalanche is dropped, since ShapeIds are a dense run in the
top half of `u32` while `HashMap` indexes on the low bits.

The symbol profile that found this was previously recorded as structurally
unobtainable. It needs no debug build — only `CARGO_PROFILE_RELEASE_STRIP=none`
plus `PERRY_DEBUG_SYMBOLS=1`, which skips perry's own post-link `strip`. The
"zero `_ZN` frames in `nm`" observation was that `strip`, not LTO
internalization.

Refs #8125, #8113, #8122.
