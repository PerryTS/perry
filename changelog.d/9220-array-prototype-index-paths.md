### fix(runtime): honor inherited array indices in writes and borrowed methods

Indexed assignment on an array with a recorded custom prototype now performs
the inherited descriptor walk before creating an own element. Prototype
setters therefore run with the array as their receiver, inherited non-writable
data properties reject the assignment, and inherited writable data properties
still allow the normal own-property creation.

The generic array-like engine used by `Array.prototype.<method>.call(array)`
now uses the same recorded-prototype classification for `Get` and
`HasProperty`. Prototype-filled holes are consequently visible to `join`,
`indexOf`, `map`, `forEach`, and the other generic methods. Default-chain
arrays retain their existing fast paths, while Proxy prototypes keep their
dedicated trap handling. Fixes #9220 and #9221.

The `[[Set]]` owner walk takes the same chain hops the `[[Get]]` walk takes:
`Object.create(p)` models its link with a synthetic class id rather than a
recorded prototype (#809), so without that hop an inherited accessor two links
up was still silently replaced by an own element.

Every one of the three new gates leads with the existing `array_static_proto_recorded`
process latch, so a program that never retargets an array keeps the previous
code path exactly — the strict store's number lane does not even read the slot
it is about to write, and the cold store tail performs no prototype-registry
probe.
