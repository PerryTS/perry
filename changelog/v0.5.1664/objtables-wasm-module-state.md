Delete the WebAssembly Module wrapper address registry. The trusted host handle and brand now live in a typed native payload owned by the wrapper's meta record; copied public fields cannot confer a brand. The module registry's move, death and thread-exit work disappears.

Validation: a moving-wrapper and collection test passes; removing the meta-to-native-state edge makes its own negative control fail at the intended assertion. A dedicated Node parity gap covers genuine modules and copied public fields.
