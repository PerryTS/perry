Fixed private field, method, and brand checks spuriously accessing constructor
`this` before `super()`. Lexical private-brand context now loads without a
JavaScript `this` access, allowing Arborist's `PackumentCache.#heapLimit` read
before its base constructor runs while preserving real `this` TDZ errors and
private receiver checks.
