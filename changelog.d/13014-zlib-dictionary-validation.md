- Validate zlib `options.dictionary` using canonical public byte-type brands.
  Accept Buffer, TypedArray, DataView, and ArrayBuffer values; allow `undefined`.
  Report invalid values with property-specific `options.dictionary` type errors.
  Root options and dictionary values across allocating property reads.
  Preserve existing valid-data and CRC32 behavior.
  Dictionary application, getter-count parity, and full-module completion remain separate.
