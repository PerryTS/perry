**A strided constant-fill kernel for the sieve idiom, and `11_prime_sieve`
beats node** (11 ms → 5 ms against node's 6 on an idle machine; primes
identical).

`for (j = A; j < B; j += S) arr[j] = false` — the sieve's inner loop — paid
the inline dense-store guard chain (~10 header/flag/bounds checks) per
element, for a receiver that cannot change mid-loop. The isolation matrix put
the entire benchmark gap on the boolean element layout: the identical sieve
over a `number[]` already ran at parity through the numeric store tiers, and
the boolean variant has no clone tier to take, so every store re-derived the
guard. `js_array_fill_range_strided_tagged` validates the receiver once and
stores the constant's NaN-box bits in a tight native loop; the sieve nest
itself went 9 ms → 1 ms, four times faster than node's.

The matcher takes `arr[j] = C` where `C` is a boolean, `null`, or `undefined`
literal, the bound and stride are integer literals or loop-invariant plain
locals, and the init shape is irrelevant — it was lowered before the matchers
run, so the kernel reads the start from the counter local, the same trick
`numeric_range_add` uses for `j = i * i`. Numbers are deliberately not
matched: a numeric fill targets a raw-f64-layout array in practice, where the
kernel must decline anyway (tag bits stored into unboxed-double storage would
be read back as doubles), so the call would be a toll with no fast path
behind it.

Decline routes, each `-1`-with-nothing-stored into the ordinary loop:
non-array receivers, frozen/sealed/descriptor arrays, raw-f64 layouts (the
generic path downgrades the layout properly), out-of-range windows (node's
semantics for an out-of-bounds strided store is growth, which the generic
path implements), and an active incremental-mark phase (overwriting pointer
slots then requires the deletion barrier the generic per-element store
emits). Each route is pinned against node's output — including the mixed
raw-f64 downgrade and the growing store — under normal and
forced-evacuation runs.
