Typed-array reads in bounds keep their element kind in the whole-program facts: a typed array born with a known length (`new Float32Array([...])`, `new Float64Array(n)`) whose buffer cannot be detached reads a Number, not `undefined`, under an integer key below that length. `matrix.fromArray(new Float32Array(...))` now keeps `Matrix4.elements` a dense `f64` array.

A Number-key read of a length-watched typed array no longer gets the named-key undefined; the in-bounds check (ElemUndef) decides whether undefined is possible.
