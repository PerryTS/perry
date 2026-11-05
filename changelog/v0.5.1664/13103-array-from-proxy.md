Fix `Array.from(proxy)` returning an empty array. Classify a proxy by its GC header and use the existing iterator or array-like protocol, preserving observable property traps.
