**Class prototypes now expose one coherent, spec-ordered own-key surface**
(#9226). `Object.getOwnPropertyNames(C.prototype)` omitted every accessor,
listed Perry's internal `@@iterator` dispatch alias as if it were a source
string key, and `Object.getOwnPropertySymbols` returned nothing — so
`Reflect.ownKeys` disagreed with `hasOwnProperty` and
`getOwnPropertyDescriptor`, both of which found the missing keys. A two-step
"list the keys, then inspect each one" walk — what decorators, DI containers,
serializers and test-framework method discovery all do — got self-contradictory
answers.

Three separate causes, not one. Accessors were missing because the names
builder read only `vtable.methods`, never `getters`/`setters`. The
`"@@iterator"` string was a *lowering* artifact: every well-known-symbol class
member was diverted away from the computed-key path and registered under a
synthetic string name, so the real Symbol key was never installed anywhere
`getOwnPropertySymbols` could see it. And the order was whatever the dispatch
hash maps happened to yield, because those maps are keyed for lookup speed and
carry no source position.

Only three well-known-symbol forms now need special lowering (a generator
`[Symbol.iterator]`, `static [Symbol.hasInstance]`, and a
`get [Symbol.toStringTag]`); the rest register a real Symbol key, with the
generator form registering both its dispatch wrapper and its Symbol key. The
synthetic dispatch aliases stay in the vtable for fast calls but are filtered
out of enumeration — a class with a source method literally named
`"@@iterator"` still lists it, because that one carries a definition-order
record and an alias does not. Definition order itself comes from the member
function's HIR id, allocated while walking the ClassBody, which is what lets
reflection reconstruct one source order across the separate method, getter,
setter and Symbol registries. Static fields keep first-install order, and a
class key that is deleted and recreated moves to the end, as `[[OwnPropertyKeys]]`
requires.

`Reflect.ownKeys` is now the union in spec order — integer-index strings
ascending, remaining strings in property-creation order, then Symbols in
property-creation order — with `getOwnPropertyNames` and
`getOwnPropertySymbols` as its two halves, and `hasOwnProperty` agreeing with
both for Symbol-keyed class members. A 41-assertion gap fixture that diverges
from `node --experimental-strip-types` on 36 of its 41 lines before the change
is byte-identical after, and a 104-assertion class-prototype differential goes
from 11 divergences to zero.
