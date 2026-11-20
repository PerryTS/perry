### Fixed

- `Buffer.prototype`'s own property set no longer contains 36 bogus entries (including two bare string literals, `"function"` and `"undefined"`, plus `DataView`/`Uint8Array.prototype`/`Object.prototype` methods that belong further up the prototype chain) and now includes Node's internal `<encoding>Slice`/`<encoding>Write` methods and the deprecated `offset`/`parent` accessors, matching `Object.getOwnPropertyNames(Buffer.prototype)` on Node 26.5.1 exactly (96 own properties, 95 enumerable via `for-in`).
