### fix(runtime): an Array subclass in a base-typed binding was read as a raw header (#7574)

`const a: number[] = new MyArr()` (where `class MyArr<T> extends Array<T> {}`)
took the raw `ArrayHeader` fast paths and **SIGSEGVed on the second `.push()`**
(exit 139). Sibling of #7570/#7573 on a different family, and the same premise:
a declared TypeScript type is a hint, never a layout fact. All five binding
forms were affected — `const`, parameter, class field, return type, `as` cast.

An Array-subclass instance is a plain `ObjectHeader` (perry has no exotic
array-object representation, and `js_array_subclass_init` keeps the elements as
ordinary indexed object properties). `ObjectHeader` overlays `ArrayHeader` field
for field: `length` reads `object_type` (= 1), `capacity` reads `class_id`, and
the element slots at +8/+16/+24 are `parent_class_id ‖ field_count`,
`keys_array` and `meta`. `1 <= class_id` passes `clean_arr_ptr`'s
length/capacity sanity check, so the forged header was accepted and the first
push stored `1.0` over `keys_array` — a live GC child edge — while `length + 1`
overwrote `object_type`. The second push dereferenced it and faulted at
`0x3ff0000000000000` (the bit pattern of the double it had just written).

**Fix.** `clean_arr_ptr` now refuses a `GC_TYPE_OBJECT`/`GC_TYPE_CLOSURE`
allocation, making all ~190 of its call sites fail-closed at once (one extra
compare on a byte the surrounding block already loads; the registry probes that
rule out header-less buffers/typed arrays are cold-arm only). On top of that,
the entry points the declared-type tiers actually reach re-enter through their
existing null branch and run the operation on the spec-generic array-like
engine — the same engine the *unannotated* form has always used. Unlike Map/Set
there is nothing to redirect *to*: an Array subclass has no hidden backing, and
minting one would split element storage away from `Object.keys` / `for…in` /
`JSON.stringify` / the generic engine.

Three sites needed more than the funnel: `forEach` must report the **receiver**
(not the dense snapshot `normalize_array_receiver` builds) as the callback's
3rd argument; `sub[i] = v` must run the Array-exotic `length` step, which was
missing on the unannotated path too (`sub[0] = 10; sub.length` read back `0`,
so the next `push` overwrote index 0); and `concat`'s all-dense bulk path had to
stop reading a refused pointer as an empty array, which would have silently
dropped `[1,2].concat(sub)`'s subclass elements.

Two codegen guards are unavoidable — the inline `Expr::ArrayPush` store and
`lower_bounded_array_index_get` emit no runtime call at all, so no runtime
funnel can reach them. Both now test `obj_type == GC_TYPE_ARRAY` and route a
miss to the slow call they already had. Both are strictly more restrictive than
what they replaced, and the bounded-index one is a net instruction *cheaper*.

Validated by `test-files/test_gap_7574_array_subclass_declared_base_type.ts`
(byte-identical to node, exit 0), six sabotage-shaped unit tests, a full revert
of both crates reproducing exit 139, a normalized-IR diff on plain-array
programs showing the only delta is the guard predicate, and a 141-test
array-family A/B with an identical failure set.

Known gap left in place: `ArraySpeciesCreate` on a subclass (node's
`sub.map(f)` returns a `MyArr`, perry a plain `Array`) — pre-existing and
identical on the unannotated path.
