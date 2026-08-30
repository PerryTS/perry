`new Uint8Array(SIZE * SIZE)` now keeps the buffer-view tier that
`new Uint8Array(SIZE)` already had.

`is_fresh_uint8array_length_expr` accepted a literal or a known-length local, but
nothing built from them, so an allocation whose size was an arithmetic expression
was never recognised as a freshly allocated owned buffer. Without that
classification the receiver gets no view at all, and with no view every element
read falls back to a runtime call — however well its index is proven, and however
inline the arithmetic around it already is.

That is why `benchmarks/suite/bench_int_arithmetic.ts` still paid 54
`js_uint8array_index_get_value` calls per pixel after the interval bounds proof
landed: not the index, the allocation. Changing only that benchmark's allocation
to a literal length, with nothing else touched, is what isolated it.

The predicate asks whether the allocation's size is FIXED, never what it is —
`length_source_from_expr` resolves the value later, with a `FnCtx` in hand, and
records no constant length when it cannot. A sum, difference or product of the
same leaves is therefore exactly as fixed as either leaf alone, and qualifies on
the same argument.

Measured on an idle Mac mini against this change's exact merge base, both
binaries built in one run, interleaved, min of five, self-timed:
`bench_int_arithmetic` 462 → 149 ms with Node at 62 — 7.5× Node down to 2.4×,
identical checksums.

Verified byte-identical to Node on the two differentials from the parent change
(eight cases around the byte read, seven around the bounds proof), plus the
probe whose interval genuinely exceeds its buffer, which must and does still
decline to the checked call rather than reading out of bounds.
