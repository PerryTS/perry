- **Native libraries: `buffer+len` takes every typed array.** A manifest
  `buffer+len` parameter accepted only `Buffer`, `Uint8Array`, `ArrayBuffer`
  and `DataView`; a `Float32Array`, `Int32Array`, `Uint16Array` (or a subarray
  of one) threw `TypeError: Expected a Buffer or Uint8Array for native buffer
  span`. It now accepts every `ArrayBufferView` and `ArrayBuffer`, passing the
  view's first byte and its byte length (`length × BYTES_PER_ELEMENT`, after
  `byteOffset`) with no copy. The new structured form
  `{ "kind": "buffer+len", "nullable": true }` passes `(NULL, 0)` for `null`,
  `undefined` or an omitted argument; the plain form still throws for them.
