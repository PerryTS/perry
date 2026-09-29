Class static accessors (`static get x()` / `static set x(v)`) are now real
accessor properties of the class's function object, so reflection,
`defineProperty`, `delete`, `Object.keys`, `propertyIsEnumerable`, `super.x`
and inherited reads all see one property with its attributes. A write to a
getter-only static is rejected (#11521): strict assignment throws the
TypeError node throws, and `Reflect.set` returns `false`, on the class and on
subclasses. A static walk that reaches a builtin parent (`class E extends
Error`) stops there instead of minting a class function object for it.
