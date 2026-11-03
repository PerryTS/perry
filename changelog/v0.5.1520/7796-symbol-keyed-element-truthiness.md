Fixed a wrong branch on a symbol-keyed property read.

Reading a symbol-keyed property off an array gave a value that every other
test agreed was a function — `typeof` said `"function"`, `Boolean()` said
`true`, `=== undefined` said `false` — but that an `if` treated as falsy:

```ts
const a = [1];
const f = (a as any)[Symbol.iterator];
if (f) { /* never ran */ }
```

The inline form `if ((a as any)[Symbol.iterator])` was correct; only storing
the value in a local first went wrong, which is what made it look so strange.

The compiler types an element read of a `number[]` as a number, and it was
doing that no matter what the index was. `a[Symbol.iterator]` is not an
element read at all — it reads a property of the array object, and the answer
is a function. So the local holding it was recorded as a number.

That mistake is expensive rather than merely imprecise. A value believed to be
a number is tested for truthiness with a floating-point comparison against
zero, and Perry stores objects, strings and functions as NaN-boxed doubles —
which really are NaN. Every comparison against a NaN is false, so the test
reported "falsy" for every function, object and string that reached it.

Both places that infer an element type now require the index to be a number
first. Requiring proof rather than just the absence of contrary evidence is
deliberate: an index the compiler cannot type may hold anything at runtime,
and the cost of answering "not a number" is one missed fast path, while the
cost of answering "number" is a branch that goes the wrong way.

Ordinary numeric indexing is untouched — `a[i]` in a loop keeps the same
inline comparison it had before, which a companion test pins down.

Closes #7796.
