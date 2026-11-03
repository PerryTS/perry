Per-unit compiles of a class relinked as another class's parent are correct.
After `Object.setPrototypeOf(B, A)`, `Reflect.setPrototypeOf(B, A)` or
`B.__proto__ = A`, `B`'s `super()` runs `A`'s constructor on a `B` instance.
When the relink sat in a different statement or module than `A` or `B`, two
per-unit facts were stronger than the whole program's: `this` of `A`'s
constructor held only `A` instances, and a unit that merely constructed `B`
vouched that `B` was never relinked, so `super()` called `B`'s declared
parent (wrong output, for example `undefined` instead of the fields `A`'s
constructor sets). Now the unit doing the relink reports the given class, and
that class's unit runs its constructor on any receiver, as it does for a class
an `extends` names. Only a class's own unit says whether it is relinked.

Constructing `AbortSignal` through a value (`const S = AbortSignal; new S()`)
or through the live `super()` of `class X extends AbortSignal` now throws
`TypeError: Illegal constructor`, as in Node; it used to return a signal.
