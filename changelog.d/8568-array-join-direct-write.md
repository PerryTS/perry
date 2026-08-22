**Speed up `Array.prototype.join` by writing directly into GC string storage.**

Dense joins of strings, booleans, holes, `null`, and `undefined` now compute the
exact output size, allocate one `StringHeader`, and copy each piece into its
payload instead of building and freeing an intermediate Rust `String`. Joins
that invoke getters or `toString` use a rooted growable writer, preserving
exactly-once coercion, moving-GC safety, and WTF-8 lone-surrogate behavior.
