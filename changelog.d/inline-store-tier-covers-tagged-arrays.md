The inline array-store guard now covers receivers that are not raw-f64.

`lower_index_set_fast` builds a guard that tests, inline, everything the
out-of-line `js_typed_feedback_plain_array_index_set_guard` tests: array type,
not-forwarded, no element descriptors, the integrity flags, the prototype-chain
invalidation byte, and the length/capacity sanity bounds. It then jumps straight
to the store, skipping the call.

That whole tier was gated on `require_numeric_layout`, so it was built only for
statically numeric receivers. A `boolean[]` — or any downgraded `any[]` — went to
the call on every store, forever, even though the in-bounds arm below already
knows how to store a tagged value into such a receiver (that arm is exactly what
the out-of-line guard fronts today).

Only two of the guard's conditions belong to the raw-f64 store, and they are now
applied only when that store is the one being emitted:

* the receiver's raw-f64 layout bits, because the raw arm writes an unboxed
  double into the slot and that is valid only while the layout says the elements
  are pointer-free — and a downgraded receiver has those bits clear by
  definition, which is precisely why requiring them pinned `boolean[]` to the
  call tier;
* the runtime numeric-tag test on the stored value, which exists because a
  `number[]` slot can genuinely receive a non-number and the raw arm would write
  its NaN-boxed tag verbatim. The tagged arm stores the box as a box.

Measured on an idle Mac mini, all binaries built in one run, interleaved, min of
five, self-timed:

| | main | with #9246 | this change | node |
|---|---:|---:|---:|---:|
| boolean-store loop | 207 ms | 138 ms | **64 ms** | 12 ms |
| `11_prime_sieve` | 27 ms | 20 ms | **11 ms** | 6 ms |

`11_prime_sieve` moves from 4.5× Node to 1.8×. A nested-loop read benchmark is
unchanged, as expected.

Verified against Node on a differential written for this change specifically —
the guard must reject exactly what the call rejects: a frozen array (stores
ignored), a sealed array and one under `preventExtensions` (in-bounds writes
allowed, growth refused), an element accessor descriptor (the setter must run),
extension past length, mixed types through one slot, and a store into an array
that was numeric. Byte-identical, plus five pre-existing differentials unchanged.

One case in that differential diverges from Node — an `Array.prototype` index
setter installed with `Object.defineProperty` is bypassed — and it diverges
**identically on unmodified main, for numeric receivers too**, so it is neither
caused nor widened here. Filed separately; the flag the guards consult is only
set by an index *write* to the prototype, never by `defineProperty`.
