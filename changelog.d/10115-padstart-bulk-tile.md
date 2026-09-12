**`padStart`/`padEnd` no longer fill their padding one UTF-16 code unit at
a time for an ASCII/surrogate-free pad string** (#10091), which was
79.57x Node at a 1M-unit target (24.51x at 10k, 52.61x at 100k) and grew
worse with size — `build_pad_chunk`'s per-unit loop did a modulo, a
surrogate-range check, and a byte push for every output unit, work that
can never actually branch differently when no unit in the pad string is
a surrogate.

`pad_units_surrogate_free` detects that (overwhelmingly common) case;
`build_pad_result_surrogate_free` then encodes one pad cycle once and
grows it to the needed length by doubling (`tile_by_doubling`) directly
into the same buffer the receiver's bytes are copied into — a
logarithmic number of bulk copies instead of one decision per output
unit, and one fewer full-buffer copy than before (the tiled result no
longer passes through a separate `pad_chunk` buffer on its way into the
assembled string). The general per-code-unit path — needed only when the
pad string itself contains a surrogate, since cycling it can straddle a
surrogate pair across a cycle boundary or truncate one into a lone
surrogate — is unchanged.

On the issue's own reproducer (`'!'.padStart(n*4+1, "aBcD")`), the
Perry/Node ratio at n=1,000,000 drops from 79.57x to 11.45x (10k:
24.51x → 3.88x; 100k: 52.61x → 8.75x), with the fitted log-log slope
moving from 0.948 toward Node's 0.490. Checksums matched Node at every
size, including the unchanged Unicode/astral-pad fallback path and every
spec edge case: empty pad string, an already-long-enough receiver,
fractional/negative/`Infinity` target lengths, an astral pad string
truncated mid-surrogate-pair, and a receiver that already carries a lone
surrogate being re-padded. `repeat` was already doubling-copy based and
is untouched.
