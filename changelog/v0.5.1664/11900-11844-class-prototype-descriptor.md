**Fix class prototype descriptors exposing an encoded reference (Closes #11844)**

`Object.getOwnPropertyDescriptor(C, "prototype")` now reads the value and
attributes from the class constructor's own property. A legacy shortcut
returned an encoded class-prototype reference that JSON serialized as a number
instead of the actual prototype object. The descriptor now agrees with
`C.prototype`, including identity and mutations.

The regression covers declared and derived classes, class expressions, repeated
capturing class evaluations, JSON serialization, and both Object and Reflect
descriptor APIs.
