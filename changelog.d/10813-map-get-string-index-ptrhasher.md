`MAP_STRING_INDEX` (the content-hashed side table behind string-keyed
`Map.get`/`set`/`has`/`delete` once a map grows past `SIDE_TABLE_THRESHOLD`)
hashed its inner `u64 -> Vec<u32>` table with `std::collections::HashMap`'s
default SipHash. The `u64` key there is not raw input — it is already the
FNV-1a content hash computed above it, so SipHash was paying for a second,
unrelated hash of an already-well-mixed value on every string-keyed lookup.
Switched the inner table to `PtrHasher` (a single multiply-by-Fibonacci-
constant plus an xorshift avalanche), the same treatment the sibling
`NumericIndex.hashed` table already gets for the same reason (a computed,
non-adversarial `u64` key).

This is not a HashDoS regression: two colliding `u64` FNV-1a hashes land in
the same `Vec<u32>` bucket under *any* hasher — SipHash on the outer table
could never have defended against a crafted FNV-1a collision, because the
attack surface is FNV-1a itself, which is unchanged. `HashMap<K, V,
S>::get` also re-checks `K: Eq` on every candidate regardless of `S`, so a
hasher swap cannot change which entry a lookup resolves to, only how fast it
gets there — a property pinned by a new test that inserts 10,000 distinct
string keys (forcing real bucket collisions at both the outer map-pointer
and inner content-hash levels) and asserts every key still resolves to its
own value, including reverse-order reads, content-equal-but-freshly-
allocated keys, adversarial shared-prefix misses, and delete-half-then-
verify.

Measured as a flat constant-factor win across an N-sweep from 16 to 4,096
map entries (well past the growth threshold): 115-121 instructions saved
per lookup at every size tested, for both hits (interned and dynamically
built keys) and misses — not a small-map-only effect. (Absolute figures
were taken under `PERRY_NO_AUTO_OPTIMIZE=1`, whose own overhead varies
wildly by workload -- ~0.3% on string concat, 8x on `String.prototype.
split` elsewhere -- so treat the absolutes as flag-qualified; the 115-121
delta is unaffected, since both arms of every comparison shared the flag
and the same runtime archive otherwise.) An initial reading of one shape
(dynamically-rebuilt keys) showed an apparent ~38-instruction-per-doubling
growth with N; that turned out to be a probe artifact (the decimal key
suffix `"key" + (i % size)` grows a digit as `size` grows, so key length
was correlating with N) and vanished under a length-controlled follow-up.

This change is **not** a fix for the separately-reported (#10697)
string-Map slowdown on small (below-`SIDE_TABLE_THRESHOLD`) maps: that
session profiled the generic dispatch path directly and found
`find_key_index_cold` running on every lookup of a four-entry map, with
`jsvalue_eq` doing ~149 instructions of generic equality on a key already
known at compile time to be a string, plus `string_view_from_bits`
re-deriving what codegen had already proven -- ~361 instructions against
node's ~50. A related but narrower observation surfaced while chasing that
repro (`m.get(arr[i])` dispatches to the specialized string-keyed path,
but binding the same expression through a local first, `let key = arr[i];
m.get(key)`, does not) -- confirmed to require both an inline index
expression *and* a `<string, number>`-annotated map, not the binding shape
alone. Neither is a hashing cost; both are codegen type-proof gaps, tracked
and owned separately from this change.
