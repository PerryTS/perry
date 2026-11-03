### fix(runtime): classify typed arrays before object headers

Fix #7930: generic property reads on two-element typed arrays no longer return
`undefined` for `.length` and `.byteLength`. `TypedArrayHeader::length` occupies
the payload offset used by `ObjectHeader` for its object-type word, so length 2
previously collided with `OBJECT_TYPE_ERROR == 2` and entered Error-object
dispatch even though the typed-array constructor had produced the correct
length.

The dynamic getter now consults the authoritative typed-array registry before
interpreting any header-shaped payload, roots the receiver across property-key
allocation, and delegates to the canonical typed-array property path. This
preserves own-property/accessor precedence, prototype mutations, indexed reads,
and the remaining typed-array builtins. A regression constructs
`Int32Array(2.5)`, verifies ToIndex produced length 2, and covers generic
`.length` and `.byteLength` reads.
