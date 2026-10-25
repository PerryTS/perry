Fixed `new Alias()` where `const Alias = globalThis.X` (or a destructured form) and the program declares its own `class X`: it now constructs the global, not the same-named program class.
