### Added

- **Zero-copy native memory views for `bun:ffi` (#6562).**
  `toArrayBuffer` and `toBuffer` now wrap caller-owned native memory without
  copying it, including byte offsets and NUL-terminated spans when the length
  is omitted. The GC owns only a non-moving wrapper and removes its external
  backing-store entry when collected; it never frees the native allocation.
  Optimized `Uint8Array` construction now resolves the actual backing pointer,
  preserving bidirectional native/JavaScript aliasing.
