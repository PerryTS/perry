Regression tests pin that a wasm `join` inside a call argument and the array
bridge calls (`toReversed`, `toSorted`, `toSpliced`, `with`, `copyWithin`,
`entries`, `keys`, `values`) open their own argument frame (fix in #14742).
