Plain objects now read `constructor` from the intrinsic `Object.prototype`
property instead of the mutable `globalThis.Object` binding (#11868).
Reassigning or deleting the global no longer changes existing or newly created
objects' constructors. The ordinary prototype lookup also observes replacement,
deletion, and accessors on `Object.prototype.constructor`, while preserving own
properties and null prototypes.

The regression covers object literals, empty objects, JSON-parsed objects,
`Object.create`, computed reads, and `Reflect.get` with an explicit receiver.
