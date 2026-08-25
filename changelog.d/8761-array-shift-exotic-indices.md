Fixed `Array.prototype.shift` on arrays whose indexed operations are
observable. The dense `memmove` fast path is kept for ordinary arrays, but a
receiver carrying index accessors / custom-attribute descriptors, sparse
storage past the dense backing store, or an indexed property inherited from
`Array.prototype` / `Object.prototype` now runs the specified live
`HasProperty` / `Get` / `Set` / `Delete` sequence, so inherited indices,
holes and getter side effects (freezing the receiver, or making `length`
non-writable before the final `Set(O, "length", …)`) are all observed in spec
order. The dense path additionally translates the internal `TAG_HOLE`
sentinel to `undefined`, and the spec path roots the receiver and the carried
values across every accessor that can allocate or move them.
