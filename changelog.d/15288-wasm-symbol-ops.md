On `--target web`, `Symbol()`, `Symbol(description)` and `Symbol.keyFor(sym)` evaluate instead of reading `undefined`, and `sym.description` reads the symbol's description.
