Fixed two CommonJS compilation gaps exposed by Ajv and
fast-json-stringify. Packages whose TypeScript sources combine ESM declarations
with a top-level `module.exports` epilogue now stay on their published
JavaScript graph instead of mixing emitted CommonJS and source-only modules.
Hoisted CommonJS classes can also call safe module-level function declarations
that appear later in the file without losing the binding; helpers that capture
wrapper-local state remain inside the wrapper with their dependent classes.
