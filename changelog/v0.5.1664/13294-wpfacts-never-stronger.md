Per-unit facts no longer miss writes into the runtime's own objects.
`Object.prototype`, the builtin namespaces and prototypes and builtin
function values are one object for the whole program, but a unit only saw
its own writes into them: a store or accessor another unit put there, or a
store through a value another unit's code got hold of, left this unit's
literals with readonly and kind claims the write broke (a method installed
there that wrote its receiver made a numeric field read return a string).
Every unit now publishes its writes of these objects, the driver joins them
across rounds, and every unit replays them as writes of unknown values. A
unit that lets one of these objects escape (to another unit's function, to
an unknown callee, into an unknown object) hands it over: every unit's store
into ⊤ may then be a store into it. A value contract names them instead, so
a published binding holding `Object.prototype` reaches its readers as that
object and gives nothing away, and the prototype argument of
`Object.create` / `setPrototypeOf` called through a value is reached only
along a prototype chain, as before. Whole-program facts are unchanged.

The contract runtime now checks each class id against the weakest claim of
all its records (per-unit copies and literal shapes shared across modules)
instead of whichever record was linked last, and whole-program facts with
explain or profile on no longer crash on a program with a for-in loop.
Replayed writes print no explain lines.

Gap tests cover a store, an accessor, a helper storing into a handed
`Object.prototype`, a binding holding it, a callback and a registered hook
called with it, a function run on it by `forEach`, and one returning it;
unit tests check that per-unit template facts never claim more than
whole-program facts on those programs, that a contract naming
`Object.prototype` and `Object.create`'s prototype cost a literal nothing,
that contract records meet, and that the explain statistics count every
instance kind.
