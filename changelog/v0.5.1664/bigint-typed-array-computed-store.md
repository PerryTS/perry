`big[k] = v` on a statically `BigInt64Array` / `BigUint64Array` receiver with a
key not proven to be an array index (`offset / 8` of an untyped value) was
lowered through the plain-`Array` runtime-key setter, which replaced the typed
array with a fresh empty `Array`. The store now goes through the guarded
dynamic typed-array setter. OpenCode v2.1.2's TUI failed at startup because
`@opentui/core` fills its native session record this way.
