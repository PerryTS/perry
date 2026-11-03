### Fixed

- `Array.isArray` now checks the current runtime value of reassigned locals instead of folding from the binding's initializer. This fixes both false negatives after assigning an array and false positives after assigning a non-array (#7844).
