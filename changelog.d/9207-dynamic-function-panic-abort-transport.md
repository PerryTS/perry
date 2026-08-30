### Fixed

- Unsupported constructs in runtime-built `Function` bodies now throw a catchable diagnostic instead of aborting at the Function-constructor or closure-dispatch FFI boundaries.
