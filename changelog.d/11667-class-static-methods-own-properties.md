Fixed class static methods not being ordinary own properties of the class.
A static method, named or computed, is now a writable, non-enumerable,
configurable data property of the class function object, whose value is the
method's own function object: `Object.getOwnPropertyDescriptor(C, "m").value
=== C.m`, `C.m === Sub.m`, and replacing, redefining (`Object.defineProperty`,
`Reflect.set`, `Object.assign`) or deleting it is seen by every later call,
including `C.m()` call sites compiled before the change. A deleted static or
prototype method no longer reappears on read, and `getOwnPropertyNames` lists
a class's keys in creation order.
