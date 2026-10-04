`process.getBuiltinModule(id)` reached any way other than the literal
`process.getBuiltinModule(...)` call (an alias, `process?.getBuiltinModule?.(id)`)
returned a module whose functions all returned `undefined`, because only the
literal call armed the dynamic module dispatch. The entry prologue now arms it
whenever the program mentions `getBuiltinModule`.
A module asked for only through `getBuiltinModule("node:x")` is also kept in
auto-optimized builds now: it was never imported, so its implementation was
stripped and every method returned `undefined` even with dispatch armed.
