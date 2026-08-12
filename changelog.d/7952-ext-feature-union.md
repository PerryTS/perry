### Fixed

- **Native extension archives in release packages now share the shipped
  runtime and stdlib feature set (#7358).** Building each `perry-ext-*` crate
  alone could leave a partial second runtime in standalone binaries, attaching
  HTTP work to an async reactor the JavaScript event loop did not drive. Release
  packaging now selects the compiler, both static wrapper crates, and each
  extension in one Cargo invocation.
