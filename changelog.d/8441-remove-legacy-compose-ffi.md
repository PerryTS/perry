### Fixed

- Removed `perry-container-compose`'s unused legacy `ffi` feature and duplicate
  `js_compose_*` exports, which implemented a 4-byte string header incompatible
  with the runtime's 20-byte string ABI. Compose FFI remains available through
  the canonical stack-handle exports in `perry-stdlib`.
