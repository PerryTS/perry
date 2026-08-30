### Fixed

- Unsupported constructs in runtime-built `Function` bodies now throw a catchable diagnostic in shipped panic-abort binaries instead of aborting at the Function-constructor FFI boundary.
