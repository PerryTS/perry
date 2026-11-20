Object-literal methods are dynamic-`this` functions on every literal path.
A literal with a computed key, an accessor, a spread or a `__proto__` entry
(every Effect, fp-ts and Schema prototype) used to emit its methods as
closures with a captured `this`, patched after construction. A call through
any other receiver (`Object.create(Proto).m()`, `m.call(x)`) then missed the
probe's call path and rebound a cloned closure in the generic method tower
on every call (~0.9 us per call, now ~30 ns), a detached call saw the literal
instead of `undefined`, and a computed method got no `name`.

A typed class getter read admits the receiver's family layout as well as its
birth ShapeId, as direct method sites do. Instances a constructor builds
carry the family layout, so every such read took the generic by-name path
(~480 ns, now ~10 ns).

Private fields and methods work on instances of classes that extend a native
class (`class X extends http.Agent { #m() {} }`, agent-base under npm): the
holder of their private elements is the handle's property bag.

`new Uint16Array(array)` over a dense Array with intrinsic iteration reads
its elements instead of one generic `next` dispatch per element (entities'
decode tables at startup: 49 ms to 2.8 ms). A rest-parameter array is built
with one allocation and one layout rebuild.
