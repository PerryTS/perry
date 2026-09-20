`TextEncoder` and `TextDecoder` instances are now ordinary objects with their
own identity. They used to travel as small registry integers under
`POINTER_TAG`, so a value's identity was its registry id: `new TextEncoder()
=== new TextEncoder()` was `true` (every encoder shared one stateless sentinel
id), two encoders collapsed into a single `Map` or `Set` key, and a `WeakMap`
entry stored under one encoder was readable through an unrelated one (#10821).

An instance is now a `GC_TYPE_OBJECT` with a real ShapeId, a family class id and
a per-family prototype — identical in kind to an object TypeScript itself
creates — so the whole object surface matches node without a per-kind arm
anywhere: `typeof` is `"object"`, `Object.keys` / `getOwnPropertyNames` are `[]`,
`JSON.stringify` is `{}` (it was `null`), `{...d}` is empty, `for...in` yields
the WebIDL-enumerable prototype members, `instanceof` works through the ordinary
prototype walk, `Object.getPrototypeOf(d) === TextDecoder.prototype`, and
`TextDecoder.prototype.decode.call({})` throws a `TypeError` on a foreign
receiver. `decode` / `encode` / `encodeInto` are real methods on the prototype
and `encoding` / `fatal` / `ignoreBOM` are real accessors, all found by ordinary
lookup instead of a handle-dispatch table.

A decoder's entire state — which of the ~40 fixed encodings, `fatal`,
`ignoreBOM` — packs into one word in the object's own meta record, so
`DECODER_REGISTRY` is deleted: it was a mutex-guarded map that grew one entry per
`new TextDecoder()` and was never pruned, i.e. a leak and a lock on the decode
path, both now gone.

This is the first family of the "honest tags" invariant (#340/#341): a
`POINTER_TAG` value should always be a dereferenceable GC cell, never a small
registry integer. No codegen change — these natives already return an `i64` that
codegen boxes with `POINTER_TAG`, and returning the object address instead of an
id is identical at the C ABI. The emitted small-handle guards and the `addr_class`
band predicates stay exactly as they are until every family has moved.
