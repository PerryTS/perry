A class member with a computed key the compiler knows (a string or number
literal, a well-known symbol, `Symbol.for("lit")`) is now an ordinary member,
so a class with `*[Symbol.iterator]()` keeps its static prototype and direct
field lowering (three.js Vector2/3/4, Color, Quaternion, Euler:
`Vector4.applyMatrix4` 439 -> 55 ns, r1 664 -> 42 ns/iter). Symbol members
list after the string members (`Reflect.ownKeys` order as in node), a
getter-only `get [Symbol.toStringTag]()` is a real prototype accessor, and a
redefined method keeps its first position with the last definition.
