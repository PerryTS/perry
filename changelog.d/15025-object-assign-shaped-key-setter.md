### Fixed

- `Object.assign` no longer kills the program with SIGTRAP when a source key built at runtime (`s["s" + i] = v`) names a setter on the target's prototype chain: whole-program facts now copy such keys through `Object.assign`, object spread and object rest, so the setter runs as it does in Node.
