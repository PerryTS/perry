### Inline caches are allocated per *used* site, not per emitted site (#9708)

Every inline-cache site codegen emitted — the generic property read, the
static- and dynamic-key write ICs and their poly tail, the Symbol-keyed and
composed `o[sym].field` reads, the Array-subclass `length` / `[i]` caches,
the fused `if (a.f[i]) return a.f[i]` cache and the imported-object method
guard — owned a `[12 x i64] zeroinitializer` global: 96 B of `__bss` per
site whether or not the program ever executed it. On the Claude Code bundle
that was 262k caches, 25 MB of zero-fill, and 18.7 MB of it **dirty resident
memory at idle**, because a page is dirtied by the first cache touched on it
and the few thousand hot sites are scattered across all of them.

A site now owns an 8-byte pointer **slot**, `@perry_ic_N = private global
ptr null`. The cache words live in a runtime arena
(`perry-runtime/src/object/field_get_set/ic_slot.rs`): the miss handler
resolves the slot with `pic_slot_resolve` the first time it actually
*primes* the site, bump-allocates the words from a 64 KiB zeroed chunk and
publishes them with a compare-and-swap (two `perry/thread` agents racing on
one site agree on one cache). A miss that cannot prime — proxy, string or
small-handle receiver, a missing key, an accessor, a frozen target — never
touches the slot, so such a site costs its 8 bytes and nothing else; the
write IC's poly tail is not allocated until a fifth shape arrives. Cache
layout and every prime/evict policy are unchanged: the runtime writes the
same words through the same `PicCache` type, and `pic_slot_resolve` sizes
the allocation from that type, so the width pairing test keeps its meaning.

**Hot path.** Each inline hit path loads the slot (a load with no dependency
on the receiver, so it issues alongside the header loads) and folds `!= null`
into the receiver guard it already evaluates — one fused compare, no new
block — then reads the cache words through the loaded pointer; the runtime
entries take the slot's address. Where a site reads word 0 inside a flat
predicate (the dynamic-key write IC, the array-like index cache, the method
guard) it reads through `select(present, cache, slot)`: the slot's own 8
bytes of null are exactly the zero token an empty global used to read as, so
the branch structure and the transition-IC reachability are untouched.
Measured on x86-64 (`perf stat -e instructions:u`, perry-dev builds):

| program | base | lazy slots | delta |
|---|---:|---:|---:|
| all-generic-IC microbenchmark (95M IC ops) | 13.662 G | 14.196 G | +3.9 % (3 instr per hit: slot load, `test`, never-taken `je`) |
| `bench_object_property` | 250.9 M | 248.6 M | −0.9 % |
| `bench_json_readonly` | 2 263.2 M | 2 258.6 M | −0.2 % |
| `bench_dynamic_property_keys` | 1 129.1 M | 1 124.5 M | −0.4 % |
| `07_object_create`, `09_method_calls`, `12_binary_trees`, `14_closure` | | | ±0.00 % |

The typed-feedback IC counters (`PERRY_TYPED_FEEDBACK_TRACE`) are identical
on both arms for the microbenchmark — 81 666 674 guard passes, 18 333 339
guard failures, 18 333 339 fallback calls over 18 sites — so hit rates are
unchanged, not merely output. On the issue's target (macOS arm64) the fused
compare is a `ccmp`, so the hit-path cost there is the slot load plus one
instruction.

**Footprint.** A generated probe with 16 000 read sites of which 1 604 prime
(every 10th function runs — the scattered-hot-sites shape from the issue),
Linux x86-64, 4 KiB pages: `.bss` 2 408 752 → 997 144 B, whole-process
anonymous `Private_Dirty` 1 660 → 632 kB. The `PERRY_GC_CENSUS` side table
gains an `ic.lazy_caches` row (resolved sites, arena bytes) so a run can
assert the subject was live; the issue's macOS `vmmap` numbers are the ones
to re-measure on a bundle build.

Gap coverage: `test_gap_9708_lazy_inline_cache_slots.ts` exercises every
IC shape across the null → allocated transition — mono/poly/megamorphic
reads, a site that can never prime, a nullish first read, inherited
properties, static writes through the four ways and the poly tail, a frozen
target, rotating dynamic keys, Symbol and composed Symbol-then-field reads
with invalidation, Array-subclass `length`/index, the fused field-index
return, and hundreds of never-executed sites — and matches node byte for
byte. `array/subclass.rs` was at the 2 000-line cap, so
`js_packed_arraylike_index_get` and its cache types moved to the child module
`array/subclass_packed_index.rs`.
