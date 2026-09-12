### `toLowerCase`/`toUpperCase` gained an ASCII fast path (#10090)

`case_convert` (the shared implementation behind `String.prototype.toLowerCase`
and `toUpperCase`) previously ran every input — including pure ASCII — through
a scalar `wtf8_step` decode, a per-character `char::to_lowercase()` /
`to_uppercase()` iterator, and a re-encode loop. On a 1M-character all-ASCII
string this cost 30-33x what Node takes for the same input.

`case_convert` now checks whether the input is pure ASCII with a real
per-byte scan (`bytes.is_ascii()`) and, if so, produces the result with a
single vectorizable `to_ascii_lowercase()`/`to_ascii_uppercase()` byte-table
transform instead. Everything else — Unicode special casing (`ß`→`SS`,
Cherokee, Deseret, the default-locale `İ`→`i` + combining dot, Greek final
sigma), WTF-8 lone-surrogate round-tripping, and locale-aware casing in
`locale.rs` — is unchanged and continues through the original scalar loop.

The ASCII check deliberately does **not** reuse the existing `is_ascii_string`
helper (an O(1) `byte_len == utf16_len` proxy used elsewhere in this file):
that aggregate can be true for malformed WTF-8 where a stray continuation
byte and a truncated multi-byte lead cancel out in the unit count, even
though the bytes are not ASCII. A regression test
(`case_convert_rejects_the_aggregate_ascii_lie`) locks this in.
