Collapse the inline guarded element read for a dynamically typed receiver
(`a[i]` on an erased or union type) from ~51 basic blocks and two runtime call
sites per site to 21 blocks and one call. The dense-Array arm and the
elements-backed Array-subclass probe stay inline and byte-identical; the
shape-carried subclass IC tower (which never fired in the shipped
configuration), the lazy-JSON probe and the spill paths move behind the
existing `js_packed_arraylike_index_get` dispatcher; the typed-array ladder
collapses from eight element kinds onto four element widths, with the brand
test decided on the tag alone. Per site 345 → 173 IR instructions. On
prettier/plugins/flow.mjs `.text` shrinks a further 15.8 % on top of the
budget and property-get changes, ordinary workloads keep their instruction
counts and RSS, and dynamic-receiver typed-array and plain-array reads run
10–32 % fewer instructions. The dispatcher now checks `GC_FLAG_FORWARDED`
before probing a lazy JSON array, which #10098 made movable.
