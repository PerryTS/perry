`--target web`: `for...of` over a generator, Map, Set, string or untyped value
iterated nothing, array destructuring bound `undefined`, and a generator lost
its local variables between resumes. `for...of` and array destructuring now
drive the iterator protocol as on native targets, closing the iterator on
`break`.
