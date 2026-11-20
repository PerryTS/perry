### Tests

- Pin the `gc-leaf-function` marker on Perry's inline-assembly loop barrier in
  both text and native LLVM emission. This closes the remaining regression gap
  for #8121: the existing hand-written IR tests covered the LLVM behavior but
  could not detect either Perry emitter dropping the marker and reintroducing
  the `rewrite-statepoints-for-gc` SIGBUS.
