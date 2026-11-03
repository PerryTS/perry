Fixed class members reached through an ordinary object whose class id is 0 but whose prototype chain reaches a class, such as `Object.setPrototypeOf(Object.create(null), C.prototype)`, `Object.create(C.prototype)` and `Object.create(new C())` (#11201). Three things went wrong on this receiver shape:

- A class method, called directly or through `.call`/`.apply`/`.bind`, got the owner's internal prototype marker as `this`. The body saw `typeof this === "number"` and every `this.x` read `undefined`. `canonical_bound_method_receiver` now also accepts a class-id-0 object as the receiver.
- An inherited class setter was skipped, so the write created an own data property instead. The `[[Set]]` walk now checks the class vtable of a class-backed link (`class_link_accessor_set`).
- A class getter that read an inherited property of its own receiver got `undefined`. The prototype-resolution cycle guard mistook the getter body's new lookup for a cycle. Getter bodies now run behind a `UserCodeResolutionBoundary`.

Once `Object.create` stops minting a synthetic class id per call (#11166), every `Object.create`d object has this shape.
