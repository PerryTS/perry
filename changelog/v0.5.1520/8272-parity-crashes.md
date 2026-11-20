### Fixed

- Fixed `cluster.fork()` HTTP workers losing their worker identity after
  bootstrap, which prevented shared-port coordination and `listening` events.
- Fixed a WebAssembly export-call crash by using the standard
  `WebAssembly.instantiate()` result shape and safe exported-function wrappers.
- The parity sweep now identifies interactive, server-lifecycle, and
  external-service fixtures explicitly instead of reporting their expected
  long-running behavior as runtime crashes.
