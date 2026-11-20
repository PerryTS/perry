`Symbol.iterator in headers` and `.constructor` on
Response/Request/Headers/Blob/File/FormData handles were not resolved, and
`Object.fromEntries(headers)` dereferenced the handle id and segfaulted.
Fetch handles resolve own and inherited constructors without consulting mutable
global bindings. Symbol presence checks do not invoke getters, and
`Object.fromEntries` materializes handle entries once without reading a heap
header from a handle id.
