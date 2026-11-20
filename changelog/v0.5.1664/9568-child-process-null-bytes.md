### Fixed

- **`child_process` now rejects embedded null bytes synchronously through both
  direct imports and CommonJS-default namespaces (#9537).** The CJS method-call
  dispatcher bypassed codegen-only validators, so `spawn`, `spawnSync`,
  `execFile`, and related calls handed invalid file/argument strings to the OS
  and later reported `UNKNOWN`. Runtime entry points now validate command,
  file, and indexed argument strings before constructing a process, and
  OS-facing `cwd`, `argv0`, `shell`, and environment strings use Node's exact
  `ERR_INVALID_ARG_VALUE` message, including the property name and escaped
  received value.
