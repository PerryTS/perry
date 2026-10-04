Private fields are typed inline slots, and a class with private elements keeps
the compiled class paths (#11791).

The layout of a class reserves an inline slot for each private field of its
chain, and initializing a private field is an append like any key-add: the
slot becomes an `F64` lane when the initializer stores a number. A compiled
`this.#x` / `o.#x` read is one ShapeId compare and a slot load, and a write
stores a raw double into an `F64` lane or a barriered value into any other
slot; the runtime is called only on a miss. A write now checks the brand after
evaluating its right-hand side, as `PrivateSet` does.

When a class's construction is deterministic, the compiler also knows the
shape its finished instances end on (the birth keys plus the private entries,
lanes and brands) and gives it a static ShapeId. The class guards accept it,
so public fields, method calls and the proven-receiver method clones of a
class with private elements take the same fast paths as any other class, and
a private access compares against that id first. `recv.#x` has the declared
type of the field.

`in`, `Object.hasOwn`, `hasOwnProperty`, `Object.keys` and `Object.assign` no
longer look up a separate private-entry summary: the own-key lookup reads the
entry it finds, and the enumerable-only readers drop private entries by the
attribute they already check. `structuredClone` copies only enumerable
properties, so a clone no longer carries an instance's private fields as
properties (a `postMessage` copy already did not); a strict write to a
read-only field of a frozen class instance names the class on the
class-field path too.

On the #10501 benchmark (instructions per operation, public class: 224) the
lru-cache style class now costs 331 (before: 447), a private field read and
write 169 (before: 239) and a private method call updating a public field 188
(before: 273).
