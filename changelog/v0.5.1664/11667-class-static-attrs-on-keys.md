A class static's attributes (`writable`/`enumerable`/`configurable`, set by
`Object.defineProperty`, `Object.freeze` or a class's intrinsic `name` and
`length`) are now the key attributes of the class function object's own
properties, as for any ordinary object, instead of a separate per-class table.
Deleting such a static and assigning it again yields an ordinary writable,
enumerable property (the old table kept the deleted key's attributes).
