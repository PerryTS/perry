### `perf(iterator)`: one allocation per `.next()`, not five (#7564)

Every `.next()` the runtime answered itself allocated **five** heap objects to
deliver one value, and four were the same constant on every call: the `"value"`
and `"done"` key strings, the two-element keys array holding them, and a shape
install keyed on that array's address. There were **five** copies of the
constructor, not the three the issue named — `array/iter_object.rs`,
`collection_iter_object.rs`, `string/iter_object.rs`, `buffer/iter.rs` and
`iterator_helpers.rs` — reached by, respectively, array iteration and
`node:sqlite`, `Map`/`Set` method iterators, string iteration, `Buffer`/typed
arrays, and `Iterator.from(...)`.

They collapse into one `crate::iter_result` that shares a keys array per key
order per thread, so the steady-state path performs exactly one allocation: the
result object the language actually requires. Sharing is not a new idea here —
it is already how object literals and `JSON.parse` results work. The array
carries `GC_FLAG_SHAPE_SHARED`, and every path that would mutate an object's
key list (`field_set_by_name`, `delete_rest`, `proxy::put_value`) clones before
writing. `node:sqlite`'s `{ done, value }` order is observable through
`Object.keys`, so it keeps its own array rather than being normalized.

**The shape install was worth more than the allocations.**
`shape_id_for_keys_ensure` keys the shape table on the keys array's *address*,
so a fresh array per `.next()` minted a fresh shape id per `.next()`: every
read of `.value`/`.done` off a result was a guaranteed inline-cache miss
(12.18% of the profile in the issue), and the shape table grew without bound.
One shared array means one shape id for every iterator result in the program.

**A stale-from-space deref went with it.** Four of the five copies ran five
allocations back to back with every intermediate in a bare Rust local, so a
copying minor inside allocation *k* moved what locals 1..k-1 named and rewrote
only slots it could see — which a Rust local is not. Under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` this took a SIGBUS at
`iterator_helpers::make_iter_result + 172`. The fix is structural rather than
more roots: with one allocation, both pointers used after it are re-read from
storage the collector rewrites — the object from its `RuntimeHandleScope`
handle, the keys array from the scanned thread-local.

`iterator_step` — the *read* side of the protocol, in the same call chain — had
the identical defect and is fixed with it. It allocated `"next"`, `"done"` and
`"value"` on every step and held the iterator object, the result object and the
earlier keys across those allocations *and* across a call into the user's own
`next`, which can run arbitrary JS.

`ITER_RESULT_KEYS` is exactly the "runtime-side cache of a raw heap pointer"
the static `gc_root_dominance_check.py` cannot see, so its scanner is
registered in `gc_init` in the same commit, with mark, rewrite, registration
and copy-on-write tests. The tests are sabotage-verified: making the scanner
visit only slot 0 fails mark and rewrite; dropping `GC_FLAG_SHAPE_SHARED` fails
the copy-on-write test.

**Measured**, A/B on identical sources with a full
`-p perry -p perry-runtime-static -p perry-stdlib-static` rebuild per arm, 9
interleaved rounds each: `map.values()` **2.10×**, `string` **1.73×**,
`typedarray.entries()` **1.85×**, `array` manual `.next()` **1.88×** (min; the
medians agree within 5%). Deterministic and host-independent: the same workload
drives **16 → 5** garbage collections. Wall-clock was taken on the shared dev
machine because the pinned quiet host was in use, so treat the ratios as
approximate and the collection count as exact.

Three unrelated pre-existing defects were found and filed while validating
this, all verified present on the pre-fix runtime: #7576 (`Iterator.from(x)`
returns an already-exhausted iterator and `.map`/`.filter` return `undefined`,
so the whole iterator-helpers surface is dead) and #7577
(`js_generator_attach_prototype` derefs retired from-space memory). The gap
test documents why it routes around each.
