Pass a `Uint8Array` argument through the typed-array spec ABI (`TaPtr`) like
the other numeric kinds (#13774). `new Uint8Array(n)` lowers to
`Uint8ArrayNew`, which the call-site pre-pass did not recognize, so the callee
kept the boxed ABI and `s += a[i]` went through the dynamic string-or-number
add. The pre-pass now proves `Uint8ArrayNew` bindings under the same rules as
`TypedArrayNew`, and a `Uint8ArrayGet` off a `TaPtr` parameter takes the same
checked load as `IndexGet`: a bounds compare against the hoisted length and a
byte load, with no per-read receiver guard.
A `+= 1` loop summing a 2^20-element `Uint8Array` 400 times drops from 1354 ms
to 318 ms, level with `Uint16Array` (335 ms).
