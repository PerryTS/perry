### Tests

Two node-parity tests pin the receiver a method body sees: sloppy `this` is
bound once per activation (one wrapper for a primitive, `globalThis` for
nullish, objects unchanged), and object-literal and prototype methods called
through a method site get their receiver on every route (nested calls, arrows,
throws, `call`/`apply`, getters, constructors, generators and async methods).
