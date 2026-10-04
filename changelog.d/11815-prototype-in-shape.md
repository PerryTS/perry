An ordinary object's prototype is now a fact of its shape (Refs #10507). A
ShapeId already names its receivers' prototype identity, and that identity now
leads back to the prototype through one word per prototype. So `new F()`,
`Object.getPrototypeOf`, inherited reads, `instanceof` and the method and
accessor sites read a function-constructor instance's prototype from its
ShapeId, and the instance carries no per-instance metadata record. `new F()`
drops from ~1,900 to ~1,165 instructions and 152 bytes per instance. The word
is traced through the receivers that carry the identity, like a shape's key
list, so a prototype stays alive exactly as long as something reaches it.
Also fixed: `Object.create(null) instanceof Object` was true; a class instance
re-parented with `Object.setPrototypeOf` was still `instanceof` its class;
`Object.create(fn) instanceof Function` was false; an `Object.create(null)`
object re-parented with `Object.setPrototypeOf` kept a null-prototype shape.
