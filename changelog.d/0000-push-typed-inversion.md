### Changed

- `a.push(i)` where `a` has a static numeric-layout proof (e.g. a `number[]` local or parameter) and `i` is an integer-provenance local now takes the full inline append tier instead of the three-call guarded-numeric tier. The typed receiver was slower than an untyped one for the most common append shape; it is now ~25× faster and ahead of node on the isolated operation.
