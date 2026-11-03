### Fixed

- `Buffer.isBuffer` now rejects non-Buffer byte-storage objects such as
  `ArrayBuffer` and `DataView`, while Buffer instances inherit through
  `Buffer.prototype` and `Uint8Array.prototype` as they do in Node.
